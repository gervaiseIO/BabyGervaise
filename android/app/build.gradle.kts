import java.io.File

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val userHome = System.getProperty("user.home")
val androidApiLevel = 29
val androidSdkRoot = System.getenv("ANDROID_SDK_ROOT")
    ?: System.getenv("ANDROID_HOME")
    ?: "$userHome/Library/Android/sdk"
val workspaceRoot = rootProject.projectDir.resolve("..").canonicalFile
val rustCoreDir = workspaceRoot.resolve("rust_core")
val scriptsDir = workspaceRoot.resolve("scripts")
val generatedRustJniLibs = layout.buildDirectory.dir("generated/rustJniLibs")
val rustBuildScript = scriptsDir.resolve("build-android-rust.sh")
val rustupPathPrefix = listOf(
    "/opt/homebrew/opt/rustup/bin",
    "/usr/local/opt/rustup/bin",
    "${System.getProperty("user.home")}/.cargo/bin",
).filter { candidate -> File(candidate).exists() }
    .joinToString(separator = ":")
val gradleExecPath = listOfNotNull(
    rustupPathPrefix.takeIf { it.isNotBlank() },
    System.getenv("PATH"),
).joinToString(separator = ":")

android {
    namespace = "io.gervaise.babygervaise"
    compileSdk = 36

    defaultConfig {
        applicationId = "io.gervaise.babygervaise"
        minSdk = androidApiLevel
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"

        ndk {
            abiFilters += "arm64-v8a"
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
        }
    }

    buildFeatures {
        compose = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    composeOptions {
        kotlinCompilerExtensionVersion = "1.5.14"
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    packaging {
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
    }

    sourceSets["main"].assets.srcDirs("src/main/assets", "../../config")
    sourceSets["main"].jniLibs.srcDir(generatedRustJniLibs)
}

fun registerRustTask(name: String, profile: String) = tasks.register(name, Exec::class.java) {
    workingDir = workspaceRoot
    commandLine(
        rustBuildScript.absolutePath,
        workspaceRoot.absolutePath,
        profile,
        generatedRustJniLibs.get().asFile.absolutePath,
        androidApiLevel.toString(),
        "arm64-v8a",
        "aarch64-linux-android",
    )
    inputs.files(
        rustCoreDir.resolve("Cargo.toml"),
        rustCoreDir.resolve("Cargo.lock"),
    )
    inputs.dir(rustCoreDir.resolve("src"))
    outputs.dir(generatedRustJniLibs)
    environment("PATH", gradleExecPath)
    environment("HOME", userHome)
    environment("CARGO_HOME", "$userHome/.cargo")
    environment("RUSTUP_HOME", "$userHome/.rustup")
    environment("ANDROID_SDK_ROOT", androidSdkRoot)
    environment("ANDROID_HOME", androidSdkRoot)
}

val buildRustAndroidDebug by registerRustTask("buildRustAndroidDebug", "debug")
val buildRustAndroidRelease by registerRustTask("buildRustAndroidRelease", "release")

tasks.matching { it.name == "preDebugBuild" }.configureEach {
    dependsOn(buildRustAndroidDebug)
}

tasks.matching { it.name == "preReleaseBuild" }.configureEach {
    dependsOn(buildRustAndroidRelease)
}

dependencies {
    implementation(project(":bridge"))

    implementation(platform("androidx.compose:compose-bom:2024.06.00"))
    implementation("androidx.activity:activity-compose:1.9.0")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.2")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.2")
    implementation("androidx.lifecycle:lifecycle-viewmodel-ktx:2.8.2")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.8.1")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.6.3")

    debugImplementation("androidx.compose.ui:ui-tooling")
    debugImplementation("androidx.compose.ui:ui-test-manifest")

    testImplementation("junit:junit:4.13.2")
    androidTestImplementation(platform("androidx.compose:compose-bom:2024.06.00"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.1")
    androidTestImplementation("androidx.test.ext:junit:1.1.5")
}
