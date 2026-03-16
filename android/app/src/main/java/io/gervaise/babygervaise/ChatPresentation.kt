package io.gervaise.babygervaise

import io.gervaise.babygervaise.bridge.ChatMessage
import io.gervaise.babygervaise.bridge.CoreJson
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonPrimitive

internal sealed interface ConversationTimelineItem {
    val key: String

    data class UserMessage(
        val message: ChatMessage,
    ) : ConversationTimelineItem {
        override val key: String = "user-${message.id}"
    }

    data class AssistantMessage(
        val message: ChatMessage,
    ) : ConversationTimelineItem {
        override val key: String = "assistant-${message.id}"
    }

    data class SystemMessage(
        val card: SystemCardPresentation,
        override val key: String,
    ) : ConversationTimelineItem

    data class Processing(
        val turnId: String,
    ) : ConversationTimelineItem {
        override val key: String = "processing-$turnId"
    }
}

internal data class SystemCardPresentation(
    val title: String,
    val body: String,
    val supportingLines: List<String> = emptyList(),
    val tone: SystemTone = SystemTone.Neutral,
    val icon: SystemIcon = SystemIcon.Info,
    val comparisonText: String = body,
)

internal enum class SystemTone {
    Neutral,
    Progress,
    Positive,
    Warning,
    Error,
}

internal enum class SystemIcon {
    Info,
    Assistant,
    Spotify,
    Hue,
    Device,
    Warning,
    Error,
    Success,
}

internal fun buildConversationTimeline(uiState: BabyGervaiseUiState): List<ConversationTimelineItem> {
    val items = mutableListOf<ConversationTimelineItem>()
    val systemBodiesByTurn = mutableMapOf<String, MutableList<String>>()
    var hasPendingIndicator = false

    uiState.bootstrapState.messages.forEach { message ->
        when (message.role) {
            "user" -> {
                items += ConversationTimelineItem.UserMessage(message)
            }

            "assistant" -> {
                if (message.turnId == uiState.pendingTurnId && message.content.isBlank()) {
                    items += ConversationTimelineItem.Processing(message.turnId)
                    hasPendingIndicator = true
                } else if (message.content.isNotBlank()) {
                    val sanitizedContent = sanitizeAssistantChatContent(message.content)
                    if (sanitizedContent.isBlank()) {
                        return@forEach
                    }
                    val normalizedContent = normalizeForComparison(sanitizedContent)
                    val shouldHideDuplicate = systemBodiesByTurn[message.turnId]
                        ?.any { it == normalizedContent }
                        ?: false
                    if (!shouldHideDuplicate) {
                        items += ConversationTimelineItem.AssistantMessage(
                            message.copy(content = sanitizedContent),
                        )
                    }
                }
            }

            "tool" -> {
                val card = formatToolSystemCard(
                    displayPayload = message.displayJson,
                    fallbackPayload = message.content,
                )
                items += ConversationTimelineItem.SystemMessage(
                    card = card,
                    key = "tool-${message.id}",
                )
                systemBodiesByTurn.getOrPut(message.turnId) { mutableListOf() }
                    .add(normalizeForComparison(card.comparisonText))
            }

            "system" -> {
                items += ConversationTimelineItem.SystemMessage(
                    card = SystemCardPresentation(
                        title = "System update",
                        body = message.content.trim(),
                        tone = SystemTone.Neutral,
                        icon = SystemIcon.Info,
                    ),
                    key = "system-${message.id}",
                )
            }
        }
    }

    if (uiState.pendingTurnId != null && !hasPendingIndicator) {
        items += ConversationTimelineItem.Processing(uiState.pendingTurnId)
    }

    activeSystemCard(uiState)?.let { card ->
        items += ConversationTimelineItem.SystemMessage(
            card = card,
            key = "active-system-status",
        )
    }

    return items
}

private fun activeSystemCard(uiState: BabyGervaiseUiState): SystemCardPresentation? {
    uiState.initializationError?.let { error ->
        return SystemCardPresentation(
            title = "Gervaise isn't ready",
            body = error,
            tone = SystemTone.Error,
            icon = SystemIcon.Error,
        )
    }

    val activeToolStatus = uiState.activeToolStatus
    if (uiState.pendingTurnId != null && activeToolStatus != null) {
        return formatPendingToolCard(activeToolStatus, uiState.toolStatus)
    }

    val status = uiState.toolStatus?.trim().orEmpty()
    if (status.isBlank() || status == "HGIE ready." || status == "Waiting for Gervaise." || status == "Gervaise is thinking.") {
        return null
    }

    if (uiState.pendingTurnId != null && status.contains("thinking", ignoreCase = true)) {
        return null
    }

    val tone = when {
        status.contains("couldn't", ignoreCase = true) ||
            status.contains("failed", ignoreCase = true) ||
            status.contains("error", ignoreCase = true) ||
            status.contains("not ready", ignoreCase = true) -> SystemTone.Error

        status.contains("needs", ignoreCase = true) ||
            status.contains("sign-in", ignoreCase = true) ||
            status.contains("attention", ignoreCase = true) -> SystemTone.Warning

        else -> SystemTone.Neutral
    }
    val icon = when {
        status.contains("spotify", ignoreCase = true) -> SystemIcon.Spotify
        tone == SystemTone.Error -> SystemIcon.Error
        tone == SystemTone.Warning -> SystemIcon.Warning
        else -> SystemIcon.Info
    }
    val title = when {
        status.contains("browser", ignoreCase = true) -> "Spotify sign-in"
        tone == SystemTone.Error -> "Something needs attention"
        tone == SystemTone.Warning -> "System notice"
        else -> "System update"
    }

    return SystemCardPresentation(
        title = title,
        body = status,
        tone = tone,
        icon = icon,
    )
}

