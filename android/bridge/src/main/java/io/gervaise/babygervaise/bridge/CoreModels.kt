package io.gervaise.babygervaise.bridge

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject

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
enum class MessageContentType {
    @SerialName("plain_text")
    PLAIN_TEXT,

    @SerialName("tool_result")
    TOOL_RESULT,
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
    @SerialName("content_type")
    val contentType: MessageContentType = MessageContentType.PLAIN_TEXT,
    @SerialName("display_json")
    val displayJson: String? = null,
    @SerialName("visible_summary")
    val visibleSummary: String? = null,
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
data class UsageStats(
    val calls: Long = 0,
    @SerialName("tokens_in")
    val tokensIn: Long? = null,
    @SerialName("tokens_out")
    val tokensOut: Long? = null,
    @SerialName("latency_avg_ms")
    val latencyAvgMs: Long? = null,
    @SerialName("latency_latest_ms")
    val latencyLatestMs: Long? = null,
    @SerialName("tokens_per_second")
    val tokensPerSecond: Long? = null,
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
data class ToolLogEntry(
    @SerialName("created_at")
    val createdAt: String,
    @SerialName("tool_name")
    val toolName: String,
    val action: String,
    @SerialName("arguments_json")
    val argumentsJson: String,
    @SerialName("result_json")
    val resultJson: String,
    val success: Boolean,
    @SerialName("latency_ms")
    val latencyMs: Long,
)

@Serializable
data class TurnTraceSummary(
    @SerialName("turn_id")
    val turnId: String,
    @SerialName("created_at")
    val createdAt: String,
    @SerialName("user_input_summary")
    val userInputSummary: String,
    @SerialName("input_source")
    val inputSource: String = "text",
    @SerialName("plan_kind")
    val planKind: String,
    @SerialName("fallback_plan_kind")
    val fallbackPlanKind: String? = null,
    @SerialName("context_policy")
    val contextPolicy: String? = null,
    @SerialName("model_stages")
    val modelStages: List<String> = emptyList(),
    @SerialName("memory_used")
    val memoryUsed: Boolean = false,
    @SerialName("tool_consulted")
    val toolConsulted: Boolean = false,
    @SerialName("tool_used")
    val toolUsed: Boolean,
    @SerialName("nano_first_beat_used")
    val nanoFirstBeatUsed: Boolean = false,
    @SerialName("cloud_escalated")
    val cloudEscalated: Boolean = false,
    @SerialName("cloud_used")
    val cloudUsed: Boolean,
    @SerialName("selected_cloud_profile")
    val selectedCloudProfile: String? = null,
    @SerialName("delivery_mode")
    val deliveryMode: String = "PENDING",
    @SerialName("final_route")
    val finalRoute: String = "pending",
    @SerialName("error_summary")
    val errorSummary: String? = null,
    @SerialName("total_latency_ms")
    val totalLatencyMs: Long,
    @SerialName("final_visible_output")
    val finalVisibleOutput: String,
    @SerialName("had_fallback")
    val hadFallback: Boolean,
)

@Serializable
data class ModelTraceEntry(
    val timestamp: String,
    @SerialName("turn_id")
    val turnId: String,
    @SerialName("stage_name")
    val stageName: String,
    @SerialName("prompt_mode")
    val promptMode: String? = null,
    val lane: String? = null,
    val provider: String? = null,
    val model: String? = null,
    val status: String,
    @SerialName("latency_ms")
    val latencyMs: Long,
    @SerialName("displayed_text")
    val displayedText: String? = null,
    @SerialName("discarded_text")
    val discardedText: String? = null,
    @SerialName("raw_input")
    val rawInput: String? = null,
    @SerialName("raw_output")
    val rawOutput: String? = null,
    @SerialName("normalized_output")
    val normalizedOutput: String? = null,
)

@Serializable
data class DecisionTraceEntry(
    val timestamp: String,
    @SerialName("turn_id")
    val turnId: String,
    val name: String,
    @SerialName("plan_kind")
    val planKind: String? = null,
    @SerialName("fallback_plan_kind")
    val fallbackPlanKind: String? = null,
    @SerialName("reason_codes")
    val reasonCodes: List<String> = emptyList(),
    val detail: String? = null,
)

@Serializable
data class ToolActionAvailability(
    @SerialName("action_id")
    val actionId: String,
    val label: String,
    val enabled: Boolean,
    val reason: String? = null,
)

@Serializable
data class ToolDetailLine(
    val label: String,
    val value: String,
)

@Serializable
data class ToolOverviewEntry(
    @SerialName("tool_id")
    val toolId: String,
    @SerialName("display_name")
    val displayName: String,
    val category: String,
    val available: Boolean,
    val integrated: Boolean,
    @SerialName("auth_state")
    val authState: String,
    @SerialName("health_state")
    val healthState: String,
    @SerialName("next_step")
    val nextStep: String,
    val summary: String,
    @SerialName("account_label")
    val accountLabel: String? = null,
    @SerialName("capability_summary")
    val capabilitySummary: String? = null,
    @SerialName("detail_lines")
    val detailLines: List<ToolDetailLine> = emptyList(),
    val actions: List<ToolActionAvailability> = emptyList(),
)

@Serializable
data class ToolsOverview(
    val catalog: List<ToolOverviewEntry> = emptyList(),
    @SerialName("available_tools")
    val availableTools: List<String> = emptyList(),
    @SerialName("integrated_tools")
    val integratedTools: List<String> = emptyList(),
)

@Serializable
data class DiagnosticIssue(
    val timestamp: String,
    val subsystem: String,
    val level: String,
    val summary: String,
    val detail: String? = null,
)

@Serializable
data class DiagnosticsOverview(
    @SerialName("turn_summaries")
    val turnSummaries: List<TurnTraceSummary> = emptyList(),
    @SerialName("model_traces")
    val modelTraces: List<ModelTraceEntry> = emptyList(),
    @SerialName("decision_events")
    val decisionEvents: List<DecisionTraceEntry> = emptyList(),
    val issues: List<DiagnosticIssue> = emptyList(),
    @SerialName("recent_logs")
    val recentLogs: List<LogViewerEntry> = emptyList(),
    @SerialName("recent_tool_logs")
    val recentToolLogs: List<ToolLogEntry> = emptyList(),
)

@Serializable
data class ToolExecutionResult(
    val tool: String,
    val action: String,
    val summary: String,
    @SerialName("state_json")
    val stateJson: JsonElement,
    @SerialName("result_json")
    val resultJson: JsonElement,
)

@Serializable
data class NoteActivityEvent(
    @SerialName("note_key")
    val noteKey: String,
    @SerialName("relative_path")
    val relativePath: String,
    @SerialName("title_snapshot")
    val titleSnapshot: String,
    @SerialName("event_type")
    val eventType: String,
    @SerialName("occurred_at")
    val occurredAt: String,
)

@Serializable
data class NanoRuntimeStatus(
    val enabled: Boolean,
    val availability: String,
    val detail: String,
    val provider: String,
    val model: String,
    val active: Boolean,
)

@Serializable
data class RuntimeProfileSummary(
    val id: String,
    val label: String,
    val provider: String,
    val model: String,
    val enabled: Boolean,
    val available: Boolean,
    val selected: Boolean,
)

@Serializable
data class RuntimeOverview(
    val nano: NanoRuntimeStatus,
    @SerialName("selected_cloud_profile_id")
    val selectedCloudProfileId: String? = null,
    @SerialName("selected_cloud_profile_label")
    val selectedCloudProfileLabel: String? = null,
    @SerialName("cloud_profiles")
    val cloudProfiles: List<RuntimeProfileSummary> = emptyList(),
)

@Serializable
data class OverviewSnapshot(
    @SerialName("previous_context")
    val previousContext: ContextLevel,
    @SerialName("model_stats")
    val modelStats: ModelStats,
    @SerialName("cloud_stats")
    val cloudStats: UsageStats = UsageStats(),
    @SerialName("nano_stats")
    val nanoStats: UsageStats = UsageStats(),
    @SerialName("memory_stats")
    val memoryStats: MemoryStats,
    @SerialName("system_stats")
    val systemStats: SystemStats,
    val runtime: RuntimeOverview,
    val tools: ToolsOverview = ToolsOverview(),
    val diagnostics: DiagnosticsOverview = DiagnosticsOverview(),
    @SerialName("tool_states")
    val toolStates: Map<String, JsonElement>,
    @SerialName("recent_logs")
    val recentLogs: List<LogViewerEntry>,
    @SerialName("recent_tool_logs")
    val recentToolLogs: List<ToolLogEntry>,
    @SerialName("turn_summaries")
    val turnSummaries: List<TurnTraceSummary> = emptyList(),
    @SerialName("model_traces")
    val modelTraces: List<ModelTraceEntry> = emptyList(),
    @SerialName("decision_events")
    val decisionEvents: List<DecisionTraceEntry> = emptyList(),
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
            cloudStats = UsageStats(),
            nanoStats = UsageStats(),
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
            runtime = RuntimeOverview(
                nano = NanoRuntimeStatus(
                    enabled = false,
                    availability = "unavailable",
                    detail = "Nano is unavailable.",
                    provider = "gemini",
                    model = "gemini-nano",
                    active = false,
                ),
            ),
            tools = ToolsOverview(),
            diagnostics = DiagnosticsOverview(),
            toolStates = emptyMap(),
            recentLogs = emptyList(),
            recentToolLogs = emptyList(),
            turnSummaries = emptyList(),
            modelTraces = emptyList(),
            decisionEvents = emptyList(),
        )
    }
}

@Serializable
data class DebugLogEntry(
    val subsystem: String,
    val level: String,
    val message: String,
    @SerialName("turn_id")
    val turnId: String? = null,
    val fields: JsonObject? = null,
)

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

    data class DebugLog(
        val entry: DebugLogEntry,
    ) : CoreEvent
}
