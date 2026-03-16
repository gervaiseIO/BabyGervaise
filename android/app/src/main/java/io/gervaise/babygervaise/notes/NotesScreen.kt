package io.gervaise.babygervaise.notes

import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.rounded.Create
import androidx.compose.material.icons.rounded.FolderOpen
import androidx.compose.material.icons.rounded.Search
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import io.gervaise.babygervaise.Screen
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter

@Composable
fun NotesSurface(
    route: Screen.NotesRoute,
    notesState: NotesUiState,
    snackbarHostState: SnackbarHostState,
    onNavigateBack: () -> Unit,
    onOpenSearch: () -> Unit,
    onCloseSearch: () -> Unit,
    onVaultSelected: (Uri) -> Unit,
    onBodyChanged: (String) -> Unit,
    onQueryChanged: (String) -> Unit,
    onOpenNote: (NoteListItem) -> Unit,
) {
    when (route) {
        Screen.NotesRoute.Onboarding -> NotesOnboardingScreen(
            snackbarHostState = snackbarHostState,
            onNavigateBack = onNavigateBack,
            onVaultSelected = onVaultSelected,
        )

        Screen.NotesRoute.Editor -> NotesEditorScreen(
            notesState = notesState,
            snackbarHostState = snackbarHostState,
            onNavigateBack = onNavigateBack,
            onOpenSearch = onOpenSearch,
            onBodyChanged = onBodyChanged,
        )

        Screen.NotesRoute.Search -> NotesSearchScreen(
            notesState = notesState,
            snackbarHostState = snackbarHostState,
            onNavigateBack = onCloseSearch,
            onQueryChanged = onQueryChanged,
            onOpenNote = onOpenNote,
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun NotesOnboardingScreen(
    snackbarHostState: SnackbarHostState,
    onNavigateBack: () -> Unit,
    onVaultSelected: (Uri) -> Unit,
) {
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
        if (uri != null) {
            onVaultSelected(uri)
        }
    }

    Scaffold(
        snackbarHost = { SnackbarHost(hostState = snackbarHostState) },
        topBar = {
            TopAppBar(
                title = { Text("Notes") },
                navigationIcon = {
                    IconButton(onClick = onNavigateBack) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Rounded.ArrowBack,
                            contentDescription = "Back to chat",
                        )
                    }
                },
            )
        },
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding)
                .padding(horizontal = 24.dp, vertical = 32.dp),
            verticalArrangement = Arrangement.spacedBy(18.dp),
        ) {
            Surface(
                modifier = Modifier.fillMaxWidth(),
                shape = MaterialTheme.shapes.extraLarge,
                color = MaterialTheme.colorScheme.surfaceContainerLow,
            ) {
                Column(
                    modifier = Modifier.padding(20.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        Icon(
                            imageVector = Icons.Rounded.Create,
                            contentDescription = null,
                            tint = MaterialTheme.colorScheme.primary,
                        )
                    }
                    Text(
                        text = "Bring your vault. Keep your markdown.",
                        style = MaterialTheme.typography.headlineSmall,
                        fontWeight = FontWeight.SemiBold,
                    )
                    Text(
                        text = "Notes works with an existing Obsidian vault or any blank folder. Baby Gervaise writes clean Markdown and keeps indexing artifacts inside .gervaise.",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    TextButton(
                        onClick = { launcher.launch(null) },
                        modifier = Modifier.testTag("notes-pick-folder"),
                    ) {
                        Icon(
                            imageVector = Icons.Rounded.FolderOpen,
                            contentDescription = null,
                        )
                        Text(
                            text = "Choose folder",
                            modifier = Modifier.padding(start = 8.dp),
                        )
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun NotesEditorScreen(
    notesState: NotesUiState,
    snackbarHostState: SnackbarHostState,
    onNavigateBack: () -> Unit,
    onOpenSearch: () -> Unit,
    onBodyChanged: (String) -> Unit,
) {
    val focusRequester = remember { FocusRequester() }

    LaunchedEffect(Unit) {
        focusRequester.requestFocus()
    }

    Scaffold(
        snackbarHost = { SnackbarHost(hostState = snackbarHostState) },
        topBar = {
            TopAppBar(
                title = { Text(notesState.activeVault?.displayName ?: "Notes") },
                navigationIcon = {
                    IconButton(onClick = onNavigateBack) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Rounded.ArrowBack,
                            contentDescription = "Back to chat",
                        )
                    }
                },
            )
        },
        floatingActionButton = {
            FloatingActionButton(
                onClick = onOpenSearch,
                modifier = Modifier.testTag("notes-search-fab"),
            ) {
                Icon(
                    imageVector = Icons.Rounded.Search,
                    contentDescription = "Search notes",
                )
            }
        },
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding)
                .padding(horizontal = 20.dp, vertical = 16.dp),
        ) {
            Text(
                text = notesState.editor.displayTitle,
                style = MaterialTheme.typography.headlineMedium,
                fontWeight = FontWeight.SemiBold,
                modifier = Modifier.testTag("notes-editor-title"),
            )
            Text(
                text = notesLastEditedLabel(notesState.editor.lastSavedAt),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(top = 6.dp),
            )
            if (notesState.editor.saveError != null) {
                Text(
                    text = notesState.editor.saveError,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.padding(top = 8.dp),
                )
            }
            HorizontalDivider(modifier = Modifier.padding(vertical = 16.dp))
            Box(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .background(
                        color = MaterialTheme.colorScheme.surfaceContainerLowest,
                        shape = MaterialTheme.shapes.extraLarge,
                    )
                    .padding(20.dp)
                    .imePadding()
                    .navigationBarsPadding(),
            ) {
                if (notesState.editor.body.isBlank()) {
                    Text(
                        text = "Start writing...",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                BasicTextField(
                    value = notesState.editor.body,
                    onValueChange = onBodyChanged,
                    modifier = Modifier
                        .fillMaxSize()
                        .focusRequester(focusRequester)
                        .testTag("notes-editor-body"),
                    textStyle = MaterialTheme.typography.bodyLarge.copy(
                        color = MaterialTheme.colorScheme.onSurface,
                    ),
                )
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun NotesSearchScreen(
    notesState: NotesUiState,
    snackbarHostState: SnackbarHostState,
    onNavigateBack: () -> Unit,
    onQueryChanged: (String) -> Unit,
    onOpenNote: (NoteListItem) -> Unit,
) {
    val items = if (notesState.searchQuery.isBlank()) {
        notesState.recentNotes
    } else {
        notesState.searchResults
    }

    Scaffold(
        snackbarHost = { SnackbarHost(hostState = snackbarHostState) },
        topBar = {
            TopAppBar(
                title = { Text("Search / Recents") },
                navigationIcon = {
                    IconButton(onClick = onNavigateBack) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Rounded.ArrowBack,
                            contentDescription = "Back to editor",
                        )
                    }
                },
            )
        },
    ) { innerPadding ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding)
                .padding(horizontal = 16.dp, vertical = 12.dp),
        ) {
            OutlinedTextField(
                value = notesState.searchQuery,
                onValueChange = onQueryChanged,
                modifier = Modifier
                    .fillMaxWidth()
                    .testTag("notes-search-input"),
                placeholder = { Text("Search by title, path, or excerpt") },
                singleLine = true,
            )
            LazyColumn(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(top = 12.dp)
                    .testTag("notes-search-results"),
                contentPadding = PaddingValues(bottom = 24.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                if (notesState.isCatalogScanning) {
                    item(key = "scan-state") {
                        Text(
                            text = "Scanning your vault in the background…",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                if (items.isEmpty()) {
                    item(key = "empty") {
                        Text(
                            text = if (notesState.searchQuery.isBlank()) {
                                "No recent notes yet."
                            } else {
                                "No notes matched that search."
                            },
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                items(
                    items = items,
                    key = { item -> item.noteRef.noteKey },
                ) { item ->
                    Surface(
                        modifier = Modifier
                            .fillMaxWidth()
                            .clickable { onOpenNote(item) },
                        shape = MaterialTheme.shapes.large,
                        color = MaterialTheme.colorScheme.surfaceContainerLow,
                    ) {
                        Column(
                            modifier = Modifier.padding(16.dp),
                            verticalArrangement = Arrangement.spacedBy(6.dp),
                        ) {
                            Text(
                                text = item.title,
                                style = MaterialTheme.typography.titleMedium,
                                fontWeight = FontWeight.Medium,
                            )
                            Text(
                                text = item.noteRef.relativePath,
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                            if (item.excerpt.isNotBlank()) {
                                Text(
                                    text = item.excerpt,
                                    style = MaterialTheme.typography.bodyMedium,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

private fun notesLastEditedLabel(lastSavedAt: String?): String {
    val timestamp = runCatching { Instant.parse(lastSavedAt) }.getOrNull()
    return if (timestamp == null) {
        "Last edited not saved yet"
    } else {
        val formatter = DateTimeFormatter.ofPattern("MMM d, HH:mm")
            .withZone(ZoneId.systemDefault())
        "Last edited ${formatter.format(timestamp)}"
    }
}