private fun formatPendingToolCard(
    status: ActiveToolStatus,
    statusText: String?,
): SystemCardPresentation {
    val tool = status.tool.lowercase()
    val action = status.action.lowercase()
    val body = when {
        !statusText.isNullOrBlank() && statusText != "Gervaise is thinking." -> statusText
        tool == "spotify" -> when (action) {
            "play" -> "Searching Spotify and checking playback devices."
            "pause" -> "Pausing playback on Spotify."
            "next" -> "Skipping ahead on Spotify."
            "previous" -> "Going back on Spotify."
            "set_volume" -> "Adjusting Spotify volume."
            "select_device" -> "Looking for the requested Spotify device."
            "connect", "get_connection_state" -> "Checking Spotify connection details."
            else -> "Working with Spotify."
        }

        tool == "hue" -> when (action) {
            "set_power" -> "Updating your Hue lights."
            "set_brightness" -> "Adjusting Hue brightness."
            "set_color" -> "Changing Hue color."
            "activate_scene" -> "Activating a Hue scene."
            else -> "Working with Hue."
        }

        else -> "Working with ${tool.replaceFirstChar { it.uppercase() }}."
    }

    val title = when (tool) {
        "spotify" -> when (action) {
            "play" -> "Trying Spotify"
            "connect" -> "Connecting Spotify"
            else -> "Working with Spotify"
        }

        "hue" -> "Updating Hue"
        else -> "Working"
    }

    return SystemCardPresentation(
        title = title,
        body = body,
        tone = SystemTone.Progress,
        icon = when (tool) {
            "spotify" -> SystemIcon.Spotify
            "hue" -> SystemIcon.Hue
            else -> SystemIcon.Assistant
        },
    )
}

private fun formatToolSystemCard(
    displayPayload: String?,
    fallbackPayload: String,
): SystemCardPresentation {
    val visibleCard = displayPayload
        ?.takeIf { it.isNotBlank() }
        ?.let { payload ->
            runCatching { CoreJson.json.parseToJsonElement(payload) as? JsonObject }
                .getOrNull()
        }
        ?.let(::formatVisibleToolCard)
    if (visibleCard != null) {
        return visibleCard
    }

    val payload = runCatching { CoreJson.json.parseToJsonElement(fallbackPayload) }.getOrNull()
    val objectPayload = payload as? JsonObject
    if (objectPayload == null) {
        return SystemCardPresentation(
            title = "System update",
            body = fallbackPayload.trim().ifBlank { "Gervaise completed a tool action." },
            tone = SystemTone.Neutral,
            icon = SystemIcon.Info,
            comparisonText = fallbackPayload.trim(),
        )
    }

    return when (objectPayload.string("tool")?.lowercase()) {
        "spotify" -> formatSpotifyToolCard(objectPayload)
        "hue" -> formatHueToolCard(objectPayload)
        else -> formatGenericToolCard(objectPayload)
    }
}

private fun formatVisibleToolCard(payload: JsonObject): SystemCardPresentation? {
    val title = payload.string("title") ?: return null
    val body = payload.string("body") ?: return null
    return SystemCardPresentation(
        title = title,
        body = body,
        supportingLines = payload.stringArray("supporting_lines").orEmpty(),
        tone = payload.string("tone").toSystemTone(),
        icon = payload.string("icon").toSystemIcon(),
        comparisonText = payload.string("comparison_text") ?: body,
    )
}

private fun String?.toSystemTone(): SystemTone =
    when (this?.lowercase()) {
        "progress" -> SystemTone.Progress
        "positive" -> SystemTone.Positive
        "warning" -> SystemTone.Warning
        "error" -> SystemTone.Error
        else -> SystemTone.Neutral
    }

