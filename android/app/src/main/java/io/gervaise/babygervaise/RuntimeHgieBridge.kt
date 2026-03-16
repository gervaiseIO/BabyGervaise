package io.gervaise.babygervaise

import io.gervaise.babygervaise.bridge.ChatMessage
import io.gervaise.babygervaise.bridge.CoreJson
import io.gervaise.babygervaise.bridge.InputSource
import java.time.Instant
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive

class RuntimeHgieBridge(
    private val runtime: BabyGervaiseRuntime,
    scope: CoroutineScope,
) : HgieBridge {
    private val eventsFlow = MutableSharedFlow<HgieEvent>(extraBufferCapacity = 64)
    private val assistantStateByTurn = mutableMapOf<String, AssistantBubble>()
    private val seenUserMessageIds = mutableSetOf<Long>()
    private val seenToolMessageIds = mutableSetOf<Long>()
    private var hasBootstrapped = false
    private var activeActionKey: String? = null
    private var activeActionDetail: String? = null
    private var lastInteractionErrorId: String? = null

    override val events: Flow<HgieEvent> = eventsFlow.asSharedFlow()

    init {
        scope.launch {
            runtime.state.collectLatest(::handleState)
        }
    }

    override suspend fun submitUserInput(text: String) {
        val trimmed = text.trim()
        if (trimmed.isEmpty()) {
            return
        }

        val submitted = runtime.submitUserTurn(
            text = trimmed,
            inputSource = InputSource.TEXT,
        )
        if (!submitted) {
            return
        }

        val snapshot = runtime.state.value
        val turnId = snapshot.pendingTurnId ?: "turn-${System.currentTimeMillis()}"
        val timestampMs = snapshot.bootstrapState.messages
            .lastOrNull { message -> message.role == "user" && message.turnId == turnId }
            ?.createdAt
            .toEpochMillis()

        eventsFlow.emit(
            HgieEvent.UserSubmitted(
                item = UserBubble(
                    id = "user-$turnId",
                    timestampMs = timestampMs,
                    turnId = turnId,
                    text = trimmed,
                ),
            ),
        )
    }

    private suspend fun handleState(state: BabyGervaiseCoreState) {
        if (!hasBootstrapped) {
            if (!shouldFinalizeInteractionBootstrap(state)) {
                return
            }
            val bootstrapItems = buildBootstrapItems(state)
            assistantStateByTurn.putAll(
                bootstrapItems
                    .filterIsInstance<AssistantBubble>()
                    .associateBy { item -> item.turnId },
            )
            seenUserMessageIds += state.bootstrapState.messages
                .filter { message -> message.role == "user" }
                .map { message -> message.id }
            seenToolMessageIds += state.bootstrapState.messages
                .filter { message -> message.role == "tool" }
                .map { message -> message.id }
            activeActionKey = state.activeToolStatus?.eventKey()
            activeActionDetail = state.toolStatus.normalizedActionDetail()
            lastInteractionErrorId = state.interactionError?.id
            hasBootstrapped = true
            eventsFlow.emit(HgieEvent.Bootstrap(items = bootstrapItems))
            return
        }

        emitTranscriptUpdates(state)
        emitActiveActionUpdates(state)
        emitInteractionErrors(state)
    }

    private suspend fun emitTranscriptUpdates(state: BabyGervaiseCoreState) {
        state.bootstrapState.messages.forEach { message ->
            when (message.role) {
                "user" -> emitUserMessageUpdate(message)
                "assistant" -> emitAssistantMessageUpdate(
                    message = message,
                    isStreaming = state.pendingTurnId == message.turnId,
                )
                "tool" -> emitToolResultUpdate(message)
            }
        }
    }

    private suspend fun emitUserMessageUpdate(message: ChatMessage) {
        if (!seenUserMessageIds.add(message.id)) {
            return
        }

        eventsFlow.emit(
            HgieEvent.UserSubmitted(
                item = UserBubble(
                    id = "user-${message.turnId}",
                    timestampMs = message.createdAt.toEpochMillis(),
                    turnId = message.turnId,
                    text = message.content,
                ),
            ),
        )
    }

    private suspend fun emitAssistantMessageUpdate(
        message: ChatMessage,
        isStreaming: Boolean,
    ) {
        val sanitized = sanitizeAssistantText(message.content)
        if (sanitized.isBlank() && !isStreaming) {
            return
        }

        val nextBubble = AssistantBubble(
            id = "assistant-${message.turnId}",
            timestampMs = message.createdAt.toEpochMillis(),
            turnId = message.turnId,
            text = sanitized,
            isStreaming = isStreaming,
        )
        if (assistantStateByTurn[message.turnId] == nextBubble) {
            return
        }

        assistantStateByTurn[message.turnId] = nextBubble
        eventsFlow.emit(HgieEvent.AssistantReply(item = nextBubble))
    }

    private suspend fun emitToolResultUpdate(message: ChatMessage) {
        if (!seenToolMessageIds.add(message.id)) {
            return
        }

        val payload = parseInteractionToolMessage(message) ?: return
        val timestampMs = message.createdAt.toEpochMillis()
        if (payload.isFailure) {
            eventsFlow.emit(
                HgieEvent.ErrorEvent(
                    item = ErrorCard(
                        id = "error-tool-${message.id}",
                        timestampMs = timestampMs,
                        turnId = message.turnId,
                        title = payload.title,
                        detail = payload.body,
                        supportingLines = payload.supportingLines,
                    ),
                ),
            )
        } else {
            eventsFlow.emit(
                HgieEvent.ActionCompleted(
                    item = ActionResultCard(
                        id = "result-tool-${message.id}",
                        timestampMs = timestampMs,
                        turnId = message.turnId,
                        tool = payload.toolLabel,
                        title = payload.title,
                        detail = payload.body,
                        status = payload.statusLabel,
                        supportingLines = payload.supportingLines,
                        tone = payload.tone,
                    ),
                ),
            )
        }
    }

    private suspend fun emitActiveActionUpdates(state: BabyGervaiseCoreState) {
        val activeStatus = state.activeToolStatus
        if (activeStatus == null) {
            activeActionKey = null
            activeActionDetail = null
            return
        }

        val eventKey = activeStatus.eventKey()
        val detail = state.toolStatus.normalizedActionDetail()
        if (eventKey != activeActionKey) {
            activeActionKey = eventKey
            activeActionDetail = detail
            eventsFlow.emit(
                HgieEvent.ActionStarted(
                    item = ActionCard(
                        id = "action-$eventKey",
                        timestampMs = System.currentTimeMillis(),
                        turnId = activeStatus.turnId,
                        tool = activeStatus.tool.prettyLabel(),
                        title = activeStatus.action.prettyAction(),
                        status = activeStatus.status.prettyStatus(),
                        detail = detail,
                    ),
                ),
            )
            return
        }

        if (!detail.isNullOrBlank() && detail != activeActionDetail) {
            activeActionDetail = detail
            eventsFlow.emit(
                HgieEvent.ActionProgress(
                    item = ProgressCard(
                        id = "progress-$eventKey-${detail.normalizedKey()}",
                        timestampMs = System.currentTimeMillis(),
                        turnId = activeStatus.turnId,
                        tool = activeStatus.tool.prettyLabel(),
                        title = activeStatus.action.prettyAction(),
                        detail = detail,
                    ),
                ),
            )
        }
    }

    private suspend fun emitInteractionErrors(state: BabyGervaiseCoreState) {
        val interactionError = state.interactionError ?: return
        if (interactionError.id == lastInteractionErrorId) {
            return
        }

        lastInteractionErrorId = interactionError.id
        eventsFlow.emit(
            HgieEvent.ErrorEvent(
                item = ErrorCard(
                    id = "runtime-error-${interactionError.id}",
                    timestampMs = interactionError.timestampMs,
                    title = "Something needs attention",
                    detail = interactionError.message,
                ),
            ),
        )
    }

    private fun buildBootstrapItems(state: BabyGervaiseCoreState): List<InteractionItem> =
        buildList {
            addAll(projectBootstrapTranscriptItems(state.bootstrapState.messages, state.pendingTurnId))

            state.activeToolStatus?.let { activeStatus ->
                add(
                    ActionCard(
                        id = "action-${activeStatus.eventKey()}",
                        timestampMs = System.currentTimeMillis(),
                        turnId = activeStatus.turnId,
                        tool = activeStatus.tool.prettyLabel(),
                        title = activeStatus.action.prettyAction(),
                        status = activeStatus.status.prettyStatus(),
                        detail = state.toolStatus.normalizedActionDetail(),
                    ),
                )
            }

            state.interactionError?.let { interactionError ->
                add(
                    ErrorCard(
                        id = "runtime-error-${interactionError.id}",
                        timestampMs = interactionError.timestampMs,
                        title = "Something needs attention",
                        detail = interactionError.message,
                    ),
                )
            }
        }
}

