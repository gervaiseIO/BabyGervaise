package io.gervaise.babygervaise

import android.app.Application
import android.net.Uri
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import io.gervaise.babygervaise.bridge.ContextLevel
import io.gervaise.babygervaise.notes.NoteListItem
import io.gervaise.babygervaise.notes.NotesController
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

class BabyGervaiseViewModel(
    application: Application,
) : AndroidViewModel(application) {
    private val runtime = (application as BabyGervaiseApplication).runtime
    private val interactionViewModel = InteractionViewModel(
        bridge = RuntimeHgieBridge(runtime = runtime, scope = viewModelScope),
        scope = viewModelScope,
    )
    private val notesController = NotesController(
        application = application,
        runtime = runtime,
        scope = viewModelScope,
    )
    private val _uiState = MutableStateFlow(BabyGervaiseUiState())
    val uiState: StateFlow<BabyGervaiseUiState> = _uiState.asStateFlow()

    init {
        observeRuntimeState()
        observeInteractionState()
        observeNotesState()
        observeRuntimeMessages()
        observeNotesMessages()
    }

    fun updateDraft(value: String) {
        interactionViewModel.updateDraft(value)
    }

    fun openOverview() {
        if (_uiState.value.screen is Screen.Notes) {
            notesController.flushPendingSave()
        }
        _uiState.update { current ->
            current.copy(screen = Screen.Overview)
        }
        runtime.refreshOverviewSnapshot(
            force = true,
            allowAmbientTriggers = false,
        )
    }

    fun openChat() {
        if (_uiState.value.screen is Screen.Notes) {
            notesController.flushPendingSave()
        }
        _uiState.update { current ->
            current.copy(screen = Screen.Chat)
        }
    }

    fun openNotes() {
        val route = notesController.prepareEntry()
        _uiState.update { current ->
            current.copy(screen = Screen.Notes(route))
        }
    }

    fun openNotesSearch() {
        notesController.flushPendingSave()
        _uiState.update { current ->
            current.copy(screen = Screen.Notes(Screen.NotesRoute.Search))
        }
    }

    fun closeNotesSearch() {
        _uiState.update { current ->
            current.copy(screen = Screen.Notes(Screen.NotesRoute.Editor))
        }
    }

    fun configureNotesVault(uri: Uri) {
        notesController.configureVault(uri)
        _uiState.update { current ->
            current.copy(screen = Screen.Notes(Screen.NotesRoute.Editor))
        }
    }

    fun updateNoteBody(value: String) {
        notesController.updateBody(value)
    }

    fun updateNoteSearchQuery(query: String) {
        notesController.updateSearchQuery(query)
    }

    fun openSearchedNote(item: NoteListItem) {
        notesController.openSearchResult(item)
        _uiState.update { current ->
            current.copy(screen = Screen.Notes(Screen.NotesRoute.Editor))
        }
    }

    fun submitDraft() {
        interactionViewModel.submitDraft()
    }

    fun muteCloudWorking() {
        interactionViewModel.muteCloudWorking()
    }

    fun submitSuggestion(
        cardId: String,
        optionId: String,
    ) {
        interactionViewModel.submitSuggestion(
            cardId = cardId,
            optionId = optionId,
        )
    }

    fun updatePreviousContext(level: ContextLevel) {
        runtime.setPreviousContext(level)
    }

    fun beginToolAuth(tool: String) {
        runtime.beginToolAuth(tool)
    }

    fun disconnectTool(tool: String) {
        runtime.disconnectTool(tool)
    }

    fun refreshToolState(tool: String) {
        runtime.refreshToolState(tool)
    }

    fun updateCloudProfile(profileId: String) {
        runtime.setCloudProfile(profileId)
    }

    fun onAppResumed() {
        runtime.onAppResumed()
    }

    fun onAppBackgrounded() {
        notesController.flushPendingSave()
    }

    fun consumeSnackbar() {
        _uiState.update { current ->
            current.copy(snackbarMessage = null)
        }
    }

    fun handleSpotifyAuthRedirect(callbackUrl: String) {
        runtime.handleToolAuthRedirect("spotify", callbackUrl)
    }

    private fun observeRuntimeState() {
        viewModelScope.launch {
            runtime.state.collectLatest { coreState ->
                _uiState.update { current ->
                    current.copy(
                        bootstrapState = coreState.bootstrapState,
                        overviewSnapshot = coreState.overviewSnapshot,
                        pendingTurnId = coreState.pendingTurnId,
                        activeToolStatus = coreState.activeToolStatus,
                        toolStatus = coreState.toolStatus,
                        isInitializing = coreState.isInitializing,
                        initializationError = coreState.initializationError,
                        isCoreReady = coreState.isCoreReady,
                        canSubmitTurns = coreState.canSubmitTurns,
                    )
                }
                interactionViewModel.updateComposerState(
                    canSubmit = coreState.canSubmitTurns,
                    isSending = coreState.pendingTurnId != null,
                )
                interactionViewModel.updateDeliveryState(coreState.deliveryState)
            }
        }
    }

    private fun observeInteractionState() {
        viewModelScope.launch {
            interactionViewModel.uiState.collectLatest { interactionState ->
                _uiState.update { current ->
                    current.copy(
                        draft = interactionState.draft,
                        interactionState = interactionState,
                    )
                }
            }
        }
    }

    private fun observeNotesState() {
        viewModelScope.launch {
            notesController.uiState.collectLatest { notesState ->
                _uiState.update { current ->
                    current.copy(notesState = notesState)
                }
            }
        }
    }

    private fun observeRuntimeMessages() {
        viewModelScope.launch {
            runtime.messages.collectLatest(::showSnackbar)
        }
    }

    private fun observeNotesMessages() {
        viewModelScope.launch {
            notesController.messages.collectLatest(::showSnackbar)
        }
    }

    private fun showSnackbar(message: String) {
        _uiState.update { current ->
            current.copy(snackbarMessage = message)
        }
    }
}
