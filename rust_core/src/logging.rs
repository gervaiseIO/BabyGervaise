use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalLogEntry {
    pub created_at: String,
    pub level: ContextLevel,
    pub recent_count: usize,
    pub semantic_count: usize,
    pub query_text: String,
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
pub struct MemoryStats {
    pub message_count: i64,
    pub stored_memories: i64,
    pub vector_count: i64,
    pub retrieval_count: i64,
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
pub struct OverviewSnapshot {
    pub previous_context: ContextLevel,
    pub model_stats: ModelStats,
    pub memory_stats: MemoryStats,
    pub system_stats: SystemStats,
    pub tool_states: Map<String, Value>,
    pub recent_logs: Vec<LogViewerEntry>,
}
