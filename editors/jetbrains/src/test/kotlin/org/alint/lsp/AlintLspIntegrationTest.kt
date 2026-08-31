package org.alint.lsp

import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.testFramework.HeavyPlatformTestCase
import com.intellij.testFramework.PsiTestUtil
import com.intellij.util.ui.UIUtil
import com.redhat.devtools.lsp4ij.LSPIJUtils
import com.redhat.devtools.lsp4ij.LanguageServerItem
import com.redhat.devtools.lsp4ij.LanguageServiceAccessor
import org.eclipse.lsp4j.Diagnostic
import java.io.File
import java.net.URI

/**
 * Headless behavioral e2e for the JetBrains integration: opens a file in
 * a real on-disk project, lets LSP4IJ connect + spawn `alint lsp`, and
 * asserts an alint diagnostic arrives — the one thing buildPlugin
 * (headless load) and verifyPlugin (compat) don't cover.
 *
 * Heavy fixture (not light): `alint lsp` walks the real filesystem from
 * the LSP `rootUri` (= the project base path), so the `.alint.yml` +
 * fixture file must exist on disk. Requires the `alint` binary via
 * $ALINT_TEST_BINARY; skips when unset (e.g. a plain unit-test run).
 */
class AlintLspIntegrationTest : HeavyPlatformTestCase() {

    fun testAlintServerPublishesDiagnostics() {
        val alintBinary = System.getenv("ALINT_TEST_BINARY")
        if (alintBinary.isNullOrBlank()) {
            println("SKIP: ALINT_TEST_BINARY not set")
            return
        }

        val base = File(project.basePath!!)
        base.mkdirs()
        File(base, ".alint.yml").writeText(
            """
            version: 1
            rules:
              - id: no-todo
                kind: file_content_forbidden
                paths: "**/*.py"
                pattern: "TODO"
                level: error
            """.trimIndent() + "\n",
        )
        val badPy = File(base, "bad.py").apply { writeText("x = 1  # TODO\n") }
        LocalFileSystem.getInstance().refresh(false)
        val vFile = LocalFileSystem.getInstance().refreshAndFindFileByIoFile(badPy)
            ?: error("could not find bad.py in VFS")

        // Register the project dir as a content root so LSP4IJ sends it as
        // the LSP rootUri/workspaceFolder — alint resolves its workspace
        // (and discovers .alint.yml) from that, not the process CWD.
        val baseDir = LocalFileSystem.getInstance().refreshAndFindFileByIoFile(base)
            ?: error("could not find project base in VFS")
        PsiTestUtil.addContentRoot(module, baseDir)

        AlintSettings.getInstance().alintPath = alintBinary
        FileEditorManager.getInstance(project).openFile(vFile, true)

        val uri = LSPIJUtils.toUri(vFile)
        // Matching + connecting the file to the "alint" server (mapped to
        // all files) starts the process and sends didOpen. Hold the future
        // and poll it — blocking .get() on the test's EDT would deadlock
        // LSP4IJ's own EDT work.
        val serversFuture = LanguageServiceAccessor.getInstance(project)
            .getLanguageServers(vFile, { true }, { true })

        val ok = waitUntil(60_000) {
            val servers = serversFuture.getNow(null) ?: return@waitUntil false
            servers.any { diagnosticsFor(it, uri).isNotEmpty() }
        }

        val servers = serversFuture.getNow(null)
        val messages = servers.orEmpty().flatMap { diagnosticsFor(it, uri) }.map { it.message ?: "" }
        assertTrue(
            "no diagnostics from alint within timeout " +
                "(servers=${servers?.size}, statuses=${servers?.map { it.serverWrapper.serverStatus }})",
            ok,
        )
        assertTrue(
            "expected the no-todo diagnostic, got $messages",
            messages.any { it.contains("TODO") || it.contains("forbidden") },
        )
    }

    private fun diagnosticsFor(item: LanguageServerItem, uri: URI): List<Diagnostic> {
        val data = item.serverWrapper.getLSPVirtualFileData(uri) ?: return emptyList()
        return data.diagnosticsForServer.diagnostics.toList()
    }

    /** Poll `cond`, pumping the IDE event queue (LSP4IJ is async on the EDT). */
    private fun waitUntil(timeoutMs: Long, cond: () -> Boolean): Boolean {
        val deadline = System.currentTimeMillis() + timeoutMs
        while (System.currentTimeMillis() < deadline) {
            if (cond()) return true
            UIUtil.dispatchAllInvocationEvents()
            Thread.sleep(100)
        }
        return cond()
    }
}
