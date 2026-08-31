package org.alint.lsp

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Test

/**
 * Locks the build-time version-resource wiring. `AlintNotifier.pluginVersion()`
 * reads `alint-lsp/version.txt` from the classpath instead of calling any
 * platform plugin-lookup API (both `PluginManagerCore.getPlugin` and
 * `PluginManager.getPluginByClass` were rejected by JetBrains Marketplace
 * validation as internal-API usage during the v0.11.0 cut). If the
 * `generateVersionResource` gradle task or its `sourceSets["main"].resources`
 * wiring breaks, this test surfaces it before a release.
 */
class PluginVersionResourceTest {
    @Test
    fun version_resource_is_on_the_classpath_and_matches_gradle_version() {
        val stream = PluginVersionResourceTest::class.java.classLoader
            .getResourceAsStream("alint-lsp/version.txt")
        assertNotNull("alint-lsp/version.txt missing from classpath; generateVersionResource task wiring broken", stream)
        val content = stream!!.use { it.readBytes().toString(Charsets.UTF_8).trim() }

        // The gradle build passes its `project.version` to test runs via this
        // system property (see build.gradle.kts -> tasks.test). If the file
        // content drifts from the build's idea of the version, fail.
        val gradleVersion = System.getProperty("alint.test.plugin.version")
        assertNotNull(
            "alint.test.plugin.version system property not set; build.gradle.kts -> tasks.test wiring broken",
            gradleVersion,
        )
        assertEquals(gradleVersion, content)
    }
}
