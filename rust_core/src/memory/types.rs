use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::logging::TurnTraceSummary;
use crate::{ChatMessage, ContextLevel, InputSource, MessageContentType};

use super::durable::RetrievedMemory;

#[derive(Debug, Clone)]
pub struct AppendMessageRequest {
    pub role: String,
    pub content: String,
    pub turn_id: String,
    pub input_source: InputSource,
    pub content_type: MessageContentType,
    pub display_json: Option<String>,
    pub visible_summary: Option<String>,
    pub meta_json: Option<Value>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TranscriptSliceQuery {
    pub context_level: ContextLevel,
    #[serde(default)]
    pub exclude_message_id: Option<i64>,
    #[serde(default)]
    pub limit_override: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct DurableRecallQuery {
    pub query_text: String,
    pub context_level: ContextLevel,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DurableMemoryCounts {
    pub active_count: i64,
    pub superseded_count: i64,
    pub all_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DurableMemoryRecord {
    pub id: i64,
    pub kind: String,
    pub text: String,
    pub salience: f32,
    #[serde(default)]
    pub source_message_id: Option<i64>,
    pub created_at: String,
    #[serde(default)]
    pub canonical_key: Option<String>,
    pub status: String,
    #[serde(default)]
    pub source_type: Option<String>,
    #[serde(default)]
    pub source_ref: Option<String>,
    #[serde(default)]
    pub supersedes_memory_id: Option<i64>,
    #[serde(default)]
    pub last_recalled_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkingThreadStatus {
    #[default]
    Focused,
    Active,
    Cooling,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkingThread {
    pub key: String,
    pub status: WorkingThreadStatus,
    pub topic_label: String,
    pub synopsis: String,
    pub last_touched_turn_id: String,
    pub last_touched_at: String,
    #[serde(default)]
    pub message_refs: Vec<i64>,
    #[serde(default)]
    pub durable_memory_ids: Vec<i64>,
    #[serde(default)]
    pub score: f32,
    #[serde(default)]
    pub stale_turns: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EntityAnchor {
    pub key: String,
    pub label: String,
    pub kind: String,
    pub thread_key: String,
    pub last_seen_turn_id: String,
    #[serde(default)]
    pub stale_turns: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OpenLoop {
    pub key: String,
    pub label: String,
    pub thread_key: String,
    pub opened_turn_id: String,
    pub last_touched_turn_id: String,
    pub expires_after_turns: u32,
    #[serde(default)]
    pub stale_turns: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InteractionConstraint {
    pub key: String,
    pub text: String,
    pub source: String,
    pub last_confirmed_turn_id: String,
    pub expires_after_turns: u32,
    #[serde(default)]
    pub stale_turns: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkingToolOutcome {
    pub tool_name: String,
    pub action: String,
    pub summary: String,
    pub turn_id: String,
    pub created_at: String,
    #[serde(default)]
    pub thread_key: String,
    #[serde(default)]
    pub stale_turns: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkingMemorySourceRefs {
    #[serde(default)]
    pub message_ids: Vec<i64>,
    #[serde(default)]
    pub durable_memory_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkingMemorySnapshot {
    pub turn_id: String,
    pub created_at: String,
    pub version: i64,
    #[serde(default)]
    pub focus_thread_key: Option<String>,
    #[serde(default)]
    pub threads: Vec<WorkingThread>,
    #[serde(default)]
    pub entity_anchors: Vec<EntityAnchor>,
    #[serde(default)]
    pub open_loops: Vec<OpenLoop>,
    #[serde(default)]
    pub interaction_constraints: Vec<InteractionConstraint>,
    #[serde(default)]
    pub recent_tool_outcomes: Vec<WorkingToolOutcome>,
    #[serde(default)]
    pub source_refs: WorkingMemorySourceRefs,
}

#[derive(Debug, Clone)]
pub struct HydrateWorkingMemoryRequest {
    pub turn_id: String,
    pub user_text: String,
    pub context_level: ContextLevel,
    pub exclude_message_id: Option<i64>,
    pub latest_turn_summary: Option<TurnTraceSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ThreadBridge {
    pub thread_key: String,
    pub synopsis: String,
    pub status: WorkingThreadStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WarmContext {
    #[serde(default)]
    pub recent_messages: Vec<ChatMessage>,
    #[serde(default)]
    pub thread_bridges: Vec<ThreadBridge>,
    #[serde(default)]
    pub durable_memories: Vec<RetrievedMemory>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContinuityMode {
    OnThread,
    Pivot,
    Return,
    OpenLoop,
    #[default]
    Ambiguous,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ContinuitySignal {
    #[serde(default)]
    pub mode: ContinuityMode,
    #[serde(default)]
    pub focused_thread_key: Option<String>,
    #[serde(default)]
    pub focused_thread_label: Option<String>,
    #[serde(default)]
    pub matched_thread_key: Option<String>,
    #[serde(default)]
    pub matched_thread_status: Option<WorkingThreadStatus>,
    #[serde(default)]
    pub matched_open_loop_key: Option<String>,
    #[serde(default)]
    pub open_loop_match: bool,
    #[serde(default)]
    pub continuity_confidence: f32,
    #[serde(default)]
    pub selected_thread_message_ids: Vec<i64>,
    #[serde(default)]
    pub selected_thread_memory_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecallExplanation {
    pub memory_used: bool,
    pub strong_hit: bool,
    #[serde(default)]
    pub continuity: ContinuitySignal,
    #[serde(default)]
    pub source_breakdown: Value,
    #[serde(default)]
    pub selected_message_ids: Vec<i64>,
    #[serde(default)]
    pub selected_memory_ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecallStats {
    pub latency_ms: i64,
    pub recent_count: usize,
    pub semantic_count: usize,
    pub working_thread_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecallBudget {
    #[serde(default)]
    pub max_recent_messages: Option<usize>,
    #[serde(default)]
    pub max_durable_memories: Option<usize>,
    #[serde(default)]
    pub include_cold_candidates: bool,
}

#[derive(Debug, Clone)]
pub struct RecallRequest {
    pub turn_id: String,
    pub query_text: String,
    pub intent: String,
    pub context_level: ContextLevel,
    pub budget: RecallBudget,
    pub working_memory: Option<WorkingMemorySnapshot>,
    pub exclude_message_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecallBundle {
    pub working_memory: WorkingMemorySnapshot,
    pub warm_context: WarmContext,
    #[serde(default)]
    pub cold_candidates: Vec<RetrievedMemory>,
    pub refs_used: WorkingMemorySourceRefs,
    pub explanation: RecallExplanation,
    pub stats: RecallStats,
}

#[derive(Debug, Clone)]
pub struct RefreshWorkingMemoryRequest {
    pub turn_id: String,
    pub user_text: String,
    pub assistant_visible_text: String,
    pub tool_summary: Option<String>,
    pub current_turn_message_ids: Vec<i64>,
    pub recall_bundle: RecallBundle,
    pub previous_snapshot: Option<WorkingMemorySnapshot>,
}

#[derive(Debug, Clone, Default)]
pub struct PersistTurnRequest {
    pub turn_summary: Option<TurnTraceSummary>,
    pub working_memory: Option<WorkingMemorySnapshot>,
}
