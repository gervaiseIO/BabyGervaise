pub mod hgie;
pub mod logging;
pub mod memory;
pub mod model;
pub mod model_runtime;
pub mod prompt_translator;
pub mod tools;

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use hgie::HgieEngine;
use logging::ToolLogEntry;
use model::ModelGateway;
use model_runtime::{
    DefaultModelRuntime, DisabledNanoAdapter, GatewayCompatRuntime, ModelRuntime,
    RuntimeConfigResolver,
};
use prompt_translator::PromptTranslator;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::logging::OverviewSnapshot;
use crate::memory::MemoryStore;
use crate::tools::{ToolExecutionResult, ToolExecutor};

#[cfg(target_os = "android")]
use std::sync::{LazyLock, Mutex};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ContextLevel {
    Low,
    Medium,
    High,
}

impl ContextLevel {
    pub fn recent_turn_limit(self) -> usize {
        match self {
            Self::Low => 4,
            Self::Medium => 8,
            Self::High => 16,
        }
    }

    pub fn semantic_limit(self) -> usize {
        match self {
            Self::Low => 2,
            Self::Medium => 5,
            Self::High => 8,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InputSource {
    Text,
    Voice,
}

impl InputSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Voice => "voice",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "voice" => Self::Voice,
            _ => Self::Text,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageContentType {
    PlainText,
    ToolResult,
}

impl MessageContentType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PlainText => "plain_text",
            Self::ToolResult => "tool_result",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "tool_result" => Self::ToolResult,
            _ => Self::PlainText,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub turn_id: String,
    pub input_source: InputSource,
    pub created_at: String,
    #[serde(default = "default_message_content_type")]
    pub content_type: MessageContentType,
    #[serde(default)]
    pub display_json: Option<String>,
    #[serde(default)]
    pub visible_summary: Option<String>,
}

fn default_message_content_type() -> MessageContentType {
    MessageContentType::PlainText
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapState {
    pub previous_context: ContextLevel,
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelReasoningConfig {
    pub effort: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub provider: String,
    pub api_key: String,
    pub model: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_stream_enabled")]
    pub stream: bool,
    #[serde(default)]
    pub reasoning: Option<ModelReasoningConfig>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

impl ModelConfig {
    pub fn with_defaults(mut self, profile_id: &str) -> Self {
        if self.endpoint.trim().is_empty() {
            self.endpoint = match self.provider.to_lowercase().as_str() {
                "openai" => "https://api.openai.com/v1/chat/completions".to_owned(),
                "gemini" => format!(
                    "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
                    self.model
                ),
                _ => self.endpoint,
            };
        }
        if self.label.is_none() {
            self.label = Some(self.display_label(profile_id));
        }
        self
    }

    pub fn display_label(&self, profile_id: &str) -> String {
        self.label.clone().unwrap_or_else(|| {
            profile_id
                .split('_')
                .map(|part| {
                    let mut chars = part.chars();
                    match chars.next() {
                        Some(first) => {
                            format!("{}{}", first.to_uppercase(), chars.as_str())
                        }
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
    }

    pub fn has_usable_api_key(&self) -> bool {
        let api_key = self.api_key.trim();
        !api_key.is_empty() && !Self::api_key_looks_placeholder(api_key)
    }

    fn api_key_looks_placeholder(api_key: &str) -> bool {
        let normalized = api_key.trim().to_ascii_uppercase();
        normalized == "YOUR_KEY"
            || (normalized.starts_with("YOUR_") && normalized.ends_with("_KEY"))
            || normalized.contains("PLACEHOLDER")
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AppConfig {
    pub default_previous_context: ContextLevel,
    pub vector_dimensions: usize,
    pub memory_salience_threshold: f32,
    pub stream_chunk_size: usize,
    pub max_recent_messages_per_turn: usize,
    pub max_model_logs: usize,
    #[serde(default = "default_idle_resume_threshold_seconds")]
    pub idle_resume_threshold_seconds: u64,
    #[serde(default = "default_ambient_cooldown_seconds")]
    pub ambient_cooldown_seconds: u64,
}

fn default_stream_enabled() -> bool {
    true
}

fn default_timeout_ms() -> u64 {
    45_000
}

fn default_enabled() -> bool {
    true
}

fn default_idle_resume_threshold_seconds() -> u64 {
    15 * 60
}

fn default_ambient_cooldown_seconds() -> u64 {
    10 * 60
}

pub trait CoreCallbacks: Send + Sync {
    fn emit(&self, event_type: &str, payload_json: String);
}

pub struct NoopCallbacks;

impl CoreCallbacks for NoopCallbacks {
    fn emit(&self, _event_type: &str, _payload_json: String) {}
}

pub struct BabyGervaiseCore {
    memory: MemoryStore,
    tools: ToolExecutor,
    engine: HgieEngine,
    runtime: Arc<dyn ModelRuntime>,
    callbacks: Arc<dyn CoreCallbacks>,
    app_config: AppConfig,
}

impl BabyGervaiseCore {
    pub fn init(
        app_files_dir: impl AsRef<Path>,
        asset_config_dir: impl AsRef<Path>,
        callbacks: Arc<dyn CoreCallbacks>,
    ) -> Result<Self> {
        Self::init_with_nano(
            app_files_dir,
            asset_config_dir,
            callbacks,
            Arc::new(DisabledNanoAdapter::new(
                "Nano is unavailable in this runtime.",
            )),
        )
    }

    pub fn init_with_nano(
        app_files_dir: impl AsRef<Path>,
        asset_config_dir: impl AsRef<Path>,
        callbacks: Arc<dyn CoreCallbacks>,
        nano: Arc<dyn model_runtime::NanoAdapter>,
    ) -> Result<Self> {
        let runtime_config = RuntimeConfigResolver::load(asset_config_dir.as_ref())?;
        let prompt_config =
            load_config::<PromptConfig>(asset_config_dir.as_ref(), "prompt_config.json")?;
        let app_config = load_config::<AppConfig>(asset_config_dir.as_ref(), "app_config.json")?;
        let runtime = Arc::new(DefaultModelRuntime::from_resolver(runtime_config, nano)?)
            as Arc<dyn ModelRuntime>;
        Self::with_model_runtime(
            app_files_dir,
            asset_config_dir,
            callbacks,
            prompt_config,
            app_config,
            runtime,
        )
    }

    pub fn with_model_gateway(
        app_files_dir: impl AsRef<Path>,
        asset_config_dir: impl AsRef<Path>,
        callbacks: Arc<dyn CoreCallbacks>,
        model_config: ModelConfig,
        prompt_config: PromptConfig,
        app_config: AppConfig,
        model: Arc<dyn ModelGateway>,
    ) -> Result<Self> {
        let runtime = Arc::new(GatewayCompatRuntime::new(
            model_config.display_label("compat"),
            model,
        )) as Arc<dyn ModelRuntime>;
        Self::with_model_runtime(
            app_files_dir,
            asset_config_dir,
            callbacks,
            prompt_config,
            app_config,
            runtime,
        )
    }

    pub fn with_model_runtime(
        app_files_dir: impl AsRef<Path>,
        asset_config_dir: impl AsRef<Path>,
        callbacks: Arc<dyn CoreCallbacks>,
        prompt_config: PromptConfig,
        app_config: AppConfig,
        runtime: Arc<dyn ModelRuntime>,
    ) -> Result<Self> {
        let db_path = app_files_dir.as_ref().join("baby_gervaise.sqlite3");
        let memory = MemoryStore::new(db_path, &app_config)?;
        if let Some(default_profile) = runtime
            .selected_cloud_profile_id()
            .or_else(|| runtime.default_selected_cloud_profile_id())
        {
            let selected = memory.get_selected_cloud_profile().unwrap_or_else(|_| None);
            let next_profile = selected.unwrap_or(default_profile);
            let _ = runtime.set_selected_cloud_profile(&next_profile);
            let _ = memory.set_selected_cloud_profile(&next_profile);
        }
        let tools = ToolExecutor::with_spotify(
            memory.clone(),
            app_files_dir.as_ref(),
            asset_config_dir.as_ref(),
        );
        let engine = HgieEngine::new(
            memory.clone(),
            tools.clone(),
            runtime.clone(),
            PromptTranslator::new(prompt_config),
            app_config,
        );

        Ok(Self {
            memory,
            tools,
            engine,
            runtime,
            callbacks,
            app_config,
        })
    }

    pub fn submit_user_turn(
        &self,
        turn_id: &str,
        text: &str,
        input_source: InputSource,
    ) -> Result<()> {
        self.engine
            .execute_turn(turn_id, text, input_source, self.callbacks.as_ref())?;
        Ok(())
    }

    pub fn handle_spotify_auth_callback(&self, turn_id: &str, callback_url: &str) -> Result<()> {
        self.handle_tool_auth_callback("spotify", turn_id, callback_url)
    }

    pub fn load_bootstrap_state(&self) -> Result<BootstrapState> {
        self.memory
            .load_bootstrap_state(self.app_config.default_previous_context)
    }

    pub fn load_overview_state(&self) -> Result<OverviewSnapshot> {
        let previous_context = self
            .memory
            .get_previous_context(self.app_config.default_previous_context)?;
        let runtime_overview = self.runtime.overview();
        let tools_overview = self.tools.overview()?;
        let current_model_name = runtime_overview
            .selected_cloud_profile_label
            .clone()
            .unwrap_or_else(|| "unconfigured".to_owned());
        self.memory.load_overview(
            previous_context,
            runtime_overview,
            &current_model_name,
            tools_overview,
        )
    }

    pub fn execute_tool_action(
        &self,
        tool: &str,
        action: &str,
        arguments: Value,
    ) -> Result<ToolExecutionResult> {
        let started_at = Instant::now();
        let result = self
            .tools
            .execute_named(tool, action, arguments.clone())
            .with_context(|| format!("failed to execute {tool}.{action}"))?;

        self.memory.log_tool_call(&ToolLogEntry {
            created_at: now_rfc3339(),
            tool_name: tool.to_owned(),
            action: action.to_owned(),
            arguments_json: serde_json::to_string(&arguments)?,
            result_json: serde_json::to_string(&result.result_json)?,
            success: result.is_success(),
            latency_ms: started_at.elapsed().as_millis().min(i64::MAX as u128) as i64,
        })?;
        emit_diagnostic_log(
            self.callbacks.as_ref(),
            "tools",
            if result.is_success() {
                "info"
            } else {
                "warning"
            },
            &format!(
                "tool={} action={} status={}",
                tool, action, result.result_json["status"]
            ),
            None,
            json!({
                "tool": tool,
                "action": action,
                "status": result.result_json.get("status").and_then(Value::as_str),
            }),
        );

        Ok(result)
    }

    pub fn begin_tool_auth(&self, tool: &str) -> Result<ToolExecutionResult> {
        let result = self.tools.begin_tool_auth(tool)?;
        self.log_tool_lifecycle(tool, "begin_auth", &result)?;
        Ok(result)
    }

    pub fn disconnect_tool(&self, tool: &str) -> Result<ToolExecutionResult> {
        let result = self.tools.disconnect_tool(tool)?;
        self.log_tool_lifecycle(tool, "disconnect", &result)?;
        Ok(result)
    }

    pub fn refresh_tool_state(&self, tool: &str) -> Result<ToolExecutionResult> {
        let result = self.tools.refresh_tool_state(tool)?;
        self.log_tool_lifecycle(tool, "refresh_state", &result)?;
        Ok(result)
    }

    pub fn handle_tool_auth_callback(
        &self,
        tool: &str,
        turn_id: &str,
        callback_url: &str,
    ) -> Result<()> {
        match crate::tools::ToolName::parse(tool)? {
            crate::tools::ToolName::Spotify => {
                emit_diagnostic_log(
                    self.callbacks.as_ref(),
                    "tools",
                    "info",
                    "spotify auth callback received",
                    Some(turn_id),
                    json!({ "tool": tool }),
                );
                self.engine.complete_spotify_auth_callback(
                    turn_id,
                    callback_url,
                    self.callbacks.as_ref(),
                )?;
                Ok(())
            }
            crate::tools::ToolName::Hue => {
                Err(anyhow!("tool does not support auth callbacks: {tool}"))
            }
        }
    }

    pub fn set_previous_context(&self, level: ContextLevel) -> Result<()> {
        self.memory.set_previous_context(level)
    }

    pub fn set_selected_cloud_profile(&self, profile_id: &str) -> Result<()> {
        self.runtime.set_selected_cloud_profile(profile_id)?;
        self.memory.set_selected_cloud_profile(profile_id)?;
        emit_diagnostic_log(
            self.callbacks.as_ref(),
            "model",
            "info",
            "cloud profile updated",
            None,
            json!({ "selected_cloud_profile": profile_id }),
        );
        Ok(())
    }

    pub fn submit_ambient_event(
        &self,
        turn_id: &str,
        event_type: &str,
        payload_json: Value,
    ) -> Result<Option<ChatMessage>> {
        self.engine
            .execute_ambient(turn_id, event_type, payload_json, self.callbacks.as_ref())
    }

    pub fn record_note_activity(
        &self,
        note_key: &str,
        relative_path: &str,
        title_snapshot: &str,
        event_type: &str,
        occurred_at: &str,
    ) -> Result<()> {
        self.memory.record_note_activity(
            note_key,
            relative_path,
            title_snapshot,
            event_type,
            occurred_at,
        )
    }

    pub fn can_accept_turns(&self) -> bool {
        self.runtime.can_accept_turns()
    }

    fn log_tool_lifecycle(
        &self,
        tool: &str,
        action: &str,
        result: &ToolExecutionResult,
    ) -> Result<()> {
        self.memory.log_tool_call(&ToolLogEntry {
            created_at: now_rfc3339(),
            tool_name: tool.to_owned(),
            action: action.to_owned(),
            arguments_json: "{}".to_owned(),
            result_json: serde_json::to_string(&result.result_json)?,
            success: result.is_success(),
            latency_ms: 0,
        })?;
        emit_diagnostic_log(
            self.callbacks.as_ref(),
            "tools",
            if result.is_success() {
                "info"
            } else {
                "warning"
            },
            &format!("tool lifecycle {}.{}", tool, action),
            None,
            json!({
                "tool": tool,
                "action": action,
                "status": result.result_json.get("status").and_then(Value::as_str),
            }),
        );
        Ok(())
    }
}

pub use prompt_translator::PromptConfig;

pub fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

pub(crate) fn emit_diagnostic_log(
    callbacks: &dyn CoreCallbacks,
    subsystem: &str,
    level: &str,
    message: &str,
    turn_id: Option<&str>,
    fields: Value,
) {
    callbacks.emit(
        "diagnostic_log",
        json!({
            "subsystem": subsystem,
            "level": level,
            "message": message,
            "turn_id": turn_id,
            "fields": fields,
        })
        .to_string(),
    );
}

pub(crate) fn load_config<T: DeserializeOwned>(config_dir: &Path, base_name: &str) -> Result<T> {
    let merged = load_config_value(config_dir, base_name)?;
    serde_json::from_value(merged).with_context(|| {
        format!(
            "invalid config payload for {}",
            config_dir.join(base_name).display()
        )
    })
}

pub(crate) fn load_config_value(config_dir: &Path, base_name: &str) -> Result<Value> {
    let base_path = config_dir.join(base_name);
    let local_path = config_dir.join(base_name.replace(".json", ".local.json"));
    let base_value = read_json_file(&base_path)?;
    Ok(if local_path.exists() {
        merge_json(base_value, read_json_file(&local_path)?)
    } else {
        base_value
    })
}

fn read_json_file(path: &Path) -> Result<Value> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file {}", path.display()))?;
    serde_json::from_str(&contents)
        .with_context(|| format!("invalid JSON in config file {}", path.display()))
}

fn merge_json(base: Value, overlay: Value) -> Value {
    match (base, overlay) {
        (Value::Object(mut base_map), Value::Object(overlay_map)) => {
            for (key, overlay_value) in overlay_map {
                let merged = match base_map.remove(&key) {
                    Some(base_value) => merge_json(base_value, overlay_value),
                    None => overlay_value,
                };
                base_map.insert(key, merged);
            }
            Value::Object(base_map)
        }
        (_, overlay) => overlay,
    }
}

#[cfg(target_os = "android")]
mod android_bridge {
    use super::*;
    use jni::objects::{GlobalRef, JClass, JObject, JString, JValue};
    use jni::sys::jstring;
    use jni::{JNIEnv, JavaVM};

    static CORE_INSTANCE: LazyLock<Mutex<Option<BabyGervaiseCore>>> =
        LazyLock::new(|| Mutex::new(None));

    struct AndroidCallbacks {
        vm: JavaVM,
        callback_ref: GlobalRef,
    }

    impl CoreCallbacks for AndroidCallbacks {
        fn emit(&self, event_type: &str, payload_json: String) {
            if let Ok(mut env) = self.vm.attach_current_thread() {
                if let (Ok(event_type), Ok(payload_json)) =
                    (env.new_string(event_type), env.new_string(payload_json))
                {
                    let event_type_object = JObject::from(event_type);
                    let payload_object = JObject::from(payload_json);
                    let _ = env.call_method(
                        self.callback_ref.as_obj(),
                        "onCoreEvent",
                        "(Ljava/lang/String;Ljava/lang/String;)V",
                        &[
                            JValue::Object(&event_type_object),
                            JValue::Object(&payload_object),
                        ],
                    );
                }
            }
        }
    }

    fn read_string(env: &mut JNIEnv<'_>, input: JString<'_>) -> Result<String> {
        Ok(env.get_string(&input)?.into())
    }

    fn with_core_mut<T, F>(mutator: F) -> Result<T>
    where
        F: FnOnce(&BabyGervaiseCore) -> Result<T>,
    {
        let guard = CORE_INSTANCE
            .lock()
            .map_err(|_| anyhow!("core mutex poisoned"))?;
        let core = guard
            .as_ref()
            .ok_or_else(|| anyhow!("core has not been initialized"))?;
        mutator(core)
    }

    fn clear_pending_exception(env: &mut JNIEnv<'_>) {
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
    }

    fn emit_assistant_error(turn_id: Option<&str>, error: &str) -> Result<()> {
        with_core_mut(|core| {
            core.callbacks.emit(
                "assistant_error",
                json!({
                    "turnId": turn_id,
                    "error": error,
                })
                .to_string(),
            );
            Ok(())
        })
    }

    fn throw_error(env: &mut JNIEnv<'_>, error: anyhow::Error) {
        if env.exception_check().unwrap_or(false) {
            return;
        }
        let _ = env.throw_new("java/lang/IllegalStateException", error.to_string());
    }

    #[no_mangle]
    pub extern "system" fn Java_io_gervaise_babygervaise_bridge_NativeBabyGervaise_nativeInit(
        mut env: JNIEnv,
        _class: JClass,
        app_files_dir: JString,
        config_dir: JString,
        callbacks: JObject,
        nano_host: JObject,
    ) {
        let result = (|| {
            let app_files_dir = read_string(&mut env, app_files_dir)?;
            let config_dir = read_string(&mut env, config_dir)?;
            let vm = env.get_java_vm()?;
            let nano_vm = env.get_java_vm()?;
            let callback_ref = env.new_global_ref(callbacks)?;
            let nano_ref = env.new_global_ref(nano_host)?;
            let callbacks =
                Arc::new(AndroidCallbacks { vm, callback_ref }) as Arc<dyn CoreCallbacks>;
            let nano = Arc::new(crate::model_runtime::AndroidNanoAdapter::new(
                nano_vm, nano_ref,
            )) as Arc<dyn crate::model_runtime::NanoAdapter>;
            let core =
                BabyGervaiseCore::init_with_nano(app_files_dir, config_dir, callbacks, nano)?;
            let mut guard = CORE_INSTANCE
                .lock()
                .map_err(|_| anyhow!("core mutex poisoned"))?;
            *guard = Some(core);
            Ok(())
        })();

        if let Err(error) = result {
            throw_error(&mut env, error);
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_io_gervaise_babygervaise_bridge_NativeBabyGervaise_nativeSubmitUserTurn(
        mut env: JNIEnv,
        _class: JClass,
        turn_id: JString,
        text: JString,
        input_source: JString,
    ) {
        let result = (|| {
            let turn_id = read_string(&mut env, turn_id)?;
            let text = read_string(&mut env, text)?;
            let input_source = InputSource::from_str(&read_string(&mut env, input_source)?);
            match with_core_mut(|core| core.submit_user_turn(&turn_id, &text, input_source)) {
                Ok(()) => Ok(()),
                Err(error) => {
                    clear_pending_exception(&mut env);
                    emit_assistant_error(Some(&turn_id), &error.to_string())?;
                    Ok(())
                }
            }
        })();

        if let Err(error) = result {
            throw_error(&mut env, error);
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_io_gervaise_babygervaise_bridge_NativeBabyGervaise_nativeHandleSpotifyAuthCallback(
        mut env: JNIEnv,
        _class: JClass,
        turn_id: JString,
        callback_url: JString,
    ) {
        let result = (|| {
            let turn_id = read_string(&mut env, turn_id)?;
            let callback_url = read_string(&mut env, callback_url)?;
            with_core_mut(|core| core.handle_spotify_auth_callback(&turn_id, &callback_url))
        })();

        if let Err(error) = result {
            throw_error(&mut env, error);
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_io_gervaise_babygervaise_bridge_NativeBabyGervaise_nativeHandleToolAuthCallback(
        mut env: JNIEnv,
        _class: JClass,
        tool: JString,
        turn_id: JString,
        callback_url: JString,
    ) {
        let result = (|| {
            let tool = read_string(&mut env, tool)?;
            let turn_id = read_string(&mut env, turn_id)?;
            let callback_url = read_string(&mut env, callback_url)?;
            with_core_mut(|core| core.handle_tool_auth_callback(&tool, &turn_id, &callback_url))
        })();

        if let Err(error) = result {
            throw_error(&mut env, error);
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_io_gervaise_babygervaise_bridge_NativeBabyGervaise_nativeExecuteToolAction(
        mut env: JNIEnv,
        _class: JClass,
        tool: JString,
        action: JString,
        arguments_json: JString,
    ) -> jstring {
        let result = (|| {
            let tool = read_string(&mut env, tool)?;
            let action = read_string(&mut env, action)?;
            let arguments_json = read_string(&mut env, arguments_json)?;
            let arguments = serde_json::from_str::<Value>(&arguments_json)
                .with_context(|| format!("invalid tool arguments JSON for {tool}.{action}"))?;
            let outcome =
                with_core_mut(|core| core.execute_tool_action(&tool, &action, arguments))?;
            Ok(serde_json::to_string(&outcome)?)
        })();

        match result {
            Ok(payload) => env
                .new_string(payload)
                .map(|value| value.into_raw())
                .unwrap_or(std::ptr::null_mut()),
            Err(error) => {
                throw_error(&mut env, error);
                std::ptr::null_mut()
            }
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_io_gervaise_babygervaise_bridge_NativeBabyGervaise_nativeBeginToolAuth(
        mut env: JNIEnv,
        _class: JClass,
        tool: JString,
    ) -> jstring {
        let result = (|| {
            let tool = read_string(&mut env, tool)?;
            let outcome = with_core_mut(|core| core.begin_tool_auth(&tool))?;
            Ok(serde_json::to_string(&outcome)?)
        })();

        match result {
            Ok(payload) => env
                .new_string(payload)
                .map(|value| value.into_raw())
                .unwrap_or(std::ptr::null_mut()),
            Err(error) => {
                throw_error(&mut env, error);
                std::ptr::null_mut()
            }
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_io_gervaise_babygervaise_bridge_NativeBabyGervaise_nativeDisconnectTool(
        mut env: JNIEnv,
        _class: JClass,
        tool: JString,
    ) -> jstring {
        let result = (|| {
            let tool = read_string(&mut env, tool)?;
            let outcome = with_core_mut(|core| core.disconnect_tool(&tool))?;
            Ok(serde_json::to_string(&outcome)?)
        })();

        match result {
            Ok(payload) => env
                .new_string(payload)
                .map(|value| value.into_raw())
                .unwrap_or(std::ptr::null_mut()),
            Err(error) => {
                throw_error(&mut env, error);
                std::ptr::null_mut()
            }
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_io_gervaise_babygervaise_bridge_NativeBabyGervaise_nativeRefreshToolState(
        mut env: JNIEnv,
        _class: JClass,
        tool: JString,
    ) -> jstring {
        let result = (|| {
            let tool = read_string(&mut env, tool)?;
            let outcome = with_core_mut(|core| core.refresh_tool_state(&tool))?;
            Ok(serde_json::to_string(&outcome)?)
        })();

        match result {
            Ok(payload) => env
                .new_string(payload)
                .map(|value| value.into_raw())
                .unwrap_or(std::ptr::null_mut()),
            Err(error) => {
                throw_error(&mut env, error);
                std::ptr::null_mut()
            }
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_io_gervaise_babygervaise_bridge_NativeBabyGervaise_nativeLoadBootstrapState(
        mut env: JNIEnv,
        _class: JClass,
    ) -> jstring {
        let result = with_core_mut(|core| {
            let snapshot = core.load_bootstrap_state()?;
            Ok(serde_json::to_string(&snapshot)?)
        });

        match result {
            Ok(payload) => env
                .new_string(payload)
                .map(|value| value.into_raw())
                .unwrap_or(std::ptr::null_mut()),
            Err(error) => {
                throw_error(&mut env, error);
                std::ptr::null_mut()
            }
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_io_gervaise_babygervaise_bridge_NativeBabyGervaise_nativeLoadOverviewState(
        mut env: JNIEnv,
        _class: JClass,
    ) -> jstring {
        let result = with_core_mut(|core| {
            let snapshot = core.load_overview_state()?;
            Ok(serde_json::to_string(&snapshot)?)
        });

        match result {
            Ok(payload) => env
                .new_string(payload)
                .map(|value| value.into_raw())
                .unwrap_or(std::ptr::null_mut()),
            Err(error) => {
                throw_error(&mut env, error);
                std::ptr::null_mut()
            }
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_io_gervaise_babygervaise_bridge_NativeBabyGervaise_nativeSetPreviousContext(
        mut env: JNIEnv,
        _class: JClass,
        level: JString,
    ) {
        let result = (|| {
            let level = match read_string(&mut env, level)?.as_str() {
                "low" => ContextLevel::Low,
                "high" => ContextLevel::High,
                _ => ContextLevel::Medium,
            };
            with_core_mut(|core| core.set_previous_context(level))
        })();

        if let Err(error) = result {
            throw_error(&mut env, error);
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_io_gervaise_babygervaise_bridge_NativeBabyGervaise_nativeSetCloudProfile(
        mut env: JNIEnv,
        _class: JClass,
        profile_id: JString,
    ) {
        let result = (|| {
            let profile_id = read_string(&mut env, profile_id)?;
            with_core_mut(|core| core.set_selected_cloud_profile(&profile_id))
        })();

        if let Err(error) = result {
            throw_error(&mut env, error);
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_io_gervaise_babygervaise_bridge_NativeBabyGervaise_nativeSubmitAmbientEvent(
        mut env: JNIEnv,
        _class: JClass,
        turn_id: JString,
        event_type: JString,
        payload_json: JString,
    ) {
        let result = (|| {
            let turn_id = read_string(&mut env, turn_id)?;
            let event_type = read_string(&mut env, event_type)?;
            let payload_json = read_string(&mut env, payload_json)?;
            let payload = serde_json::from_str::<Value>(&payload_json)
                .with_context(|| format!("invalid ambient payload for {event_type}"))?;
            match with_core_mut(|core| {
                let _ = core.submit_ambient_event(&turn_id, &event_type, payload)?;
                Ok(())
            }) {
                Ok(()) => Ok(()),
                Err(error) => {
                    clear_pending_exception(&mut env);
                    emit_assistant_error(Some(&turn_id), &error.to_string())?;
                    Ok(())
                }
            }
        })();

        if let Err(error) = result {
            throw_error(&mut env, error);
        }
    }

    #[no_mangle]
    pub extern "system" fn Java_io_gervaise_babygervaise_bridge_NativeBabyGervaise_nativeRecordNoteActivity(
        mut env: JNIEnv,
        _class: JClass,
        note_key: JString,
        relative_path: JString,
        title_snapshot: JString,
        event_type: JString,
        occurred_at: JString,
    ) {
        let result = (|| {
            let note_key = read_string(&mut env, note_key)?;
            let relative_path = read_string(&mut env, relative_path)?;
            let title_snapshot = read_string(&mut env, title_snapshot)?;
            let event_type = read_string(&mut env, event_type)?;
            let occurred_at = read_string(&mut env, occurred_at)?;
            with_core_mut(|core| {
                core.record_note_activity(
                    &note_key,
                    &relative_path,
                    &title_snapshot,
                    &event_type,
                    &occurred_at,
                )
            })
        })();

        if let Err(error) = result {
            throw_error(&mut env, error);
        }
    }
}
