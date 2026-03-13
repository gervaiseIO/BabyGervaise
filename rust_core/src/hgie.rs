use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::logging::{ModelLogEntry, RetrievalLogEntry, ToolLogEntry};
use crate::memory::{MemoryStore, RetrievedMemory};
use crate::model::{ModelGateway, ModelMessage, ModelRequest};
use crate::tools::{ToolExecutionResult, ToolExecutor, ToolName, ToolRequest};
use crate::{
    now_rfc3339, AppConfig, ChatMessage, ContextLevel, CoreCallbacks, InputSource, ModelConfig,
    PromptConfig,
};

const SPOTIFY_AUTH_PROMPT: &str =
    "You need to sign in to Spotify first. Do you want to do that now?";
const SPOTIFY_AUTH_DECLINED_REPLY: &str = "Okay. We can connect Spotify whenever you want.";

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

pub struct HgieEngine {
    memory: MemoryStore,
    tools: ToolExecutor,
    model: Arc<dyn ModelGateway>,
    model_config: ModelConfig,
    prompt_config: PromptConfig,
    app_config: AppConfig,
}

impl HgieEngine {
    pub fn new(
        memory: MemoryStore,
        tools: ToolExecutor,
        model: Arc<dyn ModelGateway>,
        model_config: ModelConfig,
        prompt_config: PromptConfig,
        app_config: AppConfig,
    ) -> Self {
        Self {
            memory,
            tools,
            model,
            model_config,
            prompt_config,
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
        let user_message = self
            .memory
            .append_message("user", text, turn_id, input_source, None)
            .context("failed to persist user message")?;

        let context_level = self
            .memory
            .get_previous_context(self.app_config.default_previous_context)?;
        let recent_messages = self
            .memory
            .load_recent_messages(context_level, Some(user_message.id))
            .context("failed to load recent messages")?;
        let semantic_memories = self
            .memory
            .semantic_search(text, context_level)
            .context("failed to retrieve semantic memories")?;

        self.memory.log_retrieval(&RetrievalLogEntry {
            created_at: now_rfc3339(),
            level: context_level,
            recent_count: recent_messages.len(),
            semantic_count: semantic_memories.len(),
            query_text: text.to_owned(),
        })?;

        if let Some(assistant_message) = self.maybe_handle_spotify_auth_turn(
            turn_id,
            text,
            input_source,
            user_message.id,
            &recent_messages,
            callbacks,
        )? {
            return Ok(assistant_message);
        }

        let request = ModelRequest {
            messages: self.build_prompt(
                &recent_messages,
                &semantic_memories,
                context_level,
                text,
            )?,
        };

        let model_response = self.model.send_turn(&request);

        let model_log = match &model_response {
            Ok(response) => ModelLogEntry {
                created_at: now_rfc3339(),
                model_name: self.model.model_name().to_owned(),
                prompt: response.prompt_json.clone(),
                raw_output: response.raw_output.clone(),
                input_tokens: response.input_tokens,
                output_tokens: response.output_tokens,
                latency_ms: response.latency_ms,
                http_status: response.http_status,
                error_text: None,
            },
            Err(error) => ModelLogEntry {
                created_at: now_rfc3339(),
                model_name: self.model.model_name().to_owned(),
                prompt: serde_json::to_string_pretty(&request.messages)
                    .unwrap_or_else(|_| "[]".to_owned()),
                raw_output: String::new(),
                input_tokens: None,
                output_tokens: None,
                latency_ms: 0,
                http_status: None,
                error_text: Some(error.to_string()),
            },
        };
        self.memory.log_model_call(&model_log)?;

        let model_response = model_response?;
        let envelope = parse_turn_envelope(&model_response.raw_output)?;
        let assistant_message = self.finish_assistant_turn(
            turn_id,
            input_source,
            envelope.assistant_reply.trim().to_owned(),
            envelope.tool_request.as_ref(),
            Some(user_message.id),
            callbacks,
        )?;

        for candidate in envelope.memory_candidates {
            if candidate.salience >= self.app_config.memory_salience_threshold {
                self.memory.store_memory_item(
                    &candidate.kind,
                    &candidate.text,
                    candidate.salience,
                    Some(assistant_message.id),
                )?;
            }
        }

        Ok(assistant_message)
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

        self.finish_assistant_turn(
            turn_id,
            InputSource::Text,
            String::new(),
            Some(&request),
            None,
            callbacks,
        )
    }

    fn maybe_handle_spotify_auth_turn(
        &self,
        turn_id: &str,
        text: &str,
        input_source: InputSource,
        user_message_id: i64,
        recent_messages: &[ChatMessage],
        callbacks: &dyn CoreCallbacks,
    ) -> Result<Option<ChatMessage>> {
        let lower = text.trim().to_ascii_lowercase();
        let awaiting_confirmation = recent_messages
            .iter()
            .rev()
            .find(|message| message.role == "assistant")
            .map(|message| message.content.trim() == SPOTIFY_AUTH_PROMPT)
            .unwrap_or(false);

        if awaiting_confirmation {
            if is_affirmative(&lower) {
                let auth_status = self
                    .tools
                    .execute_named("spotify", "auth_status", json!({}))?;
                if tool_result_status(&auth_status.result_json) == Some("success") {
                    return self
                        .finish_assistant_turn(
                            turn_id,
                            input_source,
                            "You're connected to Spotify now. What would you like to listen to?"
                                .to_owned(),
                            None,
                            Some(user_message_id),
                            callbacks,
                        )
                        .map(Some);
                }

                let request = ToolRequest {
                    tool: ToolName::Spotify,
                    action: "start_auth".to_owned(),
                    arguments: json!({}),
                };
                return self
                    .finish_assistant_turn(
                        turn_id,
                        input_source,
                        String::new(),
                        Some(&request),
                        Some(user_message_id),
                        callbacks,
                    )
                    .map(Some);
            }

            if is_negative(&lower) {
                return self
                    .finish_assistant_turn(
                        turn_id,
                        input_source,
                        SPOTIFY_AUTH_DECLINED_REPLY.to_owned(),
                        None,
                        Some(user_message_id),
                        callbacks,
                    )
                    .map(Some);
            }
        }

        if !is_spotify_related_input(&lower) {
            return Ok(None);
        }

        let auth_status = self
            .tools
            .execute_named("spotify", "auth_status", json!({}))?;
        match tool_result_status(&auth_status.result_json) {
            Some("requires_auth") => self
                .finish_assistant_turn(
                    turn_id,
                    input_source,
                    SPOTIFY_AUTH_PROMPT.to_owned(),
                    None,
                    Some(user_message_id),
                    callbacks,
                )
                .map(Some),
            Some("error") => self
                .finish_assistant_turn(
                    turn_id,
                    input_source,
                    auth_status.summary,
                    None,
                    Some(user_message_id),
                    callbacks,
                )
                .map(Some),
            _ => Ok(None),
        }
    }

    fn finish_assistant_turn(
        &self,
        turn_id: &str,
        input_source: InputSource,
        mut assistant_reply: String,
        tool_request: Option<&ToolRequest>,
        source_message_id: Option<i64>,
        callbacks: &dyn CoreCallbacks,
    ) -> Result<ChatMessage> {
        let mut tool_message: Option<String> = None;

        if let Some(tool_request) = tool_request {
            let tool_result =
                self.execute_tool_and_log(turn_id, tool_request, source_message_id, callbacks)?;
            self.emit_external_effects(turn_id, &tool_result, callbacks);
            let tool_summary = tool_result.summary.clone();
            tool_message = Some(serde_json::to_string_pretty(&tool_result.result_json)?);

            if assistant_reply.trim().is_empty() {
                assistant_reply = tool_summary;
            } else {
                assistant_reply = format!("{assistant_reply}\n\n{tool_summary}");
            }
        }

        callbacks.emit(
            "assistant_started",
            json!({
                "turnId": turn_id
            })
            .to_string(),
        );

        for chunk in chunk_text(&assistant_reply, self.app_config.stream_chunk_size) {
            callbacks.emit(
                "assistant_chunk",
                json!({
                    "turnId": turn_id,
                    "chunk": chunk
                })
                .to_string(),
            );
        }

        if let Some(tool_payload) = &tool_message {
            self.memory
                .append_message("tool", tool_payload, turn_id, input_source, None)?;
        }

        let assistant_message = self
            .memory
            .append_message("assistant", &assistant_reply, turn_id, input_source, None)
            .context("failed to persist assistant message")?;

        callbacks.emit(
            "assistant_completed",
            json!({
                "turnId": turn_id,
                "message": assistant_message
            })
            .to_string(),
        );

        Ok(assistant_message)
    }

    fn execute_tool_and_log(
        &self,
        turn_id: &str,
        tool_request: &ToolRequest,
        source_message_id: Option<i64>,
        callbacks: &dyn CoreCallbacks,
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
                self.memory.log_tool_call(&ToolLogEntry {
                    created_at: now_rfc3339(),
                    tool_name: tool_name.clone(),
                    action: tool_request.action.clone(),
                    arguments_json: serde_json::to_string(&tool_request.arguments)?,
                    result_json: serde_json::to_string(&failure_payload)?,
                    success: false,
                    latency_ms: elapsed_millis(tool_started_at),
                })?;
                return Err(error);
            }
        };

