use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};

use crate::logging::{
    ModelLogEntry, TraceEventRecord, TracePayloadAttachment, TraceVisibilityClass, TurnTraceSummary,
};
use crate::memory::{
    ContinuityMode, GroundedMemoryWrite, HydrateWorkingMemoryRequest, MemoryStore, RecallBudget,
    RecallBundle, RecallRequest, RefreshWorkingMemoryRequest, RetrievedMemory,
    WorkingMemorySnapshot,
};
use crate::model_runtime::{
    AmbientRequest, CloudReasoningRequest, FirstBeatRequest, ModelRuntime, NanoReplyRequest,
};
use crate::prompt_translator::{
    parse_internal_model_output, AmbientLineTranslationRequest, BeatPromptContext, BehaviorPolicy,
    CompiledPromptArtifact, ParsedTurnOutputKind, PromptMode, PromptOutputContract,
    PromptTaskFrame, PromptTranslator,
};
use crate::tools::{ToolExecutionResult, ToolExecutor, ToolName, ToolRequest};
use crate::{
    emit_diagnostic_log, now_rfc3339, AppConfig, ChatMessage, ContextLevel, CoreCallbacks,
    InputSource, MessageContentType,
};

pub use crate::prompt_translator::{parse_turn_envelope, MemoryCandidate, TurnEnvelope};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnPlanKind {
    DirectNano,
    RecallNano,
    CloudEscalated,
    ToolDirect,
    CloudTool,
}

impl TurnPlanKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::DirectNano => "direct_nano",
            Self::RecallNano => "recall_nano",
            Self::CloudEscalated => "cloud_escalated",
            Self::ToolDirect => "tool_direct",
            Self::CloudTool => "cloud_tool",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackPlanKind {
    FallbackNano,
    FallbackErrorVisible,
}

impl FallbackPlanKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::FallbackNano => "fallback_nano",
            Self::FallbackErrorVisible => "fallback_error_visible",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextPolicy {
    None,
    TranscriptOnly,
    DurableOnly,
    TranscriptPlusDurable,
}

impl ContextPolicy {
    fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::TranscriptOnly => "transcript_only",
            Self::DurableOnly => "durable_only",
            Self::TranscriptPlusDurable => "transcript_plus_durable",
        }
    }
}

pub type ContextRecipe = ContextPolicy;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BeatKind {
    Acknowledge,
    Primary,
}

impl BeatKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Acknowledge => "acknowledge",
            Self::Primary => "primary",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutionRole {
    FastAck,
    LocalReply,
    MemoryReply,
    CloudReply,
    DeepReasoning,
    StructuredToolDecision,
    DeterministicTool,
}

impl ExecutionRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::FastAck => "fast_ack",
            Self::LocalReply => "local_reply",
            Self::MemoryReply => "memory_reply",
            Self::CloudReply => "cloud_reply",
            Self::DeepReasoning => "deep_reasoning",
            Self::StructuredToolDecision => "structured_tool_decision",
            Self::DeterministicTool => "deterministic_tool",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeliveryPolicy {
    SingleVisibleMessage,
    AckThenAppend,
    ToolSummaryOnly,
}

impl DeliveryPolicy {
    fn as_str(self) -> &'static str {
        match self {
            Self::SingleVisibleMessage => "single_visible_message",
            Self::AckThenAppend => "ack_then_append",
            Self::ToolSummaryOnly => "tool_summary_only",
        }
    }
}

#[derive(Debug, Clone)]
enum ToolPolicy {
    None,
    Exact(ToolRequest),
    AllowStructuredDecision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BeatPlan {
    kind: BeatKind,
    role: ExecutionRole,
    context_recipe: ContextRecipe,
    output_contract: PromptOutputContract,
    visible: bool,
    optional: bool,
}

#[derive(Debug, Clone, Copy)]
struct RuntimeCapabilities {
    local_reply: bool,
    fast_ack: bool,
    cloud_reasoning: bool,
}

#[derive(Debug, Clone)]
struct TurnIntent {
    strong_memory_hit: bool,
    recall_like: bool,
    deep: bool,
    tool_intent: crate::tools::ToolIntentMatch,
    continuity_mode: ContinuityMode,
    runtime_capabilities: RuntimeCapabilities,
}

#[derive(Debug, Clone)]
struct TurnExecutionPlan {
    plan_kind: TurnPlanKind,
    context_policy: ContextPolicy,
    include_tool_guidance: bool,
    prefer_memory_facts: bool,
    tool_consulted: bool,
    tool_policy: ToolPolicy,
    delivery_policy: DeliveryPolicy,
    beats: Vec<BeatPlan>,
    reason_codes: Vec<String>,
}

type TurnPlan = TurnExecutionPlan;

impl TurnExecutionPlan {
    fn ack_beat(&self) -> Option<BeatPlan> {
        self.beats
            .iter()
            .find(|beat| beat.kind == BeatKind::Acknowledge)
            .copied()
    }

    fn primary_beat(&self) -> Option<BeatPlan> {
        self.beats
            .iter()
            .find(|beat| beat.kind == BeatKind::Primary)
            .copied()
    }

    fn uses_cloud(&self) -> bool {
        self.beats.iter().any(|beat| {
            matches!(
                beat.role,
                ExecutionRole::CloudReply
                    | ExecutionRole::DeepReasoning
                    | ExecutionRole::StructuredToolDecision
            )
        })
    }
}

enum RuntimeInvocation {
    FirstBeat(FirstBeatRequest),
    NanoReply(NanoReplyRequest),
    CloudReasoning(CloudReasoningRequest),
}

struct RuntimeInvocationResult {
    raw_output: String,
    logs: Vec<ModelLogEntry>,
}

struct ToolDelivery {
    summary: String,
    transcript_content: String,
    transcript_display_json: String,
    visible_summary: String,
}

struct TurnTraceState {
    stage_seq: i64,
    summary: TurnTraceSummary,
}

struct PersistedTurnMessages {
    assistant_message: ChatMessage,
    tool_message_id: Option<i64>,
}

struct TurnMemoryContext {
    working_snapshot: Option<WorkingMemorySnapshot>,
    recall_bundle: Option<RecallBundle>,
    recent_messages: Vec<ChatMessage>,
    semantic_memories: Vec<RetrievedMemory>,
}

pub struct HgieEngine {
    memory: MemoryStore,
    tools: ToolExecutor,
    runtime: Arc<dyn ModelRuntime>,
    prompt_translator: PromptTranslator,
    app_config: AppConfig,
}

impl HgieEngine {
    pub fn new(
        memory: MemoryStore,
        tools: ToolExecutor,
        runtime: Arc<dyn ModelRuntime>,
        prompt_translator: PromptTranslator,
        app_config: AppConfig,
    ) -> Self {
        Self {
            memory,
            tools,
            runtime,
            prompt_translator,
            app_config,
        }
    }

