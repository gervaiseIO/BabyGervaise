package io.gervaise.babygervaise

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

class InteractionViewModel(
    private val bridge: HgieBridge,
    private val scope: CoroutineScope,
) {
    private val _uiState = MutableStateFlow(InteractionUiState())
    val uiState: StateFlow<InteractionUiState> = _uiState.asStateFlow()

    init {
        scope.launch {
            bridge.events.collectLatest { event ->
                _uiState.update { current ->
                    InteractionTimelineReducer.reduce(
                        current = current,
                        event = event,
                    )
                }
            }
        }
    }

    fun updateDraft(value: String) {
        _uiState.update { current ->
            current.copy(draft = value)
        }
    }

    fun updateComposerState(
        canSubmit: Boolean,
        isSending: Boolean,
    ) {
        _uiState.update { current ->
            current.copy(
                canSubmit = canSubmit,
                isSending = isSending,
            )
        }
    }

    fun updateDeliveryState(deliveryState: DeliveryState) {
        _uiState.update { current ->
            current.copy(
                deliveryState = deliveryState,
                isCloudWorkingMuted = if (deliveryState == DeliveryState.CLOUD_WORKING) {
                    current.isCloudWorkingMuted
                } else {
                    false
                },
            )
        }
    }

    fun muteCloudWorking() {
        _uiState.update { current ->
            if (current.deliveryState != DeliveryState.CLOUD_WORKING) {
                current
            } else {
                current.copy(isCloudWorkingMuted = true)
            }
        }
    }

    fun submitDraft() {
        val text = _uiState.value.draft.trim()
        if (text.isEmpty() || !_uiState.value.isComposerEnabled) {
            return
        }

        _uiState.update { current ->
            current.copy(draft = "")
        }
        scope.launch {
            bridge.submitUserInput(text)
        }
    }

    fun submitSuggestion(
        cardId: String,
        optionId: String,
    ) {
        val card = _uiState.value.items
            .filterIsInstance<SuggestionCard>()
            .firstOrNull { item -> item.id == cardId }
            ?: return
        if (card.isConsumed) {
            return
        }

        val option = card.options.firstOrNull { candidate -> candidate.id == optionId } ?: return
        _uiState.update { current ->
            InteractionTimelineReducer.consumeSuggestion(
                current = current,
                cardId = cardId,
                optionId = optionId,
            )
        }
        scope.launch {
            bridge.submitUserInput(option.value)
        }
    }
}
