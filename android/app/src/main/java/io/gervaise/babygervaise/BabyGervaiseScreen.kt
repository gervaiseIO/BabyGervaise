package io.gervaise.babygervaise

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.keyframes
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Send
import androidx.compose.material.icons.rounded.CheckCircleOutline
import androidx.compose.material.icons.rounded.EditNote
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.rounded.Info
import androidx.compose.material.icons.rounded.Lightbulb
import androidx.compose.material.icons.rounded.MusicNote
import androidx.compose.material.icons.rounded.SmartToy
import androidx.compose.material.icons.rounded.Speaker
import androidx.compose.material.icons.rounded.Tune
import androidx.compose.material.icons.rounded.WarningAmber
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
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
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.viewmodel.compose.viewModel
import io.gervaise.babygervaise.bridge.ChatMessage
import io.gervaise.babygervaise.bridge.ContextLevel
import io.gervaise.babygervaise.bridge.CoreJson
import io.gervaise.babygervaise.bridge.DecisionTraceEntry
import io.gervaise.babygervaise.bridge.DiagnosticIssue
import io.gervaise.babygervaise.bridge.LogViewerEntry
import io.gervaise.babygervaise.bridge.ModelTraceEntry
import io.gervaise.babygervaise.bridge.OverviewSnapshot
import io.gervaise.babygervaise.bridge.ToolOverviewEntry
import io.gervaise.babygervaise.bridge.TurnTraceSummary
import io.gervaise.babygervaise.notes.NoteListItem
import io.gervaise.babygervaise.notes.NotesSurface
import kotlinx.coroutines.launch

@Composable
fun BabyGervaiseRoute(
    viewModel: BabyGervaiseViewModel = viewModel(),
) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    val snackbarHostState = remember { SnackbarHostState() }
    val lifecycleOwner = LocalLifecycleOwner.current

    LaunchedEffect(uiState.snackbarMessage) {
        val message = uiState.snackbarMessage ?: return@LaunchedEffect
        snackbarHostState.showSnackbar(message)
        viewModel.consumeSnackbar()
    }

    androidx.compose.runtime.DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            when (event) {
                Lifecycle.Event.ON_RESUME -> viewModel.onAppResumed()
                Lifecycle.Event.ON_STOP -> viewModel.onAppBackgrounded()
                else -> Unit
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose {
            lifecycleOwner.lifecycle.removeObserver(observer)
        }
    }

    BabyGervaiseApp(
        uiState = uiState,
        snackbarHostState = snackbarHostState,
        onDraftChanged = viewModel::updateDraft,
        onSubmit = viewModel::submitDraft,
        onStopCloudWorking = viewModel::muteCloudWorking,
        onSuggestionSelected = viewModel::submitSuggestion,
        onOpenOverview = viewModel::openOverview,
        onOpenChat = viewModel::openChat,
        onOpenNotes = viewModel::openNotes,
        onOpenNotesSearch = viewModel::openNotesSearch,
        onCloseNotesSearch = viewModel::closeNotesSearch,
        onNoteBodyChanged = viewModel::updateNoteBody,
        onNotesVaultSelected = viewModel::configureNotesVault,
        onOpenSearchedNote = viewModel::openSearchedNote,
        onNoteSearchQueryChanged = viewModel::updateNoteSearchQuery,
        onPreviousContextChanged = viewModel::updatePreviousContext,
        onCloudProfileChanged = viewModel::updateCloudProfile,
        onBeginToolAuth = viewModel::beginToolAuth,
        onRefreshToolState = viewModel::refreshToolState,
        onDisconnectTool = viewModel::disconnectTool,
    )
}

