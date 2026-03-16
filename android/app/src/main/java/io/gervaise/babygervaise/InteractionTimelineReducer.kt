package io.gervaise.babygervaise

internal object InteractionTimelineReducer {
    fun reduce(
        current: InteractionUiState,
        event: HgieEvent,
    ): InteractionUiState =
        when (event) {
            is HgieEvent.Bootstrap -> current.copy(items = event.items)
            is HgieEvent.UserSubmitted -> current.copy(items = appendOrReplace(current.items, event.item))
            is HgieEvent.AssistantReply -> current.copy(items = appendOrReplace(current.items, event.item))
            is HgieEvent.ActionStarted -> current.copy(items = appendOrReplace(current.items, event.item))
            is HgieEvent.ActionProgress -> current.copy(items = appendOrReplace(current.items, event.item))
            is HgieEvent.ActionCompleted -> current.copy(
                items = insertAfterTurnActivity(current.items, event.item),
            )

            is HgieEvent.LiveStateUpdate -> current.copy(items = upsertLiveState(current.items, event.item))
            is HgieEvent.Suggestion -> current.copy(items = appendOrReplace(current.items, event.item))
            is HgieEvent.ErrorEvent -> current.copy(items = insertAfterTurnActivity(current.items, event.item))
        }

    fun consumeSuggestion(
        current: InteractionUiState,
        cardId: String,
        optionId: String,
    ): InteractionUiState =
        current.copy(
            items = current.items.map { item ->
                if (item is SuggestionCard && item.id == cardId) {
                    item.copy(
                        selectedOptionId = optionId,
                        isConsumed = true,
                    )
                } else {
                    item
                }
            },
        )

    private fun appendOrReplace(
        items: List<InteractionItem>,
        nextItem: InteractionItem,
    ): List<InteractionItem> {
        val index = items.indexOfFirst { item -> item.id == nextItem.id }
        if (index == -1) {
            return items + nextItem
        }
        return items.toMutableList().also { mutable ->
            mutable[index] = nextItem
        }
    }

    private fun upsertLiveState(
        items: List<InteractionItem>,
        nextItem: LiveStateCard,
    ): List<InteractionItem> {
        val index = items.indexOfFirst { item -> item.id == nextItem.id }
        if (index == -1) {
            return items + nextItem
        }
        return items.toMutableList().also { mutable ->
            mutable[index] = nextItem
        }
    }

    private fun insertAfterTurnActivity(
        items: List<InteractionItem>,
        nextItem: InteractionItem,
    ): List<InteractionItem> {
        val existingIndex = items.indexOfFirst { item -> item.id == nextItem.id }
        if (existingIndex != -1) {
            return items.toMutableList().also { mutable ->
                mutable[existingIndex] = nextItem
            }
        }

        val turnId = when (nextItem) {
            is ActionResultCard -> nextItem.turnId
            is ErrorCard -> nextItem.turnId
            else -> null
        } ?: return items + nextItem

        val insertIndex = items.indexOfLast { item ->
            when (item) {
                is UserBubble -> item.turnId == turnId
                is AssistantBubble -> item.turnId == turnId
                is ActionCard -> item.turnId == turnId
                is ProgressCard -> item.turnId == turnId
                is ActionResultCard -> item.turnId == turnId
                is ErrorCard -> item.turnId == turnId
                else -> false
            }
        }

        if (insertIndex == -1) {
            return items + nextItem
        }

        return buildList(items.size + 1) {
            addAll(items.subList(0, insertIndex + 1))
            add(nextItem)
            addAll(items.subList(insertIndex + 1, items.size))
        }
    }
}
