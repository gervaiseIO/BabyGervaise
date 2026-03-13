package io.gervaise.babygervaise

import androidx.compose.animation.AnimatedVisibility
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
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
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
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import io.gervaise.babygervaise.bridge.ChatMessage
import io.gervaise.babygervaise.bridge.ContextLevel
import io.gervaise.babygervaise.bridge.CoreJson
import io.gervaise.babygervaise.bridge.LogViewerEntry
import kotlinx.coroutines.launch

@Composable
fun BabyGervaiseRoute(
    viewModel: BabyGervaiseViewModel = viewModel(),
) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
    val snackbarHostState = remember { SnackbarHostState() }

    LaunchedEffect(uiState.snackbarMessage) {
        val message = uiState.snackbarMessage ?: return@LaunchedEffect
        snackbarHostState.showSnackbar(message)
        viewModel.consumeSnackbar()
    }

    BabyGervaiseApp(
        uiState = uiState,
        snackbarHostState = snackbarHostState,
        onDraftChanged = viewModel::updateDraft,
        onSubmit = viewModel::submitDraft,
        onToggleScreen = viewModel::toggleScreen,
        onPreviousContextChanged = viewModel::updatePreviousContext,
    )
}

@Composable
fun BabyGervaiseApp(
    uiState: BabyGervaiseUiState,
    snackbarHostState: SnackbarHostState,
    onDraftChanged: (String) -> Unit,
    onSubmit: () -> Unit,
    onToggleScreen: () -> Unit,
    onPreviousContextChanged: (ContextLevel) -> Unit,
) {
    val messages = uiState.bootstrapState.messages
    val timelineState = rememberLazyListState()
    val scope = rememberCoroutineScope()
    val isChatScreen = uiState.screen == Screen.CHAT

    LaunchedEffect(messages.size, uiState.pendingTurnId, uiState.screen) {
        if (isChatScreen && messages.isNotEmpty()) {
            timelineState.animateScrollToItem(messages.lastIndex)
        }
    }

    Scaffold(
        containerColor = MaterialTheme.colorScheme.background,
        topBar = {
            AppTopBar(
                isChat = isChatScreen,
                onToggleScreen = onToggleScreen,
            )
        },
        bottomBar = {
            if (isChatScreen) {
                ChatComposerBar(
                    uiState = uiState,
                    onDraftChanged = onDraftChanged,
                    onSubmit = onSubmit,
                    isComposerEnabled = uiState.isCoreReady,
                    onFocused = {
                        scope.launch {
                            if (messages.isNotEmpty()) {
                                timelineState.animateScrollToItem(messages.lastIndex)
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
                Screen.CHAT -> ChatScreen(
                    uiState = uiState,
                    timelineState = timelineState,
                )

                Screen.OVERVIEW -> OverviewScreen(
                    uiState = uiState,
                    onPreviousContextChanged = onPreviousContextChanged,
                )
            }

            AnimatedVisibility(
                modifier = Modifier
                    .align(Alignment.Center)
                    .padding(24.dp),
                visible = uiState.isInitializing && messages.isEmpty(),
            ) {
                CircularProgressIndicator()
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun AppTopBar(
    isChat: Boolean,
    onToggleScreen: () -> Unit,
) {
    TopAppBar(
        title = {
            Text(
                text = "Baby Gervaise",
                style = MaterialTheme.typography.titleLarge,
            )
        },
        actions = {
            TextButton(onClick = onToggleScreen) {
                Text(if (isChat) "Overview" else "Chat")
            }
        },
    )
}

@Composable
private fun ChatScreen(
    uiState: BabyGervaiseUiState,
    timelineState: LazyListState,
) {
    LazyColumn(
        state = timelineState,
        modifier = Modifier
            .fillMaxSize()
            .testTag("timeline"),
        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        if (uiState.bootstrapState.messages.isEmpty()) {
            item(key = "empty-state") {
                EmptyConversationState()
            }
        }

        items(
            items = uiState.bootstrapState.messages,
            key = { message -> "${message.turnId}-${message.id}-${message.role}" },
        ) { message ->
            MessageBubble(
                message = message,
                isPending = uiState.pendingTurnId == message.turnId,
            )
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
    Surface(tonalElevation = 2.dp) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .imePadding()
                .navigationBarsPadding()
                .padding(horizontal = 16.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(
                text = "Previous context: ${uiState.bootstrapState.previousContext.asStatusLabel()}",
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                text = uiState.statusText,
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
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
        modifier = Modifier.fillMaxWidth(),
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
        Button(
            onClick = onSubmit,
            enabled = isEnabled && !isPending && draft.isNotBlank(),
            modifier = Modifier.testTag("send-button"),
        ) {
            Text(if (isPending) "Sending..." else "Send")
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
private fun MessageBubble(
    message: ChatMessage,
    isPending: Boolean,
) {
    val isUserMessage = message.role == "user"
    val containerColor = when (message.role) {
        "user" -> MaterialTheme.colorScheme.primaryContainer
        "tool" -> MaterialTheme.colorScheme.surfaceContainerHighest
        else -> MaterialTheme.colorScheme.surfaceContainerLow
    }
    val widthFraction = when (message.role) {
        "user" -> 0.84f
        else -> 0.92f
    }
    val contentColor = when (message.role) {
        "user" -> MaterialTheme.colorScheme.onPrimaryContainer
        else -> MaterialTheme.colorScheme.onSurface
    }

    Column(
        modifier = Modifier.fillMaxWidth(),
        horizontalAlignment = if (isUserMessage) Alignment.End else Alignment.Start,
    ) {
        Surface(
            modifier = Modifier.fillMaxWidth(widthFraction),
            color = containerColor,
            contentColor = contentColor,
            shape = MaterialTheme.shapes.large,
        ) {
            Column(
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                if (message.role != "user" && message.role != "assistant") {
                    Text(
                        text = message.role,
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Text(
                    text = message.content.ifBlank { if (isPending) "…" else "" },
                    style = MaterialTheme.typography.bodyLarge,
                )
            }
        }
    }
}

@Composable
private fun OverviewScreen(
    uiState: BabyGervaiseUiState,
    onPreviousContextChanged: (ContextLevel) -> Unit,
) {
    LazyColumn(
        modifier = Modifier
            .fillMaxSize()
            .testTag("overview-screen")
            .padding(horizontal = 16.dp),
        contentPadding = PaddingValues(vertical = 16.dp),
        verticalArrangement = Arrangement.spacedBy(24.dp),
    ) {
        item(key = "overview-metrics") {
            OverviewSection(title = "Overview") {
                SectionSurface {
                    MetricGroup(
                        title = "Model",
                        lines = listOf(
                            "Name: ${uiState.overviewSnapshot.modelStats.modelName}",
                            "Requests: ${uiState.overviewSnapshot.modelStats.totalRequests}",
                            "Tokens in/out: ${uiState.overviewSnapshot.modelStats.totalInputTokens} / ${uiState.overviewSnapshot.modelStats.totalOutputTokens}",
                            "Latency avg/latest: ${uiState.overviewSnapshot.modelStats.averageLatencyMs}ms / ${uiState.overviewSnapshot.modelStats.latestLatencyMs}ms",
                        ),
                    )
                    HorizontalDivider()
                    MetricGroup(
                        title = "Memory",
                        lines = listOf(
                            "Messages: ${uiState.overviewSnapshot.memoryStats.messageCount}",
                            "Stored memories: ${uiState.overviewSnapshot.memoryStats.storedMemories}",
                            "Vectors: ${uiState.overviewSnapshot.memoryStats.vectorCount}",
                            "Retrievals: ${uiState.overviewSnapshot.memoryStats.retrievalCount}",
                        ),
                    )
                    HorizontalDivider()
                    MetricGroup(
                        title = "System",
                        lines = listOf(
                            "Interactions: ${uiState.overviewSnapshot.systemStats.totalInteractions}",
                            "Tool calls: ${uiState.overviewSnapshot.systemStats.toolCalls}",
                            "Errors: ${uiState.overviewSnapshot.systemStats.errorCount}",
                        ),
                    )
                }
            }
        }

        item(key = "previous-context") {
            PreviousContextSelector(
                selected = uiState.overviewSnapshot.previousContext,
                onSelected = onPreviousContextChanged,
            )
        }

        item(key = "tool-state") {
            OverviewSection(title = "Tool State") {
                if (uiState.overviewSnapshot.toolStates.isEmpty()) {
                    Text(
                        text = "No tool state recorded yet.",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                } else {
                    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        uiState.overviewSnapshot.toolStates.forEach { (key, value) ->
                            ToolStateItem(
                                key = key,
                                content = CoreJson.prettyPrint(value),
                            )
                        }
                    }
                }
            }
        }

        item(key = "logs") {
            OverviewSection(title = "Raw Model Logs") {
                if (uiState.overviewSnapshot.recentLogs.isEmpty()) {
                    Text(
                        text = "No model logs yet.",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                } else {
                    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        uiState.overviewSnapshot.recentLogs.forEach { entry ->
                            LogEntry(entry)
                        }
                    }
                }
            }
        }
    }
}

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
private fun MetricGroup(
    title: String,
    lines: List<String>,
) {
    Column(
        modifier = Modifier.padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Text(
            text = title,
            style = MaterialTheme.typography.titleSmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        lines.forEach { line ->
            Text(
                text = line,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
    }
}

@Composable
private fun ToolStateItem(
    key: String,
    content: String,
) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(
            text = key,
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        JsonPanel(content = content)
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
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
