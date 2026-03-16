use std::collections::VecDeque;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use anyhow::Result;
use baby_gervaise_core::hgie::parse_turn_envelope;
use baby_gervaise_core::logging::{NanoRuntimeStatus, RuntimeOverview, RuntimeProfileSummary};
use baby_gervaise_core::memory::{
    cosine_similarity, vectorize_text, ContinuityMode, HydrateWorkingMemoryRequest, MemoryStore,
    RecallBudget, RecallBundle, RecallRequest, WorkingMemorySnapshot, WorkingThreadStatus,
};
use baby_gervaise_core::model::{ModelGateway, ModelRequest, ModelResponse};
use baby_gervaise_core::model_runtime::{
    AmbientRequest, AmbientResult, CloudReasoningRequest, CloudReasoningResult, FirstBeatRequest,
    FirstBeatResult, ModelRuntime, NanoReplyRequest, NanoReplyResult, RuntimeConfigResolver,
};
use baby_gervaise_core::tools::ToolExecutor;
use baby_gervaise_core::{
    AppConfig, BabyGervaiseCore, ContextLevel, CoreCallbacks, InputSource, MessageContentType,
    ModelConfig, PromptConfig,
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

fn runtime_overview(nano_active: bool, cloud_available: bool) -> RuntimeOverview {
    RuntimeOverview {
        nano: NanoRuntimeStatus {
            enabled: true,
            availability: if nano_active {
                "available".to_owned()
            } else {
                "unavailable".to_owned()
            },
            detail: if nano_active {
                "Gemini Nano is ready.".to_owned()
            } else {
                "Gemini Nano is unavailable.".to_owned()
            },
            provider: "gemini".to_owned(),
            model: "gemini-nano".to_owned(),
            active: nano_active,
        },
        selected_cloud_profile_id: cloud_available.then(|| "compat".to_owned()),
        selected_cloud_profile_label: cloud_available.then(|| "Compat".to_owned()),
        cloud_profiles: if cloud_available {
            vec![RuntimeProfileSummary {
                id: "compat".to_owned(),
                label: "Compat".to_owned(),
                provider: "openai".to_owned(),
                model: "test-model".to_owned(),
                enabled: true,
                available: true,
                selected: true,
            }]
        } else {
            Vec::new()
        },
    }
}

#[derive(Default)]
struct SequencedRuntime {
    calls: Mutex<Vec<&'static str>>,
}

impl ModelRuntime for SequencedRuntime {
    fn run_first_beat(&self, _request: &FirstBeatRequest) -> Result<FirstBeatResult> {
        self.calls.lock().unwrap().push("first_beat");
        Ok(FirstBeatResult {
            text: "Sure -".to_owned(),
            logs: Vec::new(),
        })
    }

    fn run_nano_reply(&self, _request: &NanoReplyRequest) -> Result<NanoReplyResult> {
        self.calls.lock().unwrap().push("nano_reply");
        Ok(NanoReplyResult {
            raw_output: json!({
                "assistant_reply":"let me explain.",
                "tool_request":null,
                "memory_candidates":[]
            })
            .to_string(),
            logs: Vec::new(),
        })
    }

    fn run_cloud_reasoning(
        &self,
        _request: &CloudReasoningRequest,
    ) -> Result<CloudReasoningResult> {
        self.calls.lock().unwrap().push("cloud_reasoning");
        Ok(CloudReasoningResult {
            raw_output: json!({
                "assistant_reply":"let me explain.",
                "tool_request":null,
                "memory_candidates":[]
            })
            .to_string(),
            logs: Vec::new(),
        })
    }

    fn run_ambient(&self, _request: &AmbientRequest) -> Result<Option<AmbientResult>> {
        self.calls.lock().unwrap().push("ambient");
        Ok(Some(AmbientResult {
            text: "Back again.".to_owned(),
            logs: Vec::new(),
        }))
    }

    fn overview(&self) -> RuntimeOverview {
        runtime_overview(true, true)
    }

    fn can_accept_turns(&self) -> bool {
        true
    }

    fn selected_cloud_profile_id(&self) -> Option<String> {
        Some("compat".to_owned())
    }

    fn set_selected_cloud_profile(&self, _profile_id: &str) -> Result<()> {
        Ok(())
    }

    fn default_selected_cloud_profile_id(&self) -> Option<String> {
        Some("compat".to_owned())
    }
}

struct FixedRuntime {
    first_beat: String,
    reply_output: String,
}

impl ModelRuntime for FixedRuntime {
    fn run_first_beat(&self, _request: &FirstBeatRequest) -> Result<FirstBeatResult> {
        Ok(FirstBeatResult {
            text: self.first_beat.clone(),
            logs: Vec::new(),
        })
    }

    fn run_nano_reply(&self, _request: &NanoReplyRequest) -> Result<NanoReplyResult> {
        Ok(NanoReplyResult {
            raw_output: self.reply_output.clone(),
            logs: Vec::new(),
        })
    }

    fn run_cloud_reasoning(
        &self,
        _request: &CloudReasoningRequest,
    ) -> Result<CloudReasoningResult> {
        Ok(CloudReasoningResult {
            raw_output: self.reply_output.clone(),
            logs: Vec::new(),
        })
    }

    fn run_ambient(&self, _request: &AmbientRequest) -> Result<Option<AmbientResult>> {
        Ok(None)
    }

    fn overview(&self) -> RuntimeOverview {
        runtime_overview(true, true)
    }

    fn can_accept_turns(&self) -> bool {
        true
    }

    fn selected_cloud_profile_id(&self) -> Option<String> {
        Some("compat".to_owned())
    }

    fn set_selected_cloud_profile(&self, _profile_id: &str) -> Result<()> {
        Ok(())
    }

    fn default_selected_cloud_profile_id(&self) -> Option<String> {
        Some("compat".to_owned())
    }
}

struct RecordingPlanRuntime {
    calls: Mutex<Vec<&'static str>>,
    nano_active: bool,
    cloud_available: bool,
    first_beat: String,
    nano_output: String,
    cloud_output: String,
}

impl ModelRuntime for RecordingPlanRuntime {
    fn run_first_beat(&self, _request: &FirstBeatRequest) -> Result<FirstBeatResult> {
        self.calls.lock().unwrap().push("first_beat");
        Ok(FirstBeatResult {
            text: self.first_beat.clone(),
            logs: Vec::new(),
        })
    }

    fn run_nano_reply(&self, _request: &NanoReplyRequest) -> Result<NanoReplyResult> {
        self.calls.lock().unwrap().push("nano_reply");
        Ok(NanoReplyResult {
            raw_output: self.nano_output.clone(),
            logs: Vec::new(),
        })
    }

    fn run_cloud_reasoning(
        &self,
        _request: &CloudReasoningRequest,
    ) -> Result<CloudReasoningResult> {
        self.calls.lock().unwrap().push("cloud_reasoning");
        Ok(CloudReasoningResult {
            raw_output: self.cloud_output.clone(),
            logs: Vec::new(),
        })
    }

    fn run_ambient(&self, _request: &AmbientRequest) -> Result<Option<AmbientResult>> {
        Ok(None)
    }

    fn overview(&self) -> RuntimeOverview {
        runtime_overview(self.nano_active, self.cloud_available)
    }

    fn can_accept_turns(&self) -> bool {
        self.nano_active || self.cloud_available
    }

    fn selected_cloud_profile_id(&self) -> Option<String> {
        self.cloud_available.then(|| "compat".to_owned())
    }

    fn set_selected_cloud_profile(&self, _profile_id: &str) -> Result<()> {
        Ok(())
    }

    fn default_selected_cloud_profile_id(&self) -> Option<String> {
        self.cloud_available.then(|| "compat".to_owned())
    }
}

struct SplitStatsRuntime;

impl ModelRuntime for SplitStatsRuntime {
    fn run_first_beat(&self, request: &FirstBeatRequest) -> Result<FirstBeatResult> {
        Ok(FirstBeatResult {
            text: "Sure -".to_owned(),
            logs: vec![baby_gervaise_core::logging::ModelLogEntry {
                created_at: "2026-01-01T10:00:00Z".to_owned(),
                model_name: "Nano first beat".to_owned(),
                prompt: request.prompt.prompt.clone(),
                raw_output: "Sure -".to_owned(),
                input_tokens: None,
                output_tokens: None,
                latency_ms: 9,
                http_status: Some(200),
                error_text: None,
            }],
        })
    }

    fn run_nano_reply(&self, _request: &NanoReplyRequest) -> Result<NanoReplyResult> {
        Ok(NanoReplyResult {
            raw_output: json!({
                "assistant_reply":"Local answer.",
                "tool_request":null,
                "memory_candidates":[]
            })
            .to_string(),
            logs: Vec::new(),
        })
    }

    fn run_cloud_reasoning(
        &self,
        _request: &CloudReasoningRequest,
    ) -> Result<CloudReasoningResult> {
        Ok(CloudReasoningResult {
            raw_output: json!({
                "assistant_reply":"Cloud answer.",
                "tool_request":null,
                "memory_candidates":[]
            })
            .to_string(),
            logs: vec![baby_gervaise_core::logging::ModelLogEntry {
                created_at: "2026-01-01T10:00:01Z".to_owned(),
                model_name: "Compat".to_owned(),
                prompt: "{}".to_owned(),
                raw_output: "{}".to_owned(),
                input_tokens: Some(11),
                output_tokens: Some(7),
                latency_ms: 12,
                http_status: Some(200),
                error_text: None,
            }],
        })
    }

    fn run_ambient(&self, _request: &AmbientRequest) -> Result<Option<AmbientResult>> {
        Ok(None)
    }

    fn overview(&self) -> RuntimeOverview {
        runtime_overview(true, true)
    }

    fn can_accept_turns(&self) -> bool {
        true
    }

    fn selected_cloud_profile_id(&self) -> Option<String> {
        Some("compat".to_owned())
    }

    fn set_selected_cloud_profile(&self, _profile_id: &str) -> Result<()> {
        Ok(())
    }

    fn default_selected_cloud_profile_id(&self) -> Option<String> {
        Some("compat".to_owned())
    }
}

struct NoCloudSelectedRuntime;

impl ModelRuntime for NoCloudSelectedRuntime {
    fn run_first_beat(&self, _request: &FirstBeatRequest) -> Result<FirstBeatResult> {
        Ok(FirstBeatResult {
            text: String::new(),
            logs: Vec::new(),
        })
    }

    fn run_nano_reply(&self, _request: &NanoReplyRequest) -> Result<NanoReplyResult> {
        Ok(NanoReplyResult {
            raw_output: json!({
                "assistant_reply":"",
                "tool_request":null,
                "memory_candidates":[]
            })
            .to_string(),
            logs: Vec::new(),
        })
    }

    fn run_cloud_reasoning(
        &self,
        _request: &CloudReasoningRequest,
    ) -> Result<CloudReasoningResult> {
        Ok(CloudReasoningResult {
            raw_output: "{}".to_owned(),
            logs: Vec::new(),
        })
    }

    fn run_ambient(&self, _request: &AmbientRequest) -> Result<Option<AmbientResult>> {
        Ok(None)
    }

    fn overview(&self) -> RuntimeOverview {
        runtime_overview(true, false)
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
        403 => "Forbidden",
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
            label: None,
            endpoint: "https://example.invalid".to_owned(),
            temperature: Some(0.3),
            timeout_ms: 1000,
            stream: true,
            reasoning: None,
            enabled: true,
        },
        PromptConfig::from_legacy_strings(
            "Stay continuous.",
            "Use memory carefully.",
            "Use tools deterministically.",
            "Return JSON.",
        ),
        AppConfig {
            default_previous_context: ContextLevel::Medium,
            vector_dimensions: 64,
            memory_salience_threshold: 0.6,
            stream_chunk_size: 12,
            max_recent_messages_per_turn: 32,
            max_model_logs: 50,
            idle_resume_threshold_seconds: 900,
            ambient_cooldown_seconds: 600,
        },
    )
}