    pub fn execute_turn(
        &self,
        turn_id: &str,
        text: &str,
        input_source: InputSource,
        callbacks: &dyn CoreCallbacks,
    ) -> Result<ChatMessage> {
        let started_at = Instant::now();
        let user_message = self
            .memory
            .append_message(
                "user",
                text,
                turn_id,
                input_source,
                MessageContentType::PlainText,
                None,
                Some(text),
                None,
            )
            .context("failed to persist user message")?;

        let mut trace_state = TurnTraceState {
            stage_seq: 0,
            summary: TurnTraceSummary {
                turn_id: turn_id.to_owned(),
                created_at: user_message.created_at.clone(),
                user_input_summary: compact_user_summary(text),
                input_source: input_source.as_str().to_owned(),
                plan_kind: "unplanned".to_owned(),
                fallback_plan_kind: None,
                context_policy: None,
                model_stages: Vec::new(),
                memory_used: false,
                tool_consulted: false,
                tool_used: false,
                nano_first_beat_used: false,
                cloud_escalated: false,
                cloud_used: false,
                selected_cloud_profile: None,
                delivery_mode: "PENDING".to_owned(),
                final_route: "pending".to_owned(),
                error_summary: None,
                total_latency_ms: 0,
                final_visible_output: String::new(),
                had_fallback: false,
            },
        };

        self.trace_event(
            &mut trace_state,
            "turn",
            "TURN_RECEIVED",
            vec![],
            Vec::new(),
            None,
            Some(vec![trace_payload(
                "raw_input",
                TraceVisibilityClass::SensitiveLocal,
                "text",
                text,
            )]),
        )?;

        let context_level = self
            .memory
            .get_previous_context(self.app_config.default_previous_context)?;
        let memory_context = self
            .load_turn_memory_context(
                turn_id,
                text,
                context_level,
                user_message.id,
                &trace_state.summary,
                callbacks,
            )
            .context("failed to load turn memory context")?;

        let behavior_policy = self.behavior_policy_for_user_turn(text);
        let runtime_capabilities = self.runtime_capabilities();
        let plan = if let Some(recall_bundle) = memory_context.recall_bundle.as_ref() {
            self.plan_turn_route(text, recall_bundle, runtime_capabilities)
        } else {
            self.plan_turn_route_compat(
                text,
                &memory_context.semantic_memories,
                runtime_capabilities,
            )
        };
        trace_state.summary.plan_kind = plan.plan_kind.as_str().to_owned();
        trace_state.summary.context_policy = Some(plan.context_policy.as_str().to_owned());
        trace_state.summary.tool_consulted = plan.tool_consulted;
        trace_state.summary.cloud_escalated = plan.uses_cloud();
        trace_state.summary.selected_cloud_profile = if plan.uses_cloud() {
            self.runtime.selected_cloud_profile_id()
        } else {
            None
        };
        self.memory.upsert_turn_summary(&trace_state.summary)?;
        self.trace_event(
            &mut trace_state,
            "decision",
            "PLAN_SELECTED",
            plan.reason_codes.clone(),
            Vec::new(),
            None,
            None,
        )?;
        emit_diagnostic_log(
            callbacks,
            "hgie",
            "info",
            "turn route selected",
            Some(turn_id),
            json!({
                "plan_kind": trace_state.summary.plan_kind,
                "tool_consulted": trace_state.summary.tool_consulted,
                "cloud_escalated": trace_state.summary.cloud_escalated,
                "selected_cloud_profile": trace_state.summary.selected_cloud_profile,
                "delivery_policy": plan.delivery_policy.as_str(),
                "beats": plan
                    .beats
                    .iter()
                    .map(|beat| json!({
                        "kind": beat.kind.as_str(),
                        "role": beat.role.as_str(),
                        "optional": beat.optional,
                    }))
                    .collect::<Vec<_>>(),
                "continuity_mode": memory_context
                    .recall_bundle
                    .as_ref()
                    .map(|bundle| bundle.explanation.continuity.mode),
            }),
        );

        let selected_recent_messages =
            if let Some(recall_bundle) = memory_context.recall_bundle.as_ref() {
                self.select_recent_messages_for_policy(recall_bundle, plan.context_policy)
            } else {
                self.select_recent_messages_from_slice(
                    &memory_context.recent_messages,
                    plan.context_policy,
                )
            };
        let selected_semantic_memories = if let Some(recall_bundle) =
            memory_context.recall_bundle.as_ref()
        {
            self.select_memories_for_policy(recall_bundle, plan.context_policy)
        } else {
            self.select_memories_from_slice(&memory_context.semantic_memories, plan.context_policy)
        };
        trace_state.summary.memory_used = memory_context
            .recall_bundle
            .as_ref()
            .map(|bundle| bundle.explanation.memory_used)
            .unwrap_or_else(|| !selected_semantic_memories.is_empty());

        let selected_message_ids = selected_recent_messages
            .iter()
            .map(|message| message.id)
            .collect::<Vec<_>>();
        let selected_memory_ids = selected_semantic_memories
            .iter()
            .map(|memory| memory.id)
            .collect::<Vec<_>>();
        let source_breakdown = memory_context
            .recall_bundle
            .as_ref()
            .map(|bundle| bundle.explanation.source_breakdown.clone())
            .unwrap_or_else(|| {
                json!({
                    "transcript": selected_recent_messages.len(),
                    "durable": selected_semantic_memories.len(),
                    "mode": "compatibility",
                })
            });
        let strong_memory_hit = memory_context
            .recall_bundle
            .as_ref()
            .map(|bundle| bundle.explanation.strong_hit)
            .unwrap_or_else(|| {
                self.memory
                    .has_strong_memory_hit(&memory_context.semantic_memories)
            });
        self.trace_event(
            &mut trace_state,
            "decision",
            "CONTEXT_SELECTED",
            Vec::new(),
            Vec::new(),
            Some(json!({
                "transcript_ids": selected_message_ids,
                "memory_ids": selected_memory_ids,
                "source_breakdown": source_breakdown,
                "strong_hit": strong_memory_hit,
                "continuity": memory_context
                    .recall_bundle
                    .as_ref()
                    .map(|bundle| &bundle.explanation.continuity),
            })),
            None,
        )?;
        emit_diagnostic_log(
            callbacks,
            "memory",
            "info",
            "memory context selected",
            Some(turn_id),
            json!({
                "recent_count": selected_recent_messages.len(),
                "semantic_count": selected_semantic_memories.len(),
                "memory_used": trace_state.summary.memory_used,
                "working_thread_count": memory_context
                    .recall_bundle
                    .as_ref()
                    .map(|bundle| bundle.stats.working_thread_count)
                    .unwrap_or(0),
                "strong_hit": strong_memory_hit,
                "continuity_mode": memory_context
                    .recall_bundle
                    .as_ref()
                    .map(|bundle| bundle.explanation.continuity.mode),
                "continuity_confidence": memory_context
                    .recall_bundle
                    .as_ref()
                    .map(|bundle| bundle.explanation.continuity.continuity_confidence)
                    .unwrap_or(0.0),
                "mode": if memory_context.recall_bundle.is_some() {
                    "memory_bundle"
                } else {
                    "compatibility"
                },
            }),
        );

        let previous_working_snapshot = memory_context.working_snapshot.clone();
        let recall_bundle = memory_context.recall_bundle.clone();
        let mut first_beat_text = String::new();
        let mut assistant_started = false;
        if let Some(ack_beat) = plan.ack_beat() {
            match self.run_fast_ack_beat(
                text,
                input_source,
                &selected_recent_messages,
                &behavior_policy,
                ack_beat,
                &mut trace_state,
            ) {
                Ok(ack_text) if !ack_text.trim().is_empty() => {
                    first_beat_text = ack_text;
                    self.begin_assistant_turn(turn_id, &first_beat_text, callbacks);
                    assistant_started = true;
                }
                Ok(_) => {}
                Err(error) if ack_beat.optional => {
                    emit_diagnostic_log(
                        callbacks,
                        "model",
                        "warning",
                        "fast ack beat was skipped",
                        Some(turn_id),
                        json!({
                            "error": error.to_string(),
                            "plan_kind": trace_state.summary.plan_kind,
                        }),
                    );
                }
                Err(error) => return Err(error),
            }
        }

        let primary_beat = plan
            .primary_beat()
            .ok_or_else(|| anyhow!("turn execution plan is missing a primary beat"))?;

        let final_result = match primary_beat.role {
            ExecutionRole::LocalReply | ExecutionRole::MemoryReply => self.run_nano_plan(
                turn_id,
                text,
                context_level,
                &behavior_policy,
                &selected_recent_messages,
                &selected_semantic_memories,
                &plan,
                &first_beat_text,
                &mut trace_state,
            ),
            ExecutionRole::CloudReply
            | ExecutionRole::DeepReasoning
            | ExecutionRole::StructuredToolDecision => self.run_cloud_plan(
                turn_id,
                text,
                context_level,
                &behavior_policy,
                &selected_recent_messages,
                &selected_semantic_memories,
                &plan,
                &first_beat_text,
                user_message.id,
                callbacks,
                &mut trace_state,
            ),
            ExecutionRole::DeterministicTool => self.run_tool_direct_plan(
                turn_id,
                input_source,
                &plan,
                &first_beat_text,
                callbacks,
                &mut trace_state,
            ),
            ExecutionRole::FastAck => Err(anyhow!(
                "fast ack beat cannot be the primary execution beat"
            )),
        };

        let completed_message = match final_result {
            Ok(turn_result) => {
                if !assistant_started {
                    self.begin_assistant_turn(turn_id, &turn_result.assistant_text, callbacks);
                }
                let persisted_turn = self.persist_completed_turn(
                    turn_id,
                    input_source,
                    &turn_result.assistant_text,
                    turn_result.tool_delivery.as_ref(),
                    callbacks,
                )?;
                self.refresh_working_memory_for_visible_turn(
                    turn_id,
                    text,
                    &turn_result.assistant_text,
                    turn_result
                        .tool_delivery
                        .as_ref()
                        .map(|delivery| delivery.summary.as_str()),
                    recall_bundle.as_ref(),
                    previous_working_snapshot.clone(),
                    collect_turn_message_ids(
                        user_message.id,
                        persisted_turn.tool_message_id,
                        persisted_turn.assistant_message.id,
                    ),
                    callbacks,
                );
                self.write_grounded_memories(text, user_message.id, &mut trace_state)?;
                trace_state.summary.total_latency_ms = elapsed_millis(started_at);
                trace_state.summary.final_visible_output = turn_result.assistant_text.clone();
                trace_state.summary.delivery_mode = self.delivery_mode(&trace_state.summary);
                trace_state.summary.final_route = self.final_route(&trace_state.summary);
                self.memory.upsert_turn_summary(&trace_state.summary)?;
                self.trace_event(
                    &mut trace_state,
                    "turn",
                    "TURN_COMPLETED",
                    Vec::new(),
                    Vec::new(),
                    None,
                    None,
                )?;
                persisted_turn.assistant_message
            }
            Err(error) => {
                self.log_prompt_issue("turn_execution", text, &error);
                let deterministic_text = if !first_beat_text.is_empty() {
                    first_beat_text.clone()
                } else {
                    "Something needs attention before I can finish that.".to_owned()
                };
                if !assistant_started {
                    self.begin_assistant_turn(turn_id, &deterministic_text, callbacks);
                }
                trace_state.summary.had_fallback = true;
                trace_state.summary.fallback_plan_kind =
                    Some(FallbackPlanKind::FallbackErrorVisible.as_str().to_owned());
                trace_state.summary.error_summary = Some(error.to_string());
                emit_diagnostic_log(
                    callbacks,
                    "hgie",
                    "error",
                    "turn fell back to visible error path",
                    Some(turn_id),
                    json!({
                        "error": error.to_string(),
                        "plan_kind": trace_state.summary.plan_kind,
                    }),
                );
                self.trace_event(
                    &mut trace_state,
                    "fallback",
                    "FALLBACK_ENTERED",
                    vec!["error_visible".to_owned()],
                    Vec::new(),
                    None,
                    None,
                )?;
                let persisted_turn = self.persist_completed_turn(
                    turn_id,
                    input_source,
                    &deterministic_text,
                    None,
                    callbacks,
                )?;
                self.refresh_working_memory_for_visible_turn(
                    turn_id,
                    text,
                    &deterministic_text,
                    None,
                    recall_bundle.as_ref(),
                    previous_working_snapshot,
                    collect_turn_message_ids(
                        user_message.id,
                        persisted_turn.tool_message_id,
                        persisted_turn.assistant_message.id,
                    ),
                    callbacks,
                );
                trace_state.summary.total_latency_ms = elapsed_millis(started_at);
                trace_state.summary.final_visible_output = deterministic_text;
                trace_state.summary.delivery_mode = self.delivery_mode(&trace_state.summary);
                trace_state.summary.final_route = self.final_route(&trace_state.summary);
                self.memory.upsert_turn_summary(&trace_state.summary)?;
                self.trace_event(
                    &mut trace_state,
                    "turn",
                    "TURN_COMPLETED",
                    Vec::new(),
                    Vec::new(),
                    None,
                    None,
                )?;
                persisted_turn.assistant_message
            }
        };

        Ok(completed_message)
    }

    pub fn execute_ambient(
        &self,
        turn_id: &str,
        event_type: &str,
        payload_json: Value,
        callbacks: &dyn CoreCallbacks,
    ) -> Result<Option<ChatMessage>> {
        if !self.authorize_ambient(event_type, &payload_json)? {
            return Ok(None);
        }

        let recent_messages = self
            .memory
            .load_recent_messages(self.app_config.default_previous_context, None)
            .context("failed to load recent messages for ambient event")?;
        let behavior_policy = BehaviorPolicy::ambient_turn();
        let ambient_prompt =
            match self
                .prompt_translator
                .compile_ambient_line(&AmbientLineTranslationRequest {
                    event_type,
                    payload_json: &payload_json,
                    recent_messages: &recent_messages,
                    policy: &behavior_policy,
                }) {
                Ok(prompt) => prompt,
                Err(error) => {
                    self.log_prompt_issue("ambient_line", event_type, &error);
                    return Ok(None);
                }
            };
        let ambient = match self.runtime.run_ambient(&AmbientRequest {
            prompt: ambient_prompt,
        }) {
            Ok(result) => result,
            Err(error) => {
                self.log_prompt_issue("ambient_runtime", event_type, &error);
                return Ok(None);
            }
        };
        let Some(ambient) = ambient else {
            return Ok(None);
        };
        if ambient.text.trim().is_empty() {
            return Ok(None);
        }
        for log_entry in &ambient.logs {
            self.memory.log_model_call(log_entry)?;
        }

        self.begin_assistant_turn(turn_id, ambient.text.trim(), callbacks);
        let persisted_turn = self.persist_completed_turn(
            turn_id,
            InputSource::Text,
            ambient.text.trim(),
            None,
            callbacks,
        )?;
        self.memory.record_ambient_emit(event_type)?;
        Ok(Some(persisted_turn.assistant_message))
    }

