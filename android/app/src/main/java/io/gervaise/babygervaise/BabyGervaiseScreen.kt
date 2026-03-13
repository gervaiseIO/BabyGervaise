package io.gervaise.babygervaise

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.sizeIn
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
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
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
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

    LaunchedEffect(messages.size, uiState.pendingTurnId, uiState.screen) {
        if (uiState.screen == Screen.CHAT && messages.isNotEmpty()) {
            timelineState.animateScrollToItem(messages.lastIndex)
        }
    }

    Scaffold(
        containerColor = Color.Transparent,
        contentWindowInsets = WindowInsets(0, 0, 0, 0),
        snackbarHost = { SnackbarHost(hostState = snackbarHostState) },
    ) { innerPadding ->
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(
                    brush = Brush.verticalGradient(
                        colors = listOf(
                            Color(0xFFF6F0E8),
                            Color(0xFFEFE4D2),
                        ),
                    ),
                )
                .padding(innerPadding),
        ) {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .statusBarsPadding()
                    .navigationBarsPadding()
                    .padding(horizontal = 18.dp, vertical = 12.dp),
            ) {
                Header(
                    isChat = uiState.screen == Screen.CHAT,
                    onToggleScreen = onToggleScreen,
                )
                Spacer(modifier = Modifier.height(18.dp))
                when (uiState.screen) {
                    Screen.CHAT -> ChatScreen(
                        uiState = uiState,
                        timelineState = timelineState,
                        onDraftChanged = onDraftChanged,
                        onSubmit = onSubmit,
                        onComposerFocused = {
                            scope.launch {
                                if (messages.isNotEmpty()) {
                                    timelineState.animateScrollToItem(messages.lastIndex)
                                }
                            }
                        },
                    )

                    Screen.OVERVIEW -> OverviewScreen(
                        uiState = uiState,
                        onPreviousContextChanged = onPreviousContextChanged,
                    )
                }
            }

            AnimatedVisibility(
                modifier = Modifier
                    .align(Alignment.Center)
                    .padding(24.dp),
                visible = uiState.isInitializing && messages.isEmpty(),
            ) {
                CircularProgressIndicator(color = MaterialTheme.colorScheme.primary)
            }
        }
    }
}

@Composable
private fun Header(
    isChat: Boolean,
    onToggleScreen: () -> Unit,
) {
    Column {
        Text(
            text = "Continuous Intelligence Prototype",
            style = MaterialTheme.typography.labelSmall,
            color = MaterialTheme.colorScheme.secondary,
        )
        Spacer(modifier = Modifier.height(10.dp))
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.Bottom,
        ) {
            Text(
                text = "Baby Gervaise",
                style = MaterialTheme.typography.headlineLarge,
                color = MaterialTheme.colorScheme.onBackground,
                modifier = Modifier.weight(1f),
            )
            Spacer(modifier = Modifier.width(12.dp))
            Button(
                onClick = onToggleScreen,
                shape = RoundedCornerShape(999.dp),
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 12.dp),
            ) {
                Text(if (isChat) "Overview" else "Back to Chat")
            }
        }
    }
}