internal data class VisibleToolPayload(
    val toolLabel: String,
    val status: String,
    val title: String,
    val body: String,
    val supportingLines: List<String>,
    val tone: InteractionTone,
) {
    val isFailure: Boolean
        get() = tone == InteractionTone.Error || tone == InteractionTone.Warning || status.isFailureStatus()

    val statusLabel: String
        get() = if (status.isBlank()) "Completed" else status.prettyStatus()
}

private fun ActiveToolStatus.eventKey(): String =
    "${turnId}-${tool.lowercase()}-${action.lowercase()}"

private fun String.prettyLabel(): String =
    split("_", "-", " ")
        .filter { part -> part.isNotBlank() }
        .joinToString(separator = " ") { part ->
            part.lowercase().replaceFirstChar { character -> character.uppercase() }
        }

private fun String.prettyAction(): String = prettyLabel()

private fun String.prettyStatus(): String = prettyLabel()

private fun String.normalizedKey(): String =
    trim().lowercase().replace(Regex("\\s+"), "-")

private fun String?.normalizedActionDetail(): String? {
    val value = this?.trim().orEmpty()
    if (value.isBlank()) {
        return null
    }
    return when (value) {
        "HGIE ready.",
        "Waiting for Gervaise.",
        "Gervaise is thinking.",
        -> null

        else -> value
    }
}