    pub fn complete_spotify_auth_callback(
        &self,
        turn_id: &str,
        callback_url: &str,
        callbacks: &dyn CoreCallbacks,
    ) -> Result<ChatMessage> {
        let request = ToolRequest {
            tool: ToolName::Spotify,
            action: "handle_callback".to_owned(),
            arguments: json!({
                "callback_url": callback_url
            }),
        };

        let mut trace_state = TurnTraceState {
            stage_seq: 0,
            summary: TurnTraceSummary {
                turn_id: turn_id.to_owned(),
                created_at: now_rfc3339(),
                user_input_summary: "Spotify auth callback".to_owned(),
                input_source: InputSource::Text.as_str().to_owned(),
                plan_kind: TurnPlanKind::ToolDirect.as_str().to_owned(),
                fallback_plan_kind: None,
                context_policy: Some(ContextPolicy::None.as_str().to_owned()),
                model_stages: Vec::new(),
                memory_used: false,
                tool_consulted: true,
                tool_used: true,
                nano_first_beat_used: false,
                cloud_escalated: false,
                cloud_used: false,
                selected_cloud_profile: None,
                delivery_mode: "TOOL_CALLBACK".to_owned(),
                final_route: "tool".to_owned(),
                error_summary: None,
                total_latency_ms: 0,
                final_visible_output: String::new(),
                had_fallback: false,
            },
        };
        self.memory.upsert_turn_summary(&trace_state.summary)?;
        self.trace_event(
            &mut trace_state,
            "decision",
            "PLAN_SELECTED",
            vec!["spotify_callback".to_owned()],
            Vec::new(),
            None,
            None,
        )?;

        self.begin_assistant_turn(turn_id, "", callbacks);
        let tool_delivery =
            self.execute_tool_delivery(turn_id, &request, callbacks, &mut trace_state)?;
        let assistant_text = tool_delivery.summary.clone();
        let persisted_turn = self.persist_completed_turn(
            turn_id,
            InputSource::Text,
            &assistant_text,
            Some(&tool_delivery),
            callbacks,
        )?;
        trace_state.summary.total_latency_ms = 0;
        trace_state.summary.final_visible_output = assistant_text;
        trace_state.summary.delivery_mode = self.delivery_mode(&trace_state.summary);
        trace_state.summary.final_route = self.final_route(&trace_state.summary);
        self.memory.upsert_turn_summary(&trace_state.summary)?;
        self.trace_event(
            &mut trace_state,
            "turn",
            "TURN_COMPLETED",
            Vec::new(),
            Vec::new(),
            None,
            None,
        )?;
        Ok(persisted_turn.assistant_message)
    }

    fn authorize_ambient(&self, event_type: &str, payload_json: &Value) -> Result<bool> {
        if !matches!(event_type, "resume_after_idle" | "capability_available") {
            return Ok(false);
        }
        if !self
            .memory
            .ambient_cooldown_elapsed(self.app_config.ambient_cooldown_seconds)?
        {
            return Ok(false);
        }
        if event_type == "resume_after_idle" {
            let idle_seconds = payload_json
                .get("idle_seconds")
                .and_then(Value::as_u64)
                .unwrap_or_default();
            return Ok(idle_seconds >= self.app_config.idle_resume_threshold_seconds);
        }
        Ok(true)
    }

    fn behavior_policy_for_user_turn(&self, text: &str) -> BehaviorPolicy {
        let lower = text.to_lowercase();
        let allow_playfulness = !lower.contains("urgent") && !lower.contains("serious");
        let allow_humor = allow_playfulness
            && !lower.contains("why")
            && !lower.contains("error")
            && !lower.contains("broken");
        BehaviorPolicy {
            allow_ambient: false,
            allow_playfulness,
            allow_humor,
            prefer_silence: false,
            allow_initiative: false,
        }
    }

    fn runtime_capabilities(&self) -> RuntimeCapabilities {
        let overview = self.runtime.overview();
        RuntimeCapabilities {
            local_reply: overview.nano.active,
            fast_ack: overview.nano.active,
            cloud_reasoning: overview
                .cloud_profiles
                .iter()
                .any(|profile| profile.available),
        }
    }

    fn plan_turn_route(
        &self,
        text: &str,
        recall_bundle: &RecallBundle,
        runtime_capabilities: RuntimeCapabilities,
    ) -> TurnPlan {
        let intent = self.build_turn_intent(
            text,
            recall_bundle.explanation.strong_hit,
            recall_bundle.explanation.continuity.mode,
            runtime_capabilities,
        );
        self.apply_continuity_to_plan(
            self.plan_turn_route_with_strong_hit(intent),
            &recall_bundle.explanation.continuity,
        )
    }

    fn plan_turn_route_compat(
        &self,
        text: &str,
        semantic_memories: &[RetrievedMemory],
        runtime_capabilities: RuntimeCapabilities,
    ) -> TurnPlan {
        self.plan_turn_route_with_strong_hit(self.build_turn_intent(
            text,
            self.memory.has_strong_memory_hit(semantic_memories),
            ContinuityMode::Ambiguous,
            runtime_capabilities,
        ))
    }

    fn build_turn_intent(
        &self,
        text: &str,
        strong_memory_hit: bool,
        continuity_mode: ContinuityMode,
        runtime_capabilities: RuntimeCapabilities,
    ) -> TurnIntent {
        let lower = text.to_lowercase();
        let word_count = lower.split_whitespace().count();
        TurnIntent {
            strong_memory_hit,
            recall_like: self.is_recall_like_turn(&lower),
            deep: word_count > 18
                || lower.contains("analyze")
                || lower.contains("compare")
                || lower.contains("explain")
                || lower.contains("summarize"),
            tool_intent: self.tools.detect_tool_intent(text),
            continuity_mode,
            runtime_capabilities,
        }
    }

    fn plan_turn_route_with_strong_hit(&self, intent: TurnIntent) -> TurnPlan {
        if let Some(exact_request) = intent.tool_intent.exact_request.clone() {
            return TurnPlan {
                plan_kind: TurnPlanKind::ToolDirect,
                context_policy: ContextPolicy::TranscriptOnly,
                include_tool_guidance: false,
                prefer_memory_facts: false,
                tool_consulted: true,
                tool_policy: ToolPolicy::Exact(exact_request),
                delivery_policy: DeliveryPolicy::ToolSummaryOnly,
                beats: vec![BeatPlan {
                    kind: BeatKind::Primary,
                    role: ExecutionRole::DeterministicTool,
                    context_recipe: ContextPolicy::TranscriptOnly,
                    output_contract: PromptOutputContract::PlainText,
                    visible: true,
                    optional: false,
                }],
                reason_codes: vec!["tool_intent_exact".to_owned()],
            };
        }
        if let Some(probable_tool) = intent.tool_intent.probable_tool {
            let mut beats = Vec::new();
            if intent.runtime_capabilities.fast_ack {
                beats.push(BeatPlan {
                    kind: BeatKind::Acknowledge,
                    role: ExecutionRole::FastAck,
                    context_recipe: ContextPolicy::TranscriptOnly,
                    output_contract: PromptOutputContract::PlainText,
                    visible: true,
                    optional: true,
                });
            }
            beats.push(BeatPlan {
                kind: BeatKind::Primary,
                role: ExecutionRole::StructuredToolDecision,
                context_recipe: ContextPolicy::TranscriptOnly,
                output_contract: PromptOutputContract::JsonEnvelope,
                visible: true,
                optional: false,
            });
            return TurnPlan {
                plan_kind: TurnPlanKind::CloudTool,
                context_policy: ContextPolicy::TranscriptOnly,
                include_tool_guidance: true,
                prefer_memory_facts: false,
                tool_consulted: true,
                tool_policy: ToolPolicy::AllowStructuredDecision,
                delivery_policy: if intent.runtime_capabilities.fast_ack {
                    DeliveryPolicy::AckThenAppend
                } else {
                    DeliveryPolicy::SingleVisibleMessage
                },
                beats,
                reason_codes: vec![format!("tool_intent_probable:{}", probable_tool.as_str())],
            };
        }

        if intent.recall_like && intent.strong_memory_hit && !intent.deep {
            return self.adapt_plan_to_runtime_capabilities(
                TurnPlan {
                    plan_kind: TurnPlanKind::RecallNano,
                    context_policy: ContextPolicy::DurableOnly,
                    include_tool_guidance: false,
                    prefer_memory_facts: true,
                    tool_consulted: false,
                    tool_policy: ToolPolicy::None,
                    delivery_policy: DeliveryPolicy::SingleVisibleMessage,
                    beats: vec![BeatPlan {
                        kind: BeatKind::Primary,
                        role: ExecutionRole::MemoryReply,
                        context_recipe: ContextPolicy::DurableOnly,
                        output_contract: PromptOutputContract::PlainText,
                        visible: true,
                        optional: false,
                    }],
                    reason_codes: vec!["strong_durable_memory_hit".to_owned()],
                },
                intent.runtime_capabilities,
            );
        }

        if intent.deep {
            let context_policy = if intent.strong_memory_hit {
                ContextPolicy::TranscriptPlusDurable
            } else {
                ContextPolicy::TranscriptOnly
            };
            let mut beats = Vec::new();
            if intent.runtime_capabilities.fast_ack {
                beats.push(BeatPlan {
                    kind: BeatKind::Acknowledge,
                    role: ExecutionRole::FastAck,
                    context_recipe: ContextPolicy::TranscriptOnly,
                    output_contract: PromptOutputContract::PlainText,
                    visible: true,
                    optional: true,
                });
            }
            beats.push(BeatPlan {
                kind: BeatKind::Primary,
                role: ExecutionRole::DeepReasoning,
                context_recipe: context_policy,
                output_contract: PromptOutputContract::JsonEnvelope,
                visible: true,
                optional: false,
            });
            return TurnPlan {
                plan_kind: TurnPlanKind::CloudEscalated,
                context_policy,
                include_tool_guidance: false,
                prefer_memory_facts: false,
                tool_consulted: false,
                tool_policy: ToolPolicy::None,
                delivery_policy: if intent.runtime_capabilities.fast_ack {
                    DeliveryPolicy::AckThenAppend
                } else {
                    DeliveryPolicy::SingleVisibleMessage
                },
                beats,
                reason_codes: vec!["deep_reasoning".to_owned()],
            };
        }

        self.adapt_plan_to_runtime_capabilities(
            TurnPlan {
                plan_kind: TurnPlanKind::DirectNano,
                context_policy: ContextPolicy::TranscriptOnly,
                include_tool_guidance: false,
                prefer_memory_facts: matches!(
                    intent.continuity_mode,
                    ContinuityMode::Return | ContinuityMode::OpenLoop
                ),
                tool_consulted: false,
                tool_policy: ToolPolicy::None,
                delivery_policy: DeliveryPolicy::SingleVisibleMessage,
                beats: vec![BeatPlan {
                    kind: BeatKind::Primary,
                    role: ExecutionRole::LocalReply,
                    context_recipe: ContextPolicy::TranscriptOnly,
                    output_contract: PromptOutputContract::PlainText,
                    visible: true,
                    optional: false,
                }],
                reason_codes: vec!["default_direct".to_owned()],
            },
            intent.runtime_capabilities,
        )
    }

