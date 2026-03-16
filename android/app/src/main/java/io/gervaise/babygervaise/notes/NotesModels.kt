package io.gervaise.babygervaise.notes

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

@Serializable
enum class NoteTitleLifecycle {
    @SerialName("system_provisional")
    SYSTEM_PROVISIONAL,

    @SerialName("system_stable")
    SYSTEM_STABLE,

    @SerialName("user_or_imported")
    USER_OR_IMPORTED,
    ;

    val wireName: String
        get() = when (this) {
            SYSTEM_PROVISIONAL -> "system_provisional"
            SYSTEM_STABLE -> "system_stable"
            USER_OR_IMPORTED -> "user_or_imported"
        }
}

data class NotesVaultSummary(
    val treeUri: String,
    val displayName: String,
    val vaultId: String,
)

data class NoteRef(
    val noteKey: String,
    val relativePath: String,
)

data class LastActiveNote(
    val noteRef: NoteRef,
    val titleSnapshot: String,
    val openedAt: String,
)

data class NoteListItem(
    val noteRef: NoteRef,
    val title: String,
    val excerpt: String,
    val titleLifecycle: NoteTitleLifecycle,
    val lastOpenedAt: String? = null,
    val lastSavedAt: String? = null,
) {
    val rankTimestamp: String?
        get() = listOf(lastOpenedAt, lastSavedAt)
            .filterNotNull()
            .maxOrNull()
}

data class NoteEditorState(
    val noteRef: NoteRef? = null,
    val title: String = UNTITLED_NOTE_TITLE,
    val titleLifecycle: NoteTitleLifecycle = NoteTitleLifecycle.SYSTEM_PROVISIONAL,
    val body: String = "",
    val mirrorsTitleToH1: Boolean = true,
    val isPersisted: Boolean = false,
    val isDirty: Boolean = false,
    val isSaving: Boolean = false,
    val isLoading: Boolean = false,
    val lastSavedAt: String? = null,
    val saveError: String? = null,
) {
    val hasMeaningfulContent: Boolean
        get() = body.isMeaningfulNoteBody()

    val displayTitle: String
        get() = title.ifBlank { UNTITLED_NOTE_TITLE }
}

data class NotesUiState(
    val activeVault: NotesVaultSummary? = null,
    val editor: NoteEditorState = NoteEditorState(),
    val recentNotes: List<NoteListItem> = emptyList(),
    val searchQuery: String = "",
    val searchResults: List<NoteListItem> = emptyList(),
    val isCatalogScanning: Boolean = false,
    val isOpeningEditor: Boolean = false,
) {
    val hasVaultConfigured: Boolean
        get() = activeVault != null
}

@Serializable
data class NotesVaultArtifact(
    @SerialName("schema_version")
    val schemaVersion: Int = 1,
    @SerialName("vault_id")
    val vaultId: String,
    @SerialName("display_name")
    val displayName: String,
    @SerialName("created_at")
    val createdAt: String,
    @SerialName("updated_at")
    val updatedAt: String,
)

@Serializable
data class NotesCatalogArtifact(
    @SerialName("schema_version")
    val schemaVersion: Int = 1,
    val notes: List<NotesCatalogEntry> = emptyList(),
)

@Serializable
data class NotesCatalogEntry(
    @SerialName("note_key")
    val noteKey: String,
    @SerialName("relative_path")
    val relativePath: String,
    val title: String,
    @SerialName("title_lifecycle")
    val titleLifecycle: NoteTitleLifecycle,
    val excerpt: String,
    @SerialName("last_opened_at")
    val lastOpenedAt: String? = null,
    @SerialName("last_saved_at")
    val lastSavedAt: String? = null,
    @SerialName("content_sha256")
    val contentSha256: String,
)

@Serializable
data class NoteMetadataArtifact(
    @SerialName("schema_version")
    val schemaVersion: Int = 1,
    @SerialName("note_key")
    val noteKey: String,
    @SerialName("vault_id")
    val vaultId: String,
    @SerialName("relative_path")
    val relativePath: String,
    val title: String,
    @SerialName("title_lifecycle")
    val titleLifecycle: NoteTitleLifecycle,
    val excerpt: String,
    @SerialName("mirrors_title_to_h1")
    val mirrorsTitleToH1: Boolean,
    @SerialName("word_count")
    val wordCount: Int,
    @SerialName("char_count")
    val charCount: Int,
    @SerialName("content_sha256")
    val contentSha256: String,
    @SerialName("last_opened_at")
    val lastOpenedAt: String? = null,
    @SerialName("last_saved_at")
    val lastSavedAt: String? = null,
)

data class ParsedMarkdownNote(
    val title: String,
    val body: String,
    val mirrorsTitleToH1: Boolean,
)

data class LoadedNoteDocument(
    val editor: NoteEditorState,
    val listItem: NoteListItem,
)

data class SaveNoteResult(
    val editor: NoteEditorState,
    val listItem: NoteListItem,
    val eventType: String,
)

const val UNTITLED_NOTE_TITLE = "Untitled note"
