package io.gervaise.babygervaise.notes

import android.app.Application
import android.net.Uri
import androidx.documentfile.provider.DocumentFile
import java.io.FileNotFoundException
import java.time.Instant
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json

class NotesRepository(
    private val application: Application,
    private val preferences: NotesPreferences = NotesPreferences(application),
    private val json: Json = Json {
        ignoreUnknownKeys = true
        prettyPrint = true
    },
    private val nowProvider: () -> Instant = { Instant.now() },
) {
    fun loadActiveVault(): NotesVaultSummary? = preferences.loadActiveVault()

    fun loadLastActiveNote(): LastActiveNote? = preferences.loadLastActiveNote()

    fun clearLastActiveNote() {
        preferences.clearLastActiveNote()
    }

    fun persistLastActiveNote(
        noteRef: NoteRef,
        titleSnapshot: String,
        openedAt: String,
    ) {
        preferences.saveLastActiveNote(noteRef, titleSnapshot, openedAt)
    }

    fun persistVaultSelection(treeUri: Uri): NotesVaultSummary {
        val root = DocumentFile.fromTreeUri(application, treeUri)
            ?: error("Unable to access the selected folder.")
        val summary = NotesVaultSummary(
            treeUri = treeUri.toString(),
            displayName = root.name ?: "Notes vault",
            vaultId = noteKeyForRelativePath(treeUri.toString()),
        )
        preferences.saveActiveVault(summary)
        preferences.clearLastActiveNote()
        return summary
    }

    suspend fun ensureVaultArtifacts(vault: NotesVaultSummary) {
        withContext(Dispatchers.IO) {
            val root = requireVaultRoot(vault)
            val now = nowTimestamp()
            val gervaiseDir = getOrCreateDirectory(root, GERVAISE_DIR_NAME)
            val metadataDir = getOrCreateDirectory(gervaiseDir, METADATA_DIR_NAME)
            getOrCreateFile(metadataDir, CATALOG_FILE_NAME, JSON_MIME_TYPE)
            val vaultFile = getOrCreateFile(gervaiseDir, VAULT_FILE_NAME, JSON_MIME_TYPE)
            val existingCatalog = loadCatalogArtifact(root)
            if (readText(vaultFile).isBlank()) {
                writeText(
                    vaultFile,
                    json.encodeToString(
                        NotesVaultArtifact(
                            vaultId = vault.vaultId,
                            displayName = vault.displayName,
                            createdAt = now,
                            updatedAt = now,
                        ),
                    ),
                )
            }
            writeCatalogArtifact(root, existingCatalog)
        }
    }

    suspend fun loadCatalogItems(vault: NotesVaultSummary): List<NoteListItem> = withContext(Dispatchers.IO) {
        ensureVaultArtifacts(vault)
        loadCatalogArtifact(requireVaultRoot(vault)).notes
            .map(::toListItem)
            .sortedByDescending { it.rankTimestamp.orEmpty() }
    }

    suspend fun scanVaultIntoCatalog(vault: NotesVaultSummary): List<NoteListItem> = withContext(Dispatchers.IO) {
        val root = requireVaultRoot(vault)
        ensureVaultArtifacts(vault)
        val entries = collectMarkdownEntries(root)
            .map { (document, relativePath) ->
                val existingMetadata = loadMetadata(root, noteKeyForRelativePath(relativePath))
                val parsed = parseStoredMarkdown(
                    content = readText(document),
                    fallbackTitle = document.name
                        ?.removeSuffix(MARKDOWN_EXTENSION)
                        ?.ifBlank { UNTITLED_NOTE_TITLE }
                        ?: UNTITLED_NOTE_TITLE,
                )
                val titleLifecycle = existingMetadata?.titleLifecycle ?: NoteTitleLifecycle.USER_OR_IMPORTED
                val mirrorsTitleToH1 = existingMetadata?.mirrorsTitleToH1 ?: parsed.mirrorsTitleToH1
                val markdown = buildStoredMarkdown(
                    title = parsed.title,
                    body = parsed.body,
                    mirrorsTitleToH1 = mirrorsTitleToH1,
                )
                val metadata = NoteMetadataArtifact(
                    noteKey = noteKeyForRelativePath(relativePath),
                    vaultId = vault.vaultId,
                    relativePath = relativePath,
                    title = existingMetadata?.title ?: parsed.title,
                    titleLifecycle = titleLifecycle,
                    excerpt = createNoteExcerpt(parsed.body),
                    mirrorsTitleToH1 = mirrorsTitleToH1,
                    wordCount = countWords(parsed.body),
                    charCount = parsed.body.length,
                    contentSha256 = contentSha256(markdown),
                    lastOpenedAt = existingMetadata?.lastOpenedAt,
                    lastSavedAt = existingMetadata?.lastSavedAt,
                )
                writeMetadata(root, metadata)
                metadata.toCatalogEntry()
            }
            .sortedByDescending { entry -> listOf(entry.lastOpenedAt, entry.lastSavedAt).filterNotNull().maxOrNull().orEmpty() }
        writeCatalogArtifact(root, NotesCatalogArtifact(notes = entries))
        entries.map(::toListItem)
    }

    suspend fun noteExists(
        vault: NotesVaultSummary,
        noteRef: NoteRef,
    ): Boolean = withContext(Dispatchers.IO) {
        resolveRelativePath(requireVaultRoot(vault), noteRef.relativePath) != null
    }

    suspend fun loadNote(
        vault: NotesVaultSummary,
        noteRef: NoteRef,
    ): LoadedNoteDocument? = withContext(Dispatchers.IO) {
        val root = requireVaultRoot(vault)
        val document = resolveRelativePath(root, noteRef.relativePath) ?: return@withContext null
        val content = readText(document)
        val fallbackTitle = document.name
            ?.removeSuffix(MARKDOWN_EXTENSION)
            ?.ifBlank { UNTITLED_NOTE_TITLE }
            ?: UNTITLED_NOTE_TITLE
        val parsed = parseStoredMarkdown(content, fallbackTitle)
        val metadata = loadMetadata(root, noteRef.noteKey)
        val title = metadata?.title ?: parsed.title
        val titleLifecycle = metadata?.titleLifecycle ?: NoteTitleLifecycle.USER_OR_IMPORTED
        val mirrorsTitleToH1 = metadata?.mirrorsTitleToH1 ?: parsed.mirrorsTitleToH1
        val editor = NoteEditorState(
            noteRef = noteRef,
            title = title,
            titleLifecycle = titleLifecycle,
            body = parsed.body,
            mirrorsTitleToH1 = mirrorsTitleToH1,
            isPersisted = true,
            isDirty = false,
            lastSavedAt = metadata?.lastSavedAt,
        )
        val listItem = NoteListItem(
            noteRef = noteRef,
            title = title,
            excerpt = metadata?.excerpt ?: createNoteExcerpt(parsed.body),
            titleLifecycle = titleLifecycle,
            lastOpenedAt = metadata?.lastOpenedAt,
            lastSavedAt = metadata?.lastSavedAt,
        )
        LoadedNoteDocument(editor = editor, listItem = listItem)
    }

    suspend fun markNoteOpened(
        vault: NotesVaultSummary,
        editor: NoteEditorState,
    ): NoteListItem = withContext(Dispatchers.IO) {
        val noteRef = requireNotNull(editor.noteRef) { "Persisted note required." }
        val root = requireVaultRoot(vault)
        val now = nowTimestamp()
        val metadata = loadMetadata(root, noteRef.noteKey)?.copy(
            title = editor.displayTitle,
            titleLifecycle = editor.titleLifecycle,
            excerpt = createNoteExcerpt(editor.body),
            mirrorsTitleToH1 = editor.mirrorsTitleToH1,
            wordCount = countWords(editor.body),
            charCount = editor.body.length,
            lastOpenedAt = now,
            lastSavedAt = editor.lastSavedAt,
            contentSha256 = contentSha256(
                buildStoredMarkdown(editor.displayTitle, editor.body, editor.mirrorsTitleToH1),
            ),
        ) ?: NoteMetadataArtifact(
            noteKey = noteRef.noteKey,
            vaultId = vault.vaultId,
            relativePath = noteRef.relativePath,
            title = editor.displayTitle,
            titleLifecycle = editor.titleLifecycle,
            excerpt = createNoteExcerpt(editor.body),
            mirrorsTitleToH1 = editor.mirrorsTitleToH1,
            wordCount = countWords(editor.body),
            charCount = editor.body.length,
            contentSha256 = contentSha256(
                buildStoredMarkdown(editor.displayTitle, editor.body, editor.mirrorsTitleToH1),
            ),
            lastOpenedAt = now,
            lastSavedAt = editor.lastSavedAt,
        )
        writeMetadata(root, metadata)
        val catalogEntry = metadata.toCatalogEntry()
        upsertCatalogEntry(root, catalogEntry)
        preferences.saveLastActiveNote(noteRef, editor.displayTitle, now)
        toListItem(catalogEntry)
    }

    suspend fun saveNote(
        vault: NotesVaultSummary,
        editor: NoteEditorState,
    ): SaveNoteResult = withContext(Dispatchers.IO) {
        val root = requireVaultRoot(vault)
        ensureVaultArtifacts(vault)
        val isNewNote = !editor.isPersisted || editor.noteRef == null
        val resolvedTitle = when (editor.titleLifecycle) {
            NoteTitleLifecycle.SYSTEM_PROVISIONAL -> deriveSystemProvisionalTitle(editor.body)
            else -> editor.displayTitle
        }
        val noteRef = editor.noteRef ?: createNoteRef(root, resolvedTitle)
        val titleLifecycle = if (isNewNote) {
            NoteTitleLifecycle.SYSTEM_STABLE
        } else {
            editor.titleLifecycle
        }
        val mirrorsTitleToH1 = if (isNewNote) {
            true
        } else {
            editor.mirrorsTitleToH1
        }
        val document = if (isNewNote) {
            createMarkdownFile(root, noteRef.relativePath)
        } else {
            resolveRelativePath(root, noteRef.relativePath)
                ?: throw FileNotFoundException("Missing note ${noteRef.relativePath}")
        }
        val markdown = buildStoredMarkdown(
            title = resolvedTitle,
            body = editor.body,
            mirrorsTitleToH1 = mirrorsTitleToH1,
        )
        writeText(document, markdown)

        val now = nowTimestamp()
        val metadata = NoteMetadataArtifact(
            noteKey = noteRef.noteKey,
            vaultId = vault.vaultId,
            relativePath = noteRef.relativePath,
            title = resolvedTitle,
            titleLifecycle = titleLifecycle,
            excerpt = createNoteExcerpt(editor.body),
            mirrorsTitleToH1 = mirrorsTitleToH1,
            wordCount = countWords(editor.body),
            charCount = editor.body.length,
            contentSha256 = contentSha256(markdown),
            lastOpenedAt = now,
            lastSavedAt = now,
        )
        writeMetadata(root, metadata)
        val catalogEntry = metadata.toCatalogEntry()
        upsertCatalogEntry(root, catalogEntry)
        preferences.saveLastActiveNote(noteRef, resolvedTitle, now)

        SaveNoteResult(
            editor = editor.copy(
                noteRef = noteRef,
                title = resolvedTitle,
                titleLifecycle = titleLifecycle,
                mirrorsTitleToH1 = mirrorsTitleToH1,
                isPersisted = true,
                isDirty = false,
                isSaving = false,
                lastSavedAt = now,
                saveError = null,
            ),
            listItem = toListItem(catalogEntry),
            eventType = if (isNewNote) "note_created" else "note_edited",
        )
    }

    private fun requireVaultRoot(vault: NotesVaultSummary): DocumentFile {
        return DocumentFile.fromTreeUri(application, Uri.parse(vault.treeUri))
            ?.takeIf { it.exists() && it.isDirectory }
            ?: throw IllegalStateException("Configured notes vault is unavailable.")
    }

    private fun createNoteRef(
        root: DocumentFile,
        title: String,
    ): NoteRef {
        var index = 1
        var relativePath: String
        do {
            val slug = slugifyTitle(title)
            val suffix = if (index == 1) "" else "-$index"
            relativePath = "$slug$suffix$MARKDOWN_EXTENSION"
            index += 1
        } while (resolveRelativePath(root, relativePath) != null)
        return NoteRef(
            noteKey = noteKeyForRelativePath(relativePath),
            relativePath = relativePath,
        )
    }

    private fun createMarkdownFile(
        root: DocumentFile,
        relativePath: String,
    ): DocumentFile {
        val segments = relativePath.split('/').filter(String::isNotBlank)
        require(segments.isNotEmpty()) { "Relative path is required." }
        var parent = root
        val directories = segments.dropLast(1)
        for (segment in directories) {
            parent = getOrCreateDirectory(parent, segment)
        }
        return parent.createFile(MARKDOWN_MIME_TYPE, segments.last())
            ?: throw IllegalStateException("Unable to create note ${segments.last()}.")
    }

    private fun collectMarkdownEntries(root: DocumentFile): List<Pair<DocumentFile, String>> {
        val collected = mutableListOf<Pair<DocumentFile, String>>()

        fun walk(
            current: DocumentFile,
            prefix: String,
        ) {
            current.listFiles().forEach { child ->
                val name = child.name ?: return@forEach
                if (name == GERVAISE_DIR_NAME) {
                    return@forEach
                }
                val relativePath = listOf(prefix, name)
                    .filter(String::isNotBlank)
                    .joinToString("/")
                when {
                    child.isDirectory -> walk(child, relativePath)
                    child.isFile && name.lowercase().endsWith(MARKDOWN_EXTENSION) -> {
                        collected += child to relativePath
                    }
                }
            }
        }

        walk(root, "")
        return collected
    }

    private fun resolveRelativePath(
        root: DocumentFile,
        relativePath: String,
    ): DocumentFile? {
        val segments = relativePath.split('/').filter(String::isNotBlank)
        var current: DocumentFile = root
        for (segment in segments) {
            current = current.findFile(segment) ?: return null
        }
        return current
    }

    private fun getOrCreateDirectory(
        parent: DocumentFile,
        name: String,
    ): DocumentFile {
        return parent.findFile(name)
            ?.takeIf { it.isDirectory }
            ?: parent.createDirectory(name)
            ?: throw IllegalStateException("Unable to create directory $name.")
    }

    private fun getOrCreateFile(
        parent: DocumentFile,
        name: String,
        mimeType: String,
    ): DocumentFile {
        return parent.findFile(name)
            ?.takeIf { it.isFile }
            ?: parent.createFile(mimeType, name)
            ?: throw IllegalStateException("Unable to create file $name.")
    }

    private fun loadCatalogArtifact(root: DocumentFile): NotesCatalogArtifact {
        val metadataDir = getOrCreateDirectory(getOrCreateDirectory(root, GERVAISE_DIR_NAME), METADATA_DIR_NAME)
        val catalogFile = getOrCreateFile(metadataDir, CATALOG_FILE_NAME, JSON_MIME_TYPE)
        val raw = readText(catalogFile)
        return if (raw.isBlank()) {
            NotesCatalogArtifact()
        } else {
            runCatching { json.decodeFromString<NotesCatalogArtifact>(raw) }
                .getOrElse { NotesCatalogArtifact() }
        }
    }

    private fun writeCatalogArtifact(
        root: DocumentFile,
        artifact: NotesCatalogArtifact,
    ) {
        val metadataDir = getOrCreateDirectory(getOrCreateDirectory(root, GERVAISE_DIR_NAME), METADATA_DIR_NAME)
        val catalogFile = getOrCreateFile(metadataDir, CATALOG_FILE_NAME, JSON_MIME_TYPE)
        writeText(catalogFile, json.encodeToString(artifact))
    }

    private fun upsertCatalogEntry(
        root: DocumentFile,
        entry: NotesCatalogEntry,
    ) {
        val existing = loadCatalogArtifact(root).notes
            .filterNot { it.noteKey == entry.noteKey }
            .plus(entry)
            .sortedByDescending { candidate ->
                listOf(candidate.lastOpenedAt, candidate.lastSavedAt)
                    .filterNotNull()
                    .maxOrNull()
                    .orEmpty()
            }
        writeCatalogArtifact(root, NotesCatalogArtifact(notes = existing))
    }

    private fun loadMetadata(
        root: DocumentFile,
        noteKey: String,
    ): NoteMetadataArtifact? {
        val metadataDir = getOrCreateDirectory(getOrCreateDirectory(root, GERVAISE_DIR_NAME), METADATA_DIR_NAME)
        val metadataFile = metadataDir.findFile(metadataFileName(noteKey)) ?: return null
        val raw = readText(metadataFile)
        if (raw.isBlank()) {
            return null
        }
        return runCatching { json.decodeFromString<NoteMetadataArtifact>(raw) }.getOrNull()
    }

    private fun writeMetadata(
        root: DocumentFile,
        metadata: NoteMetadataArtifact,
    ) {
        val metadataDir = getOrCreateDirectory(getOrCreateDirectory(root, GERVAISE_DIR_NAME), METADATA_DIR_NAME)
        val metadataFile = getOrCreateFile(metadataDir, metadataFileName(metadata.noteKey), JSON_MIME_TYPE)
        writeText(metadataFile, json.encodeToString(metadata))
    }

    private fun toListItem(entry: NotesCatalogEntry): NoteListItem = NoteListItem(
        noteRef = NoteRef(
            noteKey = entry.noteKey,
            relativePath = entry.relativePath,
        ),
        title = entry.title,
        excerpt = entry.excerpt,
        titleLifecycle = entry.titleLifecycle,
        lastOpenedAt = entry.lastOpenedAt,
        lastSavedAt = entry.lastSavedAt,
    )

    private fun NoteMetadataArtifact.toCatalogEntry(): NotesCatalogEntry = NotesCatalogEntry(
        noteKey = noteKey,
        relativePath = relativePath,
        title = title,
        titleLifecycle = titleLifecycle,
        excerpt = excerpt,
        lastOpenedAt = lastOpenedAt,
        lastSavedAt = lastSavedAt,
        contentSha256 = contentSha256,
    )

    private fun readText(file: DocumentFile): String {
        val input = application.contentResolver.openInputStream(file.uri) ?: return ""
        return input.bufferedReader().use { reader -> reader.readText() }
    }

    private fun writeText(
        file: DocumentFile,
        value: String,
    ) {
        val output = application.contentResolver.openOutputStream(file.uri, "wt")
            ?: throw IllegalStateException("Unable to open ${file.name} for writing.")
        output.bufferedWriter().use { writer ->
            writer.write(value)
        }
    }

    private fun countWords(body: String): Int {
        val trimmed = body.trim()
        return if (trimmed.isBlank()) {
            0
        } else {
            trimmed.split(Regex("""\s+""")).size
        }
    }

    private fun nowTimestamp(): String = nowProvider().toString()

    private companion object {
        const val GERVAISE_DIR_NAME = ".gervaise"
        const val METADATA_DIR_NAME = "metadata"
        const val VAULT_FILE_NAME = "vault.json"
        const val CATALOG_FILE_NAME = "catalog.json"
        const val MARKDOWN_EXTENSION = ".md"
        const val MARKDOWN_MIME_TYPE = "text/markdown"
        const val JSON_MIME_TYPE = "application/json"

        fun metadataFileName(noteKey: String): String = "$noteKey.metadata.json"
    }
}
