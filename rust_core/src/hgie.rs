use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::logging::{ModelLogEntry, RetrievalLogEntry, ToolLogEntry};
use crate::memory::{MemoryStore, RetrievedMemory};
use crate::model::{ModelGateway, ModelMessage, ModelRequest};
use crate::tools::{ToolExecutionResult, ToolExecutor, ToolRequest};
use crate::{
    now_rfc3339, AppConfig, ChatMessage, ContextLevel, CoreCallbacks, InputSource, ModelConfig,
    PromptConfig,
};

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
        model: Arc<dyn ModelGateway>,
        model_config: ModelConfig,
        prompt_config: PromptConfig,
        app_config: AppConfig,
    ) -> Self {
        let tools = ToolExecutor::new(memory.clone());
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

        let mut final_reply = envelope.assistant_reply.trim().to_owned();
        let mut tool_message: Option<String> = None;
        if let Some(tool_request) = &envelope.tool_request {
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

            let tool_result = self.execute_tool(tool_request)?;
            let tool_summary = tool_result.summary.clone();
            tool_message = Some(serde_json::to_string_pretty(&tool_result.result_json)?);

            self.memory.log_tool_call(&ToolLogEntry {
                created_at: now_rfc3339(),
                tool_name: serde_json::to_string(&tool_request.tool)?.replace('"', ""),
                action: tool_request.action.clone(),
                arguments_json: serde_json::to_string(&tool_request.arguments)?,
                result_json: serde_json::to_string(&tool_result.result_json)?,
                success: true,
            })?;

            self.memory.store_memory_item(
                "summary",
                &format!(
                    "Tool {}.{} -> {}",
                    serde_json::to_string(&tool_request.tool)?.replace('"', ""),
                    tool_request.action,
                    tool_summary
                ),
                0.75,
                Some(user_message.id),
            )?;

            if final_reply.is_empty() {
                final_reply = tool_summary;
            } else {
                final_reply = format!("{final_reply}\n\n{tool_summary}");
            }
        }

        callbacks.emit(
            "assistant_started",
            json!({
                "turnId": turn_id
            })
            .to_string(),
        );

        for chunk in chunk_text(&final_reply, self.app_config.stream_chunk_size) {
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
            .append_message("assistant", &final_reply, turn_id, input_source, None)
            .context("failed to persist assistant message")?;

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

pub fn parse_turn_envelope(raw_output: &str) -> Result<TurnEnvelope> {
    if let Ok(envelope) = serde_json::from_str::<TurnEnvelope>(raw_output.trim()) {
        return Ok(envelope);
    }

    if let (Some(start), Some(end)) = (raw_output.find('{'), raw_output.rfind('}')) {
        let candidate = &raw_output[start..=end];
        if let Ok(envelope) = serde_json::from_str::<TurnEnvelope>(candidate) {
            return Ok(envelope);
        }
    }

    Ok(TurnEnvelope {
        assistant_reply: raw_output.trim().to_owned(),
        tool_request: None,
        memory_candidates: Vec::new(),
    })
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