    fn adapt_plan_to_runtime_capabilities(
        &self,
        mut plan: TurnPlan,
        runtime_capabilities: RuntimeCapabilities,
    ) -> TurnPlan {
        let Some(primary_beat) = plan.primary_beat() else {
            return plan;
        };
        if matches!(
            primary_beat.role,
            ExecutionRole::LocalReply | ExecutionRole::MemoryReply
        ) && !runtime_capabilities.local_reply
            && runtime_capabilities.cloud_reasoning
        {
            plan.plan_kind = TurnPlanKind::CloudEscalated;
            plan.delivery_policy = DeliveryPolicy::SingleVisibleMessage;
            plan.beats = vec![BeatPlan {
                kind: BeatKind::Primary,
                role: ExecutionRole::CloudReply,
                context_recipe: plan.context_policy,
                output_contract: PromptOutputContract::JsonEnvelope,
                visible: true,
                optional: false,
            }];
            plan.reason_codes
                .push("capability:cloud_reply_substitution".to_owned());
        }
        plan
    }

    fn apply_continuity_to_plan(
        &self,
        mut plan: TurnPlan,
        continuity: &crate::memory::ContinuitySignal,
    ) -> TurnPlan {
        match continuity.mode {
            ContinuityMode::OnThread => {
                plan.reason_codes.push("continuity:on_thread".to_owned());
            }
            ContinuityMode::Pivot => {
                plan.reason_codes.push("continuity:pivot".to_owned());
                if matches!(plan.plan_kind, TurnPlanKind::DirectNano) {
                    plan.context_policy = ContextPolicy::TranscriptOnly;
                }
            }
            ContinuityMode::Return => {
                plan.reason_codes.push("continuity:return".to_owned());
                if !continuity.selected_thread_memory_ids.is_empty()
                    && matches!(plan.context_policy, ContextPolicy::TranscriptOnly)
                    && matches!(
                        plan.plan_kind,
                        TurnPlanKind::DirectNano | TurnPlanKind::CloudEscalated
                    )
                {
                    plan.context_policy = ContextPolicy::TranscriptPlusDurable;
                }
            }
            ContinuityMode::OpenLoop => {
                plan.reason_codes.push("continuity:open_loop".to_owned());
                if !continuity.selected_thread_memory_ids.is_empty()
                    && matches!(plan.plan_kind, TurnPlanKind::DirectNano)
                    && matches!(plan.context_policy, ContextPolicy::TranscriptOnly)
                {
                    plan.context_policy = ContextPolicy::TranscriptPlusDurable;
                }
            }
            ContinuityMode::Ambiguous => {}
        }
        if let Some(primary_index) = plan
            .beats
            .iter()
            .position(|beat| beat.kind == BeatKind::Primary)
        {
            plan.beats[primary_index].context_recipe = plan.context_policy;
        }
        plan
    }

    fn is_recall_like_turn(&self, lower: &str) -> bool {
        lower.starts_with("who ")
            || lower.starts_with("what ")
            || lower.starts_with("when ")
            || lower.starts_with("where ")
            || lower.contains("who's ")
            || lower.contains("what's ")
            || lower.contains("do you remember")
            || lower.contains("remind me")
            || lower.contains("my cat")
            || lower.contains("my stack")
    }

    fn select_recent_messages_for_policy(
        &self,
        recall_bundle: &RecallBundle,
        policy: ContextPolicy,
    ) -> Vec<ChatMessage> {
        self.select_recent_messages_from_slice(&recall_bundle.warm_context.recent_messages, policy)
    }

    fn select_recent_messages_from_slice(
        &self,
        recent_messages: &[ChatMessage],
        policy: ContextPolicy,
    ) -> Vec<ChatMessage> {
        match policy {
            ContextPolicy::None | ContextPolicy::DurableOnly => Vec::new(),
            ContextPolicy::TranscriptOnly | ContextPolicy::TranscriptPlusDurable => {
                recent_messages.to_vec()
            }
        }
    }

    fn select_memories_for_policy(
        &self,
        recall_bundle: &RecallBundle,
        policy: ContextPolicy,
    ) -> Vec<RetrievedMemory> {
        self.select_memories_from_slice(&recall_bundle.warm_context.durable_memories, policy)
    }

    fn select_memories_from_slice(
        &self,
        semantic_memories: &[RetrievedMemory],
        policy: ContextPolicy,
    ) -> Vec<RetrievedMemory> {
        match policy {
            ContextPolicy::None | ContextPolicy::TranscriptOnly => Vec::new(),
            ContextPolicy::DurableOnly | ContextPolicy::TranscriptPlusDurable => {
                semantic_memories.to_vec()
            }
        }
    }

    fn load_turn_memory_context(
        &self,
        turn_id: &str,
        text: &str,
        context_level: ContextLevel,
        user_message_id: i64,
        turn_summary: &TurnTraceSummary,
        callbacks: &dyn CoreCallbacks,
    ) -> Result<TurnMemoryContext> {
        match self
            .memory
            .hydrate_working_memory(&HydrateWorkingMemoryRequest {
                turn_id: turn_id.to_owned(),
                user_text: text.to_owned(),
                context_level,
                exclude_message_id: Some(user_message_id),
                latest_turn_summary: Some(turn_summary.clone()),
            }) {
            Ok(working_snapshot) => match self.memory.recall(&RecallRequest {
                turn_id: turn_id.to_owned(),
                query_text: text.to_owned(),
                intent: "turn_context_selection".to_owned(),
                context_level,
                budget: RecallBudget {
                    max_recent_messages: Some(context_level.recent_turn_limit().saturating_mul(2)),
                    max_durable_memories: Some(context_level.semantic_limit()),
                    include_cold_candidates: false,
                },
                working_memory: Some(working_snapshot.clone()),
                exclude_message_id: Some(user_message_id),
            }) {
                Ok(recall_bundle) => Ok(TurnMemoryContext {
                    recent_messages: recall_bundle.warm_context.recent_messages.clone(),
                    semantic_memories: recall_bundle.warm_context.durable_memories.clone(),
                    working_snapshot: Some(working_snapshot),
                    recall_bundle: Some(recall_bundle),
                }),
                Err(error) => {
                    self.log_memory_context_fallback(turn_id, "recall_failed", &error, callbacks);
                    self.load_turn_memory_context_compat(text, context_level, user_message_id)
                }
            },
            Err(error) => {
                self.log_memory_context_fallback(turn_id, "hydrate_failed", &error, callbacks);
                self.load_turn_memory_context_compat(text, context_level, user_message_id)
            }
        }
    }

    fn load_turn_memory_context_compat(
        &self,
        text: &str,
        context_level: ContextLevel,
        user_message_id: i64,
    ) -> Result<TurnMemoryContext> {
        let recent_messages = self
            .memory
            .load_recent_messages(context_level, Some(user_message_id))
            .context("failed to load recent messages")?;
        let semantic_memories = self
            .memory
            .semantic_search(text, context_level)
            .context("failed to retrieve semantic memories")?;
        Ok(TurnMemoryContext {
            working_snapshot: None,
            recall_bundle: None,
            recent_messages,
            semantic_memories,
        })
    }

    fn log_memory_context_fallback(
        &self,
        turn_id: &str,
        stage: &str,
        error: &anyhow::Error,
        callbacks: &dyn CoreCallbacks,
    ) {
        emit_diagnostic_log(
            callbacks,
            "memory",
            "warning",
            "memory recall fell back to compatibility path",
            Some(turn_id),
            json!({
                "stage": stage,
                "error": error.to_string(),
            }),
        );
    }

    fn refresh_working_memory_for_visible_turn(
        &self,
        turn_id: &str,
        user_text: &str,
        assistant_visible_text: &str,
        tool_summary: Option<&str>,
        recall_bundle: Option<&RecallBundle>,
        previous_snapshot: Option<WorkingMemorySnapshot>,
        current_turn_message_ids: Vec<i64>,
        callbacks: &dyn CoreCallbacks,
    ) {
        let Some(recall_bundle) = recall_bundle else {
            return;
        };
        if let Err(error) = self
            .memory
            .refresh_working_memory(&RefreshWorkingMemoryRequest {
                turn_id: turn_id.to_owned(),
                user_text: user_text.to_owned(),
                assistant_visible_text: assistant_visible_text.to_owned(),
                tool_summary: tool_summary.map(ToOwned::to_owned),
                current_turn_message_ids,
                recall_bundle: recall_bundle.clone(),
                previous_snapshot,
            })
        {
            emit_diagnostic_log(
                callbacks,
                "memory",
                "warning",
                "working memory refresh failed",
                Some(turn_id),
                json!({
                    "error": error.to_string(),
                }),
            );
        }
    }

    fn execute_runtime_invocation(
        &self,
        invocation: RuntimeInvocation,
    ) -> Result<RuntimeInvocationResult> {
        match invocation {
            RuntimeInvocation::FirstBeat(request) => {
                let result = self.runtime.run_first_beat(&request)?;
                Ok(RuntimeInvocationResult {
                    raw_output: result.text,
                    logs: result.logs,
                })
            }
            RuntimeInvocation::NanoReply(request) => {
                let result = self.runtime.run_nano_reply(&request)?;
                Ok(RuntimeInvocationResult {
                    raw_output: result.raw_output,
                    logs: result.logs,
                })
            }
            RuntimeInvocation::CloudReasoning(request) => {
                let result = self.runtime.run_cloud_reasoning(&request)?;
                Ok(RuntimeInvocationResult {
                    raw_output: result.raw_output,
                    logs: result.logs,
                })
            }
        }
    }

