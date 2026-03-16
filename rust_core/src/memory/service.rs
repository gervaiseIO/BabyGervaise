use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;

use crate::logging::{
    ModelLogEntry, OverviewSnapshot, RetrievalLogEntry, RuntimeOverview, ToolLogEntry,
    TraceEventRecord, TurnTraceSummary,
};
use crate::tools::ToolsOverview;
use crate::{
    now_rfc3339, AppConfig, BootstrapState, ChatMessage, ContextLevel, InputSource,
    MessageContentType,
};

use super::durable::{GroundedMemoryWrite, RetrievedMemory, SqliteDurableMemoryStore};
use super::observability::SqliteMemoryObservabilityStore;
use super::recall::DefaultMemoryRecallEngine;
use super::settings::SqliteSettingsStore;
use super::sqlite::SqliteMemoryDb;
use super::tool_state::SqliteToolStateStore;
use super::transcript::SqliteTranscriptStore;
use super::types::{
    AppendMessageRequest, HydrateWorkingMemoryRequest, PersistTurnRequest, RecallBundle,
    RecallRequest, RefreshWorkingMemoryRequest, TranscriptSliceQuery, WorkingMemorySnapshot,
};
use super::working::SqliteWorkingMemoryStore;
use super::{
    DurableMemoryStore, MemoryObservabilityStore, MemoryRecallEngine, ToolStateStore,
    TranscriptStore, WorkingMemoryStore,
};

#[derive(Debug, Clone)]
pub struct MemoryPolicy {
    pub default_previous_context: ContextLevel,
    pub vector_dimensions: usize,
    pub max_recent_messages_per_turn: usize,
    pub working_max_threads: usize,
    pub working_max_entities: usize,
    pub working_max_open_loops: usize,
    pub working_max_tool_outcomes: usize,
    pub cooling_turn_ttl: u32,
    pub constraint_ttl_turns: u32,
    pub recall_similarity_floor: f32,
    pub strong_hit_score: f32,
    pub strong_hit_similarity: f32,
    pub strong_hit_salience: f32,
    pub max_model_logs: usize,
}

impl MemoryPolicy {
    pub fn from_app_config(app_config: &AppConfig) -> Self {
        Self {
            default_previous_context: app_config.default_previous_context,
            vector_dimensions: app_config.vector_dimensions,
            max_recent_messages_per_turn: app_config.max_recent_messages_per_turn,
            working_max_threads: 3,
            working_max_entities: 8,
            working_max_open_loops: 4,
            working_max_tool_outcomes: 2,
            cooling_turn_ttl: 6,
            constraint_ttl_turns: 2,
            recall_similarity_floor: 0.05,
            strong_hit_score: 0.66,
            strong_hit_similarity: 0.45,
            strong_hit_salience: 0.65,
            max_model_logs: app_config.max_model_logs,
        }
    }
}

#[derive(Clone)]
pub struct MemoryService {
    settings: SqliteSettingsStore,
    transcript: SqliteTranscriptStore,
    durable: SqliteDurableMemoryStore,
    working: SqliteWorkingMemoryStore,
    tool_state: SqliteToolStateStore,
    observability: SqliteMemoryObservabilityStore,
    recall: DefaultMemoryRecallEngine,
    policy: MemoryPolicy,
}

impl MemoryService {
    pub fn new(db_path: PathBuf, app_config: &AppConfig) -> Result<Self> {
        let policy = MemoryPolicy::from_app_config(app_config);
        let db = Arc::new(SqliteMemoryDb::new(db_path)?);
        let settings = SqliteSettingsStore::new(db.clone());
        settings.ensure_previous_context(policy.default_previous_context)?;
        let transcript =
            SqliteTranscriptStore::new(db.clone(), policy.max_recent_messages_per_turn);
        let durable = SqliteDurableMemoryStore::new(db.clone(), policy.vector_dimensions);
        let working = SqliteWorkingMemoryStore::new(db.clone());
        let tool_state = SqliteToolStateStore::new(db.clone());
        let observability = SqliteMemoryObservabilityStore::new(db, policy.max_model_logs);
        let recall = DefaultMemoryRecallEngine::new(
            policy.clone(),
            transcript.clone(),
            durable.clone(),
            working.clone(),
        );

        Ok(Self {
            settings,
            transcript,
            durable,
            working,
            tool_state,
            observability,
            recall,
            policy,
        })
    }