@Composable
fun BabyGervaiseApp(
    uiState: BabyGervaiseUiState,
    snackbarHostState: SnackbarHostState,
    onDraftChanged: (String) -> Unit,
    onSubmit: () -> Unit,
    onStopCloudWorking: () -> Unit,
    onSuggestionSelected: (String, String) -> Unit,
    onOpenOverview: () -> Unit,
    onOpenChat: () -> Unit,
    onOpenNotes: () -> Unit,
    onOpenNotesSearch: () -> Unit,
    onCloseNotesSearch: () -> Unit,
    onNoteBodyChanged: (String) -> Unit,
    onNotesVaultSelected: (android.net.Uri) -> Unit,
    onOpenSearchedNote: (NoteListItem) -> Unit,
    onNoteSearchQueryChanged: (String) -> Unit,
    onPreviousContextChanged: (ContextLevel) -> Unit,
    onCloudProfileChanged: (String) -> Unit,
    onBeginToolAuth: (String) -> Unit,
    onRefreshToolState: (String) -> Unit,
    onDisconnectTool: (String) -> Unit,
) {
    val interactionState = uiState.interactionState
    val timelineState = rememberLazyListState()
    val scope = rememberCoroutineScope()
    val isChatScreen = uiState.screen is Screen.Chat

    LaunchedEffect(
        interactionState.items,
        interactionState.isSending,
        uiState.screen,
    ) {
        if (isChatScreen && interactionState.items.isNotEmpty()) {
            timelineState.animateScrollToItem(
                index = interactionState.items.lastIndex.coerceAtLeast(0),
            )
        }
    }

    val screen = uiState.screen
    if (screen is Screen.Notes) {
        NotesSurface(
            route = screen.route,
            notesState = uiState.notesState,
            snackbarHostState = snackbarHostState,
            onNavigateBack = onOpenChat,
            onOpenSearch = onOpenNotesSearch,
            onCloseSearch = onCloseNotesSearch,
            onVaultSelected = onNotesVaultSelected,
            onBodyChanged = onNoteBodyChanged,
            onQueryChanged = onNoteSearchQueryChanged,
            onOpenNote = onOpenSearchedNote,
        )
        return
    }

    Scaffold(
        containerColor = MaterialTheme.colorScheme.background,
        topBar = {
            AppTopBar(
                screen = screen,
                onOpenOverview = onOpenOverview,
                onOpenNotes = onOpenNotes,
                onOpenChat = onOpenChat,
            )
        },
        bottomBar = {
            if (isChatScreen) {
                InteractionComposerBar(
                    interactionState = interactionState,
                    onDraftChanged = onDraftChanged,
                    onSubmit = onSubmit,
                    onStopCloudWorking = onStopCloudWorking,
                    onFocused = {
                        scope.launch {
                            if (interactionState.items.isNotEmpty()) {
                                timelineState.animateScrollToItem(interactionState.items.lastIndex)
                            }
                        }
                    },
                )
            }
        },
        snackbarHost = { SnackbarHost(hostState = snackbarHostState) },
    ) { innerPadding ->
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(innerPadding),
        ) {
            when (uiState.screen) {
                Screen.Chat -> InteractionScreen(
                    interactionState = interactionState,
                    timelineState = timelineState,
                    onSuggestionSelected = onSuggestionSelected,
                )

                Screen.Overview -> OverviewScreen(
                    uiState = uiState,
                    onPreviousContextChanged = onPreviousContextChanged,
                    onCloudProfileChanged = onCloudProfileChanged,
                    onBeginToolAuth = onBeginToolAuth,
                    onRefreshToolState = onRefreshToolState,
                    onDisconnectTool = onDisconnectTool,
                )

                is Screen.Notes -> Unit
            }

            AnimatedVisibility(
                modifier = Modifier
                    .align(Alignment.Center)
                    .padding(24.dp),
                visible = uiState.isInitializing && interactionState.items.isEmpty(),
            ) {
                CircularProgressIndicator()
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun AppTopBar(
    screen: Screen,
    onOpenOverview: () -> Unit,
    onOpenNotes: () -> Unit,
    onOpenChat: () -> Unit,
) {
    TopAppBar(
        title = {
            Text(
                text = when (screen) {
                    Screen.Chat -> "Baby Gervaise"
                    Screen.Overview -> "Overview"
                    is Screen.Notes -> "Notes"
                },
                style = MaterialTheme.typography.titleLarge,
            )
        },
        actions = {
            when (screen) {
                Screen.Chat -> {
                    IconButton(onClick = onOpenNotes) {
                        Icon(
                            imageVector = Icons.Rounded.EditNote,
                            contentDescription = "Write",
                        )
                    }
                    IconButton(onClick = onOpenOverview) {
                        Icon(
                            imageVector = Icons.Rounded.Tune,
                            contentDescription = "Overview",
                        )
                    }
                }

                Screen.Overview -> {
                    TextButton(onClick = onOpenChat) {
                        Text("Chat")
                    }
                }

                is Screen.Notes -> Unit
            }
        },
    )
}

@Composable
private fun ChatScreen(
    uiState: BabyGervaiseUiState,
    timelineState: LazyListState,
) {
    val timelineItems = buildConversationTimeline(uiState)

    LazyColumn(
        state = timelineState,
        modifier = Modifier
            .fillMaxSize()
            .testTag("timeline"),
        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        if (timelineItems.isEmpty()) {
            item(key = "empty-state") {
                EmptyConversationState()
            }
        }

        items(
            items = timelineItems,
            key = { item -> item.key },
        ) { item ->
            when (item) {
                is ConversationTimelineItem.UserMessage -> UserMessageBubble(item.message)
                is ConversationTimelineItem.AssistantMessage -> AssistantMessageBubble(item.message)
                is ConversationTimelineItem.SystemMessage -> SystemMessageCard(item.card)
                is ConversationTimelineItem.Processing -> AssistantProcessingBubble()
            }
        }
    }
}

@Composable
private fun ChatComposerBar(
    uiState: BabyGervaiseUiState,
    onDraftChanged: (String) -> Unit,
    onSubmit: () -> Unit,
    isComposerEnabled: Boolean,
    onFocused: () -> Unit,
) {
    Surface(
        tonalElevation = 2.dp,
        color = MaterialTheme.colorScheme.surface,
    ) {
        Composer(
            draft = uiState.draft,
            isPending = uiState.isPending,
            isEnabled = isComposerEnabled,
            onDraftChanged = onDraftChanged,
            onSubmit = onSubmit,
            onFocused = onFocused,
        )
    }
}

@Composable
private fun Composer(
    draft: String,
    isPending: Boolean,
    isEnabled: Boolean,
    onDraftChanged: (String) -> Unit,
    onSubmit: () -> Unit,
    onFocused: () -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .imePadding()
            .navigationBarsPadding()
            .padding(horizontal = 16.dp, vertical = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.Bottom,
    ) {
        OutlinedTextField(
            value = draft,
            onValueChange = onDraftChanged,
            enabled = isEnabled,
            modifier = Modifier
                .weight(1f)
                .heightIn(min = 56.dp)
                .testTag("composer-input")
                .onFocusChanged {
                    if (it.isFocused && isEnabled) {
                        onFocused()
                    }
                },
            textStyle = MaterialTheme.typography.bodyLarge,
            placeholder = {
                Text("Tell Gervaise what you need.")
            },
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
            minLines = 1,
            maxLines = 4,
        )
        if (isPending) {
            Surface(
                modifier = Modifier
                    .size(56.dp)
                    .testTag("send-button"),
                shape = CircleShape,
                color = MaterialTheme.colorScheme.primaryContainer,
                contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
            ) {
                Box(contentAlignment = Alignment.Center) {
                    CircularProgressIndicator(
                        modifier = Modifier
                            .size(22.dp)
                            .testTag("send-progress"),
                        strokeWidth = 2.25.dp,
                    )
                }
            }
        } else {
            FilledIconButton(
                onClick = onSubmit,
                enabled = isEnabled && draft.isNotBlank(),
                modifier = Modifier
                    .size(56.dp)
                    .testTag("send-button"),
            ) {
                Icon(
                    imageVector = Icons.AutoMirrored.Rounded.Send,
                    contentDescription = "Send message",
                )
            }
        }
    }
}

@Composable
private fun EmptyConversationState() {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 24.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            text = "One conversation. No reset.",
            style = MaterialTheme.typography.headlineSmall,
        )
        Text(
            text = "Start speaking with Gervaise. This timeline is the whole relationship.",
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun UserMessageBubble(
    message: ChatMessage,
) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        horizontalAlignment = Alignment.End,
    ) {
        Surface(
            modifier = Modifier.fillMaxWidth(0.84f),
            color = MaterialTheme.colorScheme.primaryContainer,
            contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
            shape = MaterialTheme.shapes.extraLarge,
        ) {
            Text(
                text = message.content,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                style = MaterialTheme.typography.bodyLarge,
            )
        }
    }
}

@Composable
private fun AssistantMessageBubble(
    message: ChatMessage,
) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        horizontalAlignment = Alignment.Start,
    ) {
        Surface(
            modifier = Modifier.fillMaxWidth(0.9f),
            color = MaterialTheme.colorScheme.surfaceContainerLow,
            contentColor = MaterialTheme.colorScheme.onSurface,
            shape = MaterialTheme.shapes.extraLarge,
        ) {
            Text(
                text = message.content,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                style = MaterialTheme.typography.bodyLarge,
            )
        }
    }
}