private fun String?.isFailureStatus(): Boolean {
    val value = this?.lowercase().orEmpty()
    if (value.isBlank()) {
        return false
    }
    return value == "error" ||
        value == "warning" ||
        value.contains("failed") ||
        value.contains("forbidden") ||
        value.contains("required") ||
        value.contains("expired") ||
        value.contains("unconfigured") ||
        value.contains("not_found") ||
        value.contains("unavailable")
}

private fun String?.toInteractionTone(): InteractionTone =
    when (this?.lowercase()) {
        "progress" -> InteractionTone.Progress
        "positive" -> InteractionTone.Positive
        "warning" -> InteractionTone.Warning
        "error" -> InteractionTone.Error
        else -> InteractionTone.Neutral
    }

internal fun shouldFinalizeInteractionBootstrap(state: BabyGervaiseCoreState): Boolean =
    state.isCoreReady || state.initializationError != null

internal fun projectBootstrapTranscriptItems(
    messages: List<ChatMessage>,
    pendingTurnId: String?,
): List<InteractionItem> =
    buildList {
        messages.forEach { message ->
            when (message.role) {
                "user" -> add(
                    UserBubble(
                        id = "user-${message.turnId}",
                        timestampMs = message.createdAt.toEpochMillis(),
                        turnId = message.turnId,
                        text = message.content,
                    ),
                )

                "assistant" -> {
                    val sanitized = sanitizeAssistantText(message.content)
                    val isStreaming = pendingTurnId == message.turnId
                    if (sanitized.isNotBlank() || isStreaming) {
                        add(
                            AssistantBubble(
                                id = "assistant-${message.turnId}",
                                timestampMs = message.createdAt.toEpochMillis(),
                                turnId = message.turnId,
                                text = sanitized,
                                isStreaming = isStreaming,
                            ),
                        )
                    }
                }

                "tool" -> {
                    val payload = parseInteractionToolMessage(message) ?: return@forEach
                    val timestampMs = message.createdAt.toEpochMillis()
                    if (payload.isFailure) {
                        add(
                            ErrorCard(
                                id = "error-tool-${message.id}",
                                timestampMs = timestampMs,
                                turnId = message.turnId,
                                title = payload.title,
                                detail = payload.body,
                                supportingLines = payload.supportingLines,
                            ),
                        )
                    } else {
                        add(
                            ActionResultCard(
                                id = "result-tool-${message.id}",
                                timestampMs = timestampMs,
                                turnId = message.turnId,
                                tool = payload.toolLabel,
                                title = payload.title,
                                detail = payload.body,
                                status = payload.statusLabel,
                                supportingLines = payload.supportingLines,
                                tone = payload.tone,
                            ),
                        )
                    }
                }
            }
        }
    }

