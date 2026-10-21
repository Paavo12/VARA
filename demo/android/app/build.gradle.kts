plugins {
    id("com.android.application")
}

val heraLanUrl = (findProperty("heraLanUrl") as String?) ?: ""

android {
    namespace = "app.hera.demo"
    compileSdk = 35

    defaultConfig {
        applicationId = "app.hera.demo"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "1.0"
        buildConfigField("String", "LAN_URL", "\"$heraLanUrl\"")
    }

    buildFeatures {
        buildConfig = true
    }

    // Bundle the web app itself, so the APK also works with no server running.
    sourceSets {
        getByName("main") {
            assets.srcDirs("../../public")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