private fun String?.toSystemIcon(): SystemIcon =
    when (this?.lowercase()) {
        "assistant" -> SystemIcon.Assistant
        "spotify" -> SystemIcon.Spotify
        "hue" -> SystemIcon.Hue
        "device" -> SystemIcon.Device
        "warning" -> SystemIcon.Warning
        "error" -> SystemIcon.Error
        "success" -> SystemIcon.Success
        else -> SystemIcon.Info
    }

private fun formatSpotifyToolCard(payload: JsonObject): SystemCardPresentation {
    val action = payload.string("action").orEmpty()
    val status = payload.string("status").orEmpty()
    val reason = payload.string("reason")
    val code = payload.int("code")
    val message = payload.string("message")?.trim()
    val account = payload.string("account_display_name")
    val targetDevice = payload.objectValue("target_device")?.string("name") ?: payload.string("device_name")
    val track = payload.objectValue("track")
    val trackName = track?.string("name")
    val artists = track?.stringArray("artists").orEmpty()
    val volume = payload.int("volume_percent")
    val deviceNames = payload.arrayValue("devices")
        ?.mapNotNull { (it as? JsonObject)?.string("name") }
        .orEmpty()
        .distinct()
        .take(3)

    return when {
        status == "connected" -> {
            val body = message ?: account?.let { "Spotify is connected as $it." } ?: "Spotify is connected."
            SystemCardPresentation(
                title = "Spotify connection",
                body = body,
                supportingLines = buildList {
                    account?.takeUnless { body.contains(it) }?.let { add("Connected as $it.") }
                    targetDevice?.let { add("Playback device: $it") }
                    payload.string("token_status")
                        ?.takeIf { it == "valid" }
                        ?.let { add("Status: Ready") }
                },
                tone = SystemTone.Positive,
                icon = SystemIcon.Spotify,
            )
        }

        status == "disconnected" -> SystemCardPresentation(
            title = "Spotify disconnected",
            body = message ?: "Spotify playback control is disconnected.",
            tone = SystemTone.Neutral,
            icon = SystemIcon.Spotify,
        )

        status == "auth_started" || status == "connecting" -> SystemCardPresentation(
            title = "Spotify sign-in",
            body = message ?: "Finish signing in with Spotify to continue.",
            supportingLines = listOf("A browser step is required before playback control is available."),
            tone = SystemTone.Progress,
            icon = SystemIcon.Spotify,
        )

        status == "auth_required" || status == "auth_expired" || status == "unconfigured" -> {
            val guidance = spotifyGuidance(reason = reason, code = code, deviceNames = deviceNames)
            SystemCardPresentation(
                title = "Spotify needs attention",
                body = message ?: guidance.first ?: "Spotify needs to reconnect before playback control is available.",
                supportingLines = listOfNotNull(guidance.second),
                tone = SystemTone.Warning,
                icon = SystemIcon.Warning,
            )
        }

        status == "error" -> {
            val guidance = spotifyGuidance(reason = reason, code = code, deviceNames = deviceNames)
            SystemCardPresentation(
                title = spotifyFailureTitle(action),
                body = guidance.first ?: message ?: "Spotify couldn't complete that request.",
                supportingLines = listOfNotNull(guidance.second),
                tone = SystemTone.Error,
                icon = SystemIcon.Error,
            )
        }

        else -> {
            val trackLabel = trackName?.let { name ->
                if (artists.isNotEmpty()) {
                    "$name by ${artists.joinToString()}"
                } else {
                    name
                }
            }
            val successBody = when (action) {
                "play" -> when {
                    trackLabel != null && targetDevice != null -> "Playing $trackLabel on $targetDevice."
                    trackLabel != null -> "Playing $trackLabel."
                    else -> message ?: "Spotify started playback."
                }

                "pause" -> targetDevice?.let { "Playback paused on $it." } ?: message ?: "Spotify paused playback."
                "next" -> message ?: "Skipped to the next track on Spotify."
                "previous" -> message ?: "Went back to the previous track on Spotify."
                "set_volume" -> when {
                    volume != null && targetDevice != null -> "Volume set to $volume% on $targetDevice."
                    volume != null -> "Volume set to $volume%."
                    else -> message ?: "Spotify volume updated."
                }

                "select_device" -> targetDevice?.let { "Playback is now targeting $it." }
                    ?: message
                    ?: "Spotify device updated."

                "get_connection_state" -> message
                    ?: account?.let { "Spotify is connected as $it." }
                    ?: "Spotify connection details updated."

                else -> message ?: "Spotify updated successfully."
            }

            SystemCardPresentation(
                title = spotifySuccessTitle(action),
                body = successBody,
                supportingLines = buildList {
                    if (action != "get_connection_state") {
                        targetDevice
                            ?.takeUnless { successBody.contains(it) }
                            ?.let { add("Playback device: $it") }
                    }
                    account
                        ?.takeUnless { successBody.contains(it) }
                        ?.let { add("Connected as $it.") }
                },
                tone = SystemTone.Neutral,
                icon = SystemIcon.Spotify,
            )
        }
    }
}