    pub fn policy(&self) -> &MemoryPolicy {
        &self.policy
    }

    pub fn persist_turn(&self, request: PersistTurnRequest) -> Result<()> {
        if let Some(summary) = request.turn_summary.as_ref() {
            self.observability.upsert_turn_summary(summary)?;
        }
        if let Some(snapshot) = request.working_memory.as_ref() {
            self.working.save_snapshot(snapshot)?;
        }
        Ok(())
    }

    pub fn hydrate_working_memory(
        &self,
        request: &HydrateWorkingMemoryRequest,
    ) -> Result<WorkingMemorySnapshot> {
        self.recall.hydrate_working_memory(request)
    }

    pub fn refresh_working_memory(
        &self,
        request: &RefreshWorkingMemoryRequest,
    ) -> Result<WorkingMemorySnapshot> {
        let snapshot = self.recall.refresh_working_memory(request)?;
        self.working.save_snapshot(&snapshot)?;
        Ok(snapshot)
    }

    pub fn recall(&self, request: &RecallRequest) -> Result<RecallBundle> {
        let bundle = self.recall.recall(request)?;
        self.observability.log_recall(&RetrievalLogEntry {
            created_at: now_rfc3339(),
            level: request.context_level,
            recent_count: bundle.stats.recent_count,
            semantic_count: bundle.stats.semantic_count,
            query_text: request.query_text.clone(),
            intent: request.intent.clone(),
            latency_ms: bundle.stats.latency_ms,
            selected_message_ids_json: serde_json::to_string(
                &bundle.explanation.selected_message_ids,
            )?,
            selected_memory_ids_json: serde_json::to_string(
                &bundle.explanation.selected_memory_ids,
            )?,
            source_breakdown_json: serde_json::to_string(&bundle.explanation.source_breakdown)?,
        })?;
        Ok(bundle)
    }

    pub fn snapshot_working_memory(&self, snapshot: &WorkingMemorySnapshot) -> Result<()> {
        self.working.save_snapshot(snapshot)
    }

    pub fn promote_durable_memory(
        &self,
        request: &GroundedMemoryWrite<'_>,
    ) -> Result<super::types::DurableMemoryRecord> {
        self.durable.promote_grounded_memory(request)
    }

    pub fn has_strong_memory_hit(&self, semantic_memories: &[RetrievedMemory]) -> bool {
        self.recall.has_strong_hit(semantic_memories)
    }

    pub fn mark_memories_recalled(&self, memory_ids: &[i64]) -> Result<()> {
        self.durable.mark_recalled(memory_ids, &now_rfc3339())
    }

    pub fn get_previous_context(&self, default: ContextLevel) -> Result<ContextLevel> {
        self.settings.get_previous_context(default)
    }

    pub fn set_previous_context(&self, level: ContextLevel) -> Result<()> {
        self.settings.set_previous_context(level)
    }

    pub fn get_selected_cloud_profile(&self) -> Result<Option<String>> {
        self.settings.get_selected_cloud_profile()
    }

    pub fn set_selected_cloud_profile(&self, profile_id: &str) -> Result<()> {
        self.settings.set_selected_cloud_profile(profile_id)
    }

    pub fn ambient_cooldown_elapsed(&self, cooldown_seconds: u64) -> Result<bool> {
        self.settings.ambient_cooldown_elapsed(cooldown_seconds)
    }

    pub fn record_ambient_emit(&self, event_type: &str) -> Result<()> {
        self.settings.record_ambient_emit(event_type)
    }

    pub fn record_note_activity(
        &self,
        note_key: &str,
        relative_path: &str,
        title_snapshot: &str,
        event_type: &str,
        occurred_at: &str,
    ) -> Result<()> {
        self.settings.record_note_activity(
            note_key,
            relative_path,
            title_snapshot,
            event_type,
            occurred_at,
        )
    }

    pub fn append_message(
        &self,
        role: &str,
        content: &str,
        turn_id: &str,
        input_source: InputSource,
        content_type: MessageContentType,
        display_json: Option<&str>,
        visible_summary: Option<&str>,
        meta_json: Option<&Value>,
    ) -> Result<ChatMessage> {
        self.transcript.append_message(AppendMessageRequest {
            role: role.to_owned(),
            content: content.to_owned(),
            turn_id: turn_id.to_owned(),
            input_source,
            content_type,
            display_json: display_json.map(ToOwned::to_owned),
            visible_summary: visible_summary.map(ToOwned::to_owned),
            meta_json: meta_json.cloned(),
        })
    }