    fn run_fast_ack_beat(
        &self,
        user_text: &str,
        input_source: InputSource,
        recent_messages: &[ChatMessage],
        behavior_policy: &BehaviorPolicy,
        beat: BeatPlan,
        trace_state: &mut TurnTraceState,
    ) -> Result<String> {
        if beat.role != ExecutionRole::FastAck {
            return Err(anyhow!("ack beat requested for non-fast-ack role"));
        }
        let prompt = match self
            .prompt_translator
            .compile_task_frame(&PromptTaskFrame {
                mode: PromptMode::FirstBeat,
                output_contract: beat.output_contract,
                user_text: Some(user_text),
                source_text: None,
                status_text: None,
                event_type: None,
                payload_json: None,
                context: BeatPromptContext {
                    input_source: Some(input_source),
                    first_beat: None,
                    recent_messages,
                    semantic_memories: &[],
                    context_level: None,
                    policy: behavior_policy,
                    prefer_memory_facts: false,
                    include_tool_guidance: false,
                },
            })? {
            CompiledPromptArtifact::Nano(prompt) => prompt,
            CompiledPromptArtifact::Cloud(_) => {
                return Err(anyhow!("first beat compiled to cloud artifact"));
            }
        };
        let result =
            self.execute_runtime_invocation(RuntimeInvocation::FirstBeat(FirstBeatRequest {
                prompt: prompt.clone(),
            }))?;
        for log_entry in &result.logs {
            self.memory.log_model_call(log_entry)?;
        }
        let normalized = normalize_plain_text_output(&result.raw_output)?;
        trace_state.summary.nano_first_beat_used = true;
        self.trace_state_model_stage(trace_state, "first_beat");
        self.trace_model_event(
            trace_state,
            "FIRST_BEAT_COMPLETED",
            PromptMode::FirstBeat.as_str(),
            "nano",
            &result.logs,
            &prompt.prompt,
            &result.raw_output,
            &normalized,
            None,
        )?;
        Ok(normalized)
    }

