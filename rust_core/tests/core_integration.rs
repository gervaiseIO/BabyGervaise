use std::collections::VecDeque;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use anyhow::Result;
use baby_gervaise_core::hgie::parse_turn_envelope;
use baby_gervaise_core::memory::{cosine_similarity, vectorize_text, MemoryStore};
use baby_gervaise_core::model::{ModelGateway, ModelRequest, ModelResponse};
use baby_gervaise_core::tools::ToolExecutor;
use baby_gervaise_core::{
    AppConfig, BabyGervaiseCore, ContextLevel, CoreCallbacks, InputSource, ModelConfig,
    PromptConfig,
};
use serde_json::json;
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

struct RecordedRequest {
    request_line: String,
    body: String,
}

struct MockHttpResponse {
    status: u16,
    body: Option<String>,
}

impl MockHttpResponse {
    fn json(value: serde_json::Value) -> Self {
        Self {
            status: 200,
            body: Some(value.to_string()),
        }
    }

    fn empty(status: u16) -> Self {
        Self { status, body: None }
    }
}

struct MockHttpServer {
    base_url: String,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    handle: Option<JoinHandle<()>>,
}

impl MockHttpServer {
    fn request_lines(&self) -> Vec<String> {
        self.requests
            .lock()
            .expect("request mutex poisoned")
            .iter()
            .map(|request| request.request_line.clone())
            .collect()
    }

    fn request_bodies(&self) -> Vec<String> {
        self.requests
            .lock()
            .expect("request mutex poisoned")
            .iter()
            .map(|request| request.body.clone())
            .collect()
    }
}

impl Drop for MockHttpServer {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn spawn_mock_http_server(responses: Vec<MockHttpResponse>) -> Result<MockHttpServer> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let requests_handle = Arc::clone(&requests);
    let mut pending_responses = VecDeque::from(responses);

    let handle = thread::spawn(move || {
        while let Some(response) = pending_responses.pop_front() {
            let (mut stream, _) = listener.accept().expect("mock server accept failed");
            let recorded = read_http_request(&mut stream);
            requests_handle
                .lock()
                .expect("request mutex poisoned")
                .push(recorded);
            write_http_response(&mut stream, response);
        }
    });

    Ok(MockHttpServer {
        base_url: format!("http://{}", address),
        requests,
        handle: Some(handle),
    })
}

fn read_http_request(stream: &mut TcpStream) -> RecordedRequest {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let mut header_end = None;
    let mut content_length = 0_usize;

    loop {
        let bytes_read = stream.read(&mut chunk).expect("mock server read failed");
        if bytes_read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..bytes_read]);

        if header_end.is_none() {
            if let Some(position) = find_header_end(&buffer) {
                header_end = Some(position);
                let headers = String::from_utf8_lossy(&buffer[..position]);
                content_length = headers
                    .lines()
                    .find_map(|line| {
                        let lower = line.to_ascii_lowercase();
                        lower
                            .strip_prefix("content-length:")
                            .and_then(|value| value.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if buffer.len() >= position + content_length {
                    break;
                }
            }
        } else if let Some(position) = header_end {
            if buffer.len() >= position + content_length {
                break;
            }
        }
    }

    let header_end = header_end.unwrap_or(buffer.len());
    let request_line = String::from_utf8_lossy(&buffer[..header_end])
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();
    let body = String::from_utf8_lossy(buffer.get(header_end..).unwrap_or(&[])).to_string();

    RecordedRequest { request_line, body }
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
}

fn write_http_response(stream: &mut TcpStream, response: MockHttpResponse) {
    let body = response.body.unwrap_or_default();
    let reason = match response.status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "OK",
    };

    let payload = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        reason,
        body.len(),
        body
    );
    stream
        .write_all(payload.as_bytes())
        .expect("mock server write failed");
}

