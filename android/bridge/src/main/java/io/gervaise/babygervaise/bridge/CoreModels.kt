package io.gervaise.babygervaise.bridge

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement

@Serializable
enum class ContextLevel {
    @SerialName("low")
    LOW,

    @SerialName("medium")
    MEDIUM,

    @SerialName("high")
    HIGH,
    ;

    val wireName: String
        get() = name.lowercase()

    val displayName: String
        get() = name.lowercase().replaceFirstChar { it.uppercase() }
}

@Serializable
enum class InputSource {
    @SerialName("text")
    TEXT,

    @SerialName("voice")
    VOICE,
}

@Serializable
data class ChatMessage(
    val id: Long,
    val role: String,
    val content: String,
    @SerialName("turn_id")
    val turnId: String,
    @SerialName("input_source")
    val inputSource: InputSource,
    @SerialName("created_at")
    val createdAt: String,
)

@Serializable
data class BootstrapState(
    @SerialName("previous_context")
    val previousContext: ContextLevel,
    val messages: List<ChatMessage>,
) {
    companion object {
        val Empty = BootstrapState(
            previousContext = ContextLevel.MEDIUM,
            messages = emptyList(),
        )
    }
}

@Serializable
data class ModelStats(
    @SerialName("model_name")
    val modelName: String,
    @SerialName("total_requests")
    val totalRequests: Long,
    @SerialName("total_input_tokens")
    val totalInputTokens: Long,
    @SerialName("total_output_tokens")
    val totalOutputTokens: Long,
    @SerialName("average_latency_ms")
    val averageLatencyMs: Long,
    @SerialName("latest_latency_ms")
    val latestLatencyMs: Long,
)

@Serializable
data class MemoryStats(
    @SerialName("message_count")
    val messageCount: Long,
    @SerialName("stored_memories")
    val storedMemories: Long,
    @SerialName("vector_count")
    val vectorCount: Long,
    @SerialName("retrieval_count")
    val retrievalCount: Long,
)

@Serializable
data class SystemStats(
    @SerialName("total_interactions")
    val totalInteractions: Long,
    @SerialName("tool_calls")
    val toolCalls: Long,
    @SerialName("error_count")
    val errorCount: Long,
)

@Serializable
data class LogViewerEntry(
    val timestamp: String,
    val prompt: String,
    @SerialName("raw_output")
    val rawOutput: String,
    @SerialName("latency_ms")
    val latencyMs: Long,
    val status: Long? = null,
)

@Serializable
data class OverviewSnapshot(
    @SerialName("previous_context")
    val previousContext: ContextLevel,
    @SerialName("model_stats")
    val modelStats: ModelStats,
    @SerialName("memory_stats")
    val memoryStats: MemoryStats,
    @SerialName("system_stats")
    val systemStats: SystemStats,
    @SerialName("tool_states")
    val toolStates: Map<String, JsonElement>,
    @SerialName("recent_logs")
    val recentLogs: List<LogViewerEntry>,
) {
    companion object {
        val Empty = OverviewSnapshot(
            previousContext = ContextLevel.MEDIUM,
            modelStats = ModelStats(
                modelName = "unconfigured",
                totalRequests = 0,
                totalInputTokens = 0,
                totalOutputTokens = 0,
                averageLatencyMs = 0,
                latestLatencyMs = 0,
            ),
            memoryStats = MemoryStats(
                messageCount = 0,
                storedMemories = 0,
                vectorCount = 0,
                retrievalCount = 0,
            ),
            systemStats = SystemStats(
                totalInteractions = 0,
                toolCalls = 0,
                errorCount = 0,
            ),
            toolStates = emptyMap(),
            recentLogs = emptyList(),
        )
    }
}

sealed interface CoreEvent {
    data class AssistantStarted(val turnId: String) : CoreEvent

    data class AssistantChunk(
        val turnId: String,
        val chunk: String,
    ) : CoreEvent

    data class AssistantCompleted(
        val turnId: String,
        val message: ChatMessage,
    ) : CoreEvent

    data class ToolStatus(
        val turnId: String,
        val tool: String,
        val action: String,
        val status: String,
    ) : CoreEvent

    data class OpenExternalUrl(
        val turnId: String,
        val url: String,
        val purpose: String,
    ) : CoreEvent

    data class AssistantError(
        val turnId: String?,
        val error: String,
    ) : CoreEvent

    data class ConfigUpdated(
        val level: ContextLevel,
    ) : CoreEvent
}
