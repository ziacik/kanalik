import org.gradle.api.tasks.compile.JavaCompile

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.compose.compiler)
}

val releaseKeystoreFile = System.getenv("KANALIK_KEYSTORE_FILE")
val releaseKeystorePassword = System.getenv("KANALIK_KEYSTORE_PASSWORD")
val releaseKeyAlias = System.getenv("KANALIK_KEY_ALIAS")
val releaseKeyPassword = System.getenv("KANALIK_KEY_PASSWORD")

android {
    namespace = "sk.ziacik.androidtvplayer"
    compileSdk = 37

    defaultConfig {
        applicationId = "sk.ziacik.androidtvplayer"
        minSdk = 26
        targetSdk = 37
        // Increment both values together for a public release; changing this file publishes it. Retry after updater lint fix.
        versionCode = 5
        versionName = "0.1.4"
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    dependenciesInfo {
        includeInApk = false
        includeInBundle = false
    }

    if (releaseKeystoreFile != null) {
        signingConfigs {
            create("release") {
                storeFile = file(releaseKeystoreFile)
                storePassword = requireNotNull(releaseKeystorePassword) {
                    "KANALIK_KEYSTORE_PASSWORD is required when KANALIK_KEYSTORE_FILE is set"
                }
                keyAlias = requireNotNull(releaseKeyAlias) {
                    "KANALIK_KEY_ALIAS is required when KANALIK_KEYSTORE_FILE is set"
                }
                keyPassword = requireNotNull(releaseKeyPassword) {
                    "KANALIK_KEY_PASSWORD is required when KANALIK_KEYSTORE_FILE is set"
                }
            }
        }

        buildTypes.getByName("release") {
            signingConfig = signingConfigs.getByName("release")
        }
    }

    buildFeatures {
        compose = true
    }

    lint {
        warningsAsErrors = true
        abortOnError = true
        disable += setOf("AndroidGradlePluginVersion", "NewerVersionAvailable")
    }

    sourceSets.named("debug") {
        assets.directories.add(layout.buildDirectory.dir("generated/assets/channels").get().asFile.absolutePath)
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}


kotlin {
    compilerOptions {
        allWarningsAsErrors.set(true)
    }
}

tasks.withType<JavaCompile>().configureEach {
    options.compilerArgs.addAll(listOf("-Xlint:all", "-Werror"))
}

val copyChannelCatalog by tasks.registering(Copy::class) {
    from(rootProject.layout.projectDirectory.file("channels.json"))
    into(layout.buildDirectory.dir("generated/assets/channels"))
}

tasks.named("preBuild") {
    dependsOn(copyChannelCatalog)
}

dependencies {
    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.compose.foundation)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.media3.exoplayer)
    implementation(libs.androidx.media3.exoplayer.dash)
    implementation(libs.androidx.media3.exoplayer.hls)
    implementation(libs.androidx.media3.ui)
    implementation(libs.okhttp)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.androidx.work.runtime.ktx)

    testImplementation(libs.junit)
    testImplementation(libs.json)
    testImplementation(libs.kotlinx.coroutines.test)

    androidTestImplementation(platform(libs.androidx.compose.bom))
    androidTestImplementation(libs.androidx.compose.ui.test.junit4)
    androidTestImplementation(libs.androidx.test.runner)

    debugImplementation(libs.androidx.compose.ui.tooling)
    debugImplementation(libs.androidx.compose.ui.test.manifest)
}
