//! Small Android platform services over JNI: a toast, finishing the activity, and keeping
//! the screen on. The WiFi request lives in `wifi/android.rs` and shares these helpers.
//!
//! None of this goes through android-activity's `AndroidApp`: its methods take a lock
//! that the event loop holds while it runs, so calling the mutating ones (such as
//! `set_window_flags`) from the app, on any thread, stalls the event loop until Android
//! declares the app not responding.

use jni::errors::{Error, Result as JniResult};
use jni::objects::{Global, JObject};
use jni::signature::MethodSignature;
use jni::strings::JNIStr;
use jni::{Env, JValue, JValueOwned, JavaVM, jni_sig, jni_str};

/// `PowerManager.SCREEN_BRIGHT_WAKE_LOCK | ON_AFTER_RELEASE`: the screen stays on at full
/// brightness while the camera image is shown, as the old View's `keepScreenOn` did.
const SCREEN_BRIGHT_WAKE_LOCK: i32 = 0x0000_000a;
const ON_AFTER_RELEASE: i32 = 0x2000_0000;

/// Keeps the screen on while held; dropping it lets the screen time out again.
pub struct ScreenOn(Global<JObject<'static>>);

impl ScreenOn {
    pub fn acquire() -> Option<ScreenOn> {
        let result = with_env("keeping the screen on", |env| {
            let power = system_service(env, "power")?;
            let tag = env.new_string("EarCam:camera")?;
            let lock = call(
                env,
                &power,
                jni_str!("newWakeLock"),
                jni_sig!("(ILjava/lang/String;)Landroid/os/PowerManager$WakeLock;"),
                &[
                    JValue::Int(SCREEN_BRIGHT_WAKE_LOCK | ON_AFTER_RELEASE),
                    JValue::Object(&tag),
                ],
            )?
            .l()?;
            call(
                env,
                &lock,
                jni_str!("setReferenceCounted"),
                jni_sig!("(Z)V"),
                &[JValue::Bool(false)],
            )?;
            call(env, &lock, jni_str!("acquire"), jni_sig!("()V"), &[])?;
            env.new_global_ref(lock)
        });
        result.map(ScreenOn).map_err(|e| log::warn!("{e}")).ok()
    }
}

impl Drop for ScreenOn {
    fn drop(&mut self) {
        let result = with_env("releasing the screen", |env| {
            call(
                env,
                self.0.as_obj(),
                jni_str!("release"),
                jni_sig!("()V"),
                &[],
            )
            .map(drop)
        });
        if let Err(e) = result {
            log::warn!("{e}");
        }
    }
}

/// Shows a short system message. Works from any thread: a Toast wants a Looper on the
/// calling thread, so one is prepared if there is none. Call it from a thread of its own,
/// not the event loop's.
pub fn toast(text: &str) {
    let result = with_env("toast", |env| {
        let looper = env.call_static_method(
            jni_str!("android/os/Looper"),
            jni_str!("myLooper"),
            jni_sig!("()Landroid/os/Looper;"),
            &[],
        )?;
        if looper.l()?.is_null() {
            env.call_static_method(
                jni_str!("android/os/Looper"),
                jni_str!("prepare"),
                jni_sig!("()V"),
                &[],
            )?;
        }
        let context = activity(env);
        let text = env.new_string(text)?;
        let toast = env
            .call_static_method(
                jni_str!("android/widget/Toast"),
                jni_str!("makeText"),
                jni_sig!(
                    "(Landroid/content/Context;Ljava/lang/CharSequence;I)Landroid/widget/Toast;"
                ),
                &[
                    JValue::Object(&context),
                    JValue::Object(&text),
                    JValue::Int(1),
                ],
            )?
            .l()?;
        call(env, &toast, jni_str!("show"), jni_sig!("()V"), &[]).map(drop)
    });
    if let Err(e) = result {
        log::warn!("{e}");
    }
}

/// Finishes the activity, as the back button should.
pub fn finish() {
    let result = with_env("finish", |env| {
        let activity = activity(env);
        call(env, &activity, jni_str!("finish"), jni_sig!("()V"), &[]).map(drop)
    });
    if let Err(e) = result {
        log::warn!("{e}");
    }
}

/// Runs `f` with this thread attached to the JVM, in a fresh local reference frame, and
/// turns any error into a message starting with `what`. A Java exception has already
/// been printed to logcat (see [`call`]) and is cleared by the time this returns.
pub(crate) fn with_env<T>(
    what: &str,
    f: impl FnOnce(&mut Env) -> JniResult<T>,
) -> Result<T, String> {
    let vm = unsafe { JavaVM::from_raw(ndk_context::android_context().vm().cast()) };
    vm.attach_current_thread(f)
        .map_err(|e| format!("{what}: {e}"))
}

/// The Activity, owned by android-activity for the life of the app.
pub(crate) fn activity<'local>(env: &Env<'local>) -> JObject<'local> {
    unsafe { JObject::from_raw(env, ndk_context::android_context().context().cast()) }
}

/// `Context.getSystemService(name)`.
pub(crate) fn system_service<'local>(
    env: &mut Env<'local>,
    name: &str,
) -> JniResult<JObject<'local>> {
    let name = env.new_string(name)?;
    let activity = activity(env);
    call(
        env,
        &activity,
        jni_str!("getSystemService"),
        jni_sig!("(Ljava/lang/String;)Ljava/lang/Object;"),
        &[JValue::Object(&name)],
    )?
    .l()
}

/// Calls a method; if it throws, the exception is printed to logcat so the error that
/// propagates has its Java cause next to it.
pub(crate) fn call<'local, 'sig, 'args>(
    env: &mut Env<'local>,
    obj: &JObject,
    name: &JNIStr,
    sig: impl AsRef<MethodSignature<'sig, 'args>>,
    args: &[JValue],
) -> JniResult<JValueOwned<'local>> {
    let result = env.call_method(obj, name, sig, args);
    if let Err(Error::JavaException) = result {
        env.exception_describe();
    }
    result
}
