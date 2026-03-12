pub mod hgie;
pub mod logging;
pub mod memory;
pub mod model;
pub mod tools;

use std::fs;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use hgie::HgieEngine;
use model::{ModelGateway, OpenAiCompatibleModel};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::logging::OverviewSnapshot;
use crate::memory::MemoryStore;

#[cfg(target_os = "android")]
use anyhow::anyhow;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub turn_id: String,
    pub input_source: InputSource,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapState {
    pub previous_context: ContextLevel,
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub provider: String,
    pub api_key: String,
    pub model: String,
    pub endpoint: String,
    pub temperature: f32,
    pub timeout_ms: u64,
    #[serde(default = "default_stream_enabled")]
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptConfig {
    pub system_prompt: String,
    pub memory_preamble: String,
    pub tool_instructions: String,
    pub response_contract: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AppConfig {
    pub default_previous_context: ContextLevel,
    pub vector_dimensions: usize,
    pub memory_salience_threshold: f32,
    pub stream_chunk_size: usize,
    pub max_recent_messages_per_turn: usize,
    pub max_model_logs: usize,
}

fn default_stream_enabled() -> bool {
    true
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
    engine: HgieEngine,
    callbacks: Arc<dyn CoreCallbacks>,
    app_config: AppConfig,
    model_name: String,
}

impl BabyGervaiseCore {
    pub fn init(
        app_files_dir: impl AsRef<Path>,
        asset_config_dir: impl AsRef<Path>,
        callbacks: Arc<dyn CoreCallbacks>,
    ) -> Result<Self> {
        let model_config =
            load_config::<ModelConfig>(asset_config_dir.as_ref(), "model_config.json")?;
        let prompt_config =
            load_config::<PromptConfig>(asset_config_dir.as_ref(), "prompt_config.json")?;
        let app_config = load_config::<AppConfig>(asset_config_dir.as_ref(), "app_config.json")?;
        let model =
            Arc::new(OpenAiCompatibleModel::new(model_config.clone())?) as Arc<dyn ModelGateway>;
        Self::with_model_gateway(
            app_files_dir,
            callbacks,
            model_config,
            prompt_config,
            app_config,
            model,
        )
    }

    pub fn with_model_gateway(
        app_files_dir: impl AsRef<Path>,
        callbacks: Arc<dyn CoreCallbacks>,
        model_config: ModelConfig,
        prompt_config: PromptConfig,
        app_config: AppConfig,
        model: Arc<dyn ModelGateway>,
    ) -> Result<Self> {
        let db_path = app_files_dir.as_ref().join("baby_gervaise.sqlite3");
        let memory = MemoryStore::new(db_path, &app_config)?;
        let engine = HgieEngine::new(
            memory.clone(),
            model.clone(),
            model_config,
            prompt_config,
            app_config,
        );

        Ok(Self {
            memory,
            engine,
            callbacks,
            app_config,
            model_name: model.model_name().to_owned(),
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

    pub fn load_bootstrap_state(&self) -> Result<BootstrapState> {
        self.memory
            .load_bootstrap_state(self.app_config.default_previous_context)
    }

    pub fn load_overview_state(&self) -> Result<OverviewSnapshot> {
        let previous_context = self
            .memory
            .get_previous_context(self.app_config.default_previous_context)?;
        self.memory
            .load_overview(previous_context, &self.model_name)
    }

    pub fn set_previous_context(&self, level: ContextLevel) -> Result<()> {
        self.memory.set_previous_context(level)
    }
}

pub fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

fn load_config<T: DeserializeOwned>(config_dir: &Path, base_name: &str) -> Result<T> {
    let base_path = config_dir.join(base_name);
    let local_path = config_dir.join(base_name.replace(".json", ".local.json"));
    let base_value = read_json_file(&base_path)?;
    let merged = if local_path.exists() {
        merge_json(base_value, read_json_file(&local_path)?)
    } else {
        base_value
    };

    serde_json::from_value(merged)
        .with_context(|| format!("invalid config payload for {}", base_path.display()))
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

    fn throw_error(env: &mut JNIEnv<'_>, error: anyhow::Error) {
        let _ = env.throw_new("java/lang/IllegalStateException", error.to_string());
    }

    #[no_mangle]
    pub extern "system" fn Java_io_gervaise_babygervaise_bridge_NativeBabyGervaise_nativeInit(
        mut env: JNIEnv,
        _class: JClass,
        app_files_dir: JString,
        config_dir: JString,
        callbacks: JObject,
    ) {
        let result = (|| {
            let app_files_dir = read_string(&mut env, app_files_dir)?;
            let config_dir = read_string(&mut env, config_dir)?;
            let vm = env.get_java_vm()?;
            let callback_ref = env.new_global_ref(callbacks)?;
            let callbacks =
                Arc::new(AndroidCallbacks { vm, callback_ref }) as Arc<dyn CoreCallbacks>;
            let core = BabyGervaiseCore::init(app_files_dir, config_dir, callbacks)?;
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
            with_core_mut(|core| core.submit_user_turn(&turn_id, &text, input_source))
        })();

        if let Err(error) = result {
            throw_error(&mut env, error);
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
}
