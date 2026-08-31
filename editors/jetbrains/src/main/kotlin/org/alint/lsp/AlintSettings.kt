package org.alint.lsp

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage

/** Application-level settings: the user's explicit path to the `alint`
 * binary (the `alint.path` equivalent of the VS Code extension). */
@Service(Service.Level.APP)
@State(name = "AlintSettings", storages = [Storage("alint.xml")])
class AlintSettings : PersistentStateComponent<AlintSettings.State> {
    data class State(var alintPath: String = "")

    private var state = State()

    override fun getState(): State = state

    override fun loadState(loaded: State) {
        state = loaded
    }

    var alintPath: String
        get() = state.alintPath
        set(value) {
            state.alintPath = value
        }

    companion object {
        fun getInstance(): AlintSettings =
            ApplicationManager.getApplication().getService(AlintSettings::class.java)
    }
}