fn structured_reply(reply: &str) -> String {
    json!({
        "assistant_reply": reply,
        "tool_request": null,
        "memory_candidates": []
    })
    .to_string()
}

fn latest_working_snapshot(
    memory: &MemoryStore,
    turn_id: &str,
    user_text: &str,
    context_level: ContextLevel,
) -> Result<WorkingMemorySnapshot> {
    memory.hydrate_working_memory(&HydrateWorkingMemoryRequest {
        turn_id: turn_id.to_owned(),
        user_text: user_text.to_owned(),
        context_level,
        exclude_message_id: None,
        latest_turn_summary: None,
    })
}

fn recall_for_query(
    memory: &MemoryStore,
    snapshot: &WorkingMemorySnapshot,
    turn_id: &str,
    user_text: &str,
) -> Result<RecallBundle> {
    memory.recall(&RecallRequest {
        turn_id: turn_id.to_owned(),
        query_text: user_text.to_owned(),
        intent: "integration_test".to_owned(),
        context_level: ContextLevel::Medium,
        budget: RecallBudget {
            max_recent_messages: Some(ContextLevel::Medium.recent_turn_limit().saturating_mul(2)),
            max_durable_memories: Some(ContextLevel::Medium.semantic_limit()),
            include_cold_candidates: false,
        },
        working_memory: Some(snapshot.clone()),
        exclude_message_id: None,
    })
}

