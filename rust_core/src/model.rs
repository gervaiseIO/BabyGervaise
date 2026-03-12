use std::io::{BufRead, BufReader};
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use reqwest::blocking::{Client, Response};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::ModelConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRequest {
    pub messages: Vec<ModelMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelResponse {
    pub prompt_json: String,
    pub raw_output: String,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub latency_ms: i64,
    pub http_status: Option<i64>,
}

pub trait ModelGateway: Send + Sync {
    fn send_turn(&self, request: &ModelRequest) -> Result<ModelResponse>;
    fn model_name(&self) -> &str;
}

pub struct OpenAiCompatibleModel {
    config: ModelConfig,
    client: Client,
}

impl OpenAiCompatibleModel {
    pub fn new(config: ModelConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_millis(config.timeout_ms))
            .build()
            .context("failed to construct HTTP client")?;
        Ok(Self { config, client })
    }

    fn request_body(&self, request: &ModelRequest) -> Value {
        let mut body = json!({
            "model": self.config.model,
            "temperature": self.config.temperature,
            "stream": self.config.stream,
            "messages": request.messages,
        });

        if self.config.stream && self.config.provider.eq_ignore_ascii_case("openai") {
            body["stream_options"] = json!({
                "include_usage": true
            });
        }

        body
    }

    fn extract_message_content(&self, body: &str) -> Result<(String, Option<i64>, Option<i64>)> {
        let value: Value = serde_json::from_str(body).context("invalid model response body")?;
        let content = value
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("model response did not include choices[0].message.content"))?
            .to_owned();
        let input_tokens = value
            .pointer("/usage/prompt_tokens")
            .and_then(Value::as_i64);
        let output_tokens = value
            .pointer("/usage/completion_tokens")
            .and_then(Value::as_i64);
        Ok((content, input_tokens, output_tokens))
    }

    fn extract_streamed_content(
        &self,
        response: Response,
    ) -> Result<(String, Option<i64>, Option<i64>)> {
        let mut raw_output = String::new();
        let mut input_tokens = None;
        let mut output_tokens = None;
        let reader = BufReader::new(response);

        for line in reader.lines() {
            let line = line.context("failed to read model stream line")?;
            if !line.starts_with("data: ") {
                continue;
            }
            let payload = &line[6..];
            if payload == "[DONE]" {
                break;
            }

            let value: Value = serde_json::from_str(payload)
                .with_context(|| format!("invalid stream payload: {payload}"))?;
            if let Some(delta) = value
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str)
            {
                raw_output.push_str(delta);
            }
            input_tokens = input_tokens.or_else(|| {
                value
                    .pointer("/usage/prompt_tokens")
                    .and_then(Value::as_i64)
            });
            output_tokens = output_tokens.or_else(|| {
                value
                    .pointer("/usage/completion_tokens")
                    .and_then(Value::as_i64)
            });
        }

        Ok((raw_output, input_tokens, output_tokens))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_model(stream: bool) -> OpenAiCompatibleModel {
        OpenAiCompatibleModel {
            config: ModelConfig {
                provider: "openai".to_owned(),
                api_key: "test-key".to_owned(),
                model: "gpt-4o-mini".to_owned(),
                endpoint: "https://api.openai.com/v1/chat/completions".to_owned(),
                temperature: 0.3,
                timeout_ms: 1_000,
                stream,
            },
            client: Client::builder().build().unwrap(),
        }
    }

    #[test]
    fn request_body_includes_stream_usage_for_openai_streams() {
        let model = test_model(true);
        let body = model.request_body(&ModelRequest {
            messages: vec![ModelMessage {
                role: "user".to_owned(),
                content: "Hello".to_owned(),
            }],
        });

        assert_eq!(
            body.pointer("/stream_options/include_usage").and_then(Value::as_bool),
            Some(true),
        );
    }

    #[test]
    fn request_body_omits_stream_usage_when_streaming_disabled() {
        let model = test_model(false);
        let body = model.request_body(&ModelRequest {
            messages: vec![ModelMessage {
                role: "user".to_owned(),
                content: "Hello".to_owned(),
            }],
        });

        assert!(body.get("stream_options").is_none());
    }
}

impl ModelGateway for OpenAiCompatibleModel {
    fn send_turn(&self, request: &ModelRequest) -> Result<ModelResponse> {
        let body = self.request_body(request);
        let prompt_json =
            serde_json::to_string_pretty(&body).context("failed to serialize prompt")?;
        let started_at = Instant::now();

        let mut builder = self
            .client
            .post(&self.config.endpoint)
            .header(CONTENT_TYPE, "application/json")
            .header(
                ACCEPT,
                if self.config.stream {
                    "text/event-stream"
                } else {
                    "application/json"
                },
            )
            .json(&body);

        if !self.config.api_key.trim().is_empty() && self.config.api_key != "YOUR_KEY" {
            builder = builder.header(AUTHORIZATION, format!("Bearer {}", self.config.api_key));
        }

        let response = builder.send().context("failed to reach model provider")?;
        let status = response.status();
        let latency_ms = started_at.elapsed().as_millis() as i64;

        if !status.is_success() {
            let error_body = response
                .text()
                .unwrap_or_else(|_| "unreadable provider error".to_owned());
            return Err(anyhow!(
                "model provider returned {} with body: {}",
                status.as_u16(),
                error_body
            ));
        }

        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();

        let (raw_output, input_tokens, output_tokens) =
            if content_type.contains("text/event-stream") {
                self.extract_streamed_content(response)?
            } else {
                let body = response
                    .text()
                    .context("failed to read model response body")?;
                self.extract_message_content(&body)?
            };

        Ok(ModelResponse {
            prompt_json,
            raw_output,
            input_tokens,
            output_tokens,
            latency_ms,
            http_status: Some(status.as_u16() as i64),
        })
    }

    fn model_name(&self) -> &str {
        &self.config.model
    }
}
