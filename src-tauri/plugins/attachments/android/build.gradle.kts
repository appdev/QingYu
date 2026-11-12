plugins { id("com.android.library"); id("org.jetbrains.kotlin.android") }
android {
    namespace = "app.miaoyan.attachments"
    compileSdk = 37
    defaultConfig { minSdk = 24 }
    compileOptions { sourceCompatibility = JavaVersion.VERSION_1_8; targetCompatibility = JavaVersion.VERSION_1_8 }
}
kotlin { compilerOptions { jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_1_8 } }
dependencies { implementation(project(":tauri-android")); implementation("androidx.core:core-ktx:1.17.0") }
