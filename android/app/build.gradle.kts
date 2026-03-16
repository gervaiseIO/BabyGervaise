import java.io.File
import javax.inject.Inject
import org.gradle.api.DefaultTask
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.file.RegularFileProperty
import org.gradle.api.provider.Property
import org.gradle.api.provider.Provider
import org.gradle.api.tasks.CacheableTask
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.InputDirectory
import org.gradle.api.tasks.InputFile
import org.gradle.api.tasks.LocalState
import org.gradle.api.tasks.OutputDirectory
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction
import org.gradle.process.ExecOperations
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.kotlin.plugin.serialization")
}

val userHome = System.getProperty("user.home")
val minAndroidApiLevel = 29
val androidSdkRoot = System.getenv("ANDROID_SDK_ROOT")
    ?: System.getenv("ANDROID_HOME")
    ?: "$userHome/Library/Android/sdk"
val workspaceRoot = rootProject.projectDir.resolve("..").canonicalFile
val rustCoreDir = workspaceRoot.resolve("rust_core")
val scriptsDir = workspaceRoot.resolve("scripts")
val rustBuildScript = scriptsDir.resolve("build-android-rust.sh")
val debugRustJniLibs = layout.buildDirectory.dir("generated/rustJniLibs/debug")
val releaseRustJniLibs = layout.buildDirectory.dir("generated/rustJniLibs/release")
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

@CacheableTask
abstract class BuildRustAndroidTask : DefaultTask() {
    @get:Inject
    abstract val execOperations: ExecOperations

    @get:Input
    abstract val buildProfile: Property<String>

    @get:Input
    abstract val androidApiLevel: Property<Int>

    @get:Input
    abstract val androidAbi: Property<String>

    @get:Input
    abstract val rustTarget: Property<String>

    @get:Input
    abstract val workspaceRootPath: Property<String>

    @get:Input
    abstract val pathEnv: Property<String>

    @get:Input
    abstract val homeDirPath: Property<String>

    @get:Input
    abstract val cargoHomePath: Property<String>

    @get:Input
    abstract val rustupHomePath: Property<String>

    @get:Input
    abstract val androidSdkRootPath: Property<String>

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val buildScriptFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val cargoManifestFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val cargoLockFile: RegularFileProperty

    @get:InputDirectory
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val rustSourceDir: DirectoryProperty

    @get:OutputDirectory
    abstract val outputDir: DirectoryProperty

    @get:LocalState
    abstract val cargoTargetDir: DirectoryProperty

    @get:LocalState
    abstract val rustTempDir: DirectoryProperty

    init {
        group = "build"
    }

    @TaskAction
    fun buildRust() {
        val outputDirFile = outputDir.get().asFile
        outputDirFile.deleteRecursively()
        outputDirFile.mkdirs()
        cargoTargetDir.get().asFile.mkdirs()
        rustTempDir.get().asFile.mkdirs()

        execOperations.exec {
            workingDir = File(workspaceRootPath.get())
            commandLine(
                buildScriptFile.get().asFile.absolutePath,
                workspaceRootPath.get(),
                buildProfile.get(),
                outputDirFile.absolutePath,
                androidApiLevel.get().toString(),
                androidAbi.get(),
                rustTarget.get(),
            )
            environment("PATH", pathEnv.get())
            environment("HOME", homeDirPath.get())
            environment("CARGO_HOME", cargoHomePath.get())
            environment("RUSTUP_HOME", rustupHomePath.get())
            environment("ANDROID_SDK_ROOT", androidSdkRootPath.get())
            environment("ANDROID_HOME", androidSdkRootPath.get())
            environment("CARGO_TARGET_DIR", cargoTargetDir.get().asFile.absolutePath)
            environment("TMPDIR", rustTempDir.get().asFile.absolutePath)
            environment("TMP", rustTempDir.get().asFile.absolutePath)
            environment("TEMP", rustTempDir.get().asFile.absolutePath)
        }
    }
}

android {
    namespace = "io.gervaise.babygervaise"
    compileSdk = 36

    defaultConfig {
        applicationId = "io.gervaise.babygervaise"
        minSdk = minAndroidApiLevel
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

    packaging {
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
    }

}

fun registerRustTask(
    name: String,
    profile: String,
    outputDir: Provider<out org.gradle.api.file.Directory>,
) = tasks.register<BuildRustAndroidTask>(name) {
    description = "Builds rust_core for Android ($profile)"
    buildProfile.set(profile)
    androidApiLevel.set(minAndroidApiLevel)
    androidAbi.set("arm64-v8a")
    rustTarget.set("aarch64-linux-android")
    workspaceRootPath.set(workspaceRoot.absolutePath)
    pathEnv.set(gradleExecPath)
    homeDirPath.set(userHome)
    cargoHomePath.set("$userHome/.cargo")
    rustupHomePath.set("$userHome/.rustup")
    androidSdkRootPath.set(androidSdkRoot)
    buildScriptFile.set(rustBuildScript)
    cargoManifestFile.set(rustCoreDir.resolve("Cargo.toml"))
    cargoLockFile.set(rustCoreDir.resolve("Cargo.lock"))
    rustSourceDir.set(rustCoreDir.resolve("src"))
    this.outputDir.set(outputDir)
    cargoTargetDir.set(layout.buildDirectory.dir("intermediates/rustCargo/$profile"))
    rustTempDir.set(layout.buildDirectory.dir("tmp/rustCargo/$profile"))
}

val buildRustAndroidDebug = registerRustTask("buildRustAndroidDebug", "debug", debugRustJniLibs)
val buildRustAndroidRelease = registerRustTask("buildRustAndroidRelease", "release", releaseRustJniLibs)

androidComponents {
    onVariants(selector().withBuildType("debug")) { variant ->
        variant.sources.assets?.addStaticSourceDirectory("../../config")
        variant.sources.jniLibs?.addGeneratedSourceDirectory(
            buildRustAndroidDebug,
            BuildRustAndroidTask::outputDir,
        )
    }

    onVariants(selector().withBuildType("release")) { variant ->
        variant.sources.assets?.addStaticSourceDirectory("../../config")
        variant.sources.jniLibs?.addGeneratedSourceDirectory(
            buildRustAndroidRelease,
            BuildRustAndroidTask::outputDir,
        )
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}

dependencies {
    implementation(project(":bridge"))

    implementation(platform("androidx.compose:compose-bom:2024.06.00"))
    implementation("androidx.activity:activity-compose:1.9.0")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.documentfile:documentfile:1.0.1")
    implementation("com.google.android.material:material:1.12.0")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.2")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.2")
    implementation("androidx.lifecycle:lifecycle-viewmodel-ktx:2.8.2")
    implementation("com.google.mlkit:genai-prompt:1.0.0-beta1")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.8.1")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.6.3")

    debugImplementation("androidx.compose.ui:ui-tooling")
    debugImplementation("androidx.compose.ui:ui-test-manifest")

    testImplementation("junit:junit:4.13.2")
    testImplementation("org.jetbrains.kotlinx:kotlinx-coroutines-test:1.8.1")
    androidTestImplementation(platform("androidx.compose:compose-bom:2024.06.00"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.1")
    androidTestImplementation("androidx.test.ext:junit:1.1.5")
}
