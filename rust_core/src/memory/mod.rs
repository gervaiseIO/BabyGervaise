mod durable;
mod observability;
mod recall;
mod service;
mod settings;
mod sqlite;
mod tool_state;
mod transcript;
mod types;
mod vector;
mod working;

use anyhow::Result;
use serde_json::Value;

use crate::logging::{
    ModelLogEntry, OverviewSnapshot, RetrievalLogEntry, RuntimeOverview, ToolLogEntry,
    TraceEventRecord, TurnTraceSummary,
};
use crate::tools::ToolsOverview;
use crate::{ChatMessage, ContextLevel};

pub use durable::{GroundedMemoryWrite, RetrievedMemory, SqliteDurableMemoryStore};
pub use observability::SqliteMemoryObservabilityStore;
pub use recall::DefaultMemoryRecallEngine;
pub use service::{MemoryPolicy, MemoryService, MemoryStore};
pub use settings::SqliteSettingsStore;
pub use sqlite::SqliteMemoryDb;
pub use tool_state::SqliteToolStateStore;
pub use transcript::SqliteTranscriptStore;
pub use types::{
    AppendMessageRequest, ContinuityMode, ContinuitySignal, DurableMemoryCounts,
    DurableMemoryRecord, DurableRecallQuery, HydrateWorkingMemoryRequest, PersistTurnRequest,
    RecallBudget, RecallBundle, RecallExplanation, RecallRequest, RecallStats,
    RefreshWorkingMemoryRequest, ThreadBridge, TranscriptSliceQuery, WarmContext,
    WorkingMemorySnapshot, WorkingMemorySourceRefs, WorkingThread, WorkingThreadStatus,
    WorkingToolOutcome,
};
pub use vector::{cosine_similarity, vectorize_text};
pub use working::SqliteWorkingMemoryStore;

pub trait TranscriptStore: Send + Sync {
    fn append_message(&self, request: AppendMessageRequest) -> Result<ChatMessage>;
    fn load_all_messages(&self) -> Result<Vec<ChatMessage>>;
    fn load_recent_messages(&self, query: TranscriptSliceQuery) -> Result<Vec<ChatMessage>>;
    fn load_turn_messages(&self, turn_id: &str) -> Result<Vec<ChatMessage>>;
    fn load_messages_by_ids(&self, message_ids: &[i64]) -> Result<Vec<ChatMessage>>;
}

pub trait WorkingMemoryStore: Send + Sync {
    fn load_latest_snapshot(&self) -> Result<Option<WorkingMemorySnapshot>>;
    fn load_snapshot_for_turn(&self, turn_id: &str) -> Result<Option<WorkingMemorySnapshot>>;
    fn save_snapshot(&self, snapshot: &WorkingMemorySnapshot) -> Result<()>;
    fn count_snapshots(&self) -> Result<i64>;
}

pub trait DurableMemoryStore: Send + Sync {
    type Hit;

    fn promote_grounded_memory(
        &self,
        request: &GroundedMemoryWrite<'_>,
    ) -> Result<DurableMemoryRecord>;
    fn search_active(&self, query: &DurableRecallQuery) -> Result<Vec<Self::Hit>>;
    fn load_active_by_ids(&self, memory_ids: &[i64]) -> Result<Vec<Self::Hit>>;
    fn mark_recalled(&self, memory_ids: &[i64], recalled_at: &str) -> Result<()>;
    fn load_counts(&self) -> Result<DurableMemoryCounts>;
}

pub trait MemoryRecallEngine: Send + Sync {
    fn hydrate_working_memory(
        &self,
        request: &HydrateWorkingMemoryRequest,
    ) -> Result<WorkingMemorySnapshot>;
    fn recall(&self, request: &RecallRequest) -> Result<RecallBundle>;
    fn refresh_working_memory(
        &self,
        request: &RefreshWorkingMemoryRequest,
    ) -> Result<WorkingMemorySnapshot>;
}

pub trait ToolStateStore: Send + Sync {
    fn get_tool_state(&self, tool_name: &str) -> Result<Option<Value>>;
    fn set_tool_state(&self, tool_name: &str, state_json: &Value, updated_at: &str) -> Result<()>;
}

pub trait MemoryObservabilityStore: Send + Sync {
    fn log_recall(&self, entry: &RetrievalLogEntry) -> Result<()>;
    fn log_model_call(&self, entry: &ModelLogEntry) -> Result<()>;
    fn log_tool_call(&self, entry: &ToolLogEntry) -> Result<()>;
    fn upsert_turn_summary(&self, summary: &TurnTraceSummary) -> Result<()>;
    fn append_trace_event(&self, event: &TraceEventRecord) -> Result<()>;
    fn load_overview(
        &self,
        previous_context: ContextLevel,
        runtime: RuntimeOverview,
        model_name: &str,
        tools: ToolsOverview,
    ) -> Result<OverviewSnapshot>;
}