fn test_configs() -> (ModelConfig, PromptConfig, AppConfig) {
    (
        ModelConfig {
            provider: "openai".to_owned(),
            api_key: "test".to_owned(),
            model: "test-model".to_owned(),
            endpoint: "https://example.invalid".to_owned(),
            temperature: Some(0.3),
            timeout_ms: 1000,
            stream: true,
            reasoning: None,
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

fn write_spotify_config(config_dir: &Path, base_url: &str) -> Result<()> {
    fs::write(
        config_dir.join("spotify_config.json"),
        serde_json::to_string_pretty(&json!({
            "client_id": "test-client",
            "client_secret": "test-secret",
            "redirect_uri": "babygervaise://spotify/callback",
            "scopes": [
                "user-read-playback-state",
                "user-modify-playback-state",
                "user-read-currently-playing"
            ],
            "accounts_base_url": base_url,
            "api_base_url": format!("{base_url}/v1"),
            "timeout_ms": 2000
        }))?,
    )?;
    Ok(())
}

fn write_placeholder_spotify_config(config_dir: &Path) -> Result<()> {
    fs::write(
        config_dir.join("spotify_config.json"),
        serde_json::to_string_pretty(&json!({
            "client_id": "test-client",
            "client_secret": "test-secret",
            "redirect_uri": "babygervaise://spotify/callback",
            "scopes": [
                "user-read-playback-state",
                "user-modify-playback-state",
                "user-read-currently-playing"
            ]
        }))?,
    )?;
    Ok(())
}

fn write_spotify_token_cache(app_files_dir: &Path) -> Result<()> {
    fs::write(
        app_files_dir.join("spotify_tokens.json"),
        serde_json::to_string_pretty(&json!({
            "access_token": "cached-token",
            "refresh_token": "cached-refresh-token",
            "expires_at": "2099-01-01T00:00:00Z"
        }))?,
    )?;
    Ok(())
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
fn parses_json_envelope_without_tool_arguments() {
    let envelope = parse_turn_envelope(
        r#"{"assistant_reply":"","tool_request":{"tool":"spotify","action":"pause"},"memory_candidates":[]}"#,
    )
    .expect("should parse envelope");

    assert!(envelope.tool_request.is_some());
    assert_eq!(
        envelope
            .tool_request
            .expect("missing tool request")
            .arguments,
        json!({})
    );
}

#[test]
fn parses_json_envelope_with_legacy_tool_request_shape() {
    let envelope = parse_turn_envelope(
        r#"{"assistant_reply":"Starting Spotify sign-in.","tool_request":{"name":"start_auth","arguments":{}},"memory_candidates":[]}"#,
    )
    .expect("should parse envelope");

    let tool_request = envelope.tool_request.expect("missing tool request");
    assert_eq!(tool_request.tool.as_str(), "spotify");
    assert_eq!(tool_request.action, "start_auth");
    assert_eq!(tool_request.arguments, json!({}));
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
fn tool_executor_runs_spotify_play_and_set_volume() -> Result<()> {
    let server = spawn_mock_http_server(vec![
        MockHttpResponse::json(json!({
            "tracks": {
                "items": [
                    {
                        "name": "Blue in Green",
                        "uri": "spotify:track:blue-in-green",
                        "album": { "name": "Kind of Blue" },
                        "artists": [{ "name": "Miles Davis" }]
                    }
                ]
            }
        })),
        MockHttpResponse::empty(204),
        MockHttpResponse::json(json!({
            "is_playing": true,
            "device": { "volume_percent": 58, "name": "Studio" },
            "item": {
                "name": "Blue in Green",
                "uri": "spotify:track:blue-in-green",
                "album": { "name": "Kind of Blue" },
                "artists": [{ "name": "Miles Davis" }]
            }
        })),
        MockHttpResponse::empty(204),
    ])?;

    let temp = tempdir()?;
    let (_, _, app_config) = test_configs();
    write_spotify_config(temp.path(), &server.base_url)?;
    write_spotify_token_cache(temp.path())?;

    let memory = MemoryStore::new(temp.path().join("baby_gervaise.sqlite3"), &app_config)?;
    let tools = ToolExecutor::with_spotify(memory, temp.path(), temp.path());

    let play = tools.execute_named("spotify", "play", json!({ "query": "Miles Davis" }))?;
    let set_volume =
        tools.execute_named("spotify", "set_volume", json!({ "volume_percent": 30 }))?;

    assert_eq!(play.result_json["status"], "success");
    assert_eq!(play.result_json["action"], "play");
    assert_eq!(play.result_json["track"], "Blue in Green");
    assert_eq!(play.result_json["artist"], "Miles Davis");
    assert_eq!(play.result_json["album"], "Kind of Blue");
    assert_eq!(set_volume.result_json["volume_percent"], 30);

    let request_lines = server.request_lines();
    let request_bodies = server.request_bodies();
    assert!(request_lines
        .iter()
        .any(|line| line.starts_with("GET /v1/search?")));
    assert!(request_lines
        .iter()
        .any(|line| line == "PUT /v1/me/player/play HTTP/1.1"));
    assert!(request_lines.iter().any(|line| {
        line == "PUT /v1/me/player/volume?volume_percent=30 HTTP/1.1"
            || line == "PUT /v1/me/player/volume?volume_percent=30& HTTP/1.1"
    }));
    assert!(request_bodies
        .iter()
        .any(|body| body.contains("spotify:track:blue-in-green")));

    Ok(())
}

#[test]
fn core_prompts_for_spotify_auth_and_starts_browser_flow() -> Result<()> {
    let temp = tempdir()?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let (model_config, prompt_config, app_config) = test_configs();
    write_placeholder_spotify_config(temp.path())?;
    let model = Arc::new(StaticModel {
        response:
            r#"{"assistant_reply":"model fallback","tool_request":null,"memory_candidates":[]}"#
                .to_owned(),
    });

    let core = BabyGervaiseCore::with_model_gateway(
        temp.path(),
        temp.path(),
        callbacks.clone(),
        model_config,
        prompt_config,
        app_config,
        model,
    )?;

    core.submit_user_turn(
        "turn-auth-1",
        "I want to listen to music on Spotify",
        InputSource::Text,
    )?;
    core.submit_user_turn("turn-auth-2", "yes", InputSource::Text)?;

    let bootstrap = core.load_bootstrap_state()?;
    let overview = core.load_overview_state()?;
    let events = callbacks.events.lock().expect("callback mutex poisoned");

    assert!(bootstrap.messages.iter().any(|message| {
        message.role == "assistant"
            && message.content
                == "You need to sign in to Spotify first. Do you want to do that now?"
    }));
    assert!(bootstrap.messages.iter().any(|message| {
        message.role == "assistant" && message.content.contains("Opening Spotify sign-in now.")
    }));
    assert_eq!(overview.system_stats.tool_calls, 1);
    assert!(events.iter().any(|(event_type, payload)| {
        event_type == "open_external_url"
            && payload.contains("\"purpose\":\"spotify_auth\"")
            && payload.contains("accounts.spotify.com/authorize")
    }));

    Ok(())
}

#[test]
fn core_completes_spotify_auth_callback_and_persists_tokens() -> Result<()> {
    let server = spawn_mock_http_server(vec![MockHttpResponse::json(json!({
        "access_token": "new-access-token",
        "refresh_token": "new-refresh-token",
        "expires_in": 3600
    }))])?;

    let temp = tempdir()?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let (model_config, prompt_config, app_config) = test_configs();
    write_spotify_config(temp.path(), &server.base_url)?;
    let model = Arc::new(StaticModel {
        response:
            r#"{"assistant_reply":"model fallback","tool_request":null,"memory_candidates":[]}"#
                .to_owned(),
    });

    let core = BabyGervaiseCore::with_model_gateway(
        temp.path(),
        temp.path(),
        callbacks.clone(),
        model_config,
        prompt_config,
        app_config,
        model,
    )?;

    core.submit_user_turn(
        "turn-auth-callback-1",
        "I want to listen to music on Spotify",
        InputSource::Text,
    )?;
    core.submit_user_turn("turn-auth-callback-2", "yes", InputSource::Text)?;

    let overview = core.load_overview_state()?;
    let spotify_state = overview
        .tool_states
        .get("spotify")
        .expect("missing spotify tool state");
    let oauth_state = spotify_state
        .get("pending_auth_state")
        .and_then(serde_json::Value::as_str)
        .expect("missing pending auth state");

    core.handle_spotify_auth_callback(
        "turn-auth-callback-3",
        &format!("babygervaise://spotify/callback?code=test-code&state={oauth_state}"),
    )?;

    let bootstrap = core.load_bootstrap_state()?;
    let token_cache = fs::read_to_string(temp.path().join("spotify_tokens.json"))?;
    let events = callbacks.events.lock().expect("callback mutex poisoned");

    assert!(bootstrap.messages.iter().any(|message| {
        message.role == "assistant"
            && message
                .content
                .contains("You're connected to Spotify now. What would you like to listen to?")
    }));
    assert!(token_cache.contains("new-refresh-token"));
    assert!(events.iter().any(|(event_type, payload)| {
        event_type == "tool_status"
            && payload.contains("\"action\":\"handle_callback\"")
            && payload.contains("\"status\":\"executing\"")
    }));

    Ok(())
}

#[test]
fn core_persists_one_continuous_timeline_and_tool_state() -> Result<()> {
    let temp = tempdir()?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let (model_config, prompt_config, app_config) = test_configs();
    write_placeholder_spotify_config(temp.path())?;
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
fn core_executes_spotify_current_playback_via_hgie() -> Result<()> {
    let server = spawn_mock_http_server(vec![MockHttpResponse::json(json!({
        "is_playing": true,
        "device": { "volume_percent": 30, "name": "Studio" },
        "item": {
            "name": "Blue in Green",
            "uri": "spotify:track:blue-in-green",
            "album": { "name": "Kind of Blue" },
            "artists": [{ "name": "Miles Davis" }]
        }
    }))])?;

    let temp = tempdir()?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let (model_config, prompt_config, app_config) = test_configs();
    write_spotify_config(temp.path(), &server.base_url)?;
    write_spotify_token_cache(temp.path())?;
    let model = Arc::new(StaticModel {
        response: r#"{
            "assistant_reply": "",
            "tool_request": {
                "tool": "spotify",
                "action": "current_playback"
            },
            "memory_candidates": []
        }"#
        .to_owned(),
    });

    let core = BabyGervaiseCore::with_model_gateway(
        temp.path(),
        temp.path(),
        callbacks.clone(),
        model_config,
        prompt_config,
        app_config,
        model,
    )?;

    core.submit_user_turn("turn-spotify-1", "What is playing?", InputSource::Text)?;
    let bootstrap = core.load_bootstrap_state()?;
    let overview = core.load_overview_state()?;

    assert_eq!(overview.system_stats.tool_calls, 1);
    assert!(overview.tool_states.contains_key("spotify"));
    assert!(bootstrap.messages.iter().any(|message| {
        message.role == "tool" && message.content.contains("\"track\": \"Blue in Green\"")
    }));
    assert!(bootstrap.messages.iter().any(|message| {
        message.role == "assistant" && message.content.contains("Blue in Green")
    }));
    assert!(callbacks
        .events
        .lock()
        .expect("callback mutex poisoned")
        .iter()
        .any(|(event_type, payload)| {
            event_type == "tool_status" && payload.contains("\"status\":\"executing\"")
        }));

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
    write_placeholder_spotify_config(temp.path())?;

    let core = BabyGervaiseCore::init(
        temp.path(),
        temp.path(),
        Arc::new(RecordingCallbacks::default()),
    );
    assert!(core.is_ok());
    Ok(())
}

#[test]
fn core_init_succeeds_when_spotify_config_is_invalid() -> Result<()> {
    let temp = tempdir()?;
    fs::write(
        temp.path().join("model_config.json"),
        r#"{
            "provider":"openai",
            "api_key":"YOUR_KEY",
            "model":"gpt-4o-mini",
            "endpoint":"https://api.openai.com/v1/chat/completions",
            "timeout_ms":1000,
            "stream":true
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
    fs::write(temp.path().join("spotify_config.json"), "{ invalid json")?;

    let core = BabyGervaiseCore::init(
        temp.path(),
        temp.path(),
        Arc::new(RecordingCallbacks::default()),
    );
    assert!(core.is_ok());
    Ok(())
}
