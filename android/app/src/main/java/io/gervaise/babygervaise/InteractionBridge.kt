package io.gervaise.babygervaise

import kotlinx.coroutines.flow.Flow

sealed interface HgieEvent {
    data class Bootstrap(
        val items: List<InteractionItem>,
    ) : HgieEvent

    data class UserSubmitted(
        val item: UserBubble,
    ) : HgieEvent

    data class AssistantReply(
        val item: AssistantBubble,
    ) : HgieEvent

    data class ActionStarted(
        val item: ActionCard,
    ) : HgieEvent

    data class ActionProgress(
        val item: ProgressCard,
    ) : HgieEvent

    data class ActionCompleted(
        val item: ActionResultCard,
    ) : HgieEvent

    data class LiveStateUpdate(
        val item: LiveStateCard,
    ) : HgieEvent

    data class Suggestion(
        val item: SuggestionCard,
    ) : HgieEvent

    data class ErrorEvent(
        val item: ErrorCard,
    ) : HgieEvent
}

interface HgieBridge {
    val events: Flow<HgieEvent>

    suspend fun submitUserInput(text: String)
}