private fun formatHueToolCard(payload: JsonObject): SystemCardPresentation {
    val action = payload.string("action").orEmpty()
    val status = payload.string("status").orEmpty()
    val message = payload.string("message")?.trim()

    val title = when {
        status == "error" -> "Hue couldn't complete that"
        action == "set_power" -> "Hue lights"
        action == "set_brightness" -> "Hue brightness"
        action == "set_color" -> "Hue color"
        action == "activate_scene" -> "Hue scene"
        else -> "Hue update"
    }

    return SystemCardPresentation(
        title = title,
        body = message ?: "Hue updated successfully.",
        supportingLines = buildList {
            payload.int("level")?.let { add("Brightness: $it%") }
            payload.string("color")?.let { add("Color: $it") }
            payload.string("scene")?.let { add("Scene: $it") }
        },
        tone = if (status == "error") SystemTone.Error else SystemTone.Positive,
        icon = if (status == "error") SystemIcon.Error else SystemIcon.Hue,
    )
}

private fun formatGenericToolCard(payload: JsonObject): SystemCardPresentation {
    val toolName = payload.string("tool")
        ?.replaceFirstChar { it.uppercase() }
        ?: "Tool"
    val status = payload.string("status")
    val action = payload.string("action")
    val message = payload.string("message")?.trim()

    return SystemCardPresentation(
        title = when {
            status == "error" -> "$toolName couldn't complete that"
            action != null -> "$toolName ${action.replace('_', ' ')}"
            else -> "$toolName update"
        },
        body = message ?: buildString {
            append("$toolName")
            action?.let { append(" ${it.replace('_', ' ')}") }
            status?.let { append(" is $it") }
            append(".")
        },
        tone = if (status == "error") SystemTone.Error else SystemTone.Neutral,
        icon = if (status == "error") SystemIcon.Error else SystemIcon.Info,
    )
}

private fun spotifyFailureTitle(action: String): String =
    when (action) {
        "play" -> "Spotify couldn't start playback"
        "pause" -> "Spotify couldn't pause playback"
        "next" -> "Spotify couldn't skip ahead"
        "previous" -> "Spotify couldn't go back"
        "set_volume" -> "Spotify couldn't change volume"
        "select_device" -> "Spotify couldn't change the device"
        "connect" -> "Spotify couldn't connect"
        else -> "Spotify couldn't complete that"
    }

private fun spotifySuccessTitle(action: String): String =
    when (action) {
        "play", "pause", "next", "previous" -> "Spotify playback"
        "set_volume" -> "Spotify volume"
        "select_device" -> "Spotify device"
        "get_connection_state", "connect" -> "Spotify connection"
        "disconnect" -> "Spotify disconnected"
        else -> "Spotify update"
    }

private fun spotifyGuidance(
    reason: String?,
    code: Int?,
    deviceNames: List<String>,
): Pair<String?, String?> =
    when (reason) {
        "premium_required" -> "Spotify Premium is required for playback control." to null
        "invalid_scope" -> {
            "Playback permissions need to be refreshed before Spotify can continue." to
                "Reconnect Spotify or refresh playback permissions."
        }

        "auth_required", "auth_expired", "auth_error" -> {
            "Spotify needs you to sign in again before playback control is available." to
                "Reconnect your account to continue."
        }

        "no_available_device" -> {
            "No Spotify playback device is available right now." to
                "Open Spotify on a device and try again."
        }

        "device_not_found" -> {
            "Gervaise couldn't find that Spotify device." to
                deviceNames
                    .takeIf { it.isNotEmpty() }
                    ?.joinToString(prefix = "Available devices: ")
        }

        else -> {
            if (code == 403) {
                "Spotify rejected that request." to
                    "Permissions may need to be refreshed, or Spotify Premium may be required."
            } else {
                null to null
            }
        }
    }

private fun normalizeForComparison(value: String): String =
    value.trim().replace(Regex("\\s+"), " ").lowercase()

private fun sanitizeAssistantChatContent(raw: String): String {
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

private fun JsonObject.string(key: String): String? =
    get(key)?.jsonPrimitive?.contentOrNull

private fun JsonObject.int(key: String): Int? =
    get(key)?.jsonPrimitive?.intOrNull

private fun JsonObject.boolean(key: String): Boolean? =
    get(key)?.jsonPrimitive?.booleanOrNull

private fun JsonObject.objectValue(key: String): JsonObject? =
    get(key) as? JsonObject

private fun JsonObject.arrayValue(key: String): JsonArray? =
    get(key) as? JsonArray

private fun JsonObject.stringArray(key: String): List<String>? =
    arrayValue(key)?.mapNotNull { it.jsonPrimitive.contentOrNull }
