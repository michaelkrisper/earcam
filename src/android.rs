//! Small Android platform services over JNI: a toast, finishing the activity, and keeping
//! the screen on. The WiFi request lives in `wifi/android.rs` and shares these helpers.
//!
//! None of this goes through android-activity's `AndroidApp`: its methods take a lock
//! that the event loop holds while it runs, so calling the mutating ones (such as
//! `set_window_flags`) from the app, on any thread, stalls the event loop until Android
//! declares the app not responding.

use jni::objects::{GlobalRef, JObject, JValue, JValueOwned};
use jni::{JNIEnv, JavaVM};

/// `PowerManager.SCREEN_BRIGHT_WAKE_LOCK | ON_AFTER_RELEASE`: the screen stays on at full
/// brightness while the camera image is shown, as the old View's `keepScreenOn` did.
const SCREEN_BRIGHT_WAKE_LOCK: i32 = 0x0000_000a;
const ON_AFTER_RELEASE: i32 = 0x2000_0000;

/// Keeps the screen on while held; dropping it lets the screen time out again.
pub struct ScreenOn(GlobalRef);

impl ScreenOn {
    pub fn acquire() -> Option<ScreenOn> {
        let result = java_vm().and_then(|vm| {
            let mut env = vm.attach_current_thread().map_err(jni_error)?;
            let power = system_service(&mut env, "power")?;
            let tag = env.new_string("EarCam:camera").map_err(jni_error)?;
            let lock = call(
                &mut env,
                &power,
                "newWakeLock",
                "(ILjava/lang/String;)Landroid/os/PowerManager$WakeLock;",
                &[
                    JValue::Int(SCREEN_BRIGHT_WAKE_LOCK | ON_AFTER_RELEASE),
                    JValue::Object(&tag),
                ],
            )?
            .l()
            .map_err(jni_error)?;
            call(
                &mut env,
                &lock,
                "setReferenceCounted",
                "(Z)V",
                &[JValue::Bool(0)],
            )?;
            call(&mut env, &lock, "acquire", "()V", &[])?;
            env.new_global_ref(lock).map_err(jni_error)
        });
        match result {
            Ok(lock) => Some(ScreenOn(lock)),
            Err(e) => {
                log::warn!("keeping the screen on: {e}");
                None
            }
        }
    }
}

impl Drop for ScreenOn {
    fn drop(&mut self) {
        let result = java_vm().and_then(|vm| {
            let mut env = vm.attach_current_thread().map_err(jni_error)?;
            call(&mut env, self.0.as_obj(), "release", "()V", &[]).map(drop)
        });
        if let Err(e) = result {
            log::warn!("releasing the screen: {e}");
        }
    }
}

/// Shows a short system message. Works from any thread: a Toast wants a Looper on the
/// calling thread, so one is prepared if there is none. Call it from a thread of its own,
/// not the event loop's.
pub fn toast(text: &str) {
    let result = java_vm().and_then(|vm| {
        let mut env = vm.attach_current_thread().map_err(jni_error)?;
        let looper = static_call(
            &mut env,
            "android/os/Looper",
            "myLooper",
            "()Landroid/os/Looper;",
            &[],
        )?;
        if looper.l().map_err(jni_error)?.is_null() {
            static_call(&mut env, "android/os/Looper", "prepare", "()V", &[])?;
        }
        let context = activity();
        let text = env.new_string(text).map_err(jni_error)?;
        let toast = static_call(
            &mut env,
            "android/widget/Toast",
            "makeText",
            "(Landroid/content/Context;Ljava/lang/CharSequence;I)Landroid/widget/Toast;",
            &[
                JValue::Object(&context),
                JValue::Object(&text),
                JValue::Int(1),
            ],
        )?
        .l()
        .map_err(jni_error)?;
        call(&mut env, &toast, "show", "()V", &[]).map(drop)
    });
    if let Err(e) = result {
        log::warn!("toast: {e}");
    }
}

/// Finishes the activity, as the back button should.
pub fn finish() {
    let result = java_vm().and_then(|vm| {
        let mut env = vm.attach_current_thread().map_err(jni_error)?;
        call(&mut env, &activity(), "finish", "()V", &[]).map(drop)
    });
    if let Err(e) = result {
        log::warn!("finish: {e}");
    }
}

pub(crate) fn java_vm() -> Result<JavaVM, String> {
    let ctx = ndk_context::android_context();
    unsafe { JavaVM::from_raw(ctx.vm().cast()) }.map_err(jni_error)
}

/// The Activity, owned by android-activity for the life of the app.
pub(crate) fn activity() -> JObject<'static> {
    unsafe { JObject::from_raw(ndk_context::android_context().context().cast()) }
}

pub(crate) fn jni_error(e: impl std::fmt::Display) -> String {
    format!("JNI: {e}")
}

/// `Context.getSystemService(name)`.
pub(crate) fn system_service<'local>(
    env: &mut JNIEnv<'local>,
    name: &str,
) -> Result<JObject<'local>, String> {
    let name = env.new_string(name).map_err(jni_error)?;
    call(
        env,
        &activity(),
        "getSystemService",
        "(Ljava/lang/String;)Ljava/lang/Object;",
        &[JValue::Object(&name)],
    )?
    .l()
    .map_err(jni_error)
}

/// Calls a method, turning a pending Java exception into an error (and clearing it, so
/// the thread can keep using JNI).
pub(crate) fn call<'local>(
    env: &mut JNIEnv<'local>,
    obj: &JObject,
    name: &str,
    sig: &str,
    args: &[JValue],
) -> Result<JValueOwned<'local>, String> {
    let result = env.call_method(obj, name, sig, args);
    check(env, result, name)
}

pub(crate) fn static_call<'local>(
    env: &mut JNIEnv<'local>,
    class: &str,
    name: &str,
    sig: &str,
    args: &[JValue],
) -> Result<JValueOwned<'local>, String> {
    let result = env.call_static_method(class, name, sig, args);
    check(env, result, name)
}

pub(crate) fn new_object<'local>(
    env: &mut JNIEnv<'local>,
    class: &str,
    sig: &str,
    args: &[JValue],
) -> Result<JObject<'local>, String> {
    let result = env.new_object(class, sig, args);
    check(env, result, class)
}

fn check<T>(env: &mut JNIEnv, result: jni::errors::Result<T>, what: &str) -> Result<T, String> {
    result.map_err(|e| {
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        format!("{what}: {e}")
    })
}
