package io.gervaise.babygervaise.notes

import android.app.Application
import android.content.Intent
import android.net.Uri
import io.gervaise.babygervaise.BabyGervaiseRuntime
import io.gervaise.babygervaise.Screen
import io.gervaise.babygervaise.bridge.NoteActivityEvent
import java.time.Instant
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

class NotesController(
    private val application: Application,
    private val runtime: BabyGervaiseRuntime,
    private val scope: CoroutineScope,
    private val repository: NotesRepository = NotesRepository(application),
    private val clock: () -> Instant = { Instant.now() },
) {
    private val _uiState = MutableStateFlow(
        NotesUiState(
            activeVault = repository.loadActiveVault(),
        ),
    )
    private val _messages = MutableSharedFlow<String>(extraBufferCapacity = 8)
    private val saveMutex = Mutex()
    private var autosaveJob: Job? = null
    private var catalogCache: List<NoteListItem> = emptyList()

    val uiState: StateFlow<NotesUiState> = _uiState.asStateFlow()
    val messages = _messages.asSharedFlow()

    init {
        _uiState.value.activeVault?.let { vault ->
            scope.launch {
                refreshCatalog(vault = vault, allowBootstrapScan = true)
            }
        }
    }

    fun prepareEntry(): Screen.NotesRoute {
        val persistedVault = repository.loadActiveVault()
        _uiState.update { current ->
            current.copy(activeVault = persistedVault)
        }
        val vault = persistedVault ?: return Screen.NotesRoute.Onboarding
        val currentEditor = _uiState.value.editor
        if (_uiState.value.activeVault?.vaultId == vault.vaultId &&
            (currentEditor.isPersisted || currentEditor.body.isNotBlank())
        ) {
            return Screen.NotesRoute.Editor
        }
        openEditorEntry(vault)
        return Screen.NotesRoute.Editor
    }

    fun configureVault(uri: Uri) {
        scope.launch {
            runCatching {
                application.contentResolver.takePersistableUriPermission(
                    uri,
                    Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
                )
                val vault = repository.persistVaultSelection(uri)
                repository.ensureVaultArtifacts(vault)
                _uiState.update { current ->
                    current.copy(
                        activeVault = vault,
                        editor = NoteEditorState(),
                    )
                }
                openEditorEntry(vault)
            }.onFailure { error ->
                _messages.tryEmit(error.message ?: "Unable to use the selected folder.")
            }
        }
    }

    fun updateBody(value: String) {
        _uiState.update { current ->
            val currentEditor = current.editor
            val nextTitle = if (currentEditor.titleLifecycle == NoteTitleLifecycle.SYSTEM_PROVISIONAL) {
                deriveSystemProvisionalTitle(value)
            } else {
                currentEditor.title
            }
            current.copy(
                editor = currentEditor.copy(
                    body = value,
                    title = nextTitle,
                    isDirty = true,
                    saveError = null,
                ),
            )
        }
        scheduleAutosave()
    }

    fun updateSearchQuery(query: String) {
        _uiState.update { current ->
            current.copy(
                searchQuery = query,
                searchResults = lexicalSearchCatalog(catalogCache, query),
            )
        }
    }

    fun openSearchResult(item: NoteListItem) {
        scope.launch {
            flushPendingSaveInternal()
            val vault = _uiState.value.activeVault ?: return@launch
            val loaded = repository.loadNote(vault, item.noteRef) ?: return@launch
            val opened = repository.markNoteOpened(vault, loaded.editor)
            recordActivity(
                noteRef = item.noteRef,
                titleSnapshot = opened.title,
                eventType = "note_opened",
            )
            applyLoadedEditor(loaded.editor.copy(lastSavedAt = opened.lastSavedAt))
            refreshCatalog(vault, allowBootstrapScan = false)
        }
    }

    fun flushPendingSave() {
        autosaveJob?.cancel()
        scope.launch {
            flushPendingSaveInternal()
        }
    }

    private fun scheduleAutosave() {
        autosaveJob?.cancel()
        autosaveJob = scope.launch {
            delay(AUTOSAVE_DEBOUNCE_MS)
            flushPendingSaveInternal()
        }
    }

    private fun openEditorEntry(vault: NotesVaultSummary) {
        _uiState.update { current ->
            current.copy(
                activeVault = vault,
                isOpeningEditor = true,
            )
        }
        scope.launch {
            scope.launch {
                refreshCatalog(vault = vault, allowBootstrapScan = true)
            }
            val lastActive = repository.loadLastActiveNote()
            val shouldResume = lastActive != null &&
                shouldResumeLastActiveNote(lastActive.openedAt, clock()) &&
                repository.noteExists(vault, lastActive.noteRef)
            if (shouldResume) {
                recordActivity(
                    noteRef = lastActive.noteRef,
                    titleSnapshot = lastActive.titleSnapshot,
                    eventType = "note_resume_candidate",
                )
                val loaded = repository.loadNote(vault, lastActive.noteRef)
                if (loaded != null) {
                    val opened = repository.markNoteOpened(vault, loaded.editor)
                    recordActivity(
                        noteRef = loaded.editor.noteRef ?: lastActive.noteRef,
                        titleSnapshot = opened.title,
                        eventType = "note_opened",
                    )
                    applyLoadedEditor(
                        loaded.editor.copy(
                            lastSavedAt = opened.lastSavedAt,
                        ),
                    )
                    refreshCatalog(vault, allowBootstrapScan = false)
                    return@launch
                }
            }
            _uiState.update { current ->
                current.copy(
                    editor = NoteEditorState(),
                    isOpeningEditor = false,
                )
            }
        }
    }

    private suspend fun applyLoadedEditor(editor: NoteEditorState) {
        _uiState.update { current ->
            current.copy(
                editor = editor.copy(
                    isDirty = false,
                    isSaving = false,
                    isLoading = false,
                ),
                isOpeningEditor = false,
            )
        }
    }

    private suspend fun flushPendingSaveInternal() {
        saveMutex.withLock {
            val vault = _uiState.value.activeVault ?: return
            val editor = _uiState.value.editor
            if (!editor.isDirty) {
                return
            }
            if (!editor.isPersisted && !editor.hasMeaningfulContent) {
                _uiState.update { current ->
                    current.copy(
                        editor = current.editor.copy(
                            isDirty = false,
                            saveError = null,
                        ),
                    )
                }
                return
            }
            _uiState.update { current ->
                current.copy(
                    editor = current.editor.copy(
                        isSaving = true,
                        saveError = null,
                    ),
                )
            }
            runCatching {
                repository.saveNote(vault, editor)
            }.onSuccess { result ->
                _uiState.update { current ->
                    current.copy(
                        editor = result.editor,
                    )
                }
                updateCatalogCache(result.listItem)
                recordActivity(
                    noteRef = result.listItem.noteRef,
                    titleSnapshot = result.listItem.title,
                    eventType = result.eventType,
                )
            }.onFailure { error ->
                _uiState.update { current ->
                    current.copy(
                        editor = current.editor.copy(
                            isSaving = false,
                            saveError = error.message ?: "Unable to save note.",
                        ),
                    )
                }
                _messages.tryEmit(error.message ?: "Unable to save note.")
            }
        }
    }

    private suspend fun refreshCatalog(
        vault: NotesVaultSummary,
        allowBootstrapScan: Boolean,
    ) {
        val existing = repository.loadCatalogItems(vault)
        catalogCache = existing
        _uiState.update { current ->
            current.copy(
                recentNotes = lexicalSearchCatalog(existing, ""),
                searchResults = lexicalSearchCatalog(existing, current.searchQuery),
            )
        }
        if (!allowBootstrapScan || existing.isNotEmpty()) {
            return
        }
        _uiState.update { current -> current.copy(isCatalogScanning = true) }
        runCatching {
            repository.scanVaultIntoCatalog(vault)
        }.onSuccess { scanned ->
            catalogCache = scanned
            _uiState.update { current ->
                current.copy(
                    recentNotes = lexicalSearchCatalog(scanned, ""),
                    searchResults = lexicalSearchCatalog(scanned, current.searchQuery),
                    isCatalogScanning = false,
                )
            }
        }.onFailure {
            _uiState.update { current -> current.copy(isCatalogScanning = false) }
        }
    }

    private fun updateCatalogCache(item: NoteListItem) {
        catalogCache = catalogCache
            .filterNot { candidate -> candidate.noteRef.noteKey == item.noteRef.noteKey }
            .plus(item)
            .sortedByDescending { candidate -> candidate.rankTimestamp.orEmpty() }
        _uiState.update { current ->
            current.copy(
                recentNotes = lexicalSearchCatalog(catalogCache, ""),
                searchResults = lexicalSearchCatalog(catalogCache, current.searchQuery),
            )
        }
    }

    private fun recordActivity(
        noteRef: NoteRef,
        titleSnapshot: String,
        eventType: String,
    ) {
        runtime.recordNoteActivity(
            NoteActivityEvent(
                noteKey = noteRef.noteKey,
                relativePath = noteRef.relativePath,
                titleSnapshot = titleSnapshot,
                eventType = eventType,
                occurredAt = clock().toString(),
            ),
        )
    }

    private companion object {
        const val AUTOSAVE_DEBOUNCE_MS = 900L
    }
}
