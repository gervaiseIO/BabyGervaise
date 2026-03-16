package io.gervaise.babygervaise.notes

import android.content.Context

class NotesPreferences(
    context: Context,
) {
    private val preferences = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)

    fun loadActiveVault(): NotesVaultSummary? {
        val treeUri = preferences.getString(KEY_TREE_URI, null) ?: return null
        val displayName = preferences.getString(KEY_DISPLAY_NAME, null) ?: "Notes vault"
        val vaultId = preferences.getString(KEY_VAULT_ID, null) ?: noteKeyForRelativePath(treeUri)
        return NotesVaultSummary(
            treeUri = treeUri,
            displayName = displayName,
            vaultId = vaultId,
        )
    }

    fun saveActiveVault(summary: NotesVaultSummary) {
        preferences.edit()
            .putString(KEY_TREE_URI, summary.treeUri)
            .putString(KEY_DISPLAY_NAME, summary.displayName)
            .putString(KEY_VAULT_ID, summary.vaultId)
            .apply()
    }

    fun clearActiveVault() {
        preferences.edit()
            .remove(KEY_TREE_URI)
            .remove(KEY_DISPLAY_NAME)
            .remove(KEY_VAULT_ID)
            .apply()
        clearLastActiveNote()
    }

    fun loadLastActiveNote(): LastActiveNote? {
        val relativePath = preferences.getString(KEY_LAST_RELATIVE_PATH, null) ?: return null
        val noteKey = preferences.getString(KEY_LAST_NOTE_KEY, null) ?: return null
        val titleSnapshot = preferences.getString(KEY_LAST_TITLE, null) ?: UNTITLED_NOTE_TITLE
        val openedAt = preferences.getString(KEY_LAST_OPENED_AT, null) ?: return null
        return LastActiveNote(
            noteRef = NoteRef(
                noteKey = noteKey,
                relativePath = relativePath,
            ),
            titleSnapshot = titleSnapshot,
            openedAt = openedAt,
        )
    }

    fun saveLastActiveNote(
        noteRef: NoteRef,
        titleSnapshot: String,
        openedAt: String,
    ) {
        preferences.edit()
            .putString(KEY_LAST_RELATIVE_PATH, noteRef.relativePath)
            .putString(KEY_LAST_NOTE_KEY, noteRef.noteKey)
            .putString(KEY_LAST_TITLE, titleSnapshot)
            .putString(KEY_LAST_OPENED_AT, openedAt)
            .apply()
    }

    fun clearLastActiveNote() {
        preferences.edit()
            .remove(KEY_LAST_RELATIVE_PATH)
            .remove(KEY_LAST_NOTE_KEY)
            .remove(KEY_LAST_TITLE)
            .remove(KEY_LAST_OPENED_AT)
            .apply()
    }

    private companion object {
        const val PREFS_NAME = "notes_preferences"
        const val KEY_TREE_URI = "active_tree_uri"
        const val KEY_DISPLAY_NAME = "active_display_name"
        const val KEY_VAULT_ID = "active_vault_id"
        const val KEY_LAST_RELATIVE_PATH = "last_relative_path"
        const val KEY_LAST_NOTE_KEY = "last_note_key"
        const val KEY_LAST_TITLE = "last_title"
        const val KEY_LAST_OPENED_AT = "last_opened_at"
    }
}