@Composable
private fun AssistantProcessingBubble() {
    Column(
        modifier = Modifier.fillMaxWidth(),
        horizontalAlignment = Alignment.Start,
    ) {
        Surface(
            modifier = Modifier
                .fillMaxWidth(0.42f)
                .testTag("assistant-processing"),
            color = MaterialTheme.colorScheme.surfaceContainerLow,
            contentColor = MaterialTheme.colorScheme.onSurface,
            shape = MaterialTheme.shapes.extraLarge,
        ) {
            Row(
                modifier = Modifier.padding(horizontal = 14.dp, vertical = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(
                    imageVector = Icons.Rounded.SmartToy,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                TypingDots()
            }
        }
    }
}

@Composable
private fun TypingDots() {
    val transition = rememberInfiniteTransition(label = "assistant-typing")
    Row(
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        repeat(3) { index ->
            val offset = index * 140
            val alpha by transition.animateFloat(
                initialValue = 0.25f,
                targetValue = 1f,
                animationSpec = infiniteRepeatable(
                    animation = keyframes {
                        durationMillis = 840
                        0.25f at 0
                        0.25f at offset
                        1f at offset + 180 using FastOutSlowInEasing
                        0.25f at offset + 360
                    },
                    repeatMode = RepeatMode.Restart,
                ),
                label = "typing-dot-$index",
            )
            Box(
                modifier = Modifier
                    .size(8.dp)
                    .alpha(alpha)
                    .background(
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        shape = CircleShape,
                    ),
            )
        }
    }
}

@Composable
private fun SystemMessageCard(
    card: SystemCardPresentation,
) {
    val colors = systemToneColors(card.tone)

    Column(
        modifier = Modifier.fillMaxWidth(),
        horizontalAlignment = Alignment.Start,
    ) {
        Surface(
            modifier = Modifier.fillMaxWidth(0.94f),
            color = colors.container,
            contentColor = colors.content,
            shape = MaterialTheme.shapes.large,
        ) {
            Row(
                modifier = Modifier.padding(horizontal = 14.dp, vertical = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = Alignment.Top,
            ) {
                Surface(
                    shape = CircleShape,
                    color = colors.iconContainer,
                    contentColor = colors.iconTint,
                ) {
                    Box(
                        modifier = Modifier
                            .size(32.dp)
                            .padding(6.dp),
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(
                            imageVector = systemIcon(card.icon),
                            contentDescription = null,
                        )
                    }
                }
                Column(
                    modifier = Modifier.weight(1f),
                    verticalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    Text(
                        text = card.title,
                        style = MaterialTheme.typography.titleSmall,
                    )
                    Text(
                        text = card.body,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    card.supportingLines.forEach { line ->
                        Text(
                            text = line,
                            style = MaterialTheme.typography.bodySmall,
                            color = colors.supporting,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun systemToneColors(tone: SystemTone): SystemToneColors =
    when (tone) {
        SystemTone.Neutral -> SystemToneColors(
            container = MaterialTheme.colorScheme.surfaceContainerHigh,
            content = MaterialTheme.colorScheme.onSurface,
            supporting = MaterialTheme.colorScheme.onSurfaceVariant,
            iconContainer = MaterialTheme.colorScheme.surfaceContainerHighest,
            iconTint = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        SystemTone.Progress -> SystemToneColors(
            container = MaterialTheme.colorScheme.secondaryContainer,
            content = MaterialTheme.colorScheme.onSecondaryContainer,
            supporting = MaterialTheme.colorScheme.onSecondaryContainer.copy(alpha = 0.8f),
            iconContainer = MaterialTheme.colorScheme.secondary,
            iconTint = MaterialTheme.colorScheme.onSecondary,
        )

        SystemTone.Positive -> SystemToneColors(
            container = MaterialTheme.colorScheme.tertiaryContainer,
            content = MaterialTheme.colorScheme.onTertiaryContainer,
            supporting = MaterialTheme.colorScheme.onTertiaryContainer.copy(alpha = 0.8f),
            iconContainer = MaterialTheme.colorScheme.tertiary,
            iconTint = MaterialTheme.colorScheme.onTertiary,
        )

        SystemTone.Warning -> SystemToneColors(
            container = MaterialTheme.colorScheme.tertiaryContainer,
            content = MaterialTheme.colorScheme.onTertiaryContainer,
            supporting = MaterialTheme.colorScheme.onTertiaryContainer.copy(alpha = 0.8f),
            iconContainer = MaterialTheme.colorScheme.tertiary,
            iconTint = MaterialTheme.colorScheme.onTertiary,
        )

        SystemTone.Error -> SystemToneColors(
            container = MaterialTheme.colorScheme.errorContainer,
            content = MaterialTheme.colorScheme.onErrorContainer,
            supporting = MaterialTheme.colorScheme.onErrorContainer.copy(alpha = 0.8f),
            iconContainer = MaterialTheme.colorScheme.error,
            iconTint = MaterialTheme.colorScheme.onError,
        )
    }

private fun systemIcon(icon: SystemIcon): ImageVector =
    when (icon) {
        SystemIcon.Info -> Icons.Rounded.Info
        SystemIcon.Assistant -> Icons.Rounded.SmartToy
        SystemIcon.Spotify -> Icons.Rounded.MusicNote
        SystemIcon.Hue -> Icons.Rounded.Lightbulb
        SystemIcon.Device -> Icons.Rounded.Speaker
        SystemIcon.Warning -> Icons.Rounded.WarningAmber
        SystemIcon.Error -> Icons.Rounded.ErrorOutline
        SystemIcon.Success -> Icons.Rounded.CheckCircleOutline
    }

private data class SystemToneColors(
    val container: Color,
    val content: Color,
    val supporting: Color,
    val iconContainer: Color,
    val iconTint: Color,
)

@Composable
private fun OverviewScreen(
    uiState: BabyGervaiseUiState,
    onPreviousContextChanged: (ContextLevel) -> Unit,
    onCloudProfileChanged: (String) -> Unit,
    onBeginToolAuth: (String) -> Unit,
    onRefreshToolState: (String) -> Unit,
    onDisconnectTool: (String) -> Unit,
) {
    val overview = uiState.overviewSnapshot
    LazyColumn(
        modifier = Modifier
            .fillMaxSize()
            .testTag("overview-screen"),
        contentPadding = PaddingValues(16.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        item(key = "overview-status") {
            OverviewSection(title = "Status") {
                SectionSurface {
                    Column(
                        modifier = Modifier.padding(16.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        Text(
                            text = deriveOverviewStatus(uiState),
                            style = MaterialTheme.typography.titleMedium,
                        )
                        Text(
                            text = "Overview is the control plane. Chat stays just Gervaise.",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
        }

        item(key = "overview-models") {
            RuntimeModelSection(
                overview = overview,
                onCloudProfileChanged = onCloudProfileChanged,
            )
        }

        item(key = "overview-memory") {
            MemoryOverviewSection(
                uiState = uiState,
                onPreviousContextChanged = onPreviousContextChanged,
            )
        }

        item(key = "overview-tools") {
            ToolsOverviewSection(
                overview = overview,
                uiState = uiState,
                onBeginToolAuth = onBeginToolAuth,
                onRefreshToolState = onRefreshToolState,
                onDisconnectTool = onDisconnectTool,
            )
        }

        item(key = "overview-diagnostics") {
            DiagnosticsOverviewSection(overview = overview)
        }

        item(key = "overview-system") {
            SystemOverviewSection(uiState = uiState)
        }
    }
}

@Composable
private fun MemoryOverviewSection(
    uiState: BabyGervaiseUiState,
    onPreviousContextChanged: (ContextLevel) -> Unit,
) {
    val overview = uiState.overviewSnapshot
    OverviewSection(title = "Memory") {
        Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            SectionSurface {
                Column(
                    modifier = Modifier.padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    SummaryValueRow(label = "Messages", value = overview.memoryStats.messageCount.toString())
                    SummaryValueRow(label = "Stored memories", value = overview.memoryStats.storedMemories.toString())
                    SummaryValueRow(label = "Vectors", value = overview.memoryStats.vectorCount.toString())
                    SummaryValueRow(label = "Retrievals", value = overview.memoryStats.retrievalCount.toString())
                }
            }
            PreviousContextSelector(
                selected = uiState.bootstrapState.previousContext,
                onSelected = onPreviousContextChanged,
            )
        }
    }
}

@Composable
private fun ToolsOverviewSection(
    overview: OverviewSnapshot,
    uiState: BabyGervaiseUiState,
    onBeginToolAuth: (String) -> Unit,
    onRefreshToolState: (String) -> Unit,
    onDisconnectTool: (String) -> Unit,
) {
    OverviewSection(title = "Tools") {
        Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            SectionSurface {
                Column(
                    modifier = Modifier.padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    SummaryValueRow(
                        label = "Available",
                        value = overview.tools.availableTools.joinToString().ifBlank { "None" },
                    )
                    SummaryValueRow(
                        label = "Integrated",
                        value = overview.tools.integratedTools.joinToString().ifBlank { "None" },
                    )
                    SummaryValueRow(
                        label = "Catalog size",
                        value = overview.tools.catalog.size.toString(),
                    )
                }
            }
            overview.tools.catalog.forEach { tool ->
                ToolOverviewCard(
                    tool = tool,
                    isBusy = uiState.activeToolStatus?.tool == tool.toolId,
                    onBeginToolAuth = onBeginToolAuth,
                    onRefreshToolState = onRefreshToolState,
                    onDisconnectTool = onDisconnectTool,
                )
            }
        }
    }
}

@Composable
private fun DiagnosticsOverviewSection(
    overview: OverviewSnapshot,
) {
    var showAdvanced by rememberSaveable { mutableStateOf(false) }
    OverviewSection(title = "Runtime / Diagnostics") {
        Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
            SectionSurface {
                Column(
                    modifier = Modifier.padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Text(
                        text = "Recent routes",
                        style = MaterialTheme.typography.titleMedium,
                    )
                    if (overview.diagnostics.turnSummaries.isEmpty()) {
                        Text(
                            text = "No turn traces yet.",
                            style = MaterialTheme.typography.bodyMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    } else {
                        overview.diagnostics.turnSummaries.take(5).forEach { summary ->
                            TurnTimelineEntry(summary = summary)
                        }
                    }
                    TextButton(onClick = { showAdvanced = !showAdvanced }) {
                        Text(if (showAdvanced) "Hide advanced diagnostics" else "Show advanced diagnostics")
                    }
                }
            }

            if (overview.diagnostics.issues.isNotEmpty()) {
                SectionSurface {
                    Column(
                        modifier = Modifier.padding(16.dp),
                        verticalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        Text(
                            text = "Recent issues",
                            style = MaterialTheme.typography.titleMedium,
                        )
                        overview.diagnostics.issues.take(6).forEach { issue ->
                            DiagnosticIssueCard(issue)
                        }
                    }
                }
            }

            if (showAdvanced) {
                AdvancedDiagnosticsSection(overview = overview)
            }
        }
    }
}

@Composable
private fun SystemOverviewSection(
    uiState: BabyGervaiseUiState,
) {
    val overview = uiState.overviewSnapshot
    OverviewSection(title = "System") {
        SectionSurface {
            Column(
                modifier = Modifier.padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                SummaryValueRow(label = "Interactions", value = overview.systemStats.totalInteractions.toString())
                SummaryValueRow(label = "Tool calls", value = overview.systemStats.toolCalls.toString())
                SummaryValueRow(label = "Errors", value = overview.systemStats.errorCount.toString())
                SummaryValueRow(label = "Core ready", value = uiState.isCoreReady.toString())
                SummaryValueRow(label = "Pending turn", value = (uiState.pendingTurnId != null).toString())
                SummaryValueRow(label = "Status", value = deriveOverviewStatus(uiState))
                SummaryValueRow(
                    label = "Cloud profile",
                    value = overview.runtime.selectedCloudProfileLabel ?: "None",
                )
            }
        }
    }
}

@Composable
private fun ToolOverviewCard(
    tool: ToolOverviewEntry,
    isBusy: Boolean,
    onBeginToolAuth: (String) -> Unit,
    onRefreshToolState: (String) -> Unit,
    onDisconnectTool: (String) -> Unit,
) {
    SectionSurface {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(
                    text = tool.displayName,
                    style = MaterialTheme.typography.titleMedium,
                )
                Text(
                    text = tool.summary,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            SummaryValueRow(label = "Available", value = tool.available.toString())
            SummaryValueRow(label = "Integrated", value = tool.integrated.toString())
            SummaryValueRow(label = "Auth", value = tool.authState)
            SummaryValueRow(label = "Health", value = tool.healthState)
            SummaryValueRow(label = "Next step", value = tool.nextStep)
            tool.accountLabel?.let { SummaryValueRow(label = "Account", value = it) }
            tool.capabilitySummary?.let { SummaryValueRow(label = "Capability", value = it) }
            tool.detailLines.forEach { detail ->
                SummaryValueRow(label = detail.label, value = detail.value)
            }
            if (tool.actions.isNotEmpty()) {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    tool.actions.forEach { action ->
                        OutlinedButton(
                            onClick = {
                                when (action.actionId) {
                                    "begin_auth" -> onBeginToolAuth(tool.toolId)
                                    "refresh_state" -> onRefreshToolState(tool.toolId)
                                    "disconnect" -> onDisconnectTool(tool.toolId)
                                }
                            },
                            enabled = action.enabled && !isBusy,
                        ) {
                            Text(action.label)
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun AdvancedDiagnosticsSection(
    overview: OverviewSnapshot,
) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        if (overview.diagnostics.modelTraces.isNotEmpty()) {
            SectionSurface {
                Column(
                    modifier = Modifier.padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Text(text = "Model traces", style = MaterialTheme.typography.titleMedium)
                    overview.diagnostics.modelTraces.take(4).forEach { entry ->
                        ModelTraceCard(entry)
                    }
                }
            }
        }
        if (overview.diagnostics.decisionEvents.isNotEmpty()) {
            SectionSurface {
                Column(
                    modifier = Modifier.padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Text(text = "Decision gates", style = MaterialTheme.typography.titleMedium)
                    overview.diagnostics.decisionEvents.take(4).forEach { entry ->
                        DecisionGateEntry(entry)
                    }
                }
            }
        }
        if (overview.diagnostics.recentLogs.isNotEmpty()) {
            SectionSurface {
                Column(
                    modifier = Modifier.padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Text(text = "Raw model logs", style = MaterialTheme.typography.titleMedium)
                    overview.diagnostics.recentLogs.take(3).forEach { entry ->
                        LogEntry(entry)
                    }
                }
            }
        }
    }
}

@Composable
private fun DiagnosticIssueCard(
    issue: DiagnosticIssue,
) {
    SectionSurface {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Text(
                text = issue.summary,
                style = MaterialTheme.typography.titleSmall,
            )
            Text(
                text = "${issue.subsystem} • ${issue.level} • ${issue.timestamp}",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            issue.detail?.let { detail ->
                Text(
                    text = detail,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

private fun deriveOverviewStatus(uiState: BabyGervaiseUiState): String =
    when {
        uiState.initializationError != null -> "Needs attention"
        !uiState.isCoreReady -> "Starting up"
        uiState.pendingTurnId != null -> "Busy"
        uiState.activeToolStatus?.status == "executing" -> "Busy"
        uiState.overviewSnapshot.runtime.nano.availability == "downloading" -> "Starting up"
        uiState.overviewSnapshot.runtime.nano.availability in setOf("error", "unavailable", "disabled") -> "Needs attention"
        else -> "Healthy"
    }

@Composable
private fun TurnTimelineEntry(
    summary: TurnTraceSummary,
) {
    SectionSurface {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                text = summary.userInputSummary.ifBlank { "Turn ${summary.turnId}" },
                style = MaterialTheme.typography.titleSmall,
            )
            TraceValueRow(label = "Input", value = summary.inputSource)
            TraceValueRow(label = "Plan", value = summary.planKind)
            TraceValueRow(label = "Context", value = summary.contextPolicy ?: "none")
            TraceValueRow(label = "Memory used", value = summary.memoryUsed.toString())
            TraceValueRow(label = "Tool consulted", value = summary.toolConsulted.toString())
            TraceValueRow(label = "Tool used", value = summary.toolUsed.toString())
            TraceValueRow(label = "Nano first beat", value = summary.nanoFirstBeatUsed.toString())
            TraceValueRow(label = "Cloud escalated", value = summary.cloudEscalated.toString())
            TraceValueRow(label = "Cloud profile", value = summary.selectedCloudProfile ?: "none")
            TraceValueRow(label = "Delivery", value = summary.deliveryMode)
            TraceValueRow(
                label = "Route",
                value = summary.finalRoute,
            )
            TraceValueRow(
                label = "Stages",
                value = summary.modelStages.joinToString(", ").ifBlank { "none" },
            )
            TraceValueRow(label = "Latency", value = "${summary.totalLatencyMs}ms")
            summary.errorSummary?.let { errorSummary ->
                TraceValueRow(label = "Error", value = errorSummary)
            }
            if (summary.finalVisibleOutput.isNotBlank()) {
                Text(
                    text = summary.finalVisibleOutput,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun DecisionGateEntry(
    entry: DecisionTraceEntry,
) {
    SectionSurface {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                text = entry.name,
                style = MaterialTheme.typography.titleSmall,
            )
            TraceValueRow(label = "Turn", value = entry.turnId)
            TraceValueRow(label = "Plan", value = entry.planKind ?: "unplanned")
            entry.fallbackPlanKind?.let { fallback ->
                TraceValueRow(label = "Fallback", value = fallback)
            }
            entry.detail?.let { detail ->
                TraceValueRow(label = "Detail", value = detail)
            }
            if (entry.reasonCodes.isNotEmpty()) {
                Text(
                    text = entry.reasonCodes.joinToString(", "),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun ModelTraceCard(
    entry: ModelTraceEntry,
) {
    SectionSurface {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Text(
                text = entry.stageName,
                style = MaterialTheme.typography.titleSmall,
            )
            TraceValueRow(label = "Turn", value = entry.turnId)
            TraceValueRow(
                label = "Lane",
                value = listOfNotNull(entry.lane, entry.provider, entry.model).joinToString(" / ").ifBlank {
                    "unknown"
                },
            )
            TraceValueRow(
                label = "Mode",
                value = entry.promptMode ?: "unspecified",
            )
            TraceValueRow(
                label = "Status",
                value = "${entry.status} • ${entry.latencyMs}ms",
            )
            entry.displayedText?.takeIf { it.isNotBlank() }?.let { displayed ->
                Text(
                    text = displayed,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
            entry.discardedText?.takeIf { it.isNotBlank() }?.let { discarded ->
                Text(
                    text = "Discarded: $discarded",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            entry.normalizedOutput?.takeIf { it.isNotBlank() && it != entry.displayedText }?.let { normalized ->
                Text(
                    text = "Normalized: $normalized",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            entry.rawInput?.takeIf { it.isNotBlank() }?.let { rawInput ->
                Text(
                    text = "Input: $rawInput",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            entry.rawOutput?.takeIf { it.isNotBlank() }?.let { rawOutput ->
                Text(
                    text = "Output: $rawOutput",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun TraceValueRow(
    label: String,
    value: String,
) {
    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(
            text = label,
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            text = value,
            style = MaterialTheme.typography.bodyMedium,
        )
    }
}

@Composable
private fun SummaryValueRow(
    label: String,
    value: String,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        verticalAlignment = Alignment.Top,
    ) {
        Text(
            text = label,
            modifier = Modifier.weight(0.38f),
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            text = value,
            modifier = Modifier.weight(0.62f),
            style = MaterialTheme.typography.bodyMedium,
            textAlign = TextAlign.End,
        )
    }
}

@Composable
private fun RuntimeModelSection(
    overview: io.gervaise.babygervaise.bridge.OverviewSnapshot,
    onCloudProfileChanged: (String) -> Unit,
) {
    var expanded by remember { mutableStateOf(false) }
    val selectedLabel = overview.runtime.selectedCloudProfileLabel ?: "Unavailable"
    val selectedProfile = overview.runtime.cloudProfiles.firstOrNull { it.selected }
    val selectedProfileStatus = when {
        selectedProfile == null -> "No cloud profile configured."
        selectedProfile.available ->
            "${selectedProfile.provider} / ${selectedProfile.model} is ready."
        else -> "${selectedProfile.label} is not ready. Check its API key."
    }

    OverviewSection(title = "Models") {
        SectionSurface {
            Column(
                modifier = Modifier.padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(
                        text = "Nano",
                        style = MaterialTheme.typography.titleMedium,
                    )
                    Text(
                            text = when (overview.runtime.nano.availability) {
                                "available" -> "Always first on-device layer"
                                "downloading" -> "Preparing the on-device first beat"
                                else -> if (hasConversationLane(overview)) {
                                    "On-device layer is warming up. Cloud conversation is available."
                                } else {
                                    "On-device or cloud conversation needs to come online."
                                }
                            },
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    SummaryValueRow(label = "Status", value = overview.runtime.nano.detail)
                    UsageTelemetryRows(stats = overview.nanoStats)
                }

                HorizontalDivider()

                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(
                        text = "Cloud model",
                        style = MaterialTheme.typography.titleMedium,
                    )
                    Box {
                        OutlinedButton(
                            onClick = { expanded = true },
                            enabled = overview.runtime.cloudProfiles.isNotEmpty(),
                            modifier = Modifier.testTag("cloud-profile-button"),
                        ) {
                            Text(selectedLabel)
                        }
                        DropdownMenu(
                            expanded = expanded,
                            onDismissRequest = { expanded = false },
                        ) {
                            overview.runtime.cloudProfiles.forEach { profile ->
                                DropdownMenuItem(
                                    text = { Text(profile.label) },
                                    onClick = {
                                        expanded = false
                                        onCloudProfileChanged(profile.id)
                                    },
                                    enabled = profile.available,
                                )
                            }
                        }
                    }
                    Text(
                        text = "Model details stay in Overview. Chat stays just Gervaise.",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    Text(
                        text = selectedProfileStatus,
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    UsageTelemetryRows(stats = overview.cloudStats)
                }
            }
        }
    }
}

@Composable
private fun UsageTelemetryRows(
    stats: io.gervaise.babygervaise.bridge.UsageStats,
) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        SummaryValueRow(label = "Requests", value = stats.calls.toString())
        SummaryValueRow(label = "Last latency", value = formatLatency(stats.latencyLatestMs))
        SummaryValueRow(label = "Average latency", value = formatLatency(stats.latencyAvgMs))
        stats.tokensPerSecond?.let { tokensPerSecond ->
            SummaryValueRow(label = "Tokens/sec", value = tokensPerSecond.toString())
        }
    }
}

private fun formatLatency(latencyMs: Long?): String =
    latencyMs?.takeIf { it > 0 }?.let { "${it}ms" } ?: "n/a"

@Composable
private fun OverviewSection(
    title: String,
    content: @Composable () -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(
            text = title,
            style = MaterialTheme.typography.titleMedium,
        )
        content()
    }
}

@Composable
private fun SectionSurface(
    content: @Composable ColumnScope.() -> Unit,
) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainerLow,
        shape = MaterialTheme.shapes.large,
    ) {
        Column(content = content)
    }
}

@Composable
private fun PreviousContextSelector(
    selected: ContextLevel,
    onSelected: (ContextLevel) -> Unit,
) {
    var expanded by remember { mutableStateOf(false) }

    OverviewSection(title = "Previous Context") {
        Box {
            OutlinedButton(
                onClick = { expanded = true },
                modifier = Modifier.testTag("previous-context-button"),
            ) {
                Text(selected.displayName)
            }
            DropdownMenu(
                expanded = expanded,
                onDismissRequest = { expanded = false },
            ) {
                ContextLevel.entries.forEach { level ->
                    DropdownMenuItem(
                        text = { Text(level.displayName) },
                        onClick = {
                            expanded = false
                            onSelected(level)
                        },
                    )
                }
            }
        }
    }
}

@Composable
private fun LogEntry(entry: LogViewerEntry) {
    var expanded by remember { mutableStateOf(false) }

    Surface(
        color = MaterialTheme.colorScheme.surfaceContainerLow,
        shape = MaterialTheme.shapes.large,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .clickable { expanded = !expanded }
                .padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(
                text = entry.timestamp,
                style = MaterialTheme.typography.titleSmall,
            )
            Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                Text(
                    text = "${entry.latencyMs}ms",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    text = "Status ${entry.status ?: "n/a"}",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (expanded) {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(
                        text = "Prompt",
                        style = MaterialTheme.typography.labelLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    JsonPanel(content = entry.prompt)
                    Text(
                        text = "Output",
                        style = MaterialTheme.typography.labelLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    JsonPanel(content = entry.rawOutput)
                }
            }
        }
    }
}

@Composable
private fun JsonPanel(content: String) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        color = MaterialTheme.colorScheme.surfaceContainerHighest,
        shape = MaterialTheme.shapes.medium,
    ) {
        Text(
            text = content,
            modifier = Modifier.padding(16.dp),
            style = MaterialTheme.typography.bodyMedium,
            fontFamily = FontFamily.Monospace,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