fn write_spotify_config(config_dir: &Path, base_url: &str) -> Result<()> {
    fs::write(
        config_dir.join("spotify_config.json"),
        serde_json::to_string_pretty(&json!({
            "client_id": "test-client",
            "client_secret": "test-secret",
            "redirect_uri": "babygervaise://spotify/callback",
            "scopes": [
                "user-read-private",
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

#[test]
fn runtime_config_resolver_loads_nested_profiles_and_skips_invalid_entries() {
    let resolver = RuntimeConfigResolver::resolve(json!({
        "nano": {
            "enabled": true,
            "provider": "gemini",
            "model": "gemini-nano"
        },
        "cloud": {
            "selected_profile": "gemini_flash",
            "profiles": {
                "gemini_flash": {
                    "provider": "gemini",
                    "model": "gemini-2.5-flash",
                    "api_key": "key",
                    "endpoint": "",
                    "enabled": true
                },
                "broken": {
                    "provider": "openai"
                }
            }
        }
    }));

    assert_eq!(
        resolver.config.cloud.selected_profile.as_deref(),
        Some("gemini_flash"),
    );
    assert!(resolver.config.cloud.profiles.contains_key("gemini_flash"));
    assert_eq!(resolver.invalid_profiles, vec!["broken"]);
}

#[test]
fn hgie_emits_nano_first_beat_before_follow_up_and_persists_one_message() -> Result<()> {
    let temp = tempdir()?;
    write_placeholder_spotify_config(temp.path())?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let sequenced = Arc::new(SequencedRuntime::default());
    let runtime = sequenced.clone() as Arc<dyn ModelRuntime>;
    let (_, prompt_config, app_config) = test_configs();
    let core = BabyGervaiseCore::with_model_runtime(
        temp.path(),
        temp.path(),
        callbacks.clone(),
        prompt_config,
        app_config,
        runtime.clone(),
    )?;

    core.submit_user_turn("turn-1", "Explain black holes", InputSource::Text)?;

    let events = callbacks.events.lock().unwrap();
    let event_types = events
        .iter()
        .map(|(event_type, _)| event_type.as_str())
        .collect::<Vec<_>>();
    let interaction_event_types = event_types
        .iter()
        .copied()
        .filter(|event_type| *event_type != "diagnostic_log")
        .collect::<Vec<_>>();
    assert_eq!(
        interaction_event_types.first().copied(),
        Some("assistant_started")
    );
    assert!(interaction_event_types.contains(&"assistant_chunk"));
    assert!(event_types.contains(&"diagnostic_log"));
    assert_eq!(
        sequenced.calls.lock().unwrap().clone(),
        vec!["first_beat", "cloud_reasoning"],
    );
    drop(events);

    let bootstrap = core.load_bootstrap_state()?;
    let assistant_messages = bootstrap
        .messages
        .iter()
        .filter(|message| message.role == "assistant")
        .collect::<Vec<_>>();
    assert_eq!(assistant_messages.len(), 1);
    assert_eq!(assistant_messages[0].content, "Sure -");

    Ok(())
}

#[test]
fn hgie_direct_local_turn_uses_single_local_reply_beat() -> Result<()> {
    let temp = tempdir()?;
    write_placeholder_spotify_config(temp.path())?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let recording = Arc::new(RecordingPlanRuntime {
        calls: Mutex::new(Vec::new()),
        nano_active: true,
        cloud_available: false,
        first_beat: "Sure -".to_owned(),
        nano_output: structured_reply("Hello back."),
        cloud_output: structured_reply("Cloud answer."),
    });
    let runtime = recording.clone() as Arc<dyn ModelRuntime>;
    let (_, prompt_config, app_config) = test_configs();
    let core = BabyGervaiseCore::with_model_runtime(
        temp.path(),
        temp.path(),
        callbacks,
        prompt_config,
        app_config,
        runtime.clone(),
    )?;

    core.submit_user_turn("turn-direct-local-1", "Hello there", InputSource::Text)?;

    let bootstrap = core.load_bootstrap_state()?;
    let assistant = bootstrap
        .messages
        .iter()
        .find(|message| message.role == "assistant")
        .expect("missing assistant message");
    assert_eq!(assistant.content, "Hello back.");
    assert_eq!(recording.calls.lock().unwrap().clone(), vec!["nano_reply"]);

    Ok(())
}

#[test]
fn hgie_hydrates_recall_and_persists_working_memory_across_turns() -> Result<()> {
    let temp = tempdir()?;
    write_placeholder_spotify_config(temp.path())?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let runtime = Arc::new(FixedRuntime {
        first_beat: "Sure -".to_owned(),
        reply_output: structured_reply("let me explain."),
    }) as Arc<dyn ModelRuntime>;
    let (_, prompt_config, app_config) = test_configs();
    let core = BabyGervaiseCore::with_model_runtime(
        temp.path(),
        temp.path(),
        callbacks,
        prompt_config,
        app_config,
        runtime,
    )?;

    core.submit_user_turn("turn-memory-1", "My cat is Mocha", InputSource::Text)?;

    let memory = MemoryStore::new(temp.path().join("baby_gervaise.sqlite3"), &app_config)?;
    let snapshot_after_first_turn = latest_working_snapshot(
        &memory,
        "inspect-memory-1",
        "Do you remember my cat is Mocha?",
        ContextLevel::Medium,
    )?;
    assert_eq!(snapshot_after_first_turn.turn_id, "turn-memory-1");
    assert!(snapshot_after_first_turn.focus_thread_key.is_some());
    assert!(!snapshot_after_first_turn.threads.is_empty());

    let recall_request = RecallRequest {
        turn_id: "inspect-recall-1".to_owned(),
        query_text: "Do you remember my cat is Mocha?".to_owned(),
        intent: "integration_test".to_owned(),
        context_level: ContextLevel::Medium,
        budget: RecallBudget {
            max_recent_messages: Some(ContextLevel::Medium.recent_turn_limit().saturating_mul(2)),
            max_durable_memories: Some(ContextLevel::Medium.semantic_limit()),
            include_cold_candidates: false,
        },
        working_memory: Some(snapshot_after_first_turn.clone()),
        exclude_message_id: None,
    };
    let recall_a = memory.recall(&recall_request)?;
    let recall_b = memory.recall(&recall_request)?;
    assert_eq!(
        recall_a.explanation.selected_message_ids,
        recall_b.explanation.selected_message_ids
    );
    assert_eq!(
        recall_a.explanation.selected_memory_ids,
        recall_b.explanation.selected_memory_ids
    );
    assert_eq!(
        recall_a.explanation.continuity,
        recall_b.explanation.continuity
    );
    assert_eq!(
        recall_a.explanation.continuity.mode,
        ContinuityMode::OnThread
    );
    assert_eq!(
        recall_a.explanation.continuity.focused_thread_key,
        snapshot_after_first_turn.focus_thread_key
    );
    assert!(!recall_a
        .explanation
        .continuity
        .selected_thread_message_ids
        .is_empty());
    assert!(recall_a.explanation.strong_hit);
    assert!(recall_a
        .warm_context
        .durable_memories
        .iter()
        .any(|memory| memory.text.to_lowercase().contains("cat")));

    core.submit_user_turn(
        "turn-memory-2",
        "Do you remember my cat is Mocha?",
        InputSource::Text,
    )?;

    let snapshot_after_second_turn = latest_working_snapshot(
        &memory,
        "inspect-memory-2",
        "Tell me about my cat Mocha",
        ContextLevel::Medium,
    )?;
    assert_eq!(snapshot_after_second_turn.turn_id, "turn-memory-2");
    assert_eq!(
        snapshot_after_second_turn.focus_thread_key,
        snapshot_after_first_turn.focus_thread_key
    );

    let overview = core.load_overview_state()?;
    let latest_turn = overview
        .diagnostics
        .turn_summaries
        .first()
        .expect("missing latest diagnostics turn");
    assert_eq!(latest_turn.plan_kind, "recall_nano");
    assert!(latest_turn.memory_used);

    let bootstrap = core.load_bootstrap_state()?;
    let assistant_messages = bootstrap
        .messages
        .iter()
        .filter(|message| message.role == "assistant")
        .collect::<Vec<_>>();
    assert_eq!(assistant_messages.len(), 2);
    assert_eq!(assistant_messages[1].content, "let me explain.");

    Ok(())
}

#[test]
fn hgie_refreshes_working_memory_on_topic_pivot_and_return() -> Result<()> {
    let temp = tempdir()?;
    write_placeholder_spotify_config(temp.path())?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let runtime = Arc::new(FixedRuntime {
        first_beat: "Sure -".to_owned(),
        reply_output: structured_reply("working through it."),
    }) as Arc<dyn ModelRuntime>;
    let (_, prompt_config, app_config) = test_configs();
    let core = BabyGervaiseCore::with_model_runtime(
        temp.path(),
        temp.path(),
        callbacks,
        prompt_config,
        app_config,
        runtime,
    )?;

    core.submit_user_turn(
        "turn-topic-1",
        "Let's design memory continuity for the assistant.",
        InputSource::Text,
    )?;

    let memory = MemoryStore::new(temp.path().join("baby_gervaise.sqlite3"), &app_config)?;
    let first_snapshot = latest_working_snapshot(
        &memory,
        "inspect-topic-1",
        "memory continuity",
        ContextLevel::Medium,
    )?;
    let first_key = first_snapshot
        .focus_thread_key
        .clone()
        .expect("missing initial focus thread");
    let pivot_recall = recall_for_query(
        &memory,
        &first_snapshot,
        "inspect-topic-pivot",
        "Switch to MCP transport boundaries and server auth.",
    )?;
    assert_eq!(
        pivot_recall.explanation.continuity.mode,
        ContinuityMode::Pivot
    );
    assert!(pivot_recall
        .explanation
        .continuity
        .selected_thread_message_ids
        .is_empty());

    core.submit_user_turn(
        "turn-topic-2",
        "Switch to MCP transport boundaries and server auth.",
        InputSource::Text,
    )?;

    let pivot_snapshot = latest_working_snapshot(
        &memory,
        "inspect-topic-2",
        "mcp boundaries",
        ContextLevel::Medium,
    )?;
    let second_key = pivot_snapshot
        .focus_thread_key
        .clone()
        .expect("missing pivot focus thread");
    assert_ne!(first_key, second_key);
    assert_eq!(
        pivot_snapshot
            .threads
            .iter()
            .find(|thread| thread.key == first_key)
            .expect("missing cooled first thread")
            .status,
        WorkingThreadStatus::Cooling
    );
    let return_recall = recall_for_query(
        &memory,
        &pivot_snapshot,
        "inspect-topic-return",
        "Back to memory continuity and interaction design.",
    )?;
    assert_eq!(
        return_recall.explanation.continuity.mode,
        ContinuityMode::Return
    );
    assert_eq!(
        return_recall
            .explanation
            .continuity
            .matched_thread_key
            .as_deref(),
        Some(first_key.as_str())
    );
    assert!(!return_recall
        .explanation
        .continuity
        .selected_thread_message_ids
        .is_empty());

    core.submit_user_turn(
        "turn-topic-3",
        "Back to memory continuity and interaction design.",
        InputSource::Text,
    )?;

    let return_snapshot = latest_working_snapshot(
        &memory,
        "inspect-topic-3",
        "memory continuity return",
        ContextLevel::Medium,
    )?;
    assert_eq!(
        return_snapshot.focus_thread_key.as_deref(),
        Some(first_key.as_str())
    );
    assert_eq!(
        return_snapshot
            .threads
            .iter()
            .find(|thread| thread.key == second_key)
            .expect("missing second thread after return")
            .status,
        WorkingThreadStatus::Cooling
    );

    Ok(())
}

#[test]
fn hgie_anchors_short_followups_to_open_loops() -> Result<()> {
    let temp = tempdir()?;
    write_placeholder_spotify_config(temp.path())?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let runtime = Arc::new(FixedRuntime {
        first_beat: "Sure -".to_owned(),
        reply_output: structured_reply("Let's inspect the loop."),
    }) as Arc<dyn ModelRuntime>;
    let (_, prompt_config, app_config) = test_configs();
    let core = BabyGervaiseCore::with_model_runtime(
        temp.path(),
        temp.path(),
        callbacks,
        prompt_config,
        app_config.clone(),
        runtime,
    )?;

    core.submit_user_turn(
        "turn-loop-1",
        "Why is the memory refresh stale?",
        InputSource::Text,
    )?;

    let memory = MemoryStore::new(temp.path().join("baby_gervaise.sqlite3"), &app_config)?;
    let first_snapshot = latest_working_snapshot(
        &memory,
        "inspect-loop-1",
        "Why is the memory refresh stale?",
        ContextLevel::Medium,
    )?;
    let first_key = first_snapshot
        .focus_thread_key
        .clone()
        .expect("missing loop focus thread");
    let loop_recall = recall_for_query(&memory, &first_snapshot, "inspect-loop-2", "and then")?;
    assert_eq!(
        loop_recall.explanation.continuity.mode,
        ContinuityMode::OpenLoop
    );
    assert!(loop_recall.explanation.continuity.open_loop_match);
    assert_eq!(
        loop_recall
            .explanation
            .continuity
            .matched_thread_key
            .as_deref(),
        Some(first_key.as_str())
    );

    core.submit_user_turn("turn-loop-2", "and then", InputSource::Text)?;

    let second_snapshot =
        latest_working_snapshot(&memory, "inspect-loop-3", "and then", ContextLevel::Medium)?;
    assert_eq!(
        second_snapshot.focus_thread_key.as_deref(),
        Some(first_key.as_str())
    );

    Ok(())
}

#[test]
fn hgie_first_turn_without_prior_snapshot_still_completes_cleanly() -> Result<()> {
    let temp = tempdir()?;
    write_placeholder_spotify_config(temp.path())?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let runtime = Arc::new(FixedRuntime {
        first_beat: "Sure -".to_owned(),
        reply_output: structured_reply("Hello back."),
    }) as Arc<dyn ModelRuntime>;
    let (_, prompt_config, app_config) = test_configs();
    let core = BabyGervaiseCore::with_model_runtime(
        temp.path(),
        temp.path(),
        callbacks,
        prompt_config,
        app_config.clone(),
        runtime,
    )?;

    core.submit_user_turn("turn-fresh-1", "Hello there", InputSource::Text)?;

    let bootstrap = core.load_bootstrap_state()?;
    assert_eq!(
        bootstrap
            .messages
            .iter()
            .filter(|message| message.role == "assistant")
            .count(),
        1
    );

    let memory = MemoryStore::new(temp.path().join("baby_gervaise.sqlite3"), &app_config)?;
    let snapshot = latest_working_snapshot(
        &memory,
        "inspect-fresh-1",
        "Hello there",
        ContextLevel::Medium,
    )?;
    assert_eq!(snapshot.turn_id, "turn-fresh-1");
    assert!(snapshot.focus_thread_key.is_some());

    Ok(())
}

#[test]
fn load_overview_state_splits_cloud_and_nano_usage_stats() -> Result<()> {
    let temp = tempdir()?;
    write_placeholder_spotify_config(temp.path())?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let runtime = Arc::new(SplitStatsRuntime) as Arc<dyn ModelRuntime>;
    let (_, prompt_config, app_config) = test_configs();
    let core = BabyGervaiseCore::with_model_runtime(
        temp.path(),
        temp.path(),
        callbacks.clone(),
        prompt_config,
        app_config,
        runtime,
    )?;

    core.submit_user_turn(
        "turn-overview-stats-1",
        "Explain black holes",
        InputSource::Text,
    )?;

    let overview = core.load_overview_state()?;
    let latest_turn = overview
        .diagnostics
        .turn_summaries
        .first()
        .expect("missing latest diagnostics turn");
    let spotify = overview
        .tools
        .catalog
        .iter()
        .find(|entry| entry.tool_id == "spotify")
        .expect("missing spotify tool overview");
    let events = callbacks.events.lock().expect("callback mutex poisoned");

    assert_eq!(overview.cloud_stats.calls, 1);
    assert_eq!(overview.cloud_stats.tokens_in, Some(11));
    assert_eq!(overview.cloud_stats.tokens_out, Some(7));
    assert_eq!(overview.cloud_stats.latency_avg_ms, Some(12));
    assert_eq!(overview.cloud_stats.latency_latest_ms, Some(12));
    assert_eq!(overview.nano_stats.calls, 1);
    assert_eq!(overview.nano_stats.tokens_in, None);
    assert_eq!(overview.nano_stats.tokens_out, None);
    assert_eq!(overview.nano_stats.latency_avg_ms, Some(9));
    assert_eq!(overview.nano_stats.latency_latest_ms, Some(9));
    assert_eq!(latest_turn.input_source, "text");
    assert!(!latest_turn.memory_used);
    assert!(!latest_turn.tool_consulted);
    assert!(!latest_turn.tool_used);
    assert!(latest_turn.nano_first_beat_used);
    assert!(latest_turn.cloud_escalated);
    assert_eq!(
        latest_turn.selected_cloud_profile.as_deref(),
        Some("compat")
    );
    assert_eq!(latest_turn.delivery_mode, "NANO_THEN_CLOUD");
    assert_eq!(latest_turn.final_route, "nano + cloud");
    assert_eq!(spotify.auth_state, "required_not_started");
    assert_eq!(spotify.next_step, "auth_required");
    assert!(overview
        .tools
        .available_tools
        .iter()
        .any(|tool| tool == "spotify"));
    assert!(overview
        .tools
        .integrated_tools
        .iter()
        .all(|tool| tool != "spotify"));
    assert!(events.iter().any(|(event_type, payload)| {
        if event_type != "diagnostic_log" {
            return false;
        }
        let payload = serde_json::from_str::<serde_json::Value>(payload).ok();
        payload
            .as_ref()
            .and_then(|value| value.get("subsystem"))
            .and_then(serde_json::Value::as_str)
            == Some("hgie")
            && payload
                .as_ref()
                .and_then(|value| value.get("message"))
                .and_then(serde_json::Value::as_str)
                == Some("turn route selected")
    }));

    Ok(())
}

#[test]
fn load_overview_state_returns_inactive_cloud_stats_when_no_cloud_is_selected() -> Result<()> {
    let temp = tempdir()?;
    write_placeholder_spotify_config(temp.path())?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let runtime = Arc::new(NoCloudSelectedRuntime) as Arc<dyn ModelRuntime>;
    let (_, prompt_config, app_config) = test_configs();
    let core = BabyGervaiseCore::with_model_runtime(
        temp.path(),
        temp.path(),
        callbacks,
        prompt_config,
        app_config,
        runtime,
    )?;

    let overview = core.load_overview_state()?;

    assert_eq!(overview.cloud_stats.calls, 0);
    assert_eq!(overview.cloud_stats.tokens_in, Some(0));
    assert_eq!(overview.cloud_stats.tokens_out, Some(0));
    assert_eq!(overview.cloud_stats.latency_avg_ms, None);
    assert_eq!(overview.cloud_stats.latency_latest_ms, None);

    Ok(())
}

#[test]
fn direct_turn_completes_when_only_cloud_is_available() -> Result<()> {
    let temp = tempdir()?;
    write_placeholder_spotify_config(temp.path())?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let recording = Arc::new(RecordingPlanRuntime {
        calls: Mutex::new(Vec::new()),
        nano_active: false,
        cloud_available: true,
        first_beat: "Sure -".to_owned(),
        nano_output: structured_reply("Local answer."),
        cloud_output: structured_reply("Cloud answer."),
    });
    let runtime = recording.clone() as Arc<dyn ModelRuntime>;
    let (_, prompt_config, app_config) = test_configs();
    let core = BabyGervaiseCore::with_model_runtime(
        temp.path(),
        temp.path(),
        callbacks,
        prompt_config,
        app_config,
        runtime,
    )?;

    core.submit_user_turn("turn-cloud-only-1", "Hello there", InputSource::Text)?;

    let bootstrap = core.load_bootstrap_state()?;
    let assistant = bootstrap
        .messages
        .iter()
        .find(|message| message.role == "assistant")
        .expect("missing assistant message");
    assert_eq!(assistant.content, "Cloud answer.");
    assert_eq!(
        recording.calls.lock().unwrap().clone(),
        vec!["cloud_reasoning"]
    );

    Ok(())
}

#[test]
fn exact_tool_turn_does_not_force_a_model_first_beat() -> Result<()> {
    let temp = tempdir()?;
    write_placeholder_spotify_config(temp.path())?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let recording = Arc::new(RecordingPlanRuntime {
        calls: Mutex::new(Vec::new()),
        nano_active: true,
        cloud_available: true,
        first_beat: "Sure -".to_owned(),
        nano_output: structured_reply("Local answer."),
        cloud_output: structured_reply("Cloud answer."),
    });
    let runtime = recording.clone() as Arc<dyn ModelRuntime>;
    let (_, prompt_config, app_config) = test_configs();
    let core = BabyGervaiseCore::with_model_runtime(
        temp.path(),
        temp.path(),
        callbacks,
        prompt_config,
        app_config,
        runtime,
    )?;

    core.submit_user_turn(
        "turn-tool-direct-1",
        "what is playing on spotify",
        InputSource::Text,
    )?;

    assert!(recording.calls.lock().unwrap().is_empty());

    Ok(())
}

#[test]
fn malformed_internal_payload_does_not_leak_into_assistant_chat() -> Result<()> {
    let temp = tempdir()?;
    write_placeholder_spotify_config(temp.path())?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let runtime = Arc::new(FixedRuntime {
        first_beat: "Hang on.".to_owned(),
        reply_output: r#"```json
{"assistant_reply":"raw","tool_request":
```"#
            .to_owned(),
    }) as Arc<dyn ModelRuntime>;
    let (_, prompt_config, app_config) = test_configs();
    let core = BabyGervaiseCore::with_model_runtime(
        temp.path(),
        temp.path(),
        callbacks,
        prompt_config,
        app_config,
        runtime,
    )?;

    core.submit_user_turn("turn-leak-1", "hello", InputSource::Text)?;

    let bootstrap = core.load_bootstrap_state()?;
    let assistant = bootstrap
        .messages
        .iter()
        .find(|message| message.role == "assistant")
        .expect("missing assistant message");

    assert_eq!(
        assistant.content,
        "Something needs attention before I can finish that."
    );
    assert!(!assistant.content.contains("assistant_reply"));
    assert!(!assistant.content.contains("```json"));

    Ok(())
}

#[test]
fn ambient_events_respect_hgie_cooldown() -> Result<()> {
    let temp = tempdir()?;
    write_placeholder_spotify_config(temp.path())?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let sequenced = Arc::new(SequencedRuntime::default());
    let runtime = sequenced as Arc<dyn ModelRuntime>;
    let (_, prompt_config, app_config) = test_configs();
    let core = BabyGervaiseCore::with_model_runtime(
        temp.path(),
        temp.path(),
        callbacks,
        prompt_config,
        app_config,
        runtime,
    )?;

    let first = core.submit_ambient_event(
        "ambient-1",
        "capability_available",
        json!({"capability":"spotify"}),
    )?;
    let second = core.submit_ambient_event(
        "ambient-2",
        "capability_available",
        json!({"capability":"spotify"}),
    )?;

    assert!(first.is_some());
    assert!(second.is_none());
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
                "user-read-private",
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
            "expires_at": "2099-01-01T00:00:00Z",
            "granted_scopes": [
                "user-read-private",
                "user-read-playback-state",
                "user-modify-playback-state",
                "user-read-currently-playing"
            ]
        }))?,
    )?;
    Ok(())
}

fn write_legacy_spotify_token_cache(app_files_dir: &Path) -> Result<()> {
    fs::write(
        app_files_dir.join("spotify_tokens.json"),
        serde_json::to_string_pretty(&json!({
            "access_token": "cached-token",
            "refresh_token": "cached-refresh-token",
            "expires_at": "2099-01-01T00:00:00Z",
            "granted_scopes": [
                "user-read-playback-state",
                "user-modify-playback-state",
                "user-read-currently-playing"
            ]
        }))?,
    )?;
    Ok(())
}

fn write_spotify_token_cache_with_account(app_files_dir: &Path) -> Result<()> {
    fs::write(
        app_files_dir.join("spotify_tokens.json"),
        serde_json::to_string_pretty(&json!({
            "access_token": "cached-token",
            "refresh_token": "cached-refresh-token",
            "expires_at": "2099-01-01T00:00:00Z",
            "granted_scopes": [
                "user-read-private",
                "user-read-playback-state",
                "user-modify-playback-state",
                "user-read-currently-playing"
            ],
            "account": {
                "display_name": "Paul Zammit",
                "spotify_user_id": "spotify-user-123"
            },
            "last_authenticated_at": "2026-03-13T09:00:00Z",
            "last_refresh_at": "2026-03-13T09:05:00Z"
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
        MockHttpResponse::json(json!({
            "devices": [
                {
                    "id": "device-1",
                    "name": "Studio",
                    "type": "Computer",
                    "is_active": true,
                    "is_restricted": false,
                    "is_private_session": false,
                    "supports_volume": true,
                    "volume_percent": 58
                }
            ]
        })),
        MockHttpResponse::empty(204),
        MockHttpResponse::json(json!({
            "is_playing": true,
            "device": {
                "id": "device-1",
                "name": "Studio",
                "type": "Computer",
                "is_active": true,
                "is_restricted": false,
                "is_private_session": false,
                "supports_volume": true,
                "volume_percent": 58
            },
            "item": {
                "name": "Blue in Green",
                "uri": "spotify:track:blue-in-green",
                "album": { "name": "Kind of Blue" },
                "artists": [{ "name": "Miles Davis" }]
            }
        })),
        MockHttpResponse::json(json!({
            "devices": [
                {
                    "id": "device-1",
                    "name": "Studio",
                    "type": "Computer",
                    "is_active": true,
                    "is_restricted": false,
                    "is_private_session": false,
                    "supports_volume": true,
                    "volume_percent": 58
                }
            ]
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
    assert_eq!(play.result_json["track"]["name"], "Blue in Green");
    assert_eq!(play.result_json["track"]["artists"][0], "Miles Davis");
    assert_eq!(play.result_json["track"]["album"], "Kind of Blue");
    assert_eq!(play.result_json["target_device"]["name"], "Studio");
    assert_eq!(set_volume.result_json["volume_percent"], 30);
    assert_eq!(set_volume.result_json["target_device"]["id"], "device-1");

    let request_lines = server.request_lines();
    let request_bodies = server.request_bodies();
    assert!(request_lines
        .iter()
        .any(|line| line.starts_with("GET /v1/search?")));
    assert!(request_lines
        .iter()
        .any(|line| line == "GET /v1/me/player/devices HTTP/1.1"));
    assert!(request_lines
        .iter()
        .any(|line| line.starts_with("PUT /v1/me/player/play")));
    assert!(request_lines
        .iter()
        .any(|line| line.starts_with("PUT /v1/me/player/volume?")));
    assert!(request_lines
        .iter()
        .any(|line| line.contains("volume_percent=30")));
    assert!(request_lines
        .iter()
        .any(|line| line.contains("device_id=device-1")));
    assert!(request_bodies
        .iter()
        .any(|body| body.contains("spotify:track:blue-in-green")));

    Ok(())
}

#[test]
fn tool_executor_transfers_fallback_device_before_playing() -> Result<()> {
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
        MockHttpResponse::json(json!({
            "devices": [
                {
                    "id": "device-2",
                    "name": "MacBook Pro",
                    "type": "Computer",
                    "is_active": false,
                    "is_restricted": false,
                    "is_private_session": false,
                    "supports_volume": true,
                    "volume_percent": 40
                }
            ]
        })),
        MockHttpResponse::empty(204),
        MockHttpResponse::empty(204),
        MockHttpResponse::json(json!({
            "is_playing": true,
            "device": {
                "id": "device-2",
                "name": "MacBook Pro",
                "type": "Computer",
                "is_active": true,
                "is_restricted": false,
                "is_private_session": false,
                "supports_volume": true,
                "volume_percent": 40
            },
            "item": {
                "name": "Blue in Green",
                "uri": "spotify:track:blue-in-green",
                "album": { "name": "Kind of Blue" },
                "artists": [{ "name": "Miles Davis" }]
            }
        })),
    ])?;

    let temp = tempdir()?;
    let (_, _, app_config) = test_configs();
    write_spotify_config(temp.path(), &server.base_url)?;
    write_spotify_token_cache(temp.path())?;

    let memory = MemoryStore::new(temp.path().join("baby_gervaise.sqlite3"), &app_config)?;
    let tools = ToolExecutor::with_spotify(memory, temp.path(), temp.path());

    let play = tools.execute_named("spotify", "play", json!({ "query": "Miles Davis" }))?;

    assert_eq!(play.result_json["status"], "success");
    assert_eq!(play.result_json["target_device"]["name"], "MacBook Pro");

    let request_lines = server.request_lines();
    let transfer_index = request_lines
        .iter()
        .position(|line| line == "PUT /v1/me/player HTTP/1.1")
        .expect("expected transfer playback request");
    let play_index = request_lines
        .iter()
        .position(|line| line.starts_with("PUT /v1/me/player/play?"))
        .expect("expected play request");
    assert!(transfer_index < play_index);

    let request_bodies = server.request_bodies();
    assert!(request_bodies
        .iter()
        .any(|body| body.contains("\"device_ids\":[\"device-2\"]")));
    assert!(request_bodies
        .iter()
        .any(|body| body.contains("spotify:track:blue-in-green")));

    Ok(())
}

#[test]
fn spotify_playback_403_is_normalized_as_playback_forbidden() -> Result<()> {
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
        MockHttpResponse::json(json!({
            "devices": [
                {
                    "id": "device-1",
                    "name": "Studio",
                    "type": "Computer",
                    "is_active": true,
                    "is_restricted": false,
                    "is_private_session": false,
                    "supports_volume": true,
                    "volume_percent": 58
                }
            ]
        })),
        MockHttpResponse {
            status: 403,
            body: Some(
                json!({
                    "error": {
                        "status": 403,
                        "message": "Playback forbidden"
                    }
                })
                .to_string(),
            ),
        },
    ])?;

    let temp = tempdir()?;
    let (_, _, app_config) = test_configs();
    write_spotify_config(temp.path(), &server.base_url)?;
    write_spotify_token_cache(temp.path())?;

    let memory = MemoryStore::new(temp.path().join("baby_gervaise.sqlite3"), &app_config)?;
    let tools = ToolExecutor::with_spotify(memory, temp.path(), temp.path());

    let play = tools.execute_named("spotify", "play", json!({ "query": "Miles Davis" }))?;

    assert_eq!(play.result_json["status"], "forbidden");
    assert_eq!(play.result_json["reason"], "playback_forbidden");
    assert_eq!(play.result_json["code"], 403);

    Ok(())
}

#[test]
fn spotify_playback_403_is_normalized_as_premium_required() -> Result<()> {
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
        MockHttpResponse::json(json!({
            "devices": [
                {
                    "id": "device-1",
                    "name": "Studio",
                    "type": "Computer",
                    "is_active": true,
                    "is_restricted": false,
                    "is_private_session": false,
                    "supports_volume": true,
                    "volume_percent": 58
                }
            ]
        })),
        MockHttpResponse {
            status: 403,
            body: Some(
                json!({
                    "error": {
                        "status": 403,
                        "message": "Premium required"
                    }
                })
                .to_string(),
            ),
        },
    ])?;

    let temp = tempdir()?;
    let (_, _, app_config) = test_configs();
    write_spotify_config(temp.path(), &server.base_url)?;
    write_spotify_token_cache(temp.path())?;

    let memory = MemoryStore::new(temp.path().join("baby_gervaise.sqlite3"), &app_config)?;
    let tools = ToolExecutor::with_spotify(memory, temp.path(), temp.path());

    let play = tools.execute_named("spotify", "play", json!({ "query": "Miles Davis" }))?;

    assert_eq!(play.result_json["status"], "forbidden");
    assert_eq!(play.result_json["reason"], "premium_required");
    assert_eq!(play.result_json["code"], 403);

    Ok(())
}

#[test]
fn spotify_tool_reports_connection_state_and_disconnects_cleanly() -> Result<()> {
    let temp = tempdir()?;
    let (_, _, app_config) = test_configs();
    write_placeholder_spotify_config(temp.path())?;
    write_spotify_token_cache_with_account(temp.path())?;

    let memory = MemoryStore::new(temp.path().join("baby_gervaise.sqlite3"), &app_config)?;
    let tools = ToolExecutor::with_spotify(memory, temp.path(), temp.path());

    let connection = tools.execute_named("spotify", "get_connection_state", json!({}))?;
    assert_eq!(connection.result_json["status"], "connected");
    assert_eq!(connection.result_json["configured"], true);
    assert_eq!(connection.result_json["connected"], true);
    assert_eq!(
        connection.result_json["account_display_name"],
        "Paul Zammit"
    );
    assert_eq!(
        connection.result_json["spotify_user_id"],
        "spotify-user-123"
    );
    assert_eq!(connection.result_json["token_status"], "valid");

    let disconnect = tools.execute_named("spotify", "disconnect", json!({}))?;
    assert_eq!(disconnect.result_json["status"], "success");
    assert_eq!(disconnect.result_json["action"], "disconnect");
    assert_eq!(
        disconnect.summary,
        "Spotify has been disconnected. You can sign in again whenever you want."
    );

    let post_disconnect = tools.execute_named("spotify", "get_connection_state", json!({}))?;
    assert_eq!(post_disconnect.result_json["status"], "disconnected");
    assert_eq!(post_disconnect.result_json["connected"], false);
    assert!(!temp.path().join("spotify_tokens.json").exists());

    Ok(())
}

#[test]
fn core_overview_includes_spotify_state_without_prior_spotify_turns() -> Result<()> {
    let temp = tempdir()?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let (model_config, prompt_config, app_config) = test_configs();
    write_placeholder_spotify_config(temp.path())?;
    write_spotify_token_cache_with_account(temp.path())?;
    let model = Arc::new(StaticModel {
        response: r#"{"assistant_reply":"Hi.","tool_request":null,"memory_candidates":[]}"#
            .to_owned(),
    });

    let core = BabyGervaiseCore::with_model_gateway(
        temp.path(),
        temp.path(),
        callbacks,
        model_config,
        prompt_config,
        app_config,
        model,
    )?;

    let overview = core.load_overview_state()?;
    let spotify_state = overview
        .tool_states
        .get("spotify")
        .expect("missing spotify overview state");

    assert_eq!(spotify_state["configured"], true);
    assert_eq!(spotify_state["connected"], true);
    assert_eq!(spotify_state["account_display_name"], "Paul Zammit");
    assert!(overview
        .tools
        .available_tools
        .iter()
        .any(|tool| tool == "spotify"));
    assert!(overview
        .tools
        .integrated_tools
        .iter()
        .any(|tool| tool == "spotify"));
    assert!(overview.tools.catalog.iter().any(|entry| {
        entry.tool_id == "spotify"
            && entry.integrated
            && entry.account_label.as_deref() == Some("Paul Zammit")
            && entry.health_state == "healthy"
    }));

    Ok(())
}

#[test]
fn spotify_connection_state_bootstraps_missing_account_from_profile_endpoint() -> Result<()> {
    let server = spawn_mock_http_server(vec![
        MockHttpResponse::json(json!({
            "id": "spotify-user-123",
            "display_name": "Paul Zammit"
        })),
        MockHttpResponse::json(json!({
            "devices": [
                {
                    "id": "device-1",
                    "name": "Studio",
                    "type": "Computer",
                    "is_active": true,
                    "is_restricted": false,
                    "is_private_session": false,
                    "supports_volume": true,
                    "volume_percent": 58
                }
            ]
        })),
        MockHttpResponse::empty(204),
    ])?;

    let temp = tempdir()?;
    let (_, _, app_config) = test_configs();
    write_spotify_config(temp.path(), &server.base_url)?;
    write_spotify_token_cache(temp.path())?;

    let memory = MemoryStore::new(temp.path().join("baby_gervaise.sqlite3"), &app_config)?;
    let tools = ToolExecutor::with_spotify(memory, temp.path(), temp.path());

    let connection = tools.execute_named("spotify", "get_connection_state", json!({}))?;
    let request_lines = server.request_lines();

    assert_eq!(connection.result_json["status"], "connected");
    assert_eq!(
        connection.result_json["account_display_name"],
        "Paul Zammit"
    );
    assert_eq!(
        connection.result_json["spotify_user_id"],
        "spotify-user-123"
    );
    assert_eq!(connection.result_json["capability_status"], "connected");
    assert!(request_lines
        .iter()
        .any(|line| line == "GET /v1/me HTTP/1.1"));
    assert!(request_lines
        .iter()
        .any(|line| line == "GET /v1/me/player/devices HTTP/1.1"));

    Ok(())
}

#[test]
fn spotify_connection_state_reports_invalid_scope_when_profile_scope_is_missing() -> Result<()> {
    let temp = tempdir()?;
    let (_, _, app_config) = test_configs();
    write_placeholder_spotify_config(temp.path())?;
    write_legacy_spotify_token_cache(temp.path())?;

    let memory = MemoryStore::new(temp.path().join("baby_gervaise.sqlite3"), &app_config)?;
    let tools = ToolExecutor::with_spotify(memory, temp.path(), temp.path());

    let connection = tools.execute_named("spotify", "get_connection_state", json!({}))?;

    assert_eq!(connection.result_json["status"], "connected");
    assert_eq!(connection.result_json["connected"], true);
    assert_eq!(connection.result_json["capability_status"], "invalid_scope");
    assert!(connection.result_json["message"]
        .as_str()
        .unwrap_or_default()
        .contains("user-read-private"));

    Ok(())
}

#[test]
fn core_executes_direct_spotify_disconnect_and_logs_it() -> Result<()> {
    let temp = tempdir()?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let (model_config, prompt_config, app_config) = test_configs();
    write_placeholder_spotify_config(temp.path())?;
    write_spotify_token_cache_with_account(temp.path())?;
    let model = Arc::new(StaticModel {
        response: r#"{"assistant_reply":"Hi.","tool_request":null,"memory_candidates":[]}"#
            .to_owned(),
    });

    let core = BabyGervaiseCore::with_model_gateway(
        temp.path(),
        temp.path(),
        callbacks,
        model_config,
        prompt_config,
        app_config,
        model,
    )?;

    let result = core.execute_tool_action("spotify", "disconnect", json!({}))?;
    let overview = core.load_overview_state()?;
    let spotify_state = overview
        .tool_states
        .get("spotify")
        .expect("missing spotify overview state");
    let latest_log = overview
        .recent_tool_logs
        .first()
        .expect("missing spotify tool log");

    assert_eq!(
        result.summary,
        "Spotify has been disconnected. You can sign in again whenever you want."
    );
    assert_eq!(spotify_state["connected"], false);
    assert_eq!(spotify_state["token_status"], "missing");
    assert_eq!(latest_log.tool_name, "spotify");
    assert_eq!(latest_log.action, "disconnect");
    assert!(latest_log.success);
    assert!(!temp.path().join("spotify_tokens.json").exists());

    Ok(())
}

#[test]
fn core_surfaces_spotify_auth_required_from_tool() -> Result<()> {
    let temp = tempdir()?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let (model_config, prompt_config, app_config) = test_configs();
    write_placeholder_spotify_config(temp.path())?;
    let model = Arc::new(StaticModel {
        response: r#"{
            "assistant_reply": "",
            "tool_request": {
                "tool": "spotify",
                "action": "play",
                "arguments": { "query": "Blue in Green" }
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

    core.submit_user_turn(
        "turn-auth-required-1",
        "Play Blue in Green",
        InputSource::Text,
    )?;

    let bootstrap = core.load_bootstrap_state()?;
    let overview = core.load_overview_state()?;
    let events = callbacks.events.lock().expect("callback mutex poisoned");

    assert!(bootstrap.messages.iter().any(|message| {
        message.role == "assistant" && message.content.contains("Spotify authentication required")
    }));
    assert_eq!(overview.system_stats.tool_calls, 1);
    assert!(!events
        .iter()
        .any(|(event_type, _)| event_type == "open_external_url"));

    Ok(())
}

#[test]
fn core_starts_spotify_browser_flow_when_model_requests_start_auth() -> Result<()> {
    let temp = tempdir()?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let (model_config, prompt_config, app_config) = test_configs();
    write_placeholder_spotify_config(temp.path())?;
    let model = Arc::new(StaticModel {
        response: r#"{
            "assistant_reply": "",
            "tool_request": {
                "tool": "spotify",
                "action": "start_auth",
                "arguments": {}
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

    core.submit_user_turn("turn-auth-start-1", "Connect Spotify", InputSource::Text)?;

    let bootstrap = core.load_bootstrap_state()?;
    let overview = core.load_overview_state()?;
    let events = callbacks.events.lock().expect("callback mutex poisoned");

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
    let server = spawn_mock_http_server(vec![
        MockHttpResponse::json(json!({
            "access_token": "new-access-token",
            "refresh_token": "new-refresh-token",
            "expires_in": 3600,
            "scope": "user-read-private user-read-playback-state user-modify-playback-state user-read-currently-playing"
        })),
        MockHttpResponse::json(json!({
            "id": "spotify-user-123",
            "display_name": "Paul Zammit"
        })),
        MockHttpResponse::json(json!({
            "devices": [
                {
                    "id": "device-1",
                    "name": "Studio",
                    "type": "Computer",
                    "is_active": true,
                    "is_restricted": false,
                    "is_private_session": false,
                    "supports_volume": true,
                    "volume_percent": 58
                }
            ]
        })),
        MockHttpResponse::json(json!({
            "is_playing": false,
            "device": {
                "id": "device-1",
                "name": "Studio",
                "type": "Computer",
                "is_active": true,
                "is_restricted": false,
                "is_private_session": false,
                "supports_volume": true,
                "volume_percent": 58
            }
        })),
    ])?;

    let temp = tempdir()?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let (model_config, prompt_config, app_config) = test_configs();
    write_spotify_config(temp.path(), &server.base_url)?;
    let model = Arc::new(StaticModel {
        response: r#"{
            "assistant_reply": "",
            "tool_request": {
                "tool": "spotify",
                "action": "start_auth",
                "arguments": {}
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

    core.submit_user_turn("turn-auth-callback-1", "Connect Spotify", InputSource::Text)?;

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
    let overview = core.load_overview_state()?;
    let token_cache = fs::read_to_string(temp.path().join("spotify_tokens.json"))?;
    let events = callbacks.events.lock().expect("callback mutex poisoned");
    let request_lines = server.request_lines();

    assert!(bootstrap.messages.iter().any(|message| {
        message.role == "assistant"
            && message
                .content
                .contains("Spotify is connected. What would you like to listen to?")
    }));
    assert!(token_cache.contains("new-refresh-token"));
    assert!(token_cache.contains("spotify-user-123"));
    assert!(token_cache.contains("Paul Zammit"));
    assert!(request_lines
        .iter()
        .any(|line| line == "GET /v1/me HTTP/1.1"));
    assert!(request_lines
        .iter()
        .any(|line| line == "GET /v1/me/player/devices HTTP/1.1"));
    assert!(request_lines
        .iter()
        .any(|line| line == "GET /v1/me/player HTTP/1.1"));
    assert_eq!(
        overview.tool_states["spotify"]["account_display_name"],
        "Paul Zammit"
    );
    assert_eq!(
        overview.tool_states["spotify"]["capability_status"],
        "connected"
    );
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
    assert_eq!(bootstrap.messages[1].role, "tool");
    assert_eq!(
        bootstrap.messages[1].content_type,
        MessageContentType::ToolResult
    );
    assert!(bootstrap.messages[1]
        .display_json
        .as_deref()
        .is_some_and(|value| value.contains("\"tool\": \"hue\"")));
    assert_eq!(bootstrap.messages[2].role, "assistant");
    assert_eq!(overview.system_stats.total_interactions, 1);
    assert_eq!(overview.system_stats.tool_calls, 1);
    assert_eq!(overview.memory_stats.stored_memories, 0);
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
fn core_ignores_model_memory_candidates_for_durable_storage() -> Result<()> {
    let temp = tempdir()?;
    let callbacks = Arc::new(RecordingCallbacks::default());
    let (model_config, prompt_config, app_config) = test_configs();
    write_placeholder_spotify_config(temp.path())?;
    let model = Arc::new(StaticModel {
        response: r#"{
            "assistant_reply": "Mocha is arriving on Tuesday evening.",
            "tool_request": null,
            "memory_candidates": [
                { "kind": "fact", "text": "Mocha arrives on Tuesday evening.", "salience": 0.92 },
                { "kind": "summary", "text": "{\"assistant_reply\":\"raw\"}", "salience": 0.95 }
            ]
        }"#
        .to_owned(),
    });

    let core = BabyGervaiseCore::with_model_gateway(
        temp.path(),
        temp.path(),
        callbacks,
        model_config,
        prompt_config,
        app_config,
        model,
    )?;

    core.submit_user_turn(
        "turn-memory-1",
        "When is Mocha arriving?",
        InputSource::Text,
    )?;

    let overview = core.load_overview_state()?;
    let memory = MemoryStore::new(temp.path().join("baby_gervaise.sqlite3"), &test_configs().2)?;
    let retrieved = memory.semantic_search("Mocha arriving", ContextLevel::Medium)?;

    assert_eq!(overview.memory_stats.stored_memories, 0);
    assert!(retrieved.is_empty());

    Ok(())
}

#[test]
fn core_executes_spotify_current_playback_via_hgie() -> Result<()> {
    let server = spawn_mock_http_server(vec![MockHttpResponse::json(json!({
        "is_playing": true,
        "device": {
            "id": "device-1",
            "name": "Studio",
            "type": "Computer",
            "is_active": true,
            "is_restricted": false,
            "is_private_session": false,
            "supports_volume": true,
            "volume_percent": 30
        },
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
        message.role == "tool"
            && message.content_type == MessageContentType::ToolResult
            && message.content.contains("Blue in Green")
            && message
                .display_json
                .as_deref()
                .is_some_and(|value| value.contains("\"tool\": \"spotify\""))
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
fn spotify_tool_returns_unavailable_when_config_is_invalid() -> Result<()> {
    let temp = tempdir()?;
    let (_, _, app_config) = test_configs();
    fs::write(temp.path().join("spotify_config.json"), "{ invalid json")?;

    let memory = MemoryStore::new(temp.path().join("baby_gervaise.sqlite3"), &app_config)?;
    let tools = ToolExecutor::with_spotify(memory, temp.path(), temp.path());
    let result = tools.execute_named("spotify", "capability_status", json!({}))?;

    assert_eq!(result.result_json["status"], "unavailable");
    assert_eq!(result.result_json["reason"], "bad_config");
    assert!(result.summary.contains("Spotify"));

    Ok(())
}

#[test]
fn spotify_connection_state_is_available_even_when_config_is_invalid() -> Result<()> {
    let temp = tempdir()?;
    let (_, _, app_config) = test_configs();
    fs::write(temp.path().join("spotify_config.json"), "{ invalid json")?;
    write_spotify_token_cache_with_account(temp.path())?;

    let memory = MemoryStore::new(temp.path().join("baby_gervaise.sqlite3"), &app_config)?;
    let tools = ToolExecutor::with_spotify(memory, temp.path(), temp.path());

    let connection = tools.execute_named("spotify", "get_connection_state", json!({}))?;
    assert_eq!(connection.result_json["status"], "error");
    assert_eq!(connection.result_json["reason"], "bad_config");
    assert_eq!(
        connection.result_json["account_display_name"],
        "Paul Zammit"
    );

    let disconnect = tools.execute_named("spotify", "disconnect", json!({}))?;
    assert_eq!(disconnect.result_json["status"], "success");
    assert!(!temp.path().join("spotify_tokens.json").exists());

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