internal fun sanitizeAssistantText(raw: String): String {
    val trimmed = raw.trim()
    if (trimmed.isBlank()) {
        return ""
    }

    val lower = trimmed.lowercase()
    val looksInternal = lower.startsWith("```json") ||
        ((trimmed.startsWith("{") && trimmed.endsWith("}")) ||
            (trimmed.startsWith("[") && trimmed.endsWith("]"))) ||
        lower.contains("\"assistant_reply\"") ||
        lower.contains("\"tool_request\"") ||
        lower.contains("\"memory_candidates\"") ||
        lower.startsWith("assistant_reply") ||
        lower.startsWith("tool_request") ||
        lower.startsWith("memory_candidates")

    return if (looksInternal) "" else trimmed
}

internal fun parseInteractionToolMessage(message: ChatMessage): VisibleToolPayload? {
    val displayPayload = message.displayJson
        ?.takeIf { payload -> payload.isNotBlank() }
        ?.let { payload ->
            runCatching {
                CoreJson.json.parseToJsonElement(payload) as? JsonObject
            }.getOrNull()
        }

    if (displayPayload != null) {
        return VisibleToolPayload(
            toolLabel = displayPayload.string("tool")?.prettyLabel() ?: "Tool",
            status = displayPayload.string("status").orEmpty(),
            title = displayPayload.string("title") ?: "Tool update",
            body = displayPayload.string("body") ?: message.content.trim(),
            supportingLines = displayPayload.stringArray("supporting_lines").orEmpty(),
            tone = displayPayload.string("tone").toInteractionTone(),
        )
    }

    val payload = runCatching {
        CoreJson.json.parseToJsonElement(message.content) as? JsonObject
    }.getOrNull()

    if (payload == null) {
        val content = message.content.trim()
        if (content.isBlank()) {
            return null
        }
        return VisibleToolPayload(
            toolLabel = "Tool",
            status = "success",
            title = "Tool update",
            body = content,
            supportingLines = emptyList(),
            tone = InteractionTone.Neutral,
        )
    }

    val toolLabel = payload.string("tool")?.prettyLabel() ?: "Tool"
    val action = payload.string("action").orEmpty()
    val status = payload.string("status").orEmpty()
    return VisibleToolPayload(
        toolLabel = toolLabel,
        status = status,
        title = when {
            status.isFailureStatus() -> "$toolLabel couldn't complete that"
            action.isNotBlank() -> "$toolLabel ${action.prettyAction()}".trim()
            else -> "$toolLabel update"
        },
        body = payload.string("message")
            ?: payload.string("summary")
            ?: payload.string("capability_summary")
            ?: message.content.trim(),
        supportingLines = buildList {
            payload.string("device_name")?.let { device -> add("Device: $device") }
            payload.string("account_display_name")?.let { name -> add("Account: $name") }
            payload.string("reason")?.let { reason -> add("Reason: ${reason.prettyAction()}") }
        },
        tone = when {
            status.isFailureStatus() -> InteractionTone.Error
            status.contains("auth", ignoreCase = true) -> InteractionTone.Warning
            else -> InteractionTone.Neutral
        },
    )
}

private fun String?.toEpochMillis(): Long =
    this?.let { value ->
        runCatching { Instant.parse(value).toEpochMilli() }.getOrNull()
    } ?: System.currentTimeMillis()

private fun JsonObject.string(key: String): String? =
    get(key)?.jsonPrimitive?.contentOrNull

private fun JsonObject.arrayValue(key: String): JsonArray? =
    get(key) as? JsonArray

private fun JsonObject.stringArray(key: String): List<String>? =
    arrayValue(key)?.mapNotNull { item -> item.jsonPrimitive.contentOrNull }
