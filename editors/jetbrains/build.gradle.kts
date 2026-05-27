// alint JetBrains plugin — Gradle build (Kotlin DSL).
//
// Built with the IntelliJ Platform Gradle Plugin 2.x and depends on
// Red Hat's LSP4IJ to host the `alint lsp` language server. One plugin
// covers the whole JetBrains suite (IDEA, PyCharm, GoLand, WebStorm,
// RustRover, CLion, Rider, Android Studio). The version coordinates
// below are validated by the editors CI job (./gradlew buildPlugin
// verifyPlugin).

plugins {
    kotlin("jvm") version "2.0.21"
    id("org.jetbrains.intellij.platform") version "2.1.0"
}

group = "org.alint"
// CI stamps the version from the release tag via `-PpluginVersion=…`
// (see release.yml); the committed default can lag like the npm / VS
// Code packages do.
version = (findProperty("pluginVersion") as String?) ?: "0.10.2"

repositories {
    mavenCentral()
    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    intellijPlatform {
        // Base IDE to compile/run against. Community covers the whole
        // platform; the plugin has no IDE-specific code.
        intellijIdeaCommunity("2024.2")
        // LSP4IJ (Red Hat) — the LSP client this plugin registers with.
        // Marketplace id com.redhat.devtools.lsp4ij; verify the version.
        plugin("com.redhat.devtools.lsp4ij", "0.7.0")
        pluginVerifier()
        zipSigner()
        // Platform test fixtures for the headless LSP integration test.
        testFramework(org.jetbrains.intellij.platform.gradle.TestFrameworkType.Platform)
    }
    // Used by the managed-download path to extract the release .tar.gz.
    implementation("org.apache.commons:commons-compress:1.27.1")
    testImplementation("junit:junit:4.13.2")
    // The platform test fixtures (UsefulTestCase) reference opentest4j,
    // which isn't pulled onto the gradle test classpath transitively.
    testImplementation("org.opentest4j:opentest4j:1.3.0")
}

intellijPlatform {
    // No Java sources or GUI (.form) files — this is Kotlin LSP glue —
    // so skip bytecode instrumentation (which would otherwise pull the
    // IntelliJ Java-compiler/ant-tasks dependency).
    instrumentCode = false

    pluginConfiguration {
        id = "org.alint.lsp"
        name = "alint"
        version = project.version.toString()
        ideaVersion {
            sinceBuild = "242"
            // No upper bound — the plugin is LSP glue and shouldn't pin
            // an untilBuild that locks out future IDE releases.
            untilBuild = provider { null }
        }
        vendor {
            name = "alint contributors"
            url = "https://alint.org"
        }
    }
    pluginVerification {
        // Verify against explicit released IDEs (the sinceBuild 242 floor
        // + one recent), NOT recommended(): recommended() reaches for the
        // newest build — including unreleased EAPs (e.g. 2025.3) that
        // aren't downloadable and fail dependency resolution. Runs in the
        // editors CI job so Marketplace-compat regressions surface
        // pre-tag.
        ides {
            ide(org.jetbrains.intellij.platform.gradle.IntelliJPlatformType.IntellijIdeaCommunity, "2024.2")
            ide(org.jetbrains.intellij.platform.gradle.IntelliJPlatformType.IntellijIdeaCommunity, "2024.3")
        }
    }
    publishing {
        // Set JETBRAINS_MARKETPLACE_TOKEN in CI (release.yml) / locally.
        token = providers.environmentVariable("JETBRAINS_MARKETPLACE_TOKEN")
    }
    signing {
        certificateChain = providers.environmentVariable("JETBRAINS_CERTIFICATE_CHAIN")
        privateKey = providers.environmentVariable("JETBRAINS_PRIVATE_KEY")
        password = providers.environmentVariable("JETBRAINS_PRIVATE_KEY_PASSWORD")
    }
}

// Target Java 17 (the IntelliJ 2024.2+ baseline) using the JDK that
// runs Gradle. Deliberately NOT `jvmToolchain(17)`, which makes Gradle
// try to *provision* a toolchain and fails on boxes without toolchain
// auto-detection or a download repo configured — build with a JDK 17+.
kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

java {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
}