    pub fn load_bootstrap_state(&self, default_level: ContextLevel) -> Result<BootstrapState> {
        Ok(BootstrapState {
            previous_context: self.settings.get_previous_context(default_level)?,
            messages: self.transcript.load_all_messages()?,
        })
    }

    pub fn load_all_messages(&self) -> Result<Vec<ChatMessage>> {
        self.transcript.load_all_messages()
    }

    pub fn load_recent_messages(
        &self,
        level: ContextLevel,
        exclude_message_id: Option<i64>,
    ) -> Result<Vec<ChatMessage>> {
        self.transcript.load_recent_messages(TranscriptSliceQuery {
            context_level: level,
            exclude_message_id,
            limit_override: None,
        })
    }

    pub fn store_grounded_memory_item(&self, request: &GroundedMemoryWrite<'_>) -> Result<()> {
        self.durable.promote_grounded_memory(request)?;
        Ok(())
    }

    pub fn semantic_search(
        &self,
        query_text: &str,
        level: ContextLevel,
    ) -> Result<Vec<RetrievedMemory>> {
        self.durable
            .search_active(&super::types::DurableRecallQuery {
                query_text: query_text.to_owned(),
                context_level: level,
                limit: None,
            })
    }

    pub fn log_model_call(&self, entry: &ModelLogEntry) -> Result<()> {
        self.observability.log_model_call(entry)
    }

    pub fn log_tool_call(&self, entry: &ToolLogEntry) -> Result<()> {
        self.observability.log_tool_call(entry)
    }

    pub fn log_retrieval(&self, entry: &RetrievalLogEntry) -> Result<()> {
        self.observability.log_recall(entry)
    }

    pub fn upsert_turn_summary(&self, summary: &TurnTraceSummary) -> Result<()> {
        self.observability.upsert_turn_summary(summary)
    }

    pub fn append_trace_event(&self, event: &TraceEventRecord) -> Result<()> {
        self.observability.append_trace_event(event)
    }

    pub fn set_tool_state(
        &self,
        tool_name: &str,
        state_json: &Value,
        updated_at: &str,
    ) -> Result<()> {
        self.tool_state
            .set_tool_state(tool_name, state_json, updated_at)
    }

    pub fn get_tool_state(&self, tool_name: &str) -> Result<Option<Value>> {
        self.tool_state.get_tool_state(tool_name)
    }

    pub fn load_overview(
        &self,
        previous_context: ContextLevel,
        runtime: RuntimeOverview,
        model_name: &str,
        tools: ToolsOverview,
    ) -> Result<OverviewSnapshot> {
        self.observability
            .load_overview(previous_context, runtime, model_name, tools)
    }
}

#[derive(Clone)]
pub struct MemoryStore {
    service: Arc<MemoryService>,
}

impl MemoryStore {
    pub fn new(db_path: PathBuf, app_config: &AppConfig) -> Result<Self> {
        Ok(Self {
            service: Arc::new(MemoryService::new(db_path, app_config)?),
        })
    }

    pub fn persist_turn(&self, request: PersistTurnRequest) -> Result<()> {
        self.service.persist_turn(request)
    }

    pub fn hydrate_working_memory(
        &self,
        request: &HydrateWorkingMemoryRequest,
    ) -> Result<WorkingMemorySnapshot> {
        self.service.hydrate_working_memory(request)
    }

    pub fn refresh_working_memory(
        &self,
        request: &RefreshWorkingMemoryRequest,
    ) -> Result<WorkingMemorySnapshot> {
        self.service.refresh_working_memory(request)
    }

    pub fn recall(&self, request: &RecallRequest) -> Result<RecallBundle> {
        self.service.recall(request)
    }

    pub fn snapshot_working_memory(&self, snapshot: &WorkingMemorySnapshot) -> Result<()> {
        self.service.snapshot_working_memory(snapshot)
    }

    pub fn promote_durable_memory(
        &self,
        request: &GroundedMemoryWrite<'_>,
    ) -> Result<super::types::DurableMemoryRecord> {
        self.service.promote_durable_memory(request)
    }

