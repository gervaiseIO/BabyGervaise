package io.gervaise.babygervaise.bridge

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.decodeFromString
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement

object CoreJson {
    val json = Json {
        ignoreUnknownKeys = true
    }

    private val prettyJson = Json {
        ignoreUnknownKeys = true
        prettyPrint = true
    }

    fun decodeBootstrapState(payloadJson: String): BootstrapState =
        json.decodeFromString(payloadJson)

    fun decodeOverviewSnapshot(payloadJson: String): OverviewSnapshot =
        json.decodeFromString(payloadJson)

    fun decodeToolExecutionResult(payloadJson: String): ToolExecutionResult =
        json.decodeFromString(payloadJson)

    fun prettyPrint(value: JsonElement): String =
        prettyJson.encodeToString(JsonElement.serializer(), value)

    fun decodeEvent(eventType: String, payloadJson: String): CoreEvent =
        when (eventType) {
            "assistant_started" -> {
                val payload = json.decodeFromString<TurnPayload>(payloadJson)
                CoreEvent.AssistantStarted(turnId = payload.turnId)
            }

            "assistant_chunk" -> {
                val payload = json.decodeFromString<AssistantChunkPayload>(payloadJson)
                CoreEvent.AssistantChunk(turnId = payload.turnId, chunk = payload.chunk)
            }

            "assistant_completed" -> {
                val payload = json.decodeFromString<AssistantCompletedPayload>(payloadJson)
                CoreEvent.AssistantCompleted(turnId = payload.turnId, message = payload.message)
            }

            "tool_status" -> {
                val payload = json.decodeFromString<ToolStatusPayload>(payloadJson)
                CoreEvent.ToolStatus(
                    turnId = payload.turnId,
                    tool = payload.tool,
                    action = payload.action,
                    status = payload.status,
                )
            }

            "open_external_url" -> {
                val payload = json.decodeFromString<OpenExternalUrlPayload>(payloadJson)
                CoreEvent.OpenExternalUrl(
                    turnId = payload.turnId,
                    url = payload.url,
                    purpose = payload.purpose,
                )
            }

            "assistant_error" -> {
                val payload = json.decodeFromString<AssistantErrorPayload>(payloadJson)
                CoreEvent.AssistantError(turnId = payload.turnId, error = payload.error)
            }

            "config_updated" -> {
                val payload = json.decodeFromString<ConfigUpdatedPayload>(payloadJson)
                CoreEvent.ConfigUpdated(level = payload.level)
            }

            "diagnostic_log" -> {
                val payload = json.decodeFromString<DebugLogEntry>(payloadJson)
                CoreEvent.DebugLog(entry = payload)
            }

            else -> error("Unsupported core event type: $eventType")
        }

    @Serializable
    private data class TurnPayload(
        @SerialName("turnId")
        val turnId: String,
    )

    @Serializable
    private data class AssistantChunkPayload(
        @SerialName("turnId")
        val turnId: String,
        val chunk: String,
    )

    @Serializable
    private data class AssistantCompletedPayload(
        @SerialName("turnId")
        val turnId: String,
        val message: ChatMessage,
    )

    @Serializable
    private data class ToolStatusPayload(
        @SerialName("turnId")
        val turnId: String,
        val tool: String,
        val action: String,
        val status: String,
    )

    @Serializable
    private data class OpenExternalUrlPayload(
        @SerialName("turnId")
        val turnId: String,
        val url: String,
        val purpose: String,
    )

    @Serializable
    private data class AssistantErrorPayload(
        @SerialName("turnId")
        val turnId: String? = null,
        val error: String,
    )

    @Serializable
    private data class ConfigUpdatedPayload(
        val level: ContextLevel,
    )
}