        self.memory.log_tool_call(&ToolLogEntry {
            created_at: now_rfc3339(),
            tool_name: tool_name.clone(),
            action: tool_request.action.clone(),
            arguments_json: serde_json::to_string(&tool_request.arguments)?,
            result_json: serde_json::to_string(&tool_result.result_json)?,
            success: tool_result.is_success(),
            latency_ms: elapsed_millis(tool_started_at),
        })?;

        if let Some(source_message_id) = source_message_id {
            self.memory.store_memory_item(
                "summary",
                &format!(
                    "Tool {}.{} -> {}",
                    tool_name,
                    tool_request.action,
                    tool_result.summary.as_str()
                ),
                0.75,
                Some(source_message_id),
            )?;
        }

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

    fn build_prompt(
        &self,
        recent_messages: &[ChatMessage],
        semantic_memories: &[RetrievedMemory],
        context_level: ContextLevel,
        current_user_message: &str,
    ) -> Result<Vec<ModelMessage>> {
        let mut messages = vec![ModelMessage {
            role: "system".to_owned(),
            content: format!(
                "{}\n\n{}\n\n{}\n\n{}\nModel: {}\nPrevious Context: {}",
                self.prompt_config.system_prompt,
                self.prompt_config.memory_preamble,
                self.prompt_config.tool_instructions,
                self.prompt_config.response_contract,
                self.model_config.model,
                context_level.as_str()
            ),
        }];

        if !semantic_memories.is_empty() {
            let joined_memories = semantic_memories
                .iter()
                .map(|memory| format!("- [{}] {}", memory.kind, memory.text))
                .collect::<Vec<_>>()
                .join("\n");
            messages.push(ModelMessage {
                role: "system".to_owned(),
                content: format!("Semantic memories:\n{joined_memories}"),
            });
        }

        for message in recent_messages {
            messages.push(ModelMessage {
                role: map_message_role(&message.role),
                content: message.content.clone(),
            });
        }

        messages.push(ModelMessage {
            role: "user".to_owned(),
            content: current_user_message.to_owned(),
        });

        Ok(messages)
    }