    pub fn has_strong_memory_hit(&self, semantic_memories: &[RetrievedMemory]) -> bool {
        self.service.has_strong_memory_hit(semantic_memories)
    }

    pub fn mark_memories_recalled(&self, memory_ids: &[i64]) -> Result<()> {
        self.service.mark_memories_recalled(memory_ids)
    }

    pub fn get_previous_context(&self, default: ContextLevel) -> Result<ContextLevel> {
        self.service.get_previous_context(default)
    }

    pub fn set_previous_context(&self, level: ContextLevel) -> Result<()> {
        self.service.set_previous_context(level)
    }

    pub fn get_selected_cloud_profile(&self) -> Result<Option<String>> {
        self.service.get_selected_cloud_profile()
    }

    pub fn set_selected_cloud_profile(&self, profile_id: &str) -> Result<()> {
        self.service.set_selected_cloud_profile(profile_id)
    }

    pub fn ambient_cooldown_elapsed(&self, cooldown_seconds: u64) -> Result<bool> {
        self.service.ambient_cooldown_elapsed(cooldown_seconds)
    }

    pub fn record_ambient_emit(&self, event_type: &str) -> Result<()> {
        self.service.record_ambient_emit(event_type)
    }

    pub fn record_note_activity(
        &self,
        note_key: &str,
        relative_path: &str,
        title_snapshot: &str,
        event_type: &str,
        occurred_at: &str,
    ) -> Result<()> {
        self.service.record_note_activity(
            note_key,
            relative_path,
            title_snapshot,
            event_type,
            occurred_at,
        )
    }

    pub fn append_message(
        &self,
        role: &str,
        content: &str,
        turn_id: &str,
        input_source: InputSource,
        content_type: MessageContentType,
        display_json: Option<&str>,
        visible_summary: Option<&str>,
        meta_json: Option<&Value>,
    ) -> Result<ChatMessage> {
        self.service.append_message(
            role,
            content,
            turn_id,
            input_source,
            content_type,
            display_json,
            visible_summary,
            meta_json,
        )
    }

    pub fn load_bootstrap_state(&self, default_level: ContextLevel) -> Result<BootstrapState> {
        self.service.load_bootstrap_state(default_level)
    }

    pub fn load_all_messages(&self) -> Result<Vec<ChatMessage>> {
        self.service.load_all_messages()
    }

    pub fn load_recent_messages(
        &self,
        level: ContextLevel,
        exclude_message_id: Option<i64>,
    ) -> Result<Vec<ChatMessage>> {
        self.service.load_recent_messages(level, exclude_message_id)
    }

    pub fn store_grounded_memory_item(&self, request: &GroundedMemoryWrite<'_>) -> Result<()> {
        self.service.store_grounded_memory_item(request)
    }

    pub fn semantic_search(
        &self,
        query_text: &str,
        level: ContextLevel,
    ) -> Result<Vec<RetrievedMemory>> {
        self.service.semantic_search(query_text, level)
    }

    pub fn log_model_call(&self, entry: &ModelLogEntry) -> Result<()> {
        self.service.log_model_call(entry)
    }

    pub fn log_tool_call(&self, entry: &ToolLogEntry) -> Result<()> {
        self.service.log_tool_call(entry)
    }

    pub fn log_retrieval(&self, entry: &RetrievalLogEntry) -> Result<()> {
        self.service.log_retrieval(entry)
    }

    pub fn upsert_turn_summary(&self, summary: &TurnTraceSummary) -> Result<()> {
        self.service.upsert_turn_summary(summary)
    }

    pub fn append_trace_event(&self, event: &TraceEventRecord) -> Result<()> {
        self.service.append_trace_event(event)
    }

    pub fn set_tool_state(
        &self,
        tool_name: &str,
        state_json: &Value,
        updated_at: &str,
    ) -> Result<()> {
        self.service
            .set_tool_state(tool_name, state_json, updated_at)
    }

    pub fn get_tool_state(&self, tool_name: &str) -> Result<Option<Value>> {
        self.service.get_tool_state(tool_name)
    }

    pub fn load_overview(
        &self,
        previous_context: ContextLevel,
        runtime: RuntimeOverview,
        model_name: &str,
        tools: ToolsOverview,
    ) -> Result<OverviewSnapshot> {
        self.service
            .load_overview(previous_context, runtime, model_name, tools)
    }
}
