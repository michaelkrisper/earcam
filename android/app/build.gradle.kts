plugins {
    id("com.android.application")
}

// The app is the Rust library in the repository root (libearcam.so), started by the
// platform's NativeActivity: this module only packages it with a manifest and an icon.
val repoRoot: Directory = rootProject.layout.projectDirectory.dir("..")
val rustJniLibs: Provider<Directory> = layout.buildDirectory.dir("rustJniLibs")

// The version is the nearest git tag (vX.Y.Z, set by CI on every merge to main), as in
// build.rs; outside a git checkout it falls back to the version in Cargo.toml.
val cargoVersion: Provider<String> = providers.fileContents(repoRoot.file("Cargo.toml")).asText.map {
    Regex("""(?m)^version\s*=\s*"([^"]+)"""").find(it)?.groupValues?.get(1)
        ?: error("no version in Cargo.toml")
}
val gitVersion: Provider<String> = providers.exec {
    commandLine("git", "describe", "--tags", "--match", "v[0-9]*.[0-9]*.[0-9]*")
    workingDir(repoRoot.asFile)
    isIgnoreExitValue = true
}.standardOutput.asText.map { it.trim().removePrefix("v") }
val appVersion: Provider<String> = gitVersion.filter { it.isNotEmpty() }.orElse(cargoVersion)

android {
    namespace = "com.krisper.earcam"
    compileSdk = 37
    ndkVersion = "30.0.16248370"

    defaultConfig {
        applicationId = "com.krisper.earcam"
        minSdk = 34
        // Below 37 until Android 17's local-network protection has been checked against
        // the UDP traffic to 192.168.1.1.
        targetSdk = 36
        val (major, minor, patch) = appVersion.get().split('.').map { it.takeWhile(Char::isDigit).toInt() }
        versionCode = major * 10_000 + minor * 100 + patch
        versionName = appVersion.get()
        ndk { abiFilters += "arm64-v8a" }
    }

    sourceSets {
        getByName("main") {
            jniLibs.directories.add(rustJniLibs.get().asFile.path)
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            // Signed with the debug key so CI produces a directly installable APK.
            signingConfig = signingConfigs.getByName("debug")
        }
    }
}

val cargoNdk by tasks.registering(Exec::class) {
    description = "Builds libearcam.so for arm64-v8a with cargo-ndk (always optimized)."
    workingDir = repoRoot.asFile
    inputs.dir(repoRoot.dir("src"))
    inputs.files(repoRoot.file("Cargo.toml"), repoRoot.file("Cargo.lock"), repoRoot.file("build.rs"))
    outputs.dir(rustJniLibs)
    val out = rustJniLibs.get().asFile.path
    // Locals only: the doFirst action must not capture the build script.
    val ndkDirectory = androidComponents.sdkComponents.ndkDirectory
    commandLine(
        "cargo", "ndk", "-t", "arm64-v8a", "--platform", "34", "-o", out,
        "rustc", "--lib", "--release", "--locked", "--crate-type", "cdylib",
    )
    doFirst { (this as Exec).environment("ANDROID_NDK_HOME", ndkDirectory.get().asFile.path) }
}

tasks.named("preBuild") { dependsOn(cargoNdk) }
