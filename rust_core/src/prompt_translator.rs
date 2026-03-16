use std::collections::HashSet;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::memory::RetrievedMemory;
use crate::model::{ModelMessage, ModelRequest};
use crate::tools::ToolRequest;
use crate::{ChatMessage, ContextLevel, InputSource};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCandidate {
    pub kind: String,
    pub text: String,
    pub salience: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnEnvelope {
    pub assistant_reply: String,
    pub tool_request: Option<ToolRequest>,
    #[serde(default)]
    pub memory_candidates: Vec<MemoryCandidate>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParsedTurnOutputKind {
    Structured,
    PlainTextFallback,
    SuppressedInternalPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedTurnOutput {
    pub assistant_reply: String,
    pub tool_request: Option<ToolRequest>,
    #[serde(default)]
    pub memory_candidates: Vec<MemoryCandidate>,
    pub looks_like_internal_payload: bool,
    pub raw_output: String,
    pub kind: ParsedTurnOutputKind,
}

impl ParsedTurnOutput {
    fn structured(envelope: TurnEnvelope, raw_output: &str) -> Self {
        Self {
            assistant_reply: envelope.assistant_reply,
            tool_request: envelope.tool_request,
            memory_candidates: envelope.memory_candidates,
            looks_like_internal_payload: false,
            raw_output: raw_output.to_owned(),
            kind: ParsedTurnOutputKind::Structured,
        }
    }

    pub fn plain_text(raw_output: &str) -> Self {
        Self {
            assistant_reply: raw_output.trim().to_owned(),
            tool_request: None,
            memory_candidates: Vec::new(),
            looks_like_internal_payload: false,
            raw_output: raw_output.to_owned(),
            kind: ParsedTurnOutputKind::PlainTextFallback,
        }
    }

    fn suppressed(raw_output: &str) -> Self {
        Self {
            assistant_reply: String::new(),
            tool_request: None,
            memory_candidates: Vec::new(),
            looks_like_internal_payload: true,
            raw_output: raw_output.to_owned(),
            kind: ParsedTurnOutputKind::SuppressedInternalPayload,
        }
    }

    pub fn to_turn_envelope(&self) -> TurnEnvelope {
        TurnEnvelope {
            assistant_reply: self.assistant_reply.clone(),
            tool_request: self.tool_request.clone(),
            memory_candidates: self.memory_candidates.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalVisibleResponse {
    pub assistant_text: String,
    pub tool_card_payload: Option<String>,
    pub memory_items_to_store: Vec<MemoryCandidate>,
    pub was_bridged: bool,
    pub was_direct: bool,
    pub was_suppressed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorPolicy {
    pub allow_ambient: bool,
    pub allow_playfulness: bool,
    pub allow_humor: bool,
    pub prefer_silence: bool,
    pub allow_initiative: bool,
}

impl BehaviorPolicy {
    pub fn direct_turn() -> Self {
        Self {
            allow_ambient: false,
            allow_playfulness: true,
            allow_humor: false,
            prefer_silence: false,
            allow_initiative: false,
        }
    }

    pub fn ambient_turn() -> Self {
        Self {
            allow_ambient: true,
            allow_playfulness: true,
            allow_humor: true,
            prefer_silence: false,
            allow_initiative: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptMode {
    FirstBeat,
    NanoReply,
    CloudReasoning,
    FollowupReasoning,
    MemoryBackedRecall,
    SummaryBridge,
    AmbientLine,
    ToolStatusExplanation,
    ErrorExplanation,
}

impl PromptMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FirstBeat => "first_beat",
            Self::NanoReply => "nano_reply",
            Self::CloudReasoning => "cloud_reasoning",
            Self::FollowupReasoning => "followup_reasoning",
            Self::MemoryBackedRecall => "memory_backed_recall",
            Self::SummaryBridge => "summary_bridge",
            Self::AmbientLine => "ambient_line",
            Self::ToolStatusExplanation => "tool_status_explanation",
            Self::ErrorExplanation => "error_explanation",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PromptOutputFormat {
    #[default]
    PlainText,
    JsonEnvelope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PromptModeConfig {
    pub instruction: String,
    pub max_context_messages: usize,
    pub max_memory_items: usize,
    pub max_words: Option<usize>,
    pub max_output_tokens: Option<u32>,
    pub output_format: PromptOutputFormat,
}

impl Default for PromptModeConfig {
    fn default() -> Self {
        Self {
            instruction: String::new(),
            max_context_messages: 4,
            max_memory_items: 0,
            max_words: None,
            max_output_tokens: None,
            output_format: PromptOutputFormat::PlainText,
        }
    }
}

impl PromptModeConfig {
    fn for_mode(mode: PromptMode) -> Self {
        match mode {
            PromptMode::FirstBeat => Self {
                instruction: "Return only the first visible conversational beat. Do not answer fully. Do not mention models.".to_owned(),
                max_context_messages: 4,
                max_memory_items: 0,
                max_words: Some(14),
                max_output_tokens: Some(48),
                output_format: PromptOutputFormat::PlainText,
            },
            PromptMode::NanoReply => Self {
                instruction: "Reply directly in plain text. Stay conversational, user-facing, and compact. Do not return JSON.".to_owned(),
                max_context_messages: 6,
                max_memory_items: 3,
                max_words: Some(60),
                max_output_tokens: Some(128),
                output_format: PromptOutputFormat::PlainText,
            },
            PromptMode::CloudReasoning => Self {
                instruction: "Continue the answer after the first beat. Do not restart with a greeting, acknowledgment, or recap. Return strict JSON with assistant_reply and tool_request only. assistant_reply must stay user-facing.".to_owned(),
                max_context_messages: 8,
                max_memory_items: 4,
                max_words: None,
                max_output_tokens: Some(192),
                output_format: PromptOutputFormat::JsonEnvelope,
            },
            PromptMode::FollowupReasoning => Self {
                instruction: "Continue the answer after the first beat. Do not restart with a greeting, acknowledgment, or recap. Return strict JSON with assistant_reply, tool_request, and memory_candidates.".to_owned(),
                max_context_messages: 8,
                max_memory_items: 4,
                max_words: None,
                max_output_tokens: Some(192),
                output_format: PromptOutputFormat::JsonEnvelope,
            },
            PromptMode::MemoryBackedRecall => Self {
                instruction: "Answer the user from the selected memory facts only. Keep it short, concrete, and conversational. If the facts are insufficient, say so briefly instead of inventing details.".to_owned(),
                max_context_messages: 4,
                max_memory_items: 3,
                max_words: Some(36),
                max_output_tokens: Some(96),
                output_format: PromptOutputFormat::PlainText,
            },
            PromptMode::SummaryBridge => Self {
                instruction: "Turn the source answer into one short user-facing continuation that preserves the facts, sounds like the same assistant, and does not mention internal systems or models.".to_owned(),
                max_context_messages: 4,
                max_memory_items: 3,
                max_words: Some(38),
                max_output_tokens: Some(96),
                output_format: PromptOutputFormat::PlainText,
            },
            PromptMode::AmbientLine => Self {
                instruction: "Return one calm ambient line only when the event materially helps the user. Do not mention models.".to_owned(),
                max_context_messages: 4,
                max_memory_items: 0,
                max_words: Some(16),
                max_output_tokens: Some(40),
                output_format: PromptOutputFormat::PlainText,
            },
            PromptMode::ToolStatusExplanation => Self {
                instruction: "Explain the current tool status in one short user-friendly line.".to_owned(),
                max_context_messages: 3,
                max_memory_items: 0,
                max_words: Some(22),
                max_output_tokens: Some(72),
                output_format: PromptOutputFormat::PlainText,
            },
            PromptMode::ErrorExplanation => Self {
                instruction: "Explain the issue calmly in one short user-friendly line without internal jargon.".to_owned(),
                max_context_messages: 3,
                max_memory_items: 0,
                max_words: Some(22),
                max_output_tokens: Some(72),
                output_format: PromptOutputFormat::PlainText,
            },
        }
    }

    fn merge_defaults(mode: PromptMode, configured: &PromptModeConfig) -> Self {
        let defaults = Self::for_mode(mode);
        Self {
            instruction: if configured.instruction.trim().is_empty() {
                defaults.instruction
            } else {
                configured.instruction.clone()
            },
            max_context_messages: if configured.max_context_messages == 0 {
                defaults.max_context_messages
            } else {
                configured.max_context_messages
            },
            max_memory_items: if configured.max_memory_items == 0 {
                defaults.max_memory_items
            } else {
                configured.max_memory_items
            },
            max_words: configured.max_words.or(defaults.max_words),
            max_output_tokens: configured.max_output_tokens.or(defaults.max_output_tokens),
            output_format: match configured.output_format {
                PromptOutputFormat::PlainText
                    if defaults.output_format == PromptOutputFormat::JsonEnvelope =>
                {
                    defaults.output_format
                }
                other => other,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PromptModesConfig {
    pub first_beat: PromptModeConfig,
    pub nano_reply: PromptModeConfig,
    pub cloud_reasoning: PromptModeConfig,
    pub followup_reasoning: PromptModeConfig,
    pub memory_backed_recall: PromptModeConfig,
    pub summary_bridge: PromptModeConfig,
    pub ambient_line: PromptModeConfig,
    pub tool_status_explanation: PromptModeConfig,
    pub error_explanation: PromptModeConfig,
}

impl Default for PromptModesConfig {
    fn default() -> Self {
        Self {
            first_beat: PromptModeConfig::for_mode(PromptMode::FirstBeat),
            nano_reply: PromptModeConfig::for_mode(PromptMode::NanoReply),
            cloud_reasoning: PromptModeConfig::for_mode(PromptMode::CloudReasoning),
            followup_reasoning: PromptModeConfig::for_mode(PromptMode::FollowupReasoning),
            memory_backed_recall: PromptModeConfig::for_mode(PromptMode::MemoryBackedRecall),
            summary_bridge: PromptModeConfig::for_mode(PromptMode::SummaryBridge),
            ambient_line: PromptModeConfig::for_mode(PromptMode::AmbientLine),
            tool_status_explanation: PromptModeConfig::for_mode(PromptMode::ToolStatusExplanation),
            error_explanation: PromptModeConfig::for_mode(PromptMode::ErrorExplanation),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PromptSharedConfig {
    pub assistant_identity: String,
    pub continuity_instruction: String,
    pub memory_preamble: String,
    pub tool_instructions: String,
    pub response_contract: String,
}

impl Default for PromptSharedConfig {
    fn default() -> Self {
        Self {
            assistant_identity: String::new(),
            continuity_instruction: String::new(),
            memory_preamble: String::new(),
            tool_instructions: String::new(),
            response_contract: String::new(),
        }
    }
}

impl PromptSharedConfig {
    fn fallback_defaults() -> Self {
        Self {
            assistant_identity:
                "You are Baby Gervaise, a continuous local-first computer partner.".to_owned(),
            continuity_instruction:
                "Stay inside one continuing conversation. Never mention threads, sessions, or starting over.".to_owned(),
            memory_preamble:
                "Use retrieved context only when it materially improves continuity or factual accuracy.".to_owned(),
            tool_instructions:
                "If a deterministic tool is required, return tool_request JSON using tool, action, and arguments. Supported tools are spotify and hue. Never claim a tool executed when it did not.".to_owned(),
            response_contract:
                "Return strict JSON with assistant_reply and tool_request. This envelope is internal system output and must not leak into visible chat.".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PromptConfig {
    pub system_prompt: String,
    pub memory_preamble: String,
    pub tool_instructions: String,
    pub response_contract: String,
    pub shared: PromptSharedConfig,
    pub modes: PromptModesConfig,
}

impl Default for PromptConfig {
    fn default() -> Self {
        Self {
            system_prompt: String::new(),
            memory_preamble: String::new(),
            tool_instructions: String::new(),
            response_contract: String::new(),
            shared: PromptSharedConfig::fallback_defaults(),
            modes: PromptModesConfig::default(),
        }
    }
}

impl PromptConfig {
    pub fn from_legacy_strings(
        system_prompt: impl Into<String>,
        memory_preamble: impl Into<String>,
        tool_instructions: impl Into<String>,
        response_contract: impl Into<String>,
    ) -> Self {
        Self {
            system_prompt: system_prompt.into(),
            memory_preamble: memory_preamble.into(),
            tool_instructions: tool_instructions.into(),
            response_contract: response_contract.into(),
            shared: PromptSharedConfig::default(),
            modes: PromptModesConfig::default(),
        }
    }

    fn shared_config(&self) -> PromptSharedConfig {
        let defaults = PromptSharedConfig::fallback_defaults();
        PromptSharedConfig {
            assistant_identity: if self.shared.assistant_identity.trim().is_empty() {
                if self.system_prompt.trim().is_empty() {
                    defaults.assistant_identity
                } else {
                    self.system_prompt.trim().to_owned()
                }
            } else {
                self.shared.assistant_identity.clone()
            },
            continuity_instruction: if self.shared.continuity_instruction.trim().is_empty() {
                defaults.continuity_instruction
            } else {
                self.shared.continuity_instruction.clone()
            },
            memory_preamble: if self.shared.memory_preamble.trim().is_empty() {
                if self.memory_preamble.trim().is_empty() {
                    defaults.memory_preamble
                } else {
                    self.memory_preamble.trim().to_owned()
                }
            } else {
                self.shared.memory_preamble.clone()
            },
            tool_instructions: if self.shared.tool_instructions.trim().is_empty() {
                if self.tool_instructions.trim().is_empty() {
                    defaults.tool_instructions
                } else {
                    self.tool_instructions.trim().to_owned()
                }
            } else {
                self.shared.tool_instructions.clone()
            },
            response_contract: if self.shared.response_contract.trim().is_empty() {
                if self.response_contract.trim().is_empty() {
                    defaults.response_contract
                } else {
                    self.response_contract.trim().to_owned()
                }
            } else {
                self.shared.response_contract.clone()
            },
        }
    }

    fn mode_config(&self, mode: PromptMode) -> PromptModeConfig {
        let configured = match mode {
            PromptMode::FirstBeat => &self.modes.first_beat,
            PromptMode::NanoReply => &self.modes.nano_reply,
            PromptMode::CloudReasoning => &self.modes.cloud_reasoning,
            PromptMode::FollowupReasoning => &self.modes.followup_reasoning,
            PromptMode::MemoryBackedRecall => &self.modes.memory_backed_recall,
            PromptMode::SummaryBridge => &self.modes.summary_bridge,
            PromptMode::AmbientLine => &self.modes.ambient_line,
            PromptMode::ToolStatusExplanation => &self.modes.tool_status_explanation,
            PromptMode::ErrorExplanation => &self.modes.error_explanation,
        };
        PromptModeConfig::merge_defaults(mode, configured)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledNanoPrompt {
    pub mode: PromptMode,
    pub prompt: String,
    pub max_output_tokens: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct CompiledInteractionPrompt {
    pub nano_prompt: CompiledNanoPrompt,
    pub cloud_request: ModelRequest,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromptOutputContract {
    PlainText,
    JsonEnvelope,
}

impl PromptOutputContract {
    fn as_format(self) -> PromptOutputFormat {
        match self {
            Self::PlainText => PromptOutputFormat::PlainText,
            Self::JsonEnvelope => PromptOutputFormat::JsonEnvelope,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BeatPromptContext<'a> {
    pub input_source: Option<InputSource>,
    pub first_beat: Option<&'a str>,
    pub recent_messages: &'a [ChatMessage],
    pub semantic_memories: &'a [RetrievedMemory],
    pub context_level: Option<ContextLevel>,
    pub policy: &'a BehaviorPolicy,
    pub prefer_memory_facts: bool,
    pub include_tool_guidance: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct PromptTaskFrame<'a> {
    pub mode: PromptMode,
    pub output_contract: PromptOutputContract,
    pub user_text: Option<&'a str>,
    pub source_text: Option<&'a str>,
    pub status_text: Option<&'a str>,
    pub event_type: Option<&'a str>,
    pub payload_json: Option<&'a Value>,
    pub context: BeatPromptContext<'a>,
}

#[derive(Debug, Clone)]
pub enum CompiledPromptArtifact {
    Nano(CompiledNanoPrompt),
    Cloud(ModelRequest),
}

impl CompiledPromptArtifact {
    fn expect_nano(self, mode: PromptMode) -> Result<CompiledNanoPrompt> {
        match self {
            Self::Nano(prompt) => Ok(prompt),
            Self::Cloud(_) => Err(anyhow!(
                "prompt task frame for mode {} compiled as a cloud artifact",
                mode.as_str()
            )),
        }
    }

    fn expect_cloud(self, mode: PromptMode) -> Result<ModelRequest> {
        match self {
            Self::Cloud(request) => Ok(request),
            Self::Nano(_) => Err(anyhow!(
                "prompt task frame for mode {} compiled as a nano artifact",
                mode.as_str()
            )),
        }
    }
}

pub struct PromptTranslator {
    config: PromptConfig,
}

impl PromptTranslator {
    pub fn new(config: PromptConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &PromptConfig {
        &self.config
    }

    pub fn compile_first_beat(
        &self,
        request: &FirstBeatTranslationRequest<'_>,
    ) -> Result<CompiledNanoPrompt> {
        let mode = PromptMode::FirstBeat;
        self.compile_task_frame(&PromptTaskFrame {
            mode,
            output_contract: PromptOutputContract::PlainText,
            user_text: Some(request.user_text),
            source_text: None,
            status_text: None,
            event_type: None,
            payload_json: None,
            context: BeatPromptContext {
                input_source: Some(request.input_source),
                first_beat: None,
                recent_messages: request.recent_messages,
                semantic_memories: &[],
                context_level: None,
                policy: request.policy,
                prefer_memory_facts: false,
                include_tool_guidance: false,
            },
        })?
        .expect_nano(mode)
    }

    pub fn compile_nano_reply(
        &self,
        request: &NanoReplyTranslationRequest<'_>,
    ) -> Result<CompiledNanoPrompt> {
        let mode = PromptMode::NanoReply;
        self.compile_task_frame(&PromptTaskFrame {
            mode,
            output_contract: PromptOutputContract::PlainText,
            user_text: Some(request.user_text),
            source_text: None,
            status_text: None,
            event_type: None,
            payload_json: None,
            context: BeatPromptContext {
                input_source: None,
                first_beat: Some(request.first_beat),
                recent_messages: request.recent_messages,
                semantic_memories: request.semantic_memories,
                context_level: Some(request.context_level),
                policy: request.policy,
                prefer_memory_facts: request.prefer_memory_facts,
                include_tool_guidance: false,
            },
        })?
        .expect_nano(mode)
    }

    pub fn compile_cloud_reasoning(
        &self,
        request: &CloudReasoningTranslationRequest<'_>,
    ) -> Result<ModelRequest> {
        let mode = PromptMode::CloudReasoning;
        self.compile_task_frame(&PromptTaskFrame {
            mode,
            output_contract: PromptOutputContract::JsonEnvelope,
            user_text: Some(request.user_text),
            source_text: None,
            status_text: None,
            event_type: None,
            payload_json: None,
            context: BeatPromptContext {
                input_source: None,
                first_beat: Some(request.first_beat),
                recent_messages: request.recent_messages,
                semantic_memories: request.semantic_memories,
                context_level: None,
                policy: request.policy,
                prefer_memory_facts: false,
                include_tool_guidance: request.include_tool_guidance,
            },
        })?
        .expect_cloud(mode)
    }

    pub fn compile_followup(
        &self,
        request: &FollowupTranslationRequest<'_>,
    ) -> Result<CompiledInteractionPrompt> {
        let mode = self.config.mode_config(PromptMode::FollowupReasoning);
        let shared = self.config.shared_config();

        let nano_prompt = join_sections([
            Some(shared.assistant_identity.clone()),
            Some(shared.continuity_instruction.clone()),
            Some(mode.instruction.clone()),
            Some(render_policy_constraints(
                request.policy,
                PromptMode::FollowupReasoning,
            )),
            Some(format!(
                "Previous Context setting: {}.",
                request.context_level.as_str()
            )),
            Some(format!(
                "First beat already shown: \"{}\"",
                compact_text(request.first_beat, 120)
            )),
            render_recent_context(request.recent_messages, mode.max_context_messages, 180),
            render_selected_memories_for_mode(
                &shared.memory_preamble,
                request.semantic_memories,
                PromptMode::FollowupReasoning,
            ),
            request
                .include_tool_guidance
                .then(|| shared.tool_instructions.clone()),
            Some(shared.response_contract.clone()),
            Some(format!(
                "User input: {}",
                compact_text(request.user_text, 240)
            )),
        ]);

        let mut cloud_messages = vec![ModelMessage {
            role: "system".to_owned(),
            content: join_sections([
                Some(shared.assistant_identity),
                Some(shared.continuity_instruction),
                Some(mode.instruction),
                Some(render_policy_constraints(
                    request.policy,
                    PromptMode::FollowupReasoning,
                )),
            ]),
        }];

        if let Some(memory_block) = render_selected_memories_for_mode(
            &shared.memory_preamble,
            request.semantic_memories,
            PromptMode::FollowupReasoning,
        ) {
            cloud_messages.push(ModelMessage {
                role: "system".to_owned(),
                content: memory_block,
            });
        }

        if request.include_tool_guidance {
            cloud_messages.push(ModelMessage {
                role: "system".to_owned(),
                content: shared.tool_instructions,
            });
        }

        cloud_messages.push(ModelMessage {
            role: "system".to_owned(),
            content: shared.response_contract,
        });
        cloud_messages.push(ModelMessage {
            role: "system".to_owned(),
            content: format!(
                "The first visible beat already shown to the user is: \"{}\". Continue after it without repeating it, without opening with a new greeting or acknowledgment, and without recapping what was already said.",
                compact_text(request.first_beat, 160)
            ),
        });

        let selected_messages =
            select_recent_messages(request.recent_messages, mode.max_context_messages);
        for message in selected_messages {
            cloud_messages.push(ModelMessage {
                role: map_message_role(&message.role),
                content: compact_text(&message.content, 320),
            });
        }

        cloud_messages.push(ModelMessage {
            role: "user".to_owned(),
            content: compact_text(request.user_text, 320),
        });

        Ok(CompiledInteractionPrompt {
            nano_prompt: CompiledNanoPrompt {
                mode: PromptMode::FollowupReasoning,
                prompt: nano_prompt,
                max_output_tokens: mode.max_output_tokens,
            },
            cloud_request: ModelRequest {
                messages: cloud_messages,
            },
        })
    }

    pub fn compile_memory_backed_recall(
        &self,
        request: &MemoryRecallTranslationRequest<'_>,
    ) -> Result<CompiledInteractionPrompt> {
        let mode = self.config.mode_config(PromptMode::MemoryBackedRecall);
        let shared = self.config.shared_config();

        let nano_prompt = join_sections([
            Some(shared.assistant_identity.clone()),
            Some(shared.continuity_instruction.clone()),
            Some(mode.instruction.clone()),
            Some(render_policy_constraints(
                request.policy,
                PromptMode::MemoryBackedRecall,
            )),
            mode.max_words
                .map(|value| format!("Length: under {value} words.")),
            render_recent_context(request.recent_messages, mode.max_context_messages, 140),
            render_selected_memories_for_mode(
                &shared.memory_preamble,
                request.semantic_memories,
                PromptMode::MemoryBackedRecall,
            ),
            Some(format!(
                "User question: {}",
                compact_text(request.user_text, 220)
            )),
        ]);

        let mut cloud_messages = vec![ModelMessage {
            role: "system".to_owned(),
            content: join_sections([
                Some(shared.assistant_identity),
                Some(shared.continuity_instruction),
                Some(mode.instruction),
                Some(render_policy_constraints(
                    request.policy,
                    PromptMode::MemoryBackedRecall,
                )),
            ]),
        }];

        if let Some(memory_block) = render_selected_memories_for_mode(
            &shared.memory_preamble,
            request.semantic_memories,
            PromptMode::MemoryBackedRecall,
        ) {
            cloud_messages.push(ModelMessage {
                role: "system".to_owned(),
                content: memory_block,
            });
        }

        let selected_messages =
            select_recent_messages(request.recent_messages, mode.max_context_messages);
        for message in selected_messages {
            cloud_messages.push(ModelMessage {
                role: map_message_role(&message.role),
                content: compact_text(&message.content, 280),
            });
        }

        cloud_messages.push(ModelMessage {
            role: "user".to_owned(),
            content: compact_text(request.user_text, 280),
        });

        Ok(CompiledInteractionPrompt {
            nano_prompt: CompiledNanoPrompt {
                mode: PromptMode::MemoryBackedRecall,
                prompt: nano_prompt,
                max_output_tokens: mode.max_output_tokens,
            },
            cloud_request: ModelRequest {
                messages: cloud_messages,
            },
        })
    }

    pub fn compile_summary_bridge(
        &self,
        request: &SummaryBridgeTranslationRequest<'_>,
    ) -> Result<CompiledInteractionPrompt> {
        let mode = self.config.mode_config(PromptMode::SummaryBridge);
        let shared = self.config.shared_config();

        let nano_prompt = join_sections([
            Some(shared.assistant_identity.clone()),
            Some(shared.continuity_instruction.clone()),
            Some(mode.instruction.clone()),
            Some(render_policy_constraints(
                request.policy,
                PromptMode::SummaryBridge,
            )),
            mode.max_words
                .map(|value| format!("Length: under {value} words.")),
            Some(format!(
                "First beat already shown: \"{}\"",
                compact_text(request.first_beat, 120)
            )),
            render_recent_context(request.recent_messages, mode.max_context_messages, 140),
            render_selected_memories_for_mode(
                &shared.memory_preamble,
                request.semantic_memories,
                PromptMode::SummaryBridge,
            ),
            Some(format!(
                "Source answer to condense: {}",
                compact_text(request.source_text, 360)
            )),
        ]);

        let mut cloud_messages = vec![ModelMessage {
            role: "system".to_owned(),
            content: join_sections([
                Some(shared.assistant_identity),
                Some(shared.continuity_instruction),
                Some(mode.instruction),
                Some(render_policy_constraints(
                    request.policy,
                    PromptMode::SummaryBridge,
                )),
            ]),
        }];

        if let Some(memory_block) = render_selected_memories_for_mode(
            &shared.memory_preamble,
            request.semantic_memories,
            PromptMode::SummaryBridge,
        ) {
            cloud_messages.push(ModelMessage {
                role: "system".to_owned(),
                content: memory_block,
            });
        }

        cloud_messages.push(ModelMessage {
            role: "system".to_owned(),
            content: format!(
                "The first visible beat already shown to the user is: \"{}\".",
                compact_text(request.first_beat, 160)
            ),
        });
        cloud_messages.push(ModelMessage {
            role: "user".to_owned(),
            content: compact_text(request.source_text, 400),
        });

        Ok(CompiledInteractionPrompt {
            nano_prompt: CompiledNanoPrompt {
                mode: PromptMode::SummaryBridge,
                prompt: nano_prompt,
                max_output_tokens: mode.max_output_tokens,
            },
            cloud_request: ModelRequest {
                messages: cloud_messages,
            },
        })
    }

    pub fn compile_ambient_line(
        &self,
        request: &AmbientLineTranslationRequest<'_>,
    ) -> Result<CompiledNanoPrompt> {
        let mode = PromptMode::AmbientLine;
        self.compile_task_frame(&PromptTaskFrame {
            mode,
            output_contract: PromptOutputContract::PlainText,
            user_text: None,
            source_text: None,
            status_text: None,
            event_type: Some(request.event_type),
            payload_json: Some(request.payload_json),
            context: BeatPromptContext {
                input_source: None,
                first_beat: None,
                recent_messages: request.recent_messages,
                semantic_memories: &[],
                context_level: None,
                policy: request.policy,
                prefer_memory_facts: false,
                include_tool_guidance: false,
            },
        })?
        .expect_nano(mode)
    }

    pub fn compile_tool_status_explanation(
        &self,
        request: &StatusExplanationTranslationRequest<'_>,
    ) -> Result<CompiledNanoPrompt> {
        self.compile_single_line_explanation(
            PromptMode::ToolStatusExplanation,
            request.status_text,
            request.recent_messages,
            request.policy,
        )
    }

    pub fn compile_error_explanation(
        &self,
        request: &StatusExplanationTranslationRequest<'_>,
    ) -> Result<CompiledNanoPrompt> {
        self.compile_single_line_explanation(
            PromptMode::ErrorExplanation,
            request.status_text,
            request.recent_messages,
            request.policy,
        )
    }

    fn compile_single_line_explanation(
        &self,
        mode_key: PromptMode,
        status_text: &str,
        recent_messages: &[ChatMessage],
        policy: &BehaviorPolicy,
    ) -> Result<CompiledNanoPrompt> {
        self.compile_task_frame(&PromptTaskFrame {
            mode: mode_key,
            output_contract: PromptOutputContract::PlainText,
            user_text: None,
            source_text: None,
            status_text: Some(status_text),
            event_type: None,
            payload_json: None,
            context: BeatPromptContext {
                input_source: None,
                first_beat: None,
                recent_messages,
                semantic_memories: &[],
                context_level: None,
                policy,
                prefer_memory_facts: false,
                include_tool_guidance: false,
            },
        })?
        .expect_nano(mode_key)
    }

    pub fn compile_task_frame(
        &self,
        frame: &PromptTaskFrame<'_>,
    ) -> Result<CompiledPromptArtifact> {
        match frame.mode {
            PromptMode::FirstBeat
            | PromptMode::NanoReply
            | PromptMode::AmbientLine
            | PromptMode::ToolStatusExplanation
            | PromptMode::ErrorExplanation => self
                .build_nano_prompt_from_task_frame(frame)
                .map(CompiledPromptArtifact::Nano),
            PromptMode::CloudReasoning => self
                .build_cloud_request_from_task_frame(frame)
                .map(CompiledPromptArtifact::Cloud),
            other => Err(anyhow!(
                "task-frame compilation is not implemented for prompt mode {}",
                other.as_str()
            )),
        }
    }

    fn build_nano_prompt_from_task_frame(
        &self,
        frame: &PromptTaskFrame<'_>,
    ) -> Result<CompiledNanoPrompt> {
        if frame.output_contract.as_format() != PromptOutputFormat::PlainText {
            return Err(anyhow!(
                "nano prompt task frame for mode {} must use plain_text output",
                frame.mode.as_str()
            ));
        }

        let mode = self.config.mode_config(frame.mode);
        let shared = self.config.shared_config();
        let prompt = match frame.mode {
            PromptMode::FirstBeat => {
                let user_text = frame
                    .user_text
                    .ok_or_else(|| anyhow!("first beat task frame is missing user_text"))?;
                let input_source = frame
                    .context
                    .input_source
                    .ok_or_else(|| anyhow!("first beat task frame is missing input_source"))?;
                join_sections([
                    Some(shared.assistant_identity),
                    Some(shared.continuity_instruction),
                    Some(mode.instruction),
                    Some(render_policy_constraints(frame.context.policy, frame.mode)),
                    mode.max_words
                        .map(|value| format!("Length: under {value} words.")),
                    render_recent_context(
                        frame.context.recent_messages,
                        mode.max_context_messages,
                        140,
                    ),
                    Some(format!(
                        "User input ({}): {}",
                        input_source.as_str(),
                        compact_text(user_text, 240)
                    )),
                ])
            }
            PromptMode::NanoReply => {
                let user_text = frame
                    .user_text
                    .ok_or_else(|| anyhow!("nano reply task frame is missing user_text"))?;
                let context_level = frame
                    .context
                    .context_level
                    .ok_or_else(|| anyhow!("nano reply task frame is missing context_level"))?;
                join_sections([
                    Some(shared.assistant_identity),
                    Some(shared.continuity_instruction),
                    Some(mode.instruction),
                    Some(render_policy_constraints(frame.context.policy, frame.mode)),
                    mode.max_words
                        .map(|value| format!("Length: under {value} words.")),
                    frame.context.first_beat.and_then(|first_beat| {
                        (!first_beat.trim().is_empty()).then(|| {
                            format!(
                                "First beat already shown: \"{}\"",
                                compact_text(first_beat, 120)
                            )
                        })
                    }),
                    Some(format!(
                        "Previous Context setting: {}.",
                        context_level.as_str()
                    )),
                    render_recent_context(
                        frame.context.recent_messages,
                        mode.max_context_messages,
                        180,
                    ),
                    render_selected_memories(
                        &shared.memory_preamble,
                        frame.context.semantic_memories,
                        mode.max_memory_items,
                        140,
                        frame.context.prefer_memory_facts,
                    ),
                    Some(format!("User input: {}", compact_text(user_text, 240))),
                ])
            }
            PromptMode::AmbientLine => {
                let event_type = frame
                    .event_type
                    .ok_or_else(|| anyhow!("ambient task frame is missing event_type"))?;
                let payload_json = frame
                    .payload_json
                    .ok_or_else(|| anyhow!("ambient task frame is missing payload_json"))?;
                join_sections([
                    Some(shared.assistant_identity),
                    Some(shared.continuity_instruction),
                    Some(mode.instruction),
                    Some(render_policy_constraints(frame.context.policy, frame.mode)),
                    mode.max_words
                        .map(|value| format!("Length: under {value} words.")),
                    Some(format!("Event: {}", event_type)),
                    Some(format!("Payload: {}", compact_json(payload_json, 220))),
                    render_recent_context(
                        frame.context.recent_messages,
                        mode.max_context_messages,
                        140,
                    ),
                ])
            }
            PromptMode::ToolStatusExplanation | PromptMode::ErrorExplanation => {
                let status_text = frame.status_text.ok_or_else(|| {
                    anyhow!("status explanation task frame is missing status_text")
                })?;
                join_sections([
                    Some(shared.assistant_identity),
                    Some(shared.continuity_instruction),
                    Some(mode.instruction),
                    Some(render_policy_constraints(frame.context.policy, frame.mode)),
                    mode.max_words
                        .map(|value| format!("Length: under {value} words.")),
                    render_recent_context(
                        frame.context.recent_messages,
                        mode.max_context_messages,
                        120,
                    ),
                    Some(format!("Status: {}", compact_text(status_text, 180))),
                ])
            }
            other => {
                return Err(anyhow!(
                    "task-frame nano compiler does not support mode {}",
                    other.as_str()
                ));
            }
        };

        Ok(CompiledNanoPrompt {
            mode: frame.mode,
            prompt,
            max_output_tokens: mode.max_output_tokens,
        })
    }

    fn build_cloud_request_from_task_frame(
        &self,
        frame: &PromptTaskFrame<'_>,
    ) -> Result<ModelRequest> {
        if frame.output_contract.as_format() != PromptOutputFormat::JsonEnvelope {
            return Err(anyhow!(
                "cloud prompt task frame for mode {} must use json_envelope output",
                frame.mode.as_str()
            ));
        }

        let mode = self.config.mode_config(frame.mode);
        let shared = self.config.shared_config();
        match frame.mode {
            PromptMode::CloudReasoning => {
                let user_text = frame
                    .user_text
                    .ok_or_else(|| anyhow!("cloud reasoning task frame is missing user_text"))?;

                let mut cloud_messages = vec![ModelMessage {
                    role: "system".to_owned(),
                    content: join_sections([
                        Some(shared.assistant_identity),
                        Some(shared.continuity_instruction),
                        Some(mode.instruction),
                        Some(render_policy_constraints(frame.context.policy, frame.mode)),
                    ]),
                }];

                if let Some(memory_block) = render_selected_memories(
                    &shared.memory_preamble,
                    frame.context.semantic_memories,
                    mode.max_memory_items,
                    160,
                    false,
                ) {
                    cloud_messages.push(ModelMessage {
                        role: "system".to_owned(),
                        content: memory_block,
                    });
                }

                if frame.context.include_tool_guidance {
                    cloud_messages.push(ModelMessage {
                        role: "system".to_owned(),
                        content: shared.tool_instructions,
                    });
                }

                cloud_messages.push(ModelMessage {
                    role: "system".to_owned(),
                    content: shared.response_contract,
                });

                if let Some(first_beat) = frame
                    .context
                    .first_beat
                    .filter(|first_beat| !first_beat.trim().is_empty())
                {
                    cloud_messages.push(ModelMessage {
                        role: "system".to_owned(),
                        content: format!(
                            "The first visible beat already shown to the user is: \"{}\". Continue after it without repeating it, without opening with a new greeting or acknowledgment, and without recapping what was already said.",
                            compact_text(first_beat, 160)
                        ),
                    });
                }

                let selected_messages = select_recent_messages(
                    frame.context.recent_messages,
                    mode.max_context_messages,
                );
                for message in selected_messages {
                    cloud_messages.push(ModelMessage {
                        role: map_message_role(&message.role),
                        content: compact_text(&message.content, 320),
                    });
                }

                cloud_messages.push(ModelMessage {
                    role: "user".to_owned(),
                    content: compact_text(user_text, 320),
                });

                Ok(ModelRequest {
                    messages: cloud_messages,
                })
            }
            other => Err(anyhow!(
                "task-frame cloud compiler does not support mode {}",
                other.as_str()
            )),
        }
    }

    pub fn finalize_visible_response(
        &self,
        request: &FinalDeliveryRequest<'_>,
    ) -> FinalVisibleResponse {
        let first_beat = sanitize_visible_text(request.first_beat);
        let parsed_visible = sanitize_visible_text(&request.parsed_output.assistant_reply);
        let cloud_direct = request.cloud_direct_text.map(sanitize_visible_text);
        let tool_summary = request.tool_summary.map(sanitize_visible_text);

        let primary_text = match request.delivery_mode {
            "cloud_result_direct" | "cloud_result_with_nano_summary" => {
                cloud_direct.unwrap_or(parsed_visible)
            }
            _ => parsed_visible,
        };

        let mut assistant_text = stitch_visible_sections(&first_beat, &primary_text);
        if request.delivery_mode == "tool_result_with_nano_bridge" {
            assistant_text = stitch_visible_sections(
                &assistant_text,
                tool_summary.as_deref().unwrap_or_default(),
            );
        }

        if assistant_text.trim().is_empty() && !first_beat.is_empty() {
            assistant_text = first_beat;
        }

        FinalVisibleResponse {
            assistant_text,
            tool_card_payload: request.tool_card_payload.map(ToOwned::to_owned),
            memory_items_to_store: request.parsed_output.memory_candidates.clone(),
            was_bridged: matches!(
                request.delivery_mode,
                "tool_result_with_nano_bridge" | "cloud_result_with_nano_summary"
            ),
            was_direct: matches!(
                request.delivery_mode,
                "direct_nano" | "memory_backed_nano_recall" | "cloud_result_direct"
            ),
            was_suppressed: request.parsed_output.kind
                == ParsedTurnOutputKind::SuppressedInternalPayload,
        }
    }

    pub fn compose_visible_reply(
        &self,
        first_beat: &str,
        assistant_reply: &str,
        tool_summary: Option<&str>,
    ) -> String {
        let first_beat = sanitize_visible_text(first_beat);
        let assistant_reply = sanitize_visible_text(assistant_reply);
        let tool_summary = tool_summary.map(sanitize_visible_text);

        let mut assistant_text = stitch_visible_sections(&first_beat, &assistant_reply);
        if let Some(tool_summary) = tool_summary.as_deref() {
            if !tool_summary.is_empty() {
                assistant_text = stitch_visible_sections(&assistant_text, tool_summary);
            }
        }
        if assistant_text.trim().is_empty() {
            return first_beat;
        }
        assistant_text
    }
}

pub struct FirstBeatTranslationRequest<'a> {
    pub user_text: &'a str,
    pub input_source: InputSource,
    pub recent_messages: &'a [ChatMessage],
    pub policy: &'a BehaviorPolicy,
}

pub struct NanoReplyTranslationRequest<'a> {
    pub user_text: &'a str,
    pub first_beat: &'a str,
    pub recent_messages: &'a [ChatMessage],
    pub semantic_memories: &'a [RetrievedMemory],
    pub context_level: ContextLevel,
    pub policy: &'a BehaviorPolicy,
    pub prefer_memory_facts: bool,
}

pub struct CloudReasoningTranslationRequest<'a> {
    pub user_text: &'a str,
    pub first_beat: &'a str,
    pub recent_messages: &'a [ChatMessage],
    pub semantic_memories: &'a [RetrievedMemory],
    pub policy: &'a BehaviorPolicy,
    pub include_tool_guidance: bool,
}

pub struct FollowupTranslationRequest<'a> {
    pub user_text: &'a str,
    pub first_beat: &'a str,
    pub recent_messages: &'a [ChatMessage],
    pub semantic_memories: &'a [RetrievedMemory],
    pub context_level: ContextLevel,
    pub policy: &'a BehaviorPolicy,
    pub include_tool_guidance: bool,
}

pub struct MemoryRecallTranslationRequest<'a> {
    pub user_text: &'a str,
    pub recent_messages: &'a [ChatMessage],
    pub semantic_memories: &'a [RetrievedMemory],
    pub policy: &'a BehaviorPolicy,
}

pub struct SummaryBridgeTranslationRequest<'a> {
    pub first_beat: &'a str,
    pub source_text: &'a str,
    pub recent_messages: &'a [ChatMessage],
    pub semantic_memories: &'a [RetrievedMemory],
    pub policy: &'a BehaviorPolicy,
}

pub struct AmbientLineTranslationRequest<'a> {
    pub event_type: &'a str,
    pub payload_json: &'a Value,
    pub recent_messages: &'a [ChatMessage],
    pub policy: &'a BehaviorPolicy,
}

pub struct StatusExplanationTranslationRequest<'a> {
    pub status_text: &'a str,
    pub recent_messages: &'a [ChatMessage],
    pub policy: &'a BehaviorPolicy,
}

pub struct FinalDeliveryRequest<'a> {
    pub delivery_mode: &'a str,
    pub first_beat: &'a str,
    pub parsed_output: &'a ParsedTurnOutput,
    pub tool_summary: Option<&'a str>,
    pub tool_card_payload: Option<&'a str>,
    pub cloud_direct_text: Option<&'a str>,
}

pub fn parse_internal_model_output(raw_output: &str) -> Result<ParsedTurnOutput> {
    let trimmed = raw_output.trim();
    for candidate in extract_json_candidates(trimmed) {
        if let Some(envelope) = try_parse_turn_envelope(candidate.as_str()) {
            return Ok(ParsedTurnOutput::structured(envelope, raw_output));
        }
        if let Some(object_candidate) = extract_json_span(candidate.as_str(), '{', '}') {
            if let Some(envelope) = try_parse_turn_envelope(object_candidate.as_str()) {
                return Ok(ParsedTurnOutput::structured(envelope, raw_output));
            }
        }
    }

    if looks_like_internal_payload(trimmed) {
        return Ok(ParsedTurnOutput::suppressed(raw_output));
    }

    Ok(ParsedTurnOutput::plain_text(raw_output))
}

pub fn parse_turn_envelope(raw_output: &str) -> Result<TurnEnvelope> {
    let parsed = parse_internal_model_output(raw_output)?;
    if parsed.kind == ParsedTurnOutputKind::SuppressedInternalPayload {
        return Err(anyhow!(
            "model output matched an internal payload shape and was suppressed"
        ));
    }
    Ok(parsed.to_turn_envelope())
}

pub fn stitch_assistant_reply(first_beat: &str, envelope: &TurnEnvelope) -> String {
    stitch_visible_sections(first_beat.trim(), envelope.assistant_reply.trim())
}

fn try_parse_turn_envelope(candidate: &str) -> Option<TurnEnvelope> {
    if let Ok(envelope) = serde_json::from_str::<TurnEnvelope>(candidate) {
        return Some(normalize_turn_envelope(envelope));
    }

    let mut value = serde_json::from_str::<Value>(candidate).ok()?;
    normalize_turn_envelope_value(&mut value);
    serde_json::from_value(value)
        .ok()
        .map(normalize_turn_envelope)
}

fn normalize_turn_envelope_value(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };

    if !object.contains_key("assistant_reply") {
        object.insert("assistant_reply".to_owned(), Value::String(String::new()));
    }
    if !object.contains_key("memory_candidates") {
        object.insert("memory_candidates".to_owned(), Value::Array(Vec::new()));
    }

    if let Some(tool_request) = object.get_mut("tool_request") {
        normalize_tool_request_value(tool_request);
    }
}

fn normalize_turn_envelope(mut envelope: TurnEnvelope) -> TurnEnvelope {
    envelope.assistant_reply = envelope.assistant_reply.trim().to_owned();
    envelope.memory_candidates = filter_memory_candidates(envelope.memory_candidates);
    envelope
}

fn normalize_tool_request_value(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };

    if !object.contains_key("action") {
        if let Some(name) = object.get("name").cloned() {
            object.insert("action".to_owned(), name);
        }
    }

    if !object.contains_key("tool") {
        if let Some(action) = object.get("action").and_then(Value::as_str) {
            if let Some(tool_name) = infer_tool_name(action) {
                object.insert("tool".to_owned(), Value::String(tool_name.to_owned()));
            }
        }
    }

    if !object.contains_key("arguments") {
        object.insert(
            "arguments".to_owned(),
            Value::Object(serde_json::Map::new()),
        );
    }
}

fn infer_tool_name(action: &str) -> Option<&'static str> {
    match action {
        "auth_status"
        | "get_connection_state"
        | "get_linked_account"
        | "start_auth"
        | "complete_auth"
        | "handle_callback"
        | "exchange_code"
        | "refresh_token"
        | "refresh_token_if_needed"
        | "disconnect"
        | "clear_tokens"
        | "unlink_account"
        | "capability_status"
        | "validate_connection"
        | "validate_scopes"
        | "get_devices"
        | "get_active_device"
        | "transfer_playback"
        | "ensure_playback_target"
        | "play"
        | "resume_playback"
        | "pause"
        | "next_track"
        | "previous_track"
        | "current_playback"
        | "currently_playing"
        | "playback_state"
        | "set_volume"
        | "search_track"
        | "search_album"
        | "search_artist"
        | "search_playlist"
        | "resolve_track_uri_from_query"
        | "search" => Some("spotify"),
        "set_power" | "set_brightness" | "set_color" | "activate_scene" => Some("hue"),
        _ => None,
    }
}

fn extract_json_candidates(raw_output: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let trimmed = raw_output.trim();
    if trimmed.is_empty() {
        return candidates;
    }

    candidates.push(trimmed.to_owned());

    for block in extract_fenced_blocks(trimmed) {
        if !block.trim().is_empty() {
            candidates.push(block);
        }
    }

    if let Some(candidate) = extract_json_span(trimmed, '{', '}') {
        candidates.push(candidate);
    }
    if let Some(candidate) = extract_json_span(trimmed, '[', ']') {
        candidates.push(candidate);
    }

    dedupe_strings(candidates)
}

fn extract_fenced_blocks(raw_output: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut remaining = raw_output;
    while let Some(start) = remaining.find("```") {
        let after_start = &remaining[start + 3..];
        let Some(end) = after_start.find("```") else {
            break;
        };
        let block = after_start[..end]
            .trim_start_matches(|character: char| character.is_whitespace())
            .trim_start_matches("json")
            .trim_start_matches(|character: char| character.is_whitespace())
            .trim()
            .to_owned();
        blocks.push(block);
        remaining = &after_start[end + 3..];
    }
    blocks
}

fn extract_json_span(raw_output: &str, open: char, close: char) -> Option<String> {
    let start = raw_output.find(open)?;
    let end = raw_output.rfind(close)?;
    (start < end).then(|| raw_output[start..=end].trim().to_owned())
}

fn filter_memory_candidates(candidates: Vec<MemoryCandidate>) -> Vec<MemoryCandidate> {
    let mut seen = HashSet::new();
    let mut filtered = Vec::new();

    for mut candidate in candidates {
        candidate.kind = compact_text(&candidate.kind, 24);
        candidate.salience = candidate.salience.clamp(0.0, 1.0);
        candidate.text = sanitize_memory_text(&candidate.text, 180);
        if candidate.text.is_empty() {
            continue;
        }

        let normalized = normalize_text_key(&candidate.text);
        if normalized.len() < 8 || !seen.insert(normalized) {
            continue;
        }

        filtered.push(candidate);
    }

    filtered
}

fn sanitize_memory_text(value: &str, limit: usize) -> String {
    let compacted = compact_text(&strip_markdown_artifacts(value), limit);
    if compacted.is_empty()
        || looks_like_internal_payload(&compacted)
        || compacted.to_lowercase().contains("prompt-debug")
        || compacted.to_lowercase().contains("\"tool\":")
    {
        String::new()
    } else {
        compacted
    }
}

fn looks_like_internal_payload(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }

    let lower = trimmed.to_lowercase();
    if lower.contains("\"assistant_reply\"")
        || lower.contains("\"tool_request\"")
        || lower.contains("\"memory_candidates\"")
        || lower.contains("assistant_reply:")
        || lower.contains("tool_request:")
        || lower.contains("memory_candidates:")
        || lower.contains("prompt-debug")
    {
        return true;
    }

    if trimmed.starts_with("```json") {
        return true;
    }

    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        return serde_json::from_str::<Value>(trimmed).is_ok();
    }

    false
}

fn sanitize_visible_text(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() || looks_like_internal_payload(trimmed) {
        return String::new();
    }
    trimmed.trim_matches('"').trim().to_owned()
}

fn stitch_visible_sections(first: &str, continuation: &str) -> String {
    let continuation = trim_duplicate_prefix(first, continuation.trim());
    if continuation.is_empty() {
        first.trim().to_owned()
    } else if first.trim().is_empty() {
        continuation
    } else {
        format!("{}\n\n{}", first.trim(), continuation)
    }
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for value in values {
        let normalized = normalize_text_key(&value);
        if seen.insert(normalized) {
            deduped.push(value);
        }
    }
    deduped
}

fn render_policy_constraints(policy: &BehaviorPolicy, mode: PromptMode) -> String {
    let mut constraints = Vec::new();
    if policy.allow_playfulness {
        constraints.push(if policy.allow_humor {
            "Tone: warm, calm, lightly human."
        } else {
            "Tone: warm, calm, not jokey."
        });
    } else {
        constraints.push("Tone: calm, plain, and restrained.");
    }
    if !policy.allow_humor {
        constraints.push("Do not use jokes or playful asides.");
    }
    if !policy.allow_initiative {
        constraints.push("Do not invent new agenda beyond the current turn.");
    }
    if policy.prefer_silence && matches!(mode, PromptMode::AmbientLine) {
        constraints.push("If the event does not clearly help, return an empty response.");
    }
    constraints.join(" ")
}

fn render_recent_context(
    recent_messages: &[ChatMessage],
    limit: usize,
    max_chars_per_message: usize,
) -> Option<String> {
    let selected = select_recent_messages(recent_messages, limit);
    if selected.is_empty() {
        return None;
    }
    let lines = selected
        .iter()
        .map(|message| {
            format!(
                "- {}: {}",
                short_role_label(&message.role),
                compact_text(&message.content, max_chars_per_message)
            )
        })
        .collect::<Vec<_>>();
    Some(format!("Recent context:\n{}", lines.join("\n")))
}

fn render_selected_memories_for_mode(
    preamble: &str,
    semantic_memories: &[RetrievedMemory],
    mode: PromptMode,
) -> Option<String> {
    let (limit, max_chars_per_memory, prefer_facts) = match mode {
        PromptMode::FirstBeat => (0, 0, false),
        PromptMode::MemoryBackedRecall => (3, 120, true),
        PromptMode::SummaryBridge => (3, 120, false),
        PromptMode::FollowupReasoning => (4, 160, false),
        _ => (0, 0, false),
    };

    if limit == 0 {
        return None;
    }

    render_selected_memories(
        preamble,
        semantic_memories,
        limit,
        max_chars_per_memory,
        prefer_facts,
    )
}

fn render_selected_memories(
    preamble: &str,
    semantic_memories: &[RetrievedMemory],
    limit: usize,
    max_chars_per_memory: usize,
    prefer_facts: bool,
) -> Option<String> {
    if limit == 0 {
        return None;
    }

    let selected = select_prompt_memories(semantic_memories, limit, prefer_facts);
    if selected.is_empty() {
        return None;
    }
    let lines = selected
        .iter()
        .map(|memory| {
            format!(
                "- [{}] {}",
                memory.kind,
                compact_text(
                    &strip_markdown_artifacts(&memory.prompt_fact_text(max_chars_per_memory)),
                    max_chars_per_memory,
                )
            )
        })
        .collect::<Vec<_>>();
    Some(format!(
        "{preamble}\nSelected memory:\n{}",
        lines.join("\n")
    ))
}

fn select_recent_messages(messages: &[ChatMessage], limit: usize) -> &[ChatMessage] {
    if limit == 0 || messages.is_empty() {
        return &[];
    }
    let start = messages.len().saturating_sub(limit);
    &messages[start..]
}

fn select_prompt_memories<'a>(
    semantic_memories: &'a [RetrievedMemory],
    limit: usize,
    prefer_facts: bool,
) -> Vec<&'a RetrievedMemory> {
    let mut selected = semantic_memories
        .iter()
        .filter(|memory| !looks_like_internal_payload(memory.text.as_str()))
        .collect::<Vec<_>>();
    selected.sort_by(|left, right| {
        right
            .relevance_score
            .partial_cmp(&left.relevance_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                if prefer_facts {
                    rank_memory_kind(&left.kind).cmp(&rank_memory_kind(&right.kind))
                } else {
                    std::cmp::Ordering::Equal
                }
            })
    });
    selected.truncate(limit);
    selected
}

fn compact_json(value: &Value, limit: usize) -> String {
    compact_text(
        &serde_json::to_string(value).unwrap_or_else(|_| "{}".to_owned()),
        limit,
    )
}

fn compact_text(value: &str, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }

    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= limit {
        normalized
    } else {
        let truncated = normalized
            .chars()
            .take(limit.saturating_sub(1))
            .collect::<String>();
        format!("{truncated}…")
    }
}

fn strip_markdown_artifacts(value: &str) -> String {
    value
        .replace("```json", " ")
        .replace("```", " ")
        .replace('`', " ")
        .replace('\n', " ")
        .replace('\r', " ")
        .replace('\t', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_text_key(value: &str) -> String {
    strip_markdown_artifacts(value)
        .to_lowercase()
        .chars()
        .filter(|character| character.is_alphanumeric() || character.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn rank_memory_kind(kind: &str) -> u8 {
    match kind.to_lowercase().as_str() {
        "fact" => 0,
        "profile" => 1,
        "preference" => 2,
        "summary" => 3,
        _ => 4,
    }
}

fn join_sections<I>(sections: I) -> String
where
    I: IntoIterator<Item = Option<String>>,
{
    sections
        .into_iter()
        .flatten()
        .map(|section| section.trim().to_owned())
        .filter(|section| !section.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn short_role_label(role: &str) -> &'static str {
    match role {
        "assistant" => "assistant",
        "tool" => "tool",
        "system" => "system",
        _ => "user",
    }
}

fn map_message_role(role: &str) -> String {
    match role {
        "assistant" => "assistant".to_owned(),
        "tool" => "system".to_owned(),
        "system" => "system".to_owned(),
        _ => "user".to_owned(),
    }
}

fn trim_duplicate_prefix(first_beat: &str, continuation: &str) -> String {
    let first = first_beat.trim();
    let continued = continuation.trim();
    if first.is_empty() {
        return continued.to_owned();
    }
    if let Some(stripped) = continued.strip_prefix(first) {
        return trim_leading_separator_span(stripped);
    }

    if let Some(stripped) = strip_matching_opening_sentence(first, continued) {
        return stripped;
    }

    if let Some(stripped) = strip_known_filler_opener(continued) {
        return stripped;
    }

    continued.to_owned()
}

fn strip_matching_opening_sentence(first_beat: &str, continuation: &str) -> Option<String> {
    let first_sentence = leading_span(first_beat, 96, &['.', '!', '?', '\n']);
    let continuation_sentence = leading_span(continuation, 96, &['.', '!', '?', '\n']);
    if first_sentence.is_empty() || continuation_sentence.is_empty() {
        return None;
    }

    let normalized_first = normalize_text_key(&first_sentence);
    let normalized_continuation = normalize_text_key(&continuation_sentence);
    if normalized_first.len() < 12 || normalized_first != normalized_continuation {
        return None;
    }

    continuation
        .strip_prefix(continuation_sentence.as_str())
        .map(trim_leading_separator_span)
}

fn strip_known_filler_opener(continuation: &str) -> Option<String> {
    let opener = leading_span(continuation, 40, &[',', ':', ';', '.', '!', '?', '—', '-']);
    let normalized = normalize_text_key(&opener);
    if !matches!(
        normalized.as_str(),
        "sure"
            | "okay"
            | "ok"
            | "alright"
            | "all right"
            | "got it"
            | "absolutely"
            | "let me explain"
            | "let me break it down"
    ) {
        return None;
    }

    continuation
        .strip_prefix(opener.as_str())
        .map(trim_leading_separator_span)
}

fn leading_span(value: &str, max_chars: usize, terminators: &[char]) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut end = trimmed.len();
    for (index, character) in trimmed.char_indices() {
        if terminators.contains(&character) {
            end = index;
            break;
        }
        if index >= max_chars {
            end = index;
            break;
        }
    }

    trimmed[..end].trim().to_owned()
}

fn trim_leading_separator_span(value: &str) -> String {
    value
        .trim_start_matches(|character: char| {
            character.is_whitespace() || matches!(character, '-' | '—' | ':' | ';' | ',' | '.')
        })
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn prompt_config_preserves_legacy_strings() {
        let config = PromptConfig::from_legacy_strings("system", "memory", "tools", "json");
        let shared = config.shared_config();

        assert_eq!(shared.assistant_identity, "system");
        assert_eq!(shared.memory_preamble, "memory");
        assert_eq!(shared.tool_instructions, "tools");
        assert_eq!(shared.response_contract, "json");
    }

    #[test]
    fn first_beat_prompt_trims_context_budget() {
        let translator = PromptTranslator::new(PromptConfig::default());
        let messages = (0..6)
            .map(|index| ChatMessage {
                id: index as i64,
                role: "user".to_owned(),
                content: format!("message {index}"),
                turn_id: format!("turn-{index}"),
                input_source: InputSource::Text,
                created_at: "2026-03-13T00:00:00Z".to_owned(),
                content_type: crate::MessageContentType::PlainText,
                display_json: None,
                visible_summary: None,
            })
            .collect::<Vec<_>>();
        let prompt = translator
            .compile_first_beat(&FirstBeatTranslationRequest {
                user_text: "hello",
                input_source: InputSource::Text,
                recent_messages: &messages,
                policy: &BehaviorPolicy::direct_turn(),
            })
            .expect("first beat prompt should compile");

        assert!(prompt.prompt.contains("message 5"));
        assert!(!prompt.prompt.contains("message 0"));
    }

    #[test]
    fn task_frame_compiles_first_beat_to_nano_prompt() {
        let translator = PromptTranslator::new(PromptConfig::default());
        let artifact = translator
            .compile_task_frame(&PromptTaskFrame {
                mode: PromptMode::FirstBeat,
                output_contract: PromptOutputContract::PlainText,
                user_text: Some("hello"),
                source_text: None,
                status_text: None,
                event_type: None,
                payload_json: None,
                context: BeatPromptContext {
                    input_source: Some(InputSource::Text),
                    first_beat: None,
                    recent_messages: &[],
                    semantic_memories: &[],
                    context_level: None,
                    policy: &BehaviorPolicy::direct_turn(),
                    prefer_memory_facts: false,
                    include_tool_guidance: false,
                },
            })
            .expect("task frame should compile");

        match artifact {
            CompiledPromptArtifact::Nano(prompt) => {
                assert_eq!(prompt.mode, PromptMode::FirstBeat);
                assert!(prompt.prompt.contains("hello"));
            }
            CompiledPromptArtifact::Cloud(_) => panic!("first beat task frame compiled to cloud"),
        }
    }

    #[test]
    fn parses_legacy_tool_request_shape() {
        let envelope = parse_turn_envelope(
            r#"{"assistant_reply":"Starting Spotify sign-in.","tool_request":{"name":"start_auth","arguments":{}},"memory_candidates":[]}"#,
        )
        .expect("envelope should parse");

        let tool_request = envelope.tool_request.expect("missing tool request");
        assert_eq!(tool_request.tool.as_str(), "spotify");
        assert_eq!(tool_request.action, "start_auth");
    }

    #[test]
    fn ambient_prompt_compacts_payload() {
        let translator = PromptTranslator::new(PromptConfig::default());
        let prompt = translator
            .compile_ambient_line(&AmbientLineTranslationRequest {
                event_type: "capability_available",
                payload_json: &json!({
                    "capability": "spotify",
                    "details": "x".repeat(400)
                }),
                recent_messages: &[],
                policy: &BehaviorPolicy::ambient_turn(),
            })
            .expect("ambient prompt should compile");

        assert!(prompt.prompt.contains("capability_available"));
        assert!(prompt.prompt.contains("spotify"));
    }

    #[test]
    fn cloud_reasoning_task_frame_omits_first_beat_instruction_when_empty() {
        let translator = PromptTranslator::new(PromptConfig::default());
        let artifact = translator
            .compile_task_frame(&PromptTaskFrame {
                mode: PromptMode::CloudReasoning,
                output_contract: PromptOutputContract::JsonEnvelope,
                user_text: Some("Explain this"),
                source_text: None,
                status_text: None,
                event_type: None,
                payload_json: None,
                context: BeatPromptContext {
                    input_source: None,
                    first_beat: Some(""),
                    recent_messages: &[],
                    semantic_memories: &[],
                    context_level: None,
                    policy: &BehaviorPolicy::direct_turn(),
                    prefer_memory_facts: false,
                    include_tool_guidance: false,
                },
            })
            .expect("cloud task frame should compile");

        match artifact {
            CompiledPromptArtifact::Cloud(request) => {
                let rendered = request
                    .messages
                    .iter()
                    .map(|message| message.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                assert!(!rendered.contains("The first visible beat already shown"));
                assert!(rendered.contains("Explain this"));
            }
            CompiledPromptArtifact::Nano(_) => panic!("cloud task frame compiled to nano"),
        }
    }

    #[test]
    fn parses_fenced_json_envelope_before_plain_fallback() {
        let parsed = parse_internal_model_output(
            r#"```json
{"assistant_reply":"Hello again.","tool_request":null,"memory_candidates":[]}
```"#,
        )
        .expect("fenced envelope should parse");

        assert_eq!(parsed.kind, ParsedTurnOutputKind::Structured);
        assert_eq!(parsed.assistant_reply, "Hello again.");
    }

    #[test]
    fn suppresses_internal_payload_when_json_wrapper_is_broken() {
        let parsed = parse_internal_model_output(
            r#"```json
{"assistant_reply":"Hello","tool_request":
```"#,
        )
        .expect("suppressed parse should still return a parsed output");

        assert_eq!(parsed.kind, ParsedTurnOutputKind::SuppressedInternalPayload);
        assert!(parsed.assistant_reply.is_empty());
    }

    #[test]
    fn memory_recall_prompt_uses_compact_memory_facts() {
        let translator = PromptTranslator::new(PromptConfig::default());
        let prompt = translator
            .compile_memory_backed_recall(&MemoryRecallTranslationRequest {
                user_text: "Who's Mocha?",
                recent_messages: &[],
                semantic_memories: &[
                    RetrievedMemory {
                        id: 1,
                        kind: "fact".to_owned(),
                        text: "Mocha is your cat arriving on Tuesday evening.".to_owned(),
                        salience: 0.9,
                        similarity: 0.8,
                        source_message_id: Some(10),
                        created_at: "2026-03-13T10:00:00Z".to_owned(),
                        relevance_score: 0.83,
                        canonical_key: Some("profile:cat".to_owned()),
                    },
                    RetrievedMemory {
                        id: 2,
                        kind: "summary".to_owned(),
                        text: "```json {\"assistant_reply\":\"raw\"} ```".to_owned(),
                        salience: 0.8,
                        similarity: 0.79,
                        source_message_id: Some(11),
                        created_at: "2026-03-13T10:01:00Z".to_owned(),
                        relevance_score: 0.8,
                        canonical_key: None,
                    },
                ],
                policy: &BehaviorPolicy::direct_turn(),
            })
            .expect("memory recall prompt should compile");

        assert!(prompt.nano_prompt.prompt.contains("Mocha is your cat"));
        assert!(!prompt.nano_prompt.prompt.contains("\"assistant_reply\""));
        assert!(prompt
            .nano_prompt
            .prompt
            .contains("User question: Who's Mocha?"));
    }

    #[test]
    fn trim_duplicate_prefix_removes_normalized_duplicate_opening_sentence() {
        let continuation = trim_duplicate_prefix(
            "Sure, let's fix Spotify.",
            "Sure let's fix Spotify. Start by reconnecting your account.",
        );

        assert_eq!(continuation, "Start by reconnecting your account.");
    }

    #[test]
    fn trim_duplicate_prefix_strips_known_filler_opener_only_at_start() {
        let continuation = trim_duplicate_prefix(
            "Checking that now.",
            "Okay, the speaker was offline for a moment.",
        );

        assert_eq!(continuation, "the speaker was offline for a moment.");
    }

    #[test]
    fn trim_duplicate_prefix_keeps_substantive_continuation_content() {
        let continuation = trim_duplicate_prefix(
            "I found the issue.",
            "The device was offline, so I switched playback to Denon.",
        );

        assert_eq!(
            continuation,
            "The device was offline, so I switched playback to Denon."
        );
    }
}