    #[allow(clippy::too_many_arguments)]
    fn run_nano_plan(
        &self,
        _turn_id: &str,
        user_text: &str,
        context_level: ContextLevel,
        behavior_policy: &BehaviorPolicy,
        recent_messages: &[ChatMessage],
        semantic_memories: &[RetrievedMemory],
        plan: &TurnPlan,
        first_beat_text: &str,
        trace_state: &mut TurnTraceState,
    ) -> Result<ResolvedTurnOutput> {
        let prompt = match self
            .prompt_translator
            .compile_task_frame(&PromptTaskFrame {
                mode: PromptMode::NanoReply,
                output_contract: plan
                    .primary_beat()
                    .map(|beat| beat.output_contract)
                    .unwrap_or(PromptOutputContract::PlainText),
                user_text: Some(user_text),
                source_text: None,
                status_text: None,
                event_type: None,
                payload_json: None,
                context: BeatPromptContext {
                    input_source: None,
                    first_beat: Some(first_beat_text),
                    recent_messages,
                    semantic_memories,
                    context_level: Some(context_level),
                    policy: behavior_policy,
                    prefer_memory_facts: plan.prefer_memory_facts,
                    include_tool_guidance: false,
                },
            })? {
            CompiledPromptArtifact::Nano(prompt) => prompt,
            CompiledPromptArtifact::Cloud(_) => {
                return Err(anyhow!("nano reply compiled to cloud artifact"));
            }
        };
        let result =
            self.execute_runtime_invocation(RuntimeInvocation::NanoReply(NanoReplyRequest {
                prompt: prompt.clone(),
            }))?;
        for log_entry in &result.logs {
            self.memory.log_model_call(log_entry)?;
        }

        let normalized = normalize_plain_text_output(&result.raw_output)?;
        self.trace_state_model_stage(trace_state, "nano_reply");
        self.trace_model_event(
            trace_state,
            "PRIMARY_STAGE_COMPLETED",
            PromptMode::NanoReply.as_str(),
            "nano",
            &result.logs,
            &prompt.prompt,
            &result.raw_output,
            &normalized,
            None,
        )?;
        self.trace_event(
            trace_state,
            "render",
            "OUTPUT_NORMALIZED",
            Vec::new(),
            Vec::new(),
            None,
            Some(vec![trace_payload(
                "normalized_output",
                TraceVisibilityClass::DebugLocal,
                "text",
                &normalized,
            )]),
        )?;

        Ok(ResolvedTurnOutput {
            assistant_text: self.prompt_translator.compose_visible_reply(
                first_beat_text,
                &normalized,
                None,
            ),
            tool_delivery: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn run_cloud_plan(
        &self,
        turn_id: &str,
        user_text: &str,
        context_level: ContextLevel,
        behavior_policy: &BehaviorPolicy,
        recent_messages: &[ChatMessage],
        semantic_memories: &[RetrievedMemory],
        plan: &TurnPlan,
        first_beat_text: &str,
        user_message_id: i64,
        callbacks: &dyn CoreCallbacks,
        trace_state: &mut TurnTraceState,
    ) -> Result<ResolvedTurnOutput> {
        self.trace_event(
            trace_state,
            "decision",
            "CLOUD_ESCALATION_TRIGGERED",
            Vec::new(),
            Vec::new(),
            None,
            None,
        )?;
        let request = match self
            .prompt_translator
            .compile_task_frame(&PromptTaskFrame {
                mode: PromptMode::CloudReasoning,
                output_contract: plan
                    .primary_beat()
                    .map(|beat| beat.output_contract)
                    .unwrap_or(PromptOutputContract::JsonEnvelope),
                user_text: Some(user_text),
                source_text: None,
                status_text: None,
                event_type: None,
                payload_json: None,
                context: BeatPromptContext {
                    input_source: None,
                    first_beat: Some(first_beat_text),
                    recent_messages,
                    semantic_memories,
                    context_level: None,
                    policy: behavior_policy,
                    prefer_memory_facts: plan.prefer_memory_facts,
                    include_tool_guidance: plan.include_tool_guidance,
                },
            })? {
            CompiledPromptArtifact::Cloud(request) => request,
            CompiledPromptArtifact::Nano(_) => {
                return Err(anyhow!("cloud reasoning compiled to nano artifact"));
            }
        };

        let cloud_result = match self.execute_runtime_invocation(RuntimeInvocation::CloudReasoning(
            CloudReasoningRequest {
                request: request.clone(),
            },
        )) {
            Ok(result) => result,
            Err(error) => {
                let prompt = serde_json::to_string_pretty(&request.messages)
                    .unwrap_or_else(|_| "[]".to_owned());
                self.memory.log_model_call(&ModelLogEntry {
                    created_at: now_rfc3339(),
                    model_name: "Cloud reasoning".to_owned(),
                    prompt: prompt.clone(),
                    raw_output: String::new(),
                    input_tokens: None,
                    output_tokens: None,
                    latency_ms: 0,
                    http_status: None,
                    error_text: Some(error.to_string()),
                })?;
                if !self.runtime_capabilities().local_reply {
                    return Err(anyhow!("cloud reasoning failed: {error}"));
                }
                self.enter_fallback(
                    trace_state,
                    FallbackPlanKind::FallbackNano,
                    vec!["cloud_runtime_error".to_owned()],
                )?;
                return self.run_fallback_nano_plan(
                    user_text,
                    context_level,
                    behavior_policy,
                    recent_messages,
                    semantic_memories,
                    first_beat_text,
                    trace_state,
                );
            }
        };
        for log_entry in &cloud_result.logs {
            self.memory.log_model_call(log_entry)?;
        }
        trace_state.summary.cloud_used = true;
        self.trace_state_model_stage(trace_state, "cloud_reasoning");
        emit_diagnostic_log(
            callbacks,
            "model",
            "info",
            "cloud reasoning completed",
            Some(turn_id),
            json!({
                "selected_cloud_profile": trace_state.summary.selected_cloud_profile,
                "plan_kind": trace_state.summary.plan_kind,
            }),
        );

        let parsed_output = parse_internal_model_output(&cloud_result.raw_output)?;
        let normalized_output = match parsed_output.kind {
            ParsedTurnOutputKind::SuppressedInternalPayload => None,
            _ => Some(parsed_output.assistant_reply.clone()),
        };
        self.trace_model_event(
            trace_state,
            "PRIMARY_STAGE_COMPLETED",
            PromptMode::CloudReasoning.as_str(),
            "cloud",
            &cloud_result.logs,
            &serde_json::to_string_pretty(&request.messages).unwrap_or_else(|_| "[]".to_owned()),
            &cloud_result.raw_output,
            normalized_output.as_deref().unwrap_or_default(),
            (parsed_output.kind == ParsedTurnOutputKind::SuppressedInternalPayload)
                .then(|| cloud_result.raw_output.as_str()),
        )?;
        self.trace_event(
            trace_state,
            "render",
            "OUTPUT_PARSED",
            Vec::new(),
            Vec::new(),
            None,
            Some(vec![trace_payload(
                "parsed_output",
                TraceVisibilityClass::DebugLocal,
                "json",
                &serde_json::to_string_pretty(&parsed_output.to_turn_envelope())
                    .unwrap_or_else(|_| "{}".to_owned()),
            )]),
        )?;

        if parsed_output.kind == ParsedTurnOutputKind::SuppressedInternalPayload {
            if !self.runtime_capabilities().local_reply {
                return Err(anyhow!(
                    "cloud reasoning returned suppressed internal payload"
                ));
            }
            self.enter_fallback(
                trace_state,
                FallbackPlanKind::FallbackNano,
                vec!["cloud_output_suppressed".to_owned()],
            )?;
            return self.run_fallback_nano_plan(
                user_text,
                context_level,
                behavior_policy,
                recent_messages,
                semantic_memories,
                first_beat_text,
                trace_state,
            );
        }

        let mut tool_delivery = None;
        if matches!(plan.plan_kind, TurnPlanKind::CloudTool) {
            if let Some(tool_request) = parsed_output.tool_request.as_ref() {
                tool_delivery = Some(self.execute_tool_delivery(
                    turn_id,
                    tool_request,
                    callbacks,
                    trace_state,
                )?);
                let _ = user_message_id;
            }
        }

        let assistant_reply = if parsed_output.assistant_reply.trim().is_empty()
            && tool_delivery.is_some()
            && first_beat_text.trim().is_empty()
        {
            tool_delivery
                .as_ref()
                .map(|delivery| delivery.summary.clone())
                .unwrap_or_default()
        } else {
            parsed_output.assistant_reply.clone()
        };
        self.trace_event(
            trace_state,
            "render",
            "OUTPUT_NORMALIZED",
            Vec::new(),
            Vec::new(),
            None,
            Some(vec![trace_payload(
                "normalized_output",
                TraceVisibilityClass::DebugLocal,
                "text",
                &assistant_reply,
            )]),
        )?;

        Ok(ResolvedTurnOutput {
            assistant_text: self.prompt_translator.compose_visible_reply(
                first_beat_text,
                &assistant_reply,
                None,
            ),
            tool_delivery,
        })
    }

    fn run_tool_direct_plan(
        &self,
        turn_id: &str,
        _input_source: InputSource,
        plan: &TurnPlan,
        first_beat_text: &str,
        callbacks: &dyn CoreCallbacks,
        trace_state: &mut TurnTraceState,
    ) -> Result<ResolvedTurnOutput> {
        let request = match &plan.tool_policy {
            ToolPolicy::Exact(request) => request,
            _ => return Err(anyhow!("tool_direct plan missing exact tool policy")),
        };
        let tool_delivery = self.execute_tool_delivery(turn_id, request, callbacks, trace_state)?;
        let assistant_text = if first_beat_text.trim().is_empty() {
            tool_delivery.summary.clone()
        } else {
            first_beat_text.to_owned()
        };
        Ok(ResolvedTurnOutput {
            assistant_text,
            tool_delivery: Some(tool_delivery),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn run_fallback_nano_plan(
        &self,
        user_text: &str,
        context_level: ContextLevel,
        behavior_policy: &BehaviorPolicy,
        recent_messages: &[ChatMessage],
        semantic_memories: &[RetrievedMemory],
        first_beat_text: &str,
        trace_state: &mut TurnTraceState,
    ) -> Result<ResolvedTurnOutput> {
        if !self.runtime_capabilities().local_reply {
            return Err(anyhow!(
                "nano fallback requested without a local reply runtime"
            ));
        }
        let prompt = match self
            .prompt_translator
            .compile_task_frame(&PromptTaskFrame {
                mode: PromptMode::NanoReply,
                output_contract: PromptOutputContract::PlainText,
                user_text: Some(user_text),
                source_text: None,
                status_text: None,
                event_type: None,
                payload_json: None,
                context: BeatPromptContext {
                    input_source: None,
                    first_beat: Some(first_beat_text),
                    recent_messages,
                    semantic_memories,
                    context_level: Some(context_level),
                    policy: behavior_policy,
                    prefer_memory_facts: false,
                    include_tool_guidance: false,
                },
            })? {
            CompiledPromptArtifact::Nano(prompt) => prompt,
            CompiledPromptArtifact::Cloud(_) => {
                return Err(anyhow!("fallback nano compiled to cloud artifact"));
            }
        };
        let result =
            self.execute_runtime_invocation(RuntimeInvocation::NanoReply(NanoReplyRequest {
                prompt: prompt.clone(),
            }))?;
        for log_entry in &result.logs {
            self.memory.log_model_call(log_entry)?;
        }
        let normalized = normalize_plain_text_output(&result.raw_output)?;
        self.trace_state_model_stage(trace_state, "fallback_nano");
        self.trace_model_event(
            trace_state,
            "PRIMARY_STAGE_COMPLETED",
            PromptMode::NanoReply.as_str(),
            "nano",
            &result.logs,
            &prompt.prompt,
            &result.raw_output,
            &normalized,
            None,
        )?;

        Ok(ResolvedTurnOutput {
            assistant_text: self.prompt_translator.compose_visible_reply(
                first_beat_text,
                &normalized,
                None,
            ),
            tool_delivery: None,
        })
    }

    fn write_grounded_memories(
        &self,
        user_text: &str,
        source_message_id: i64,
        trace_state: &mut TurnTraceState,
    ) -> Result<()> {
        let grounded = extract_grounded_memories(user_text);
        if grounded.is_empty() {
            return Ok(());
        }

        for fact in &grounded {
            self.memory
                .store_grounded_memory_item(&GroundedMemoryWrite {
                    kind: fact.kind.as_str(),
                    canonical_key: fact.canonical_key.as_str(),
                    text: fact.text.as_str(),
                    salience: 0.95,
                    source_message_id: Some(source_message_id),
                    source_type: "explicit_user_text",
                    source_ref: "messages",
                })?;
        }

        self.trace_event(
            trace_state,
            "memory",
            "DURABLE_MEMORY_WRITTEN",
            Vec::new(),
            Vec::new(),
            Some(json!({
                "count": grounded.len(),
                "keys": grounded.iter().map(|fact| fact.canonical_key.clone()).collect::<Vec<_>>(),
            })),
            None,
        )?;
        Ok(())
    }

    fn log_prompt_issue(&self, stage: &str, prompt_input: &str, error: &anyhow::Error) {
        let _ = self.memory.log_model_call(&ModelLogEntry {
            created_at: now_rfc3339(),
            model_name: "Prompt pipeline".to_owned(),
            prompt: format!("stage={stage}\ninput={prompt_input}"),
            raw_output: String::new(),
            input_tokens: None,
            output_tokens: None,
            latency_ms: 0,
            http_status: None,
            error_text: Some(error.to_string()),
        });
    }

    fn begin_assistant_turn(
        &self,
        turn_id: &str,
        first_visible_text: &str,
        callbacks: &dyn CoreCallbacks,
    ) {
        callbacks.emit(
            "assistant_started",
            json!({
                "turnId": turn_id
            })
            .to_string(),
        );

        if !first_visible_text.trim().is_empty() {
            for chunk in chunk_text(first_visible_text, self.app_config.stream_chunk_size) {
                callbacks.emit(
                    "assistant_chunk",
                    json!({
                        "turnId": turn_id,
                        "chunk": chunk
                    })
                    .to_string(),
                );
            }
        }
    }

    fn persist_completed_turn(
        &self,
        turn_id: &str,
        input_source: InputSource,
        assistant_reply: &str,
        tool_delivery: Option<&ToolDelivery>,
        callbacks: &dyn CoreCallbacks,
    ) -> Result<PersistedTurnMessages> {
        let tool_message_id = if let Some(tool_delivery) = tool_delivery {
            Some(
                self.memory
                    .append_message(
                        "tool",
                        &tool_delivery.transcript_content,
                        turn_id,
                        input_source,
                        MessageContentType::ToolResult,
                        Some(tool_delivery.transcript_display_json.as_str()),
                        Some(tool_delivery.visible_summary.as_str()),
                        None,
                    )?
                    .id,
            )
        } else {
            None
        };

        let assistant_message = self
            .memory
            .append_message(
                "assistant",
                assistant_reply,
                turn_id,
                input_source,
                MessageContentType::PlainText,
                None,
                Some(assistant_reply),
                None,
            )
            .context("failed to persist assistant message")?;

        callbacks.emit(
            "assistant_completed",
            json!({
                "turnId": turn_id,
                "message": assistant_message
            })
            .to_string(),
        );

        Ok(PersistedTurnMessages {
            assistant_message,
            tool_message_id,
        })
    }

    fn execute_tool_delivery(
        &self,
        turn_id: &str,
        tool_request: &ToolRequest,
        callbacks: &dyn CoreCallbacks,
        trace_state: &mut TurnTraceState,
    ) -> Result<ToolDelivery> {
        let tool_result =
            self.execute_tool_and_log(turn_id, tool_request, callbacks, trace_state)?;
        self.emit_external_effects(turn_id, &tool_result, callbacks);
        let visible_card = tool_result.visible_card();
        Ok(ToolDelivery {
            summary: tool_result.summary.clone(),
            transcript_content: visible_card.body.clone(),
            transcript_display_json: serde_json::to_string_pretty(&visible_card)?,
            visible_summary: visible_card.comparison_text.clone(),
        })
    }

    fn execute_tool_and_log(
        &self,
        turn_id: &str,
        tool_request: &ToolRequest,
        callbacks: &dyn CoreCallbacks,
        trace_state: &mut TurnTraceState,
    ) -> Result<ToolExecutionResult> {
        let tool_name = tool_request.tool.as_str().to_owned();
        callbacks.emit(
            "tool_status",
            json!({
                "turnId": turn_id,
                "tool": tool_request.tool,
                "action": tool_request.action,
                "status": "executing"
            })
            .to_string(),
        );

        let tool_started_at = Instant::now();
        let tool_result = match self.execute_tool(tool_request) {
            Ok(result) => result,
            Err(error) => {
                let failure_payload = json!({
                    "status": "error",
                    "action": tool_request.action,
                    "message": error.to_string()
                });
                self.memory.log_tool_call(&crate::logging::ToolLogEntry {
                    created_at: now_rfc3339(),
                    tool_name: tool_name.clone(),
                    action: tool_request.action.clone(),
                    arguments_json: serde_json::to_string(&tool_request.arguments)?,
                    result_json: serde_json::to_string(&failure_payload)?,
                    success: false,
                    latency_ms: elapsed_millis(tool_started_at),
                })?;
                emit_diagnostic_log(
                    callbacks,
                    "tools",
                    "warning",
                    "tool action failed",
                    Some(turn_id),
                    json!({
                        "tool": tool_name,
                        "action": tool_request.action,
                        "error": error.to_string(),
                    }),
                );
                return Err(error);
            }
        };

        self.memory.log_tool_call(&crate::logging::ToolLogEntry {
            created_at: now_rfc3339(),
            tool_name: tool_name.clone(),
            action: tool_request.action.clone(),
            arguments_json: serde_json::to_string(&tool_request.arguments)?,
            result_json: serde_json::to_string(&tool_result.result_json)?,
            success: tool_result.is_success(),
            latency_ms: elapsed_millis(tool_started_at),
        })?;
        trace_state.summary.tool_used = true;
        self.trace_event(
            trace_state,
            "tool",
            "TOOL_EXECUTED",
            Vec::new(),
            Vec::new(),
            Some(json!({
                "tool": tool_name,
                "action": tool_request.action,
                "status": tool_result.result_json.get("status").and_then(Value::as_str),
            })),
            Some(vec![trace_payload(
                "tool_result",
                TraceVisibilityClass::DebugLocal,
                "json",
                &serde_json::to_string_pretty(&tool_result.result_json)
                    .unwrap_or_else(|_| "{}".to_owned()),
            )]),
        )?;
        emit_diagnostic_log(
            callbacks,
            "tools",
            if tool_result.is_success() {
                "info"
            } else {
                "warning"
            },
            "tool action completed",
            Some(turn_id),
            json!({
                "tool": tool_name,
                "action": tool_request.action,
                "status": tool_result.result_json.get("status").and_then(Value::as_str),
            }),
        );

        Ok(tool_result)
    }

    fn emit_external_effects(
        &self,
        turn_id: &str,
        tool_result: &ToolExecutionResult,
        callbacks: &dyn CoreCallbacks,
    ) {
        if tool_result
            .result_json
            .get("status")
            .and_then(Value::as_str)
            == Some("auth_started")
        {
            if let Some(url) = tool_result
                .result_json
                .get("authorize_url")
                .and_then(Value::as_str)
            {
                callbacks.emit(
                    "open_external_url",
                    json!({
                        "turnId": turn_id,
                        "url": url,
                        "purpose": "spotify_auth"
                    })
                    .to_string(),
                );
            }
        }
    }

    fn execute_tool(&self, request: &ToolRequest) -> Result<ToolExecutionResult> {
        self.tools
            .execute(request)
            .map_err(|error| anyhow!("tool execution failed: {error}"))
    }

    fn trace_event(
        &self,
        trace_state: &mut TurnTraceState,
        category: &str,
        name: &str,
        reason_codes: Vec<String>,
        model_stage_names: Vec<String>,
        selected_refs_json: Option<Value>,
        payloads: Option<Vec<TracePayloadAttachment>>,
    ) -> Result<()> {
        if !model_stage_names.is_empty() {
            trace_state.summary.model_stages = model_stage_names;
        }
        trace_state.stage_seq += 1;
        self.memory.append_trace_event(&TraceEventRecord {
            turn_id: trace_state.summary.turn_id.clone(),
            stage_seq: trace_state.stage_seq,
            timestamp: now_rfc3339(),
            category: category.to_owned(),
            name: name.to_owned(),
            plan_kind: Some(trace_state.summary.plan_kind.clone()),
            fallback_plan_kind: trace_state.summary.fallback_plan_kind.clone(),
            reason_codes,
            context_policy: trace_state.summary.context_policy.clone(),
            prompt_mode: None,
            lane: None,
            provider: None,
            model: None,
            status: None,
            latency_ms: None,
            http_status: None,
            error_text: None,
            displayed_text: None,
            selected_refs_json,
            payloads: payloads.unwrap_or_default(),
        })
    }

    fn trace_model_event(
        &self,
        trace_state: &mut TurnTraceState,
        event_name: &str,
        prompt_mode: &str,
        lane: &str,
        logs: &[ModelLogEntry],
        raw_input: &str,
        raw_output: &str,
        normalized_output: &str,
        discarded_output: Option<&str>,
    ) -> Result<()> {
        let log_entry = logs.first();
        trace_state.stage_seq += 1;
        self.memory.append_trace_event(&TraceEventRecord {
            turn_id: trace_state.summary.turn_id.clone(),
            stage_seq: trace_state.stage_seq,
            timestamp: now_rfc3339(),
            category: "model".to_owned(),
            name: event_name.to_owned(),
            plan_kind: Some(trace_state.summary.plan_kind.clone()),
            fallback_plan_kind: trace_state.summary.fallback_plan_kind.clone(),
            reason_codes: Vec::new(),
            context_policy: trace_state.summary.context_policy.clone(),
            prompt_mode: Some(prompt_mode.to_owned()),
            lane: Some(lane.to_owned()),
            provider: None,
            model: log_entry.map(|entry| entry.model_name.clone()),
            status: Some(
                if log_entry
                    .and_then(|entry| entry.error_text.as_ref())
                    .is_some()
                {
                    "error".to_owned()
                } else {
                    "success".to_owned()
                },
            ),
            latency_ms: log_entry.map(|entry| entry.latency_ms),
            http_status: log_entry.and_then(|entry| entry.http_status),
            error_text: log_entry.and_then(|entry| entry.error_text.clone()),
            displayed_text: Some(normalized_output.to_owned()),
            selected_refs_json: None,
            payloads: {
                let mut payloads = vec![
                    trace_payload(
                        "raw_input",
                        TraceVisibilityClass::SensitiveLocal,
                        "text",
                        raw_input,
                    ),
                    trace_payload(
                        "raw_output",
                        TraceVisibilityClass::SensitiveLocal,
                        "text",
                        raw_output,
                    ),
                    trace_payload(
                        "normalized_output",
                        TraceVisibilityClass::DebugLocal,
                        "text",
                        normalized_output,
                    ),
                ];
                if let Some(discarded_output) = discarded_output {
                    payloads.push(trace_payload(
                        "discarded_output",
                        TraceVisibilityClass::DebugLocal,
                        "text",
                        discarded_output,
                    ));
                }
                payloads
            },
        })
    }

    fn trace_state_model_stage(&self, trace_state: &mut TurnTraceState, model_stage: &str) {
        if !trace_state
            .summary
            .model_stages
            .iter()
            .any(|stage| stage == model_stage)
        {
            trace_state
                .summary
                .model_stages
                .push(model_stage.to_owned());
        }
    }

    fn delivery_mode(&self, summary: &TurnTraceSummary) -> String {
        if summary.had_fallback {
            return if summary
                .model_stages
                .iter()
                .any(|stage| stage == "fallback_nano")
            {
                "FALLBACK_NANO".to_owned()
            } else {
                "FALLBACK_VISIBLE".to_owned()
            };
        }
        let used_local_model = summary.model_stages.iter().any(|stage| {
            matches!(
                stage.as_str(),
                "first_beat" | "nano_reply" | "fallback_nano"
            )
        });
        match (used_local_model, summary.cloud_used, summary.tool_used) {
            (true, true, true) => "NANO_THEN_CLOUD_TOOL".to_owned(),
            (true, true, false) => "NANO_THEN_CLOUD".to_owned(),
            (true, false, true) => "NANO_THEN_TOOL".to_owned(),
            (true, false, false) => "NANO_ONLY".to_owned(),
            (false, false, true) => "TOOL_ONLY".to_owned(),
            _ => "UNSPECIFIED".to_owned(),
        }
    }

    fn final_route(&self, summary: &TurnTraceSummary) -> String {
        let mut stages = Vec::new();
        if summary.model_stages.iter().any(|stage| {
            matches!(
                stage.as_str(),
                "first_beat" | "nano_reply" | "fallback_nano"
            )
        }) {
            stages.push("nano");
        }
        if summary.cloud_used {
            stages.push("cloud");
        }
        if summary.tool_used {
            stages.push("tool");
        }
        if summary.had_fallback {
            stages.push("fallback");
        }
        if stages.is_empty() {
            "none".to_owned()
        } else {
            stages.join(" + ")
        }
    }

    fn enter_fallback(
        &self,
        trace_state: &mut TurnTraceState,
        fallback_kind: FallbackPlanKind,
        reason_codes: Vec<String>,
    ) -> Result<()> {
        trace_state.summary.had_fallback = true;
        trace_state.summary.fallback_plan_kind = Some(fallback_kind.as_str().to_owned());
        self.memory.upsert_turn_summary(&trace_state.summary)?;
        self.trace_event(
            trace_state,
            "fallback",
            "FALLBACK_ENTERED",
            reason_codes,
            Vec::new(),
            None,
            None,
        )
    }
}

struct ResolvedTurnOutput {
    assistant_text: String,
    tool_delivery: Option<ToolDelivery>,
}

#[derive(Debug, Clone)]
struct GroundedFactCandidate {
    kind: String,
    canonical_key: String,
    text: String,
}

fn extract_grounded_memories(user_text: &str) -> Vec<GroundedFactCandidate> {
    let trimmed = user_text.trim();
    if trimmed.is_empty() || trimmed.ends_with('?') {
        return Vec::new();
    }
    let lower = trimmed.to_lowercase();
    let mut memories = Vec::new();

    if let Some((subject, value)) = split_after_prefix(&lower, "my ", " is ") {
        let value = sanitize_fact_fragment(value);
        if !subject.trim().is_empty() && !value.is_empty() {
            let subject = sanitize_fact_fragment(subject);
            memories.push(GroundedFactCandidate {
                kind: "profile".to_owned(),
                canonical_key: format!("profile:{}", normalize_key(subject.as_str())),
                text: format!("Your {subject} is {value}."),
            });
        }
    }

    if let Some(preference) = lower.strip_prefix("i like ") {
        let preference = sanitize_fact_fragment(preference);
        if !preference.is_empty() {
            memories.push(GroundedFactCandidate {
                kind: "preference".to_owned(),
                canonical_key: format!("preference:like:{}", normalize_key(preference.as_str())),
                text: format!("You like {preference}."),
            });
        }
    }

    if let Some(preference) = lower.strip_prefix("i prefer ") {
        let preference = sanitize_fact_fragment(preference);
        if !preference.is_empty() {
            memories.push(GroundedFactCandidate {
                kind: "preference".to_owned(),
                canonical_key: format!("preference:prefer:{}", normalize_key(preference.as_str())),
                text: format!("You prefer {preference}."),
            });
        }
    }

    if let Some(name) = lower.strip_prefix("call me ") {
        let name = sanitize_fact_fragment(name);
        if !name.is_empty() {
            memories.push(GroundedFactCandidate {
                kind: "profile".to_owned(),
                canonical_key: "profile:preferred_name".to_owned(),
                text: format!("You prefer to be called {name}."),
            });
        }
    }

    memories
}

fn normalize_plain_text_output(raw_output: &str) -> Result<String> {
    let parsed = parse_internal_model_output(raw_output)?;
    match parsed.kind {
        ParsedTurnOutputKind::SuppressedInternalPayload => {
            Err(anyhow!("plain-text stage returned an internal payload"))
        }
        ParsedTurnOutputKind::PlainTextFallback => {
            let normalized = parsed.assistant_reply.trim().to_owned();
            if normalized.is_empty() {
                Err(anyhow!("plain-text stage returned an empty response"))
            } else {
                Ok(normalized)
            }
        }
        ParsedTurnOutputKind::Structured => {
            if parsed.tool_request.is_some() {
                return Err(anyhow!("plain-text stage returned a tool request envelope"));
            }
            let normalized = parsed.assistant_reply.trim().to_owned();
            if normalized.is_empty() {
                Err(anyhow!(
                    "plain-text stage returned an empty assistant reply"
                ))
            } else {
                Ok(normalized)
            }
        }
    }
}

fn split_after_prefix<'a>(
    value: &'a str,
    prefix: &str,
    separator: &str,
) -> Option<(&'a str, &'a str)> {
    let stripped = value.strip_prefix(prefix)?;
    let (left, right) = stripped.split_once(separator)?;
    Some((left.trim(), right.trim()))
}

fn sanitize_fact_fragment(value: &str) -> String {
    value
        .trim()
        .trim_matches(|character: char| matches!(character, '.' | '!' | '?' | '"' | '\''))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_key(value: &str) -> String {
    value
        .to_lowercase()
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_")
}

fn compact_user_summary(value: &str) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= 96 {
        normalized
    } else {
        let truncated = normalized.chars().take(95).collect::<String>();
        format!("{truncated}…")
    }
}

fn trace_payload(
    slot: &str,
    visibility_class: TraceVisibilityClass,
    content_format: &str,
    content_text: &str,
) -> TracePayloadAttachment {
    TracePayloadAttachment {
        slot: slot.to_owned(),
        visibility_class,
        content_format: content_format.to_owned(),
        content_text: content_text.to_owned(),
        size_bytes: content_text.len(),
    }
}

fn chunk_text(text: &str, max_chunk_size: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if current.len() + word.len() + 1 > max_chunk_size && !current.is_empty() {
            chunks.push(current.trim().to_owned());
            current.clear();
        }
        current.push_str(word);
        current.push(' ');
    }

    if !current.trim().is_empty() {
        chunks.push(current.trim().to_owned());
    }

    if chunks.is_empty() && !text.is_empty() {
        chunks.push(text.to_owned());
    }

    chunks
}

fn collect_turn_message_ids(
    user_message_id: i64,
    tool_message_id: Option<i64>,
    assistant_message_id: i64,
) -> Vec<i64> {
    let mut message_ids = vec![user_message_id];
    if let Some(tool_message_id) = tool_message_id {
        message_ids.push(tool_message_id);
    }
    message_ids.push(assistant_message_id);
    message_ids
}

fn elapsed_millis(started_at: Instant) -> i64 {
    started_at.elapsed().as_millis().min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::{NanoRuntimeStatus, RuntimeOverview, RuntimeProfileSummary};
    use crate::memory::{
        ContinuityMode, ContinuitySignal, MemoryStore, RecallBundle, RecallExplanation,
        WarmContext, WorkingMemorySnapshot,
    };
    use crate::model_runtime::{
        AmbientResult, CloudReasoningResult, FirstBeatResult, ModelRuntime, NanoReplyResult,
    };
    use crate::prompt_translator::PromptConfig;
    use crate::tools::ToolExecutor;
    use tempfile::tempdir;

    struct NoopRuntime;

    impl ModelRuntime for NoopRuntime {
        fn run_first_beat(&self, _request: &FirstBeatRequest) -> Result<FirstBeatResult> {
            Ok(FirstBeatResult {
                text: String::new(),
                logs: Vec::new(),
            })
        }

        fn run_nano_reply(&self, _request: &NanoReplyRequest) -> Result<NanoReplyResult> {
            Ok(NanoReplyResult {
                raw_output: String::new(),
                logs: Vec::new(),
            })
        }

        fn run_cloud_reasoning(
            &self,
            _request: &CloudReasoningRequest,
        ) -> Result<CloudReasoningResult> {
            Ok(CloudReasoningResult {
                raw_output: String::new(),
                logs: Vec::new(),
            })
        }

        fn run_ambient(&self, _request: &AmbientRequest) -> Result<Option<AmbientResult>> {
            Ok(None)
        }

        fn overview(&self) -> RuntimeOverview {
            RuntimeOverview {
                nano: NanoRuntimeStatus {
                    enabled: true,
                    availability: "available".to_owned(),
                    detail: "Gemini Nano is ready.".to_owned(),
                    provider: "gemini".to_owned(),
                    model: "gemini-nano".to_owned(),
                    active: true,
                },
                selected_cloud_profile_id: Some("compat".to_owned()),
                selected_cloud_profile_label: Some("Compat".to_owned()),
                cloud_profiles: vec![RuntimeProfileSummary {
                    id: "compat".to_owned(),
                    label: "Compat".to_owned(),
                    provider: "openai".to_owned(),
                    model: "test-model".to_owned(),
                    enabled: true,
                    available: true,
                    selected: true,
                }],
            }
        }

        fn can_accept_turns(&self) -> bool {
            true
        }

        fn selected_cloud_profile_id(&self) -> Option<String> {
            None
        }

        fn set_selected_cloud_profile(&self, _profile_id: &str) -> Result<()> {
            Ok(())
        }

        fn default_selected_cloud_profile_id(&self) -> Option<String> {
            None
        }
    }

    fn test_engine() -> HgieEngine {
        let temp = tempdir().expect("tempdir").keep();
        let app_config = AppConfig {
            default_previous_context: ContextLevel::Medium,
            vector_dimensions: 32,
            memory_salience_threshold: 0.6,
            stream_chunk_size: 16,
            max_recent_messages_per_turn: 32,
            max_model_logs: 20,
            idle_resume_threshold_seconds: 900,
            ambient_cooldown_seconds: 600,
        };
        let memory = MemoryStore::new(temp.join("core.sqlite3"), &app_config).expect("memory");
        HgieEngine::new(
            memory.clone(),
            ToolExecutor::new(memory),
            Arc::new(NoopRuntime),
            PromptTranslator::new(PromptConfig::default()),
            app_config,
        )
    }

    fn retrieved_memory(text: &str, relevance_score: f32) -> RetrievedMemory {
        RetrievedMemory {
            id: 1,
            kind: "fact".to_owned(),
            text: text.to_owned(),
            salience: 0.9,
            similarity: 0.82,
            source_message_id: Some(10),
            created_at: "2026-03-13T10:00:00Z".to_owned(),
            relevance_score,
            canonical_key: Some("profile:cat".to_owned()),
        }
    }

    fn recall_bundle(strong_hit: bool, memories: Vec<RetrievedMemory>) -> RecallBundle {
        RecallBundle {
            working_memory: WorkingMemorySnapshot::default(),
            warm_context: WarmContext {
                durable_memories: memories,
                ..WarmContext::default()
            },
            explanation: RecallExplanation {
                strong_hit,
                ..RecallExplanation::default()
            },
            ..RecallBundle::default()
        }
    }

    fn recall_bundle_with_continuity(
        strong_hit: bool,
        memories: Vec<RetrievedMemory>,
        continuity: ContinuitySignal,
    ) -> RecallBundle {
        let mut bundle = recall_bundle(strong_hit, memories);
        bundle.explanation.continuity = continuity;
        bundle
    }

    #[test]
    fn plan_turn_route_keeps_greetings_direct_and_memory_recall_local() {
        let engine = test_engine();
        let runtime_capabilities = engine.runtime_capabilities();

        let greeting =
            engine.plan_turn_route("hi there", &RecallBundle::default(), runtime_capabilities);
        assert_eq!(greeting.plan_kind, TurnPlanKind::DirectNano);
        assert_eq!(greeting.context_policy, ContextPolicy::TranscriptOnly);
        assert!(greeting.ack_beat().is_none());
        assert_eq!(
            greeting.primary_beat().map(|beat| beat.role),
            Some(ExecutionRole::LocalReply)
        );

        let recall = engine.plan_turn_route(
            "who's Mocha?",
            &recall_bundle(
                true,
                vec![retrieved_memory(
                    "Mocha is your cat arriving Tuesday.",
                    0.84,
                )],
            ),
            runtime_capabilities,
        );
        assert_eq!(recall.plan_kind, TurnPlanKind::RecallNano);
        assert_eq!(recall.context_policy, ContextPolicy::DurableOnly);
        assert!(recall.ack_beat().is_none());
        assert_eq!(
            recall.primary_beat().map(|beat| beat.role),
            Some(ExecutionRole::MemoryReply)
        );
    }

    #[test]
    fn plan_turn_route_uses_cloud_for_deep_and_probable_tool_turns() {
        let engine = test_engine();
        let runtime_capabilities = engine.runtime_capabilities();

        let deep = engine.plan_turn_route(
            "Explain the tradeoffs between SQLite WAL and rollback journal modes for Android",
            &RecallBundle::default(),
            runtime_capabilities,
        );
        assert_eq!(deep.plan_kind, TurnPlanKind::CloudEscalated);
        assert!(!deep.tool_consulted);
        assert_eq!(
            deep.primary_beat().map(|beat| beat.role),
            Some(ExecutionRole::DeepReasoning)
        );
        assert_eq!(
            deep.ack_beat().map(|beat| beat.role),
            Some(ExecutionRole::FastAck)
        );

        let tool = engine.plan_turn_route(
            "Can you sort out Spotify for me?",
            &RecallBundle::default(),
            runtime_capabilities,
        );
        assert_eq!(tool.plan_kind, TurnPlanKind::CloudTool);
        assert!(tool.tool_consulted);
        assert_eq!(
            tool.primary_beat().map(|beat| beat.role),
            Some(ExecutionRole::StructuredToolDecision)
        );
        assert_eq!(
            tool.ack_beat().map(|beat| beat.role),
            Some(ExecutionRole::FastAck)
        );

        let exact_tool = engine.plan_turn_route(
            "what is playing on spotify",
            &RecallBundle::default(),
            runtime_capabilities,
        );
        assert_eq!(exact_tool.plan_kind, TurnPlanKind::ToolDirect);
        assert!(exact_tool.ack_beat().is_none());
        assert_eq!(
            exact_tool.primary_beat().map(|beat| beat.role),
            Some(ExecutionRole::DeterministicTool)
        );
    }

    #[test]
    fn plan_turn_route_uses_memory_continuity_to_adjust_context_policy() {
        let engine = test_engine();
        let runtime_capabilities = engine.runtime_capabilities();

        let returning = engine.plan_turn_route(
            "Back to the memory design thread.",
            &recall_bundle_with_continuity(
                false,
                Vec::new(),
                ContinuitySignal {
                    mode: ContinuityMode::Return,
                    selected_thread_memory_ids: vec![7],
                    ..ContinuitySignal::default()
                },
            ),
            runtime_capabilities,
        );
        assert_eq!(returning.plan_kind, TurnPlanKind::DirectNano);
        assert_eq!(
            returning.context_policy,
            ContextPolicy::TranscriptPlusDurable
        );
        assert_eq!(
            returning.primary_beat().map(|beat| beat.context_recipe),
            Some(ContextPolicy::TranscriptPlusDurable)
        );
        assert!(returning
            .reason_codes
            .iter()
            .any(|reason| reason == "continuity:return"));

        let open_loop = engine.plan_turn_route(
            "and then",
            &recall_bundle_with_continuity(
                false,
                Vec::new(),
                ContinuitySignal {
                    mode: ContinuityMode::OpenLoop,
                    open_loop_match: true,
                    ..ContinuitySignal::default()
                },
            ),
            runtime_capabilities,
        );
        assert_eq!(open_loop.plan_kind, TurnPlanKind::DirectNano);
        assert!(open_loop
            .reason_codes
            .iter()
            .any(|reason| reason == "continuity:open_loop"));
    }

    #[test]
    fn extract_grounded_memories_only_from_explicit_statements() {
        let memories = extract_grounded_memories("My cat is Mocha");
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].canonical_key, "profile:cat");
        assert_eq!(memories[0].text, "Your cat is mocha.");

        assert!(extract_grounded_memories("When is Mocha arriving?").is_empty());
    }
}