@Composable
private fun ChatScreen(
    uiState: BabyGervaiseUiState,
    timelineState: androidx.compose.foundation.lazy.LazyListState,
    onDraftChanged: (String) -> Unit,
    onSubmit: () -> Unit,
    onComposerFocused: () -> Unit,
) {
    Column(
        modifier = Modifier.fillMaxSize(),
    ) {
        LazyColumn(
            state = timelineState,
            modifier = Modifier
                .weight(1f)
                .testTag("timeline"),
            verticalArrangement = Arrangement.spacedBy(14.dp),
            contentPadding = PaddingValues(bottom = 12.dp),
        ) {
            if (uiState.bootstrapState.messages.isEmpty()) {
                item(key = "empty-state") {
                    ElevatedPanel(modifier = Modifier.fillMaxWidth()) {
                        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
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

        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = 8.dp, bottom = 10.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(
                text = "Previous Context: ${uiState.bootstrapState.previousContext.asStatusLabel()}",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                text = uiState.statusText,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        Composer(
            draft = uiState.draft,
            isPending = uiState.isPending,
            onDraftChanged = onDraftChanged,
            onSubmit = onSubmit,
            onFocused = onComposerFocused,
        )
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun Composer(
    draft: String,
    isPending: Boolean,
    onDraftChanged: (String) -> Unit,
    onSubmit: () -> Unit,
    onFocused: () -> Unit,
) {
    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .imePadding(),
        color = Color.Transparent,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .background(
                    brush = Brush.verticalGradient(
                        colors = listOf(Color.Transparent, Color(0xFFF6F0E8)),
                    ),
                )
                .padding(top = 10.dp, bottom = 4.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            OutlinedTextField(
                value = draft,
                onValueChange = onDraftChanged,
                modifier = Modifier
                    .fillMaxWidth()
                    .sizeIn(minHeight = 112.dp)
                    .testTag("composer-input")
                    .onFocusChanged {
                        if (it.isFocused) {
                            onFocused()
                        }
                    },
                textStyle = MaterialTheme.typography.bodyLarge,
                placeholder = {
                    Text("Tell Gervaise what you need.")
                },
                keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                shape = RoundedCornerShape(22.dp),
                minLines = 3,
            )
            Button(
                onClick = onSubmit,
                enabled = !isPending,
                shape = RoundedCornerShape(999.dp),
                modifier = Modifier
                    .align(Alignment.End)
                    .testTag("send-button"),
            ) {
                Text(if (isPending) "Sending..." else "Send")
            }
        }
    }
}

@Composable
private fun MessageBubble(
    message: ChatMessage,
    isPending: Boolean,
) {
    val horizontalAlignment = when (message.role) {
        "user" -> Alignment.End
        else -> Alignment.Start
    }
    val backgroundColor = when (message.role) {
        "user" -> MaterialTheme.colorScheme.primaryContainer
        "tool" -> Color(0xFFE0EFE2)
        else -> MaterialTheme.colorScheme.surface.copy(alpha = 0.92f)
    }
    val widthFraction = when (message.role) {
        "user" -> 0.84f
        "tool" -> 0.78f
        else -> 0.90f
    }

    Column(
        modifier = Modifier.fillMaxWidth(),
        horizontalAlignment = horizontalAlignment,
    ) {
        Card(
            modifier = Modifier.fillMaxWidth(widthFraction),
            shape = RoundedCornerShape(24.dp),
            colors = CardDefaults.cardColors(containerColor = backgroundColor),
        ) {
            Column(
                modifier = Modifier.padding(horizontal = 18.dp, vertical = 16.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Text(
                    text = message.role,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.secondary,
                )
                Text(
                    text = message.content.ifBlank { if (isPending) "…" else "" },
                    style = MaterialTheme.typography.bodyLarge,
                )
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun OverviewScreen(
    uiState: BabyGervaiseUiState,
    onPreviousContextChanged: (ContextLevel) -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .testTag("overview-screen")
            .verticalScroll(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(14.dp),
            verticalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            StatsCard(
                title = "Model",
                lines = listOf(
                    "Name: ${uiState.overviewSnapshot.modelStats.modelName}",
                    "Requests: ${uiState.overviewSnapshot.modelStats.totalRequests}",
                    "Tokens in/out: ${uiState.overviewSnapshot.modelStats.totalInputTokens} / ${uiState.overviewSnapshot.modelStats.totalOutputTokens}",
                    "Latency avg/latest: ${uiState.overviewSnapshot.modelStats.averageLatencyMs}ms / ${uiState.overviewSnapshot.modelStats.latestLatencyMs}ms",
                ),
            )
            StatsCard(
                title = "Memory",
                lines = listOf(
                    "Messages: ${uiState.overviewSnapshot.memoryStats.messageCount}",
                    "Stored memories: ${uiState.overviewSnapshot.memoryStats.storedMemories}",
                    "Vectors: ${uiState.overviewSnapshot.memoryStats.vectorCount}",
                    "Retrievals: ${uiState.overviewSnapshot.memoryStats.retrievalCount}",
                ),
            )
            StatsCard(
                title = "System",
                lines = listOf(
                    "Interactions: ${uiState.overviewSnapshot.systemStats.totalInteractions}",
                    "Tool calls: ${uiState.overviewSnapshot.systemStats.toolCalls}",
                    "Errors: ${uiState.overviewSnapshot.systemStats.errorCount}",
                ),
            )
        }

        ElevatedPanel(modifier = Modifier.fillMaxWidth()) {
            Column(verticalArrangement = Arrangement.spacedBy(18.dp)) {
                PreviousContextSelector(
                    selected = uiState.overviewSnapshot.previousContext,
                    onSelected = onPreviousContextChanged,
                )
                Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Text(
                        text = "Tool State",
                        style = MaterialTheme.typography.titleMedium,
                    )
                    if (uiState.overviewSnapshot.toolStates.isEmpty()) {
                        Text(
                            text = "No tool state recorded yet.",
                            style = MaterialTheme.typography.bodyLarge,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    uiState.overviewSnapshot.toolStates.forEach { (key, value) ->
                        JsonPanel(
                            content = CoreJson.prettyPrint(value),
                            key = key,
                        )
                    }
                }
            }
        }

        ElevatedPanel(modifier = Modifier.fillMaxWidth()) {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(
                    text = "Raw Model Logs",
                    style = MaterialTheme.typography.titleMedium,
                )
                if (uiState.overviewSnapshot.recentLogs.isEmpty()) {
                    Text(
                        text = "No model logs yet.",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                uiState.overviewSnapshot.recentLogs.forEach { entry ->
                    LogEntry(entry)
                }
            }
        }
    }
}

@Composable
private fun PreviousContextSelector(
    selected: ContextLevel,
    onSelected: (ContextLevel) -> Unit,
) {
    var expanded by remember { mutableStateOf(false) }
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(
            text = "Previous Context",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Box {
            Button(
                onClick = { expanded = true },
                shape = RoundedCornerShape(999.dp),
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
private fun StatsCard(
    title: String,
    lines: List<String>,
) {
    ElevatedPanel(
        modifier = Modifier.widthIn(min = 220.dp),
    ) {
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Text(
                text = title,
                style = MaterialTheme.typography.titleMedium,
            )
            lines.forEach { line ->
                Text(
                    text = line,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

@Composable
private fun LogEntry(entry: LogViewerEntry) {
    var expanded by remember { mutableStateOf(false) }
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .border(
                width = 1.dp,
                color = MaterialTheme.colorScheme.outline,
                shape = RoundedCornerShape(20.dp),
            )
            .clickable { expanded = !expanded }
            .padding(horizontal = 16.dp, vertical = 14.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
        ) {
            Text(entry.timestamp, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
            Text("${entry.latencyMs}ms", style = MaterialTheme.typography.bodyMedium)
            Text("Status ${entry.status ?: "n/a"}", style = MaterialTheme.typography.bodyMedium)
        }
        if (expanded) {
            JsonPanel(entry.prompt)
            JsonPanel(entry.rawOutput)
        }
    }
}

@Composable
private fun JsonPanel(
    content: String,
    key: String? = null,
) {
    Surface(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(18.dp),
        color = MaterialTheme.colorScheme.surfaceVariant.copy(alpha = 0.9f),
    ) {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            if (key != null) {
                Text(
                    text = key,
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.secondary,
                )
            }
            Text(
                text = content,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun ElevatedPanel(
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    Card(
        modifier = modifier,
        shape = RoundedCornerShape(24.dp),
        colors = CardDefaults.cardColors(
            containerColor = MaterialTheme.colorScheme.surface.copy(alpha = 0.92f),
        ),
        elevation = CardDefaults.cardElevation(defaultElevation = 0.dp),
    ) {
        Column(
            modifier = Modifier.padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(0.dp),
            content = content,
        )
    }
}
