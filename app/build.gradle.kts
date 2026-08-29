plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "com.krisper.earcam"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.krisper.earcam"
        minSdk = 34
        targetSdk = 35
        versionCode = 1
        versionName = "0.1"
        // Single-screen app with no localisation beyond German: keep only what is used.
        resourceConfigurations += listOf("de", "en")

        // The app has no native code of its own; the only .so comes from compose-ui.
        // Every Android 14 device is arm64, so the other three ABIs are dead weight.
        ndk {
            abiFilters += "arm64-v8a"
        }
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

    buildFeatures {
        compose = true
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
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    // Deliberately minimal: Compose UI and foundation only. Material 3 and the icon
    // packs would dominate the APK for a screen that is one image and one toggle.
    val composeBom = platform("androidx.compose:compose-bom:2024.12.01")
    implementation(composeBom)
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.7")

    testImplementation("junit:junit:4.13.2")
}
