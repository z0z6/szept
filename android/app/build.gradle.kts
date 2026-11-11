plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

android {
    namespace = "org.komunikator.app"
    compileSdk = 35

    defaultConfig {
        applicationId = "org.komunikator.app"
        minSdk = 26
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
        // Must match the targets built by scripts/build-android.sh.
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    buildFeatures {
        compose = true
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
    implementation(platform("androidx.compose:compose-bom:2024.12.01"))
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    implementation("androidx.core:core-ktx:1.13.1")
    // The system emoji set, with skin tones and recently used emoji.
    implementation("androidx.emoji2:emoji2-emojipicker:1.4.0")
    // QR scanner on top of the camera, Apache 2.0, no Google services.
    implementation("com.journeyapps:zxing-android-embedded:4.3.0")
    // Runtime of the generated Rust bindings.
    implementation("net.java.dev.jna:jna:5.15.0@aar")
}
