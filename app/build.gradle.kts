plugins {
    id("com.android.application")
}

android {
    namespace = "com.krisper.earcam"
    compileSdk = 37

    defaultConfig {
        applicationId = "com.krisper.earcam"
        minSdk = 34
        targetSdk = 36
        versionCode = 1
        versionName = "0.1"
    }

    packaging {
        resources {
            excludes += setOf(
                "DebugProbesKt.bin",
                "kotlin-tooling-metadata.json",
                "META-INF/*.version",
                "META-INF/**.kotlin_module",
            )
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            // Signed with the debug key so CI can produce a directly installable APK.
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_21
        targetCompatibility = JavaVersion.VERSION_21
    }
}

dependencies {
    // None at runtime: the platform's GLSurfaceView, BitmapFactory and sockets are all
    // this app needs, and every library would only add startup time and APK weight.
    testImplementation("junit:junit:4.13.2")
}
