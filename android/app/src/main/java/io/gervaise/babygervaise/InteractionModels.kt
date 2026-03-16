package io.gervaise.babygervaise

sealed interface InteractionItem {
    val id: String
    val timestampMs: Long
}

enum class InteractionTone {
    Neutral,
    Progress,
    Positive,
    Warning,
    Error,
}

enum class DeliveryState {
    IDLE,
    LOCAL_TYPING,
    CLOUD_WORKING,
}

data class SuggestionOption(
    val id: String,
    val label: String,
    val value: String = label,
)

data class UserBubble(
    override val id: String,
    override val timestampMs: Long,
    val turnId: String,
    val text: String,
) : InteractionItem

data class AssistantBubble(
    override val id: String,
    override val timestampMs: Long,
    val turnId: String,
    val text: String,
    val isStreaming: Boolean = false,
) : InteractionItem

data class ActionCard(
    override val id: String,
    override val timestampMs: Long,
    val turnId: String,
    val tool: String,
    val title: String,
    val status: String,
    val detail: String? = null,
) : InteractionItem

data class ProgressCard(
    override val id: String,
    override val timestampMs: Long,
    val turnId: String,
    val tool: String,
    val title: String,
    val detail: String,
) : InteractionItem

data class ActionResultCard(
    override val id: String,
    override val timestampMs: Long,
    val turnId: String,
    val tool: String,
    val title: String,
    val detail: String,
    val status: String,
    val supportingLines: List<String> = emptyList(),
    val tone: InteractionTone = InteractionTone.Neutral,
) : InteractionItem

data class LiveStateCard(
    override val id: String,
    override val timestampMs: Long,
    val title: String,
    val detail: String,
    val supportingLines: List<String> = emptyList(),
    val tone: InteractionTone = InteractionTone.Neutral,
) : InteractionItem

data class SuggestionCard(
    override val id: String,
    override val timestampMs: Long,
    val title: String,
    val options: List<SuggestionOption>,
    val selectedOptionId: String? = null,
    val isConsumed: Boolean = false,
) : InteractionItem

data class ErrorCard(
    override val id: String,
    override val timestampMs: Long,
    val turnId: String? = null,
    val title: String,
    val detail: String,
    val supportingLines: List<String> = emptyList(),
) : InteractionItem

data class InteractionUiState(
    val items: List<InteractionItem> = emptyList(),
    val draft: String = "",
    val isSending: Boolean = false,
    val canSubmit: Boolean = false,
    val deliveryState: DeliveryState = DeliveryState.IDLE,
    val isCloudWorkingMuted: Boolean = false,
) {
    val isComposerEnabled: Boolean
        get() = canSubmit && !isSending

    val showEmptyState: Boolean
        get() = items.none { item -> item !is LiveStateCard }

    val showCloudWorkingOrnament: Boolean
        get() = deliveryState == DeliveryState.CLOUD_WORKING && !isCloudWorkingMuted
}
