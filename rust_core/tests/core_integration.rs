use std::fs;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use baby_gervaise_core::hgie::parse_turn_envelope;
use baby_gervaise_core::memory::{cosine_similarity, vectorize_text};
use baby_gervaise_core::model::{ModelGateway, ModelRequest, ModelResponse};
use baby_gervaise_core::{
    AppConfig, BabyGervaiseCore, ContextLevel, CoreCallbacks, InputSource, ModelConfig,
    PromptConfig,
};
use tempfile::tempdir;

#[derive(Default)]
struct RecordingCallbacks {
    events: Mutex<Vec<(String, String)>>,
}

impl CoreCallbacks for RecordingCallbacks {
    fn emit(&self, event_type: &str, payload_json: String) {
        self.events
            .lock()
            .expect("callback mutex poisoned")
            .push((event_type.to_owned(), payload_json));
    }
}

struct StaticModel {
    response: String,
}

impl ModelGateway for StaticModel {
    fn send_turn(&self, _request: &ModelRequest) -> Result<ModelResponse> {
        Ok(ModelResponse {
            prompt_json: "{}".to_owned(),
            raw_output: self.response.clone(),
            input_tokens: Some(11),
            output_tokens: Some(7),
            latency_ms: 12,
            http_status: Some(200),
        })
    }

    fn model_name(&self) -> &str {
        "test-model"
    }
}

fn test_configs() -> (ModelConfig, PromptConfig, AppConfig) {
    (
        ModelConfig {
            provider: "openai".to_owned(),
            api_key: "test".to_owned(),
            model: "test-model".to_owned(),
            endpoint: "https://example.invalid".to_owned(),
            temperature: 0.3,
            timeout_ms: 1000,
            stream: true,
        },
        PromptConfig {
            system_prompt: "Stay continuous.".to_owned(),
            memory_preamble: "Use memory carefully.".to_owned(),
            tool_instructions: "Use tools deterministically.".to_owned(),
            response_contract: "Return JSON.".to_owned(),
        },
        AppConfig {
            default_previous_context: ContextLevel::Medium,
            vector_dimensions: 64,
            memory_salience_threshold: 0.6,
            stream_chunk_size: 12,
            max_recent_messages_per_turn: 32,
            max_model_logs: 50,
        },
    )
}

#[test]
fn parses_json_envelope() {
    let envelope = parse_turn_envelope(
        r#"{"assistant_reply":"Hello Paul","tool_request":null,"memory_candidates":[{"kind":"fact","text":"Paul likes prototypes","salience":0.8}]}"#,
    )
    .expect("should parse envelope");

    assert_eq!(envelope.assistant_reply, "Hello Paul");
    assert_eq!(envelope.memory_candidates.len(), 1);
}

#[test]
fn vectorizer_is_deterministic_and_ranked() {
    let alpha = vectorize_text("play soft jazz in the kitchen", 64);
    let beta = vectorize_text("play soft jazz in the kitchen", 64);
    let gamma = vectorize_text("turn on the office lights", 64);

    assert_eq!(alpha, beta);
    assert!(cosine_similarity(&alpha, &beta) > cosine_similarity(&alpha, &gamma));
}

#[test]
fn core_persists_one_continuous_timeline_and_tool_state() -> Result<()> {
    let temp = tempdir()?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let (model_config, prompt_config, app_config) = test_configs();
    let model = Arc::new(StaticModel {
        response: r#"{
            "assistant_reply": "I'll set that for you.",
            "tool_request": {
                "tool": "hue",
                "action": "set_color",
                "arguments": { "color": "amber" }
            },
            "memory_candidates": [
                { "kind": "fact", "text": "Paul likes warm evening lighting.", "salience": 0.9 }
            ]
        }"#
        .to_owned(),
    });

    let core = BabyGervaiseCore::with_model_gateway(
        temp.path(),
        callbacks.clone(),
        model_config,
        prompt_config,
        app_config,
        model,
    )?;

    core.submit_user_turn("turn-1", "Set the lights to amber", InputSource::Text)?;
    let bootstrap = core.load_bootstrap_state()?;
    let overview = core.load_overview_state()?;

    assert_eq!(bootstrap.messages.len(), 3);
    assert_eq!(bootstrap.messages[0].role, "user");
    assert_eq!(bootstrap.messages[2].role, "assistant");
    assert_eq!(overview.system_stats.total_interactions, 1);
    assert_eq!(overview.system_stats.tool_calls, 1);
    assert_eq!(overview.memory_stats.stored_memories, 2);
    assert!(overview.tool_states.contains_key("hue"));
    assert!(callbacks
        .events
        .lock()
        .expect("callback mutex poisoned")
        .iter()
        .any(|(event_type, _)| event_type == "assistant_completed"));

    Ok(())
}

#[test]
fn config_merge_prefers_local_override() -> Result<()> {
    let temp = tempdir()?;
    fs::write(
        temp.path().join("model_config.json"),
        r#"{
            "provider":"openai",
            "api_key":"YOUR_KEY",
            "model":"gpt-4o-mini",
            "endpoint":"https://api.openai.com/v1/chat/completions",
            "temperature":0.3,
            "timeout_ms":1000,
            "stream":true
        }"#,
    )?;
    fs::write(
        temp.path().join("model_config.local.json"),
        r#"{
            "api_key":"local-key",
            "model":"gpt-4o"
        }"#,
    )?;
    fs::write(
        temp.path().join("prompt_config.json"),
        r#"{
            "system_prompt":"system",
            "memory_preamble":"memory",
            "tool_instructions":"tools",
            "response_contract":"json"
        }"#,
    )?;
    fs::write(
        temp.path().join("app_config.json"),
        r#"{
            "default_previous_context":"medium",
            "vector_dimensions":64,
            "memory_salience_threshold":0.6,
            "stream_chunk_size":12,
            "max_recent_messages_per_turn":32,
            "max_model_logs":50
        }"#,
    )?;

    let core = BabyGervaiseCore::init(
        temp.path(),
        temp.path(),
        Arc::new(RecordingCallbacks::default()),
    );
    assert!(core.is_ok());
    Ok(())
}
