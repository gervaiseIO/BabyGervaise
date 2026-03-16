use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::tools::ToolsOverview;
use crate::ContextLevel;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelLogEntry {
    pub created_at: String,
    pub model_name: String,
    pub prompt: String,
    pub raw_output: String,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub latency_ms: i64,
    pub http_status: Option<i64>,
    pub error_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolLogEntry {
    pub created_at: String,
    pub tool_name: String,
    pub action: String,
    pub arguments_json: String,
    pub result_json: String,
    pub success: bool,
    pub latency_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalLogEntry {
    pub created_at: String,
    pub level: ContextLevel,
    pub recent_count: usize,
    pub semantic_count: usize,
    pub query_text: String,
    #[serde(default)]
    pub intent: String,
    #[serde(default)]
    pub latency_ms: i64,
    #[serde(default)]
    pub selected_message_ids_json: String,
    #[serde(default)]
    pub selected_memory_ids_json: String,
    #[serde(default)]
    pub source_breakdown_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelStats {
    pub model_name: String,
    pub total_requests: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub average_latency_ms: i64,
    pub latest_latency_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsageStats {
    pub calls: i64,
    #[serde(default)]
    pub tokens_in: Option<i64>,
    #[serde(default)]
    pub tokens_out: Option<i64>,
    #[serde(default)]
    pub latency_avg_ms: Option<i64>,
    #[serde(default)]
    pub latency_latest_ms: Option<i64>,
    #[serde(default)]
    pub tokens_per_second: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryStats {
    pub message_count: i64,
    pub stored_memories: i64,
    pub vector_count: i64,
    pub retrieval_count: i64,
    #[serde(default)]
    pub active_durable_memories: i64,
    #[serde(default)]
    pub superseded_durable_memories: i64,
    #[serde(default)]
    pub all_durable_memories: i64,
    #[serde(default)]
    pub working_snapshot_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SystemStats {
    pub total_interactions: i64,
    pub tool_calls: i64,
    pub error_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogViewerEntry {
    pub timestamp: String,
    pub prompt: String,
    pub raw_output: String,
    pub latency_ms: i64,
    pub status: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NanoRuntimeStatus {
    pub enabled: bool,
    pub availability: String,
    pub detail: String,
    pub provider: String,
    pub model: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeProfileSummary {
    pub id: String,
    pub label: String,
    pub provider: String,
    pub model: String,
    pub enabled: bool,
    pub available: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeOverview {
    pub nano: NanoRuntimeStatus,
    pub selected_cloud_profile_id: Option<String>,
    pub selected_cloud_profile_label: Option<String>,
    pub cloud_profiles: Vec<RuntimeProfileSummary>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TraceVisibilityClass {
    Public,
    DebugLocal,
    SensitiveLocal,
    SecretRefOnly,
}

impl TraceVisibilityClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::DebugLocal => "debug_local",
            Self::SensitiveLocal => "sensitive_local",
            Self::SecretRefOnly => "secret_ref_only",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracePayloadAttachment {
    pub slot: String,
    pub visibility_class: TraceVisibilityClass,
    pub content_format: String,
    pub content_text: String,
    pub size_bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TraceEventRecord {
    pub turn_id: String,
    pub stage_seq: i64,
    pub timestamp: String,
    pub category: String,
    pub name: String,
    #[serde(default)]
    pub plan_kind: Option<String>,
    #[serde(default)]
    pub fallback_plan_kind: Option<String>,
    #[serde(default)]
    pub reason_codes: Vec<String>,
    #[serde(default)]
    pub context_policy: Option<String>,
    #[serde(default)]
    pub prompt_mode: Option<String>,
    #[serde(default)]
    pub lane: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub latency_ms: Option<i64>,
    #[serde(default)]
    pub http_status: Option<i64>,
    #[serde(default)]
    pub error_text: Option<String>,
    #[serde(default)]
    pub displayed_text: Option<String>,
    #[serde(default)]
    pub selected_refs_json: Option<Value>,
    #[serde(default)]
    pub payloads: Vec<TracePayloadAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TurnTraceSummary {
    pub turn_id: String,
    pub created_at: String,
    pub user_input_summary: String,
    pub input_source: String,
    pub plan_kind: String,
    #[serde(default)]
    pub fallback_plan_kind: Option<String>,
    #[serde(default)]
    pub context_policy: Option<String>,
    #[serde(default)]
    pub model_stages: Vec<String>,
    pub memory_used: bool,
    pub tool_consulted: bool,
    pub tool_used: bool,
    pub nano_first_beat_used: bool,
    pub cloud_escalated: bool,
    pub cloud_used: bool,
    #[serde(default)]
    pub selected_cloud_profile: Option<String>,
    pub delivery_mode: String,
    pub final_route: String,
    #[serde(default)]
    pub error_summary: Option<String>,
    pub total_latency_ms: i64,
    pub final_visible_output: String,
    pub had_fallback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelTraceEntry {
    pub timestamp: String,
    pub turn_id: String,
    pub stage_name: String,
    #[serde(default)]
    pub prompt_mode: Option<String>,
    #[serde(default)]
    pub lane: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    pub status: String,
    pub latency_ms: i64,
    #[serde(default)]
    pub displayed_text: Option<String>,
    #[serde(default)]
    pub discarded_text: Option<String>,
    #[serde(default)]
    pub raw_input: Option<String>,
    #[serde(default)]
    pub raw_output: Option<String>,
    #[serde(default)]
    pub normalized_output: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DecisionTraceEntry {
    pub timestamp: String,
    pub turn_id: String,
    pub name: String,
    #[serde(default)]
    pub plan_kind: Option<String>,
    #[serde(default)]
    pub fallback_plan_kind: Option<String>,
    #[serde(default)]
    pub reason_codes: Vec<String>,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiagnosticIssue {
    pub timestamp: String,
    pub subsystem: String,
    pub level: String,
    pub summary: String,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiagnosticsOverview {
    #[serde(default)]
    pub turn_summaries: Vec<TurnTraceSummary>,
    #[serde(default)]
    pub model_traces: Vec<ModelTraceEntry>,
    #[serde(default)]
    pub decision_events: Vec<DecisionTraceEntry>,
    #[serde(default)]
    pub issues: Vec<DiagnosticIssue>,
    #[serde(default)]
    pub recent_logs: Vec<LogViewerEntry>,
    #[serde(default)]
    pub recent_tool_logs: Vec<ToolLogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverviewSnapshot {
    pub previous_context: ContextLevel,
    pub model_stats: ModelStats,
    #[serde(default)]
    pub cloud_stats: UsageStats,
    #[serde(default)]
    pub nano_stats: UsageStats,
    pub memory_stats: MemoryStats,
    pub system_stats: SystemStats,
    pub runtime: RuntimeOverview,
    #[serde(default)]
    pub tools: ToolsOverview,
    #[serde(default)]
    pub diagnostics: DiagnosticsOverview,
    #[serde(default)]
    pub tool_states: Map<String, Value>,
    #[serde(default)]
    pub recent_logs: Vec<LogViewerEntry>,
    #[serde(default)]
    pub recent_tool_logs: Vec<ToolLogEntry>,
    #[serde(default)]
    pub turn_summaries: Vec<TurnTraceSummary>,
    #[serde(default)]
    pub model_traces: Vec<ModelTraceEntry>,
    #[serde(default)]
    pub decision_events: Vec<DecisionTraceEntry>,
}
