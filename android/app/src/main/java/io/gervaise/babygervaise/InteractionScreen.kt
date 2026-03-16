package io.gervaise.babygervaise

import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.keyframes
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.Send
import androidx.compose.material.icons.rounded.CheckCircleOutline
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.rounded.Info
import androidx.compose.material.icons.rounded.Lightbulb
import androidx.compose.material.icons.rounded.SmartToy
import androidx.compose.material.icons.rounded.Stop
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import io.gervaise.babygervaise.theme.BabyGervaiseTheme

@Composable
internal fun InteractionScreen(
    interactionState: InteractionUiState,
    timelineState: LazyListState,
    onSuggestionSelected: (String, String) -> Unit,
) {
    LazyColumn(
        state = timelineState,
        modifier = Modifier
            .fillMaxSize()
            .testTag("timeline"),
        contentPadding = PaddingValues(horizontal = 16.dp, vertical = 16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        if (interactionState.showEmptyState) {
            item(key = "empty-state") {
                EmptyInteractionState()
            }
        }

        items(
            items = interactionState.items,
            key = { item -> item.id },
        ) { item ->
            InteractionRow(
                item = item,
                onSuggestionSelected = onSuggestionSelected,
            )
        }

        if (interactionState.showCloudWorkingOrnament) {
            item(key = "cloud-working-ornament") {
                CloudWorkingRow()
            }
        }
    }
}

@Composable
internal fun InteractionComposerBar(
    interactionState: InteractionUiState,
    onDraftChanged: (String) -> Unit,
    onSubmit: () -> Unit,
    onStopCloudWorking: () -> Unit,
    onFocused: () -> Unit,
) {
    Surface(
        tonalElevation = 2.dp,
        color = MaterialTheme.colorScheme.surface,
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
                value = interactionState.draft,
                onValueChange = onDraftChanged,
                enabled = interactionState.canSubmit,
                modifier = Modifier
                    .weight(1f)
                    .heightIn(min = 56.dp)
                    .testTag("composer-input")
                    .onFocusChanged { focusState ->
                        if (focusState.isFocused && interactionState.canSubmit) {
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

            if (interactionState.isSending && interactionState.deliveryState == DeliveryState.CLOUD_WORKING) {
                FilledIconButton(
                    onClick = onStopCloudWorking,
                    modifier = Modifier
                        .size(56.dp)
                        .testTag("send-button"),
                ) {
                    Icon(
                        imageVector = Icons.Rounded.Stop,
                        contentDescription = "Stop cloud working indicator",
                        modifier = Modifier.testTag("send-stop-icon"),
                    )
                }
            } else if (interactionState.isSending) {
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
                    enabled = interactionState.isComposerEnabled && interactionState.draft.isNotBlank(),
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
}

@Composable
private fun CloudWorkingRow() {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .testTag("cloud-working-indicator"),
        horizontalArrangement = Arrangement.Start,
    ) {
        Surface(
            color = MaterialTheme.colorScheme.surfaceContainer,
            contentColor = MaterialTheme.colorScheme.onSurfaceVariant,
            shape = MaterialTheme.shapes.extraLarge,
        ) {
            Row(
                modifier = Modifier.padding(horizontal = 14.dp, vertical = 10.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(
                    imageVector = Icons.Rounded.SmartToy,
                    contentDescription = null,
                    tint = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(
                    text = "Gervaise is thinking through it.",
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
        }
    }
}

@Composable
private fun InteractionRow(
    item: InteractionItem,
    onSuggestionSelected: (String, String) -> Unit,
) {
    when (item) {
        is UserBubble -> UserBubbleRow(item)
        is AssistantBubble -> AssistantBubbleRow(item)
        is ActionCard -> ActionCardRow(item)
        is ProgressCard -> ProgressCardRow(item)
        is ActionResultCard -> ActionResultRow(item)
        is LiveStateCard -> LiveStateRow(item)
        is SuggestionCard -> SuggestionRow(item, onSuggestionSelected)
        is ErrorCard -> ErrorRow(item)
    }
}

@Composable
private fun EmptyInteractionState() {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 24.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Text(
            text = "One conversation. One timeline.",
            style = MaterialTheme.typography.headlineSmall,
        )
        Text(
            text = "Talk to Gervaise and every reply, action, and live state will appear in one lane.",
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}

@Composable
private fun UserBubbleRow(
    item: UserBubble,
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
                text = item.text,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                style = MaterialTheme.typography.bodyLarge,
            )
        }
    }
}

@Composable
private fun AssistantBubbleRow(
    item: AssistantBubble,
) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        horizontalAlignment = Alignment.Start,
    ) {
        Surface(
            modifier = Modifier
                .fillMaxWidth(0.9f)
                .then(
                    if (item.isStreaming && item.text.isBlank()) {
                        Modifier.testTag("assistant-processing")
                    } else {
                        Modifier
                    },
                ),
            color = MaterialTheme.colorScheme.surfaceContainerLow,
            contentColor = MaterialTheme.colorScheme.onSurface,
            shape = MaterialTheme.shapes.extraLarge,
        ) {
            if (item.isStreaming && item.text.isBlank()) {
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
            } else {
                Text(
                    text = item.text,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                    style = MaterialTheme.typography.bodyLarge,
                )
            }
        }
    }
}

@Composable
private fun ActionCardRow(
    item: ActionCard,
) {
    TimelineCard(
        title = item.title,
        body = item.detail ?: item.status,
        supportingLines = listOf("${item.tool} • ${item.status}"),
        tone = InteractionTone.Progress,
        icon = Icons.Rounded.SmartToy,
    )
}

@Composable
private fun ProgressCardRow(
    item: ProgressCard,
) {
    TimelineCard(
        title = item.title,
        body = item.detail,
        supportingLines = listOf(item.tool),
        tone = InteractionTone.Progress,
        icon = Icons.Rounded.Info,
    )
}

@Composable
private fun ActionResultRow(
    item: ActionResultCard,
) {
    TimelineCard(
        title = item.title,
        body = item.detail,
        supportingLines = buildList {
            add("${item.tool} • ${item.status}")
            addAll(item.supportingLines)
        },
        tone = item.tone,
        icon = Icons.Rounded.CheckCircleOutline,
    )
}

@Composable
private fun LiveStateRow(
    item: LiveStateCard,
) {
    TimelineCard(
        title = item.title,
        body = item.detail,
        supportingLines = item.supportingLines,
        tone = item.tone,
        icon = Icons.Rounded.Info,
    )
}

@Composable
private fun SuggestionRow(
    item: SuggestionCard,
    onSuggestionSelected: (String, String) -> Unit,
) {
    TimelineCardContainer(
        tone = InteractionTone.Neutral,
        icon = Icons.Rounded.Lightbulb,
    ) {
        Text(
            text = item.title,
            style = MaterialTheme.typography.titleSmall,
        )
        Column(
            modifier = Modifier.padding(top = 12.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            item.options.forEach { option ->
                OutlinedButton(
                    onClick = { onSuggestionSelected(item.id, option.id) },
                    enabled = !item.isConsumed,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    Text(
                        text = when {
                            item.selectedOptionId == option.id -> "${option.label} selected"
                            else -> option.label
                        },
                    )
                }
            }
        }
    }
}

@Composable
private fun ErrorRow(
    item: ErrorCard,
) {
    TimelineCard(
        title = item.title,
        body = item.detail,
        supportingLines = item.supportingLines,
        tone = InteractionTone.Error,
        icon = Icons.Rounded.ErrorOutline,
    )
}

@Composable
private fun TimelineCard(
    title: String,
    body: String,
    supportingLines: List<String>,
    tone: InteractionTone,
    icon: ImageVector,
) {
    TimelineCardContainer(
        tone = tone,
        icon = icon,
    ) {
        Text(
            text = title,
            style = MaterialTheme.typography.titleSmall,
        )
        Text(
            text = body,
            modifier = Modifier.padding(top = 4.dp),
            style = MaterialTheme.typography.bodyMedium,
        )
        supportingLines.forEach { line ->
            Text(
                text = line,
                modifier = Modifier.padding(top = 4.dp),
                style = MaterialTheme.typography.bodySmall,
                color = timelineCardColors(tone).supporting,
            )
        }
    }
}

@Composable
private fun TimelineCardContainer(
    tone: InteractionTone,
    icon: ImageVector,
    content: @Composable () -> Unit,
) {
    val colors = timelineCardColors(tone)
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
                            imageVector = icon,
                            contentDescription = null,
                        )
                    }
                }
                Column(
                    modifier = Modifier.weight(1f),
                    verticalArrangement = Arrangement.spacedBy(2.dp),
                ) {
                    content()
                }
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
private fun timelineCardColors(tone: InteractionTone): TimelineCardColors =
    when (tone) {
        InteractionTone.Neutral -> TimelineCardColors(
            container = MaterialTheme.colorScheme.surfaceContainerHigh,
            content = MaterialTheme.colorScheme.onSurface,
            supporting = MaterialTheme.colorScheme.onSurfaceVariant,
            iconContainer = MaterialTheme.colorScheme.surfaceContainerHighest,
            iconTint = MaterialTheme.colorScheme.onSurfaceVariant,
        )

        InteractionTone.Progress -> TimelineCardColors(
            container = MaterialTheme.colorScheme.secondaryContainer,
            content = MaterialTheme.colorScheme.onSecondaryContainer,
            supporting = MaterialTheme.colorScheme.onSecondaryContainer.copy(alpha = 0.8f),
            iconContainer = MaterialTheme.colorScheme.secondary,
            iconTint = MaterialTheme.colorScheme.onSecondary,
        )

        InteractionTone.Positive -> TimelineCardColors(
            container = MaterialTheme.colorScheme.tertiaryContainer,
            content = MaterialTheme.colorScheme.onTertiaryContainer,
            supporting = MaterialTheme.colorScheme.onTertiaryContainer.copy(alpha = 0.8f),
            iconContainer = MaterialTheme.colorScheme.tertiary,
            iconTint = MaterialTheme.colorScheme.onTertiary,
        )

        InteractionTone.Warning -> TimelineCardColors(
            container = MaterialTheme.colorScheme.tertiaryContainer,
            content = MaterialTheme.colorScheme.onTertiaryContainer,
            supporting = MaterialTheme.colorScheme.onTertiaryContainer.copy(alpha = 0.8f),
            iconContainer = MaterialTheme.colorScheme.tertiary,
            iconTint = MaterialTheme.colorScheme.onTertiary,
        )

        InteractionTone.Error -> TimelineCardColors(
            container = MaterialTheme.colorScheme.errorContainer,
            content = MaterialTheme.colorScheme.onErrorContainer,
            supporting = MaterialTheme.colorScheme.onErrorContainer.copy(alpha = 0.8f),
            iconContainer = MaterialTheme.colorScheme.error,
            iconTint = MaterialTheme.colorScheme.onError,
        )
    }

private data class TimelineCardColors(
    val container: Color,
    val content: Color,
    val supporting: Color,
    val iconContainer: Color,
    val iconTint: Color,
)

@Preview(showBackground = true)
@Composable
private fun InteractionScreenPreview() {
    BabyGervaiseTheme {
        InteractionScreen(
            interactionState = InteractionUiState(items = InteractionTimelineDemoData.timeline),
            timelineState = androidx.compose.foundation.lazy.rememberLazyListState(),
            onSuggestionSelected = { _, _ -> },
        )
    }
}