    fn execute_tool(&self, request: &ToolRequest) -> Result<ToolExecutionResult> {
        self.tools
            .execute(request)
            .map_err(|error| anyhow!("tool execution failed: {error}"))
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

fn is_spotify_related_input(lower: &str) -> bool {
    lower.contains("spotify")
}

fn is_affirmative(lower: &str) -> bool {
    matches!(
        lower.trim(),
        "yes" | "yeah" | "yep" | "sure" | "ok" | "okay" | "do it" | "please"
    )
}

fn is_negative(lower: &str) -> bool {
    matches!(lower.trim(), "no" | "nope" | "not now" | "later" | "cancel")
}

fn tool_result_status(result_json: &Value) -> Option<&str> {
    result_json.get("status").and_then(Value::as_str)
}

pub fn parse_turn_envelope(raw_output: &str) -> Result<TurnEnvelope> {
    if let Some(envelope) = try_parse_turn_envelope(raw_output.trim()) {
        return Ok(envelope);
    }

    if let (Some(start), Some(end)) = (raw_output.find('{'), raw_output.rfind('}')) {
        let candidate = &raw_output[start..=end];
        if let Some(envelope) = try_parse_turn_envelope(candidate) {
            return Ok(envelope);
        }
    }

    Ok(TurnEnvelope {
        assistant_reply: raw_output.trim().to_owned(),
        tool_request: None,
        memory_candidates: Vec::new(),
    })
}

fn try_parse_turn_envelope(candidate: &str) -> Option<TurnEnvelope> {
    if let Ok(envelope) = serde_json::from_str::<TurnEnvelope>(candidate) {
        return Some(envelope);
    }

    let mut value = serde_json::from_str::<Value>(candidate).ok()?;
    normalize_turn_envelope_value(&mut value);
    serde_json::from_value(value).ok()
}

fn normalize_turn_envelope_value(value: &mut Value) {
    let Some(tool_request) = value.get_mut("tool_request").and_then(Value::as_object_mut) else {
        return;
    };

    if !tool_request.contains_key("action") {
        if let Some(name) = tool_request.remove("name") {
            tool_request.insert("action".to_owned(), name);
        }
    }

    if !tool_request.contains_key("tool") {
        if let Some(action) = tool_request.get("action").and_then(Value::as_str) {
            if let Some(tool_name) = infer_tool_name(action) {
                tool_request.insert("tool".to_owned(), Value::String(tool_name.to_owned()));
            }
        }
    }

    if !tool_request.contains_key("arguments") {
        tool_request.insert("arguments".to_owned(), json!({}));
    }
}

fn infer_tool_name(action: &str) -> Option<&'static str> {
    match action {
        "auth_status"
        | "start_auth"
        | "handle_callback"
        | "exchange_code"
        | "refresh_token"
        | "play"
        | "pause"
        | "next_track"
        | "previous_track"
        | "current_playback"
        | "set_volume"
        | "search_track"
        | "search" => Some("spotify"),
        "set_power" | "set_brightness" | "set_color" | "activate_scene" => Some("hue"),
        _ => None,
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

fn elapsed_millis(started_at: Instant) -> i64 {
    started_at.elapsed().as_millis().min(i64::MAX as u128) as i64
}
