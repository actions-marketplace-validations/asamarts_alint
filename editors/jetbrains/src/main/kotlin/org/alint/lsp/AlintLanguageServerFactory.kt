package org.alint.lsp

import com.intellij.notification.NotificationAction
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.thisLogger
import com.intellij.openapi.extensions.PluginId
import com.intellij.ide.plugins.PluginManagerCore
import com.intellij.openapi.project.Project
import com.redhat.devtools.lsp4ij.LanguageServerFactory
import com.redhat.devtools.lsp4ij.server.ProcessStreamConnectionProvider
import com.redhat.devtools.lsp4ij.server.StreamConnectionProvider

/**
 * Registers `alint lsp` with LSP4IJ. Wired in `plugin.xml` under the
 * `com.redhat.devtools.lsp4ij` server extension point.
 *
 * NOTE (verify against the pinned LSP4IJ version): the
 * `ProcessStreamConnectionProvider` setters (`setCommands` /
 * `setWorkingDirectory`) and the `LanguageServerFactory` interface are
 * LSP4IJ's documented API; if a coordinate drifts, adjust here at
 * `gradle buildPlugin` time.
 */
class AlintLanguageServerFactory : LanguageServerFactory {
    override fun createConnectionProvider(project: Project): StreamConnectionProvider =
        AlintConnectionProvider(project)
}

private class AlintConnectionProvider(project: Project) : ProcessStreamConnectionProvider() {
    init {
        val binary = AlintBinary.resolve()
        if (binary != null) {
            super.setCommands(listOf(binary, "lsp"))
            project.basePath?.let { super.setWorkingDirectory(it) }
        } else {
            thisLogger().warn("alint binary not found on PATH or in settings; server not started")
            AlintNotifier.binaryMissing(project)
        }
    }
}

/** Surfaces the "alint not found" prompt with an opt-in download action. */
object AlintNotifier {
    fun binaryMissing(project: Project) {
        val group = NotificationGroupManager.getInstance().getNotificationGroup("alint")
        group
            .createNotification(
                "alint binary not found",
                "Set its path in Settings, install it (brew / cargo / npm), or download the matching release.",
                NotificationType.WARNING,
            )
            .addAction(object : NotificationAction("Download latest") {
                override fun actionPerformed(
                    e: com.intellij.openapi.actionSystem.AnActionEvent,
                    notification: com.intellij.notification.Notification,
                ) {
                    notification.expire()
                    downloadInBackground(project)
                }
            })
            .notify(project)
    }

    private fun downloadInBackground(project: Project) {
        ApplicationManager.getApplication().executeOnPooledThread {
            val group = NotificationGroupManager.getInstance().getNotificationGroup("alint")
            try {
                // Download the alint release matching this plugin's
                // (release-stamped) version — same strategy as the VS
                // Code extension. (The Zed extension uses latest, since
                // its wasm can't read its own version.)
                val version = pluginVersion()
                val path = AlintBinary.download(version) { thisLogger().info("alint: $it") }
                AlintSettings.getInstance().alintPath = path
                group
                    .createNotification(
                        "alint downloaded",
                        "Installed to $path. Restart the alint language server (or the IDE) to use it.",
                        NotificationType.INFORMATION,
                    )
                    .notify(project)
            } catch (ex: Exception) {
                group
                    .createNotification(
                        "alint download failed",
                        ex.message ?: ex.toString(),
                        NotificationType.ERROR,
                    )
                    .notify(project)
            }
        }
    }

    private fun pluginVersion(): String =
        PluginManagerCore.getPlugin(PluginId.getId("org.alint.lsp"))?.version
            ?: error("could not determine plugin version")
}
