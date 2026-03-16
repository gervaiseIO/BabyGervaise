use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(target_os = "android")]
use serde_json::json;

use crate::logging::{ModelLogEntry, NanoRuntimeStatus, RuntimeOverview, RuntimeProfileSummary};
use crate::model::{GeminiModel, ModelGateway, ModelRequest, OpenAiCompatibleModel};
use crate::prompt_translator::{CompiledNanoPrompt, PromptMode};
use crate::{now_rfc3339, ModelConfig};

#[cfg(target_os = "android")]
use jni::objects::GlobalRef;
#[cfg(target_os = "android")]
use jni::{objects::JObject, JavaVM};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirstBeatRequest {
    pub prompt: CompiledNanoPrompt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirstBeatResult {
    pub text: String,
    pub logs: Vec<ModelLogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NanoReplyRequest {
    pub prompt: CompiledNanoPrompt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NanoReplyResult {
    pub raw_output: String,
    pub logs: Vec<ModelLogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudReasoningRequest {
    pub request: ModelRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudReasoningResult {
    pub raw_output: String,
    pub logs: Vec<ModelLogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbientRequest {
    pub prompt: CompiledNanoPrompt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbientResult {
    pub text: String,
    pub logs: Vec<ModelLogEntry>,
}

pub trait ModelRuntime: Send + Sync {
    fn run_first_beat(&self, request: &FirstBeatRequest) -> Result<FirstBeatResult>;
    fn run_nano_reply(&self, request: &NanoReplyRequest) -> Result<NanoReplyResult>;
    fn run_cloud_reasoning(&self, request: &CloudReasoningRequest) -> Result<CloudReasoningResult>;
    fn run_ambient(&self, request: &AmbientRequest) -> Result<Option<AmbientResult>>;
    fn overview(&self) -> RuntimeOverview;
    fn can_accept_turns(&self) -> bool;
    fn selected_cloud_profile_id(&self) -> Option<String>;
    fn set_selected_cloud_profile(&self, profile_id: &str) -> Result<()>;
    fn default_selected_cloud_profile_id(&self) -> Option<String>;
}

pub trait EmbeddingRuntime: Send + Sync {
    fn runtime_id(&self) -> &str;
    fn dimensions(&self) -> usize;
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

#[derive(Debug, Clone)]
pub struct LocalDeterministicEmbeddingRuntime {
    dimensions: usize,
}

impl LocalDeterministicEmbeddingRuntime {
    pub fn new(dimensions: usize) -> Self {
        Self { dimensions }
    }
}

impl EmbeddingRuntime for LocalDeterministicEmbeddingRuntime {
    fn runtime_id(&self) -> &str {
        "local_deterministic_v1"
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        Ok(crate::memory::vectorize_text(text, self.dimensions))
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|text| self.embed(text)).collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NanoRuntimeConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_nano_provider")]
    pub provider: String,
    #[serde(default = "default_nano_model")]
    pub model: String,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeCloudConfig {
    #[serde(default)]
    pub selected_profile: Option<String>,
    #[serde(default)]
    pub profiles: HashMap<String, ModelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeModelConfig {
    pub nano: NanoRuntimeConfig,
    pub cloud: RuntimeCloudConfig,
}

#[derive(Debug, Clone)]
pub struct RuntimeConfigResolver {
    pub config: RuntimeModelConfig,
    pub invalid_profiles: Vec<String>,
}

impl RuntimeConfigResolver {
    pub fn load(config_dir: &Path) -> Result<Self> {
        let merged = crate::load_config_value(config_dir, "model_config.json")?;
        Ok(Self::resolve(merged))
    }

    pub fn resolve(merged: Value) -> Self {
        if merged.get("cloud").is_none() && merged.get("provider").is_some() {
            return Self::resolve_legacy(merged);
        }

        let nano = merged
            .get("nano")
            .cloned()
            .and_then(|value| serde_json::from_value::<NanoRuntimeConfig>(value).ok())
            .unwrap_or_else(default_nano_config);

        let selected_profile = merged
            .pointer("/cloud/selected_profile")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);

        let mut profiles = HashMap::new();
        let mut invalid_profiles = Vec::new();
        if let Some(raw_profiles) = merged.pointer("/cloud/profiles").and_then(Value::as_object) {
            for (profile_id, value) in raw_profiles {
                match serde_json::from_value::<ModelConfig>(value.clone()) {
                    Ok(profile) => {
                        profiles.insert(profile_id.clone(), profile.with_defaults(profile_id));
                    }
                    Err(_) => invalid_profiles.push(profile_id.clone()),
                }
            }
        }

        Self {
            config: RuntimeModelConfig {
                nano,
                cloud: RuntimeCloudConfig {
                    selected_profile,
                    profiles,
                },
            },
            invalid_profiles,
        }
    }

    fn resolve_legacy(merged: Value) -> Self {
        let mut profiles = HashMap::new();
        if let Ok(legacy) = serde_json::from_value::<ModelConfig>(merged) {
            profiles.insert(
                "default_cloud".to_owned(),
                legacy.with_defaults("default_cloud"),
            );
        }

        Self {
            config: RuntimeModelConfig {
                nano: default_nano_config(),
                cloud: RuntimeCloudConfig {
                    selected_profile: Some("default_cloud".to_owned()),
                    profiles,
                },
            },
            invalid_profiles: Vec::new(),
        }
    }
}

fn default_enabled() -> bool {
    true
}

fn default_nano_provider() -> String {
    "gemini".to_owned()
}

fn default_nano_model() -> String {
    "gemini-nano".to_owned()
}

fn default_nano_config() -> NanoRuntimeConfig {
    NanoRuntimeConfig {
        enabled: true,
        provider: default_nano_provider(),
        model: default_nano_model(),
        max_output_tokens: Some(48),
        temperature: Some(0.4),
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NanoAvailability {
    Available,
    Downloading,
    Unavailable,
    Error,
    Disabled,
}

impl NanoAvailability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Downloading => "downloading",
            Self::Unavailable => "unavailable",
            Self::Error => "error",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NanoSnapshot {
    pub availability: NanoAvailability,
    pub detail: String,
    pub provider: String,
    pub model: String,
}

impl NanoSnapshot {
    pub fn disabled() -> Self {
        Self {
            availability: NanoAvailability::Disabled,
            detail: "Nano is disabled.".to_owned(),
            provider: "gemini".to_owned(),
            model: "gemini-nano".to_owned(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum NanoPromptMode {
    FirstBeat,
    FullReply,
    Ambient,
}

impl NanoPromptMode {
    #[cfg(target_os = "android")]
    fn as_str(&self) -> &'static str {
        match self {
            Self::FirstBeat => "first_beat",
            Self::FullReply => "full_reply",
            Self::Ambient => "ambient",
        }
    }

    fn from_prompt_mode(mode: PromptMode) -> Self {
        match mode {
            PromptMode::FirstBeat => Self::FirstBeat,
            PromptMode::AmbientLine => Self::Ambient,
            PromptMode::NanoReply => Self::FullReply,
            PromptMode::FollowupReasoning
            | PromptMode::CloudReasoning
            | PromptMode::MemoryBackedRecall
            | PromptMode::SummaryBridge
            | PromptMode::ToolStatusExplanation
            | PromptMode::ErrorExplanation => Self::FullReply,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NanoPromptResponse {
    pub text: String,
    pub snapshot: NanoSnapshot,
    pub prompt: String,
    pub latency_ms: i64,
    #[serde(default)]
    pub error_text: Option<String>,
    #[serde(default)]
    pub requested_max_output_tokens: Option<u32>,
    #[serde(default)]
    pub effective_max_output_tokens: Option<u32>,
}

pub trait NanoAdapter: Send + Sync {
    fn snapshot(&self) -> NanoSnapshot;
    fn run_prompt(
        &self,
        mode: NanoPromptMode,
        prompt: &str,
        temperature: Option<f32>,
        max_output_tokens: Option<u32>,
    ) -> Result<NanoPromptResponse>;
}

pub struct DisabledNanoAdapter {
    snapshot: NanoSnapshot,
}

impl DisabledNanoAdapter {
    pub fn new(detail: &str) -> Self {
        Self {
            snapshot: NanoSnapshot {
                availability: NanoAvailability::Unavailable,
                detail: detail.to_owned(),
                provider: "gemini".to_owned(),
                model: "gemini-nano".to_owned(),
            },
        }
    }
}

impl NanoAdapter for DisabledNanoAdapter {
    fn snapshot(&self) -> NanoSnapshot {
        self.snapshot.clone()
    }

    fn run_prompt(
        &self,
        _mode: NanoPromptMode,
        _prompt: &str,
        _temperature: Option<f32>,
        _max_output_tokens: Option<u32>,
    ) -> Result<NanoPromptResponse> {
        Err(anyhow!(self.snapshot.detail.clone()))
    }
}

#[cfg(target_os = "android")]
pub struct AndroidNanoAdapter {
    vm: JavaVM,
    host_ref: GlobalRef,
}

#[cfg(target_os = "android")]
impl AndroidNanoAdapter {
    pub fn new(vm: JavaVM, host_ref: GlobalRef) -> Self {
        Self { vm, host_ref }
    }

    fn call_string_method(
        &self,
        method_name: &str,
        signature: &str,
        args: &[jni::objects::JValue<'_, '_>],
    ) -> Result<String> {
        let mut env = self
            .vm
            .attach_current_thread()
            .context("failed to attach nano thread")?;
        let result = env
            .call_method(self.host_ref.as_obj(), method_name, signature, args)
            .with_context(|| format!("failed to call Nano host method {method_name}"))?
            .l()
            .context("Nano host returned null")?;
        let result = jni::objects::JString::from(result);
        let value: String = env.get_string(&result)?.into();
        Ok(value)
    }
}

#[cfg(target_os = "android")]
impl NanoAdapter for AndroidNanoAdapter {
    fn snapshot(&self) -> NanoSnapshot {
        let payload = self
            .call_string_method("loadNanoSnapshot", "()Ljava/lang/String;", &[])
            .unwrap_or_else(|error| {
                json!({
                    "availability": "error",
                    "detail": error.to_string(),
                    "provider": "gemini",
                    "model": "gemini-nano"
                })
                .to_string()
            });
        serde_json::from_str(&payload).unwrap_or(NanoSnapshot {
            availability: NanoAvailability::Error,
            detail: "Nano status is unreadable.".to_owned(),
            provider: "gemini".to_owned(),
            model: "gemini-nano".to_owned(),
        })
    }

    fn run_prompt(
        &self,
        mode: NanoPromptMode,
        prompt: &str,
        temperature: Option<f32>,
        max_output_tokens: Option<u32>,
    ) -> Result<NanoPromptResponse> {
        let request = json!({
            "mode": mode.as_str(),
            "prompt": prompt,
            "temperature": temperature,
            "max_output_tokens": max_output_tokens,
        })
        .to_string();

        let env = self
            .vm
            .attach_current_thread()
            .context("failed to attach nano thread")?;
        let request = env.new_string(request)?;
        let request_object = JObject::from(request);
        let payload = self.call_string_method(
            "runNanoPrompt",
            "(Ljava/lang/String;)Ljava/lang/String;",
            &[jni::objects::JValue::Object(&request_object)],
        )?;
        let response = serde_json::from_str::<NanoPromptResponse>(&payload)
            .context("failed to decode nano prompt response")?;
        if let Some(error_text) = response.error_text.clone() {
            return Err(anyhow!(error_text));
        }
        Ok(response)
    }
}

struct RuntimeState {
    selected_cloud_profile_id: Option<String>,
}

struct CloudProfileRuntime {
    summary: RuntimeProfileSummary,
    gateway: Option<Arc<dyn ModelGateway>>,
    unavailable_reason: Option<String>,
}

pub struct DefaultModelRuntime {
    nano_config: NanoRuntimeConfig,
    nano: Arc<dyn NanoAdapter>,
    cloud_profiles: BTreeMap<String, CloudProfileRuntime>,
    state: Mutex<RuntimeState>,
}

impl DefaultModelRuntime {
    pub fn from_resolver(
        resolver: RuntimeConfigResolver,
        nano: Arc<dyn NanoAdapter>,
    ) -> Result<Self> {
        let mut cloud_profiles = BTreeMap::new();

        for (profile_id, config) in &resolver.config.cloud.profiles {
            if !config.enabled {
                continue;
            }
            let mut summary = RuntimeProfileSummary {
                id: profile_id.clone(),
                label: config.display_label(profile_id),
                provider: config.provider.clone(),
                model: config.model.clone(),
                enabled: config.enabled,
                available: false,
                selected: false,
            };
            let (gateway, unavailable_reason) = if !config.has_usable_api_key() {
                (
                    None,
                    Some(format!("{} is missing a valid API key.", summary.label)),
                )
            } else {
                match build_gateway(config)
                    .with_context(|| format!("failed to initialize cloud profile {profile_id}"))
                {
                    Ok(gateway) => {
                        summary.available = true;
                        (Some(gateway), None)
                    }
                    Err(error) => (None, Some(error.to_string())),
                }
            };
            cloud_profiles.insert(
                profile_id.clone(),
                CloudProfileRuntime {
                    summary,
                    gateway,
                    unavailable_reason,
                },
            );
        }

        let initial_selected = choose_selected_profile(
            resolver.config.cloud.selected_profile.as_deref(),
            cloud_profiles
                .values()
                .map(|profile| (profile.summary.id.clone(), profile.summary.available))
                .collect(),
        );

        Ok(Self {
            nano_config: resolver.config.nano,
            nano,
            cloud_profiles,
            state: Mutex::new(RuntimeState {
                selected_cloud_profile_id: initial_selected,
            }),
        })
    }

    fn current_cloud_profile(&self) -> Option<&CloudProfileRuntime> {
        let selected = self
            .state
            .lock()
            .ok()
            .and_then(|state| state.selected_cloud_profile_id.clone());
        selected
            .as_ref()
            .and_then(|profile_id| self.cloud_profiles.get(profile_id))
            .or_else(|| self.cloud_profiles.values().next())
    }

    fn first_beat_max_output_tokens(&self, request: &FirstBeatRequest) -> Option<u32> {
        Some(normalize_nano_max_output_tokens(
            request
                .prompt
                .max_output_tokens
                .or(self.nano_config.max_output_tokens),
            48,
        ))
    }

    fn nano_reply_max_output_tokens(&self, request: &NanoReplyRequest) -> Option<u32> {
        Some(normalize_nano_max_output_tokens(
            request.prompt.max_output_tokens,
            192,
        ))
    }

    fn ambient_max_output_tokens(&self, request: &AmbientRequest) -> Option<u32> {
        Some(normalize_nano_max_output_tokens(
            request.prompt.max_output_tokens,
            40,
        ))
    }

    fn has_available_cloud_profile(&self) -> bool {
        self.cloud_profiles
            .values()
            .any(|profile| profile.summary.available)
    }
}

impl ModelRuntime for DefaultModelRuntime {
    fn run_first_beat(&self, request: &FirstBeatRequest) -> Result<FirstBeatResult> {
        let response = self.nano.run_prompt(
            NanoPromptMode::from_prompt_mode(request.prompt.mode),
            &request.prompt.prompt,
            self.nano_config.temperature,
            self.first_beat_max_output_tokens(request),
        )?;
        let text = response.text.trim().trim_matches('"').to_owned();
        if text.is_empty() {
            return Err(anyhow!("Nano did not return a first beat."));
        }

        Ok(FirstBeatResult {
            text,
            logs: vec![nano_log_entry("nano_first_beat", &response)],
        })
    }

    fn run_nano_reply(&self, request: &NanoReplyRequest) -> Result<NanoReplyResult> {
        let response = self.nano.run_prompt(
            NanoPromptMode::from_prompt_mode(request.prompt.mode),
            &request.prompt.prompt,
            Some(self.nano_config.temperature.unwrap_or(0.4) + 0.1),
            self.nano_reply_max_output_tokens(request),
        )?;
        let log_entry = nano_log_entry("nano_follow_up", &response);
        Ok(NanoReplyResult {
            raw_output: response.text,
            logs: vec![log_entry],
        })
    }

    fn run_cloud_reasoning(&self, request: &CloudReasoningRequest) -> Result<CloudReasoningResult> {
        let profile = self
            .current_cloud_profile()
            .ok_or_else(|| anyhow!("no cloud profile is available"))?;
        let gateway = profile.gateway.as_ref().ok_or_else(|| {
            anyhow!(
                "{}",
                profile
                    .unavailable_reason
                    .clone()
                    .unwrap_or_else(|| format!(
                        "cloud profile {} is unavailable",
                        profile.summary.label
                    ))
            )
        })?;
        let response = gateway.send_turn(&request.request)?;
        Ok(CloudReasoningResult {
            raw_output: response.raw_output.clone(),
            logs: vec![ModelLogEntry {
                created_at: now_rfc3339(),
                model_name: profile.summary.label.clone(),
                prompt: response.prompt_json,
                raw_output: response.raw_output,
                input_tokens: response.input_tokens,
                output_tokens: response.output_tokens,
                latency_ms: response.latency_ms,
                http_status: response.http_status,
                error_text: None,
            }],
        })
    }

    fn run_ambient(&self, request: &AmbientRequest) -> Result<Option<AmbientResult>> {
        let snapshot = self.nano.snapshot();
        if snapshot.availability != NanoAvailability::Available {
            return Ok(None);
        }

        let response = self.nano.run_prompt(
            NanoPromptMode::from_prompt_mode(request.prompt.mode),
            &request.prompt.prompt,
            self.nano_config.temperature,
            self.ambient_max_output_tokens(request),
        )?;
        let text = response.text.trim().trim_matches('"').to_owned();
        if text.is_empty() {
            return Ok(None);
        }

        Ok(Some(AmbientResult {
            text,
            logs: vec![nano_log_entry("nano_ambient", &response)],
        }))
    }

    fn overview(&self) -> RuntimeOverview {
        let snapshot = self.nano.snapshot();
        let selected_id = self.selected_cloud_profile_id();
        let profiles = self
            .cloud_profiles
            .values()
            .map(|profile| RuntimeProfileSummary {
                selected: selected_id.as_deref() == Some(profile.summary.id.as_str()),
                ..profile.summary.clone()
            })
            .collect::<Vec<_>>();
        let selected_profile = selected_id
            .as_ref()
            .and_then(|id| self.cloud_profiles.get(id))
            .map(|profile| profile.summary.label.clone());

        RuntimeOverview {
            nano: NanoRuntimeStatus {
                enabled: self.nano_config.enabled,
                availability: snapshot.availability.as_str().to_owned(),
                detail: snapshot.detail,
                provider: snapshot.provider,
                model: snapshot.model,
                active: snapshot.availability == NanoAvailability::Available,
            },
            selected_cloud_profile_id: selected_id,
            selected_cloud_profile_label: selected_profile,
            cloud_profiles: profiles,
        }
    }

    fn can_accept_turns(&self) -> bool {
        self.nano.snapshot().availability == NanoAvailability::Available
            || self.has_available_cloud_profile()
    }

    fn selected_cloud_profile_id(&self) -> Option<String> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.selected_cloud_profile_id.clone())
    }

    fn set_selected_cloud_profile(&self, profile_id: &str) -> Result<()> {
        if !self.cloud_profiles.contains_key(profile_id) {
            return Err(anyhow!("cloud profile {profile_id} is unavailable"));
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("runtime state mutex poisoned"))?;
        state.selected_cloud_profile_id = Some(profile_id.to_owned());
        Ok(())
    }

    fn default_selected_cloud_profile_id(&self) -> Option<String> {
        self.cloud_profiles.keys().next().cloned()
    }
}

pub struct GatewayCompatRuntime {
    model_name: String,
    model: Arc<dyn ModelGateway>,
}

impl GatewayCompatRuntime {
    pub fn new(model_name: String, model: Arc<dyn ModelGateway>) -> Self {
        Self { model_name, model }
    }
}

impl ModelRuntime for GatewayCompatRuntime {
    fn run_first_beat(&self, _request: &FirstBeatRequest) -> Result<FirstBeatResult> {
        Ok(FirstBeatResult {
            text: String::new(),
            logs: Vec::new(),
        })
    }

    fn run_nano_reply(&self, _request: &NanoReplyRequest) -> Result<NanoReplyResult> {
        Err(anyhow!(
            "compatibility runtime does not provide Nano replies"
        ))
    }

    fn run_cloud_reasoning(&self, request: &CloudReasoningRequest) -> Result<CloudReasoningResult> {
        let response = self.model.send_turn(&request.request)?;
        Ok(CloudReasoningResult {
            raw_output: response.raw_output.clone(),
            logs: vec![ModelLogEntry {
                created_at: now_rfc3339(),
                model_name: self.model_name.clone(),
                prompt: response.prompt_json,
                raw_output: response.raw_output,
                input_tokens: response.input_tokens,
                output_tokens: response.output_tokens,
                latency_ms: response.latency_ms,
                http_status: response.http_status,
                error_text: None,
            }],
        })
    }

    fn run_ambient(&self, _request: &AmbientRequest) -> Result<Option<AmbientResult>> {
        Ok(None)
    }

    fn overview(&self) -> RuntimeOverview {
        RuntimeOverview {
            nano: NanoRuntimeStatus {
                enabled: false,
                availability: "disabled".to_owned(),
                detail: "Compatibility runtime does not provide Nano.".to_owned(),
                provider: "gemini".to_owned(),
                model: "gemini-nano".to_owned(),
                active: false,
            },
            selected_cloud_profile_id: Some("compat".to_owned()),
            selected_cloud_profile_label: Some(self.model_name.clone()),
            cloud_profiles: vec![RuntimeProfileSummary {
                id: "compat".to_owned(),
                label: self.model_name.clone(),
                provider: "compat".to_owned(),
                model: self.model_name.clone(),
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
        Some("compat".to_owned())
    }

    fn set_selected_cloud_profile(&self, _profile_id: &str) -> Result<()> {
        Ok(())
    }

    fn default_selected_cloud_profile_id(&self) -> Option<String> {
        Some("compat".to_owned())
    }
}

fn choose_selected_profile(
    requested: Option<&str>,
    profiles: Vec<(String, bool)>,
) -> Option<String> {
    if let Some(requested) = requested {
        if profiles.iter().any(|(candidate, _)| candidate == requested) {
            return Some(requested.to_owned());
        }
    }
    profiles
        .iter()
        .find(|(_, available)| *available)
        .map(|(profile_id, _)| profile_id.clone())
        .or_else(|| {
            profiles
                .into_iter()
                .next()
                .map(|(profile_id, _)| profile_id)
        })
}

fn normalize_nano_max_output_tokens(requested: Option<u32>, fallback: u32) -> u32 {
    match requested {
        Some(0) => fallback.clamp(1, 256),
        Some(value) => value.clamp(1, 256),
        None => fallback.clamp(1, 256),
    }
}

fn nano_log_entry(stage: &str, response: &NanoPromptResponse) -> ModelLogEntry {
    ModelLogEntry {
        created_at: now_rfc3339(),
        model_name: match stage {
            "nano_first_beat" => "Nano first beat".to_owned(),
            "nano_ambient" => "Nano ambient".to_owned(),
            _ => "Nano follow-up".to_owned(),
        },
        prompt: response.prompt.clone(),
        raw_output: response.text.clone(),
        input_tokens: None,
        output_tokens: None,
        latency_ms: response.latency_ms,
        http_status: Some(200),
        error_text: None,
    }
}

fn build_gateway(config: &ModelConfig) -> Result<Arc<dyn ModelGateway>> {
    match config.provider.to_lowercase().as_str() {
        "openai" => {
            Ok(Arc::new(OpenAiCompatibleModel::new(config.clone())?) as Arc<dyn ModelGateway>)
        }
        "gemini" => Ok(Arc::new(GeminiModel::new(config.clone())?) as Arc<dyn ModelGateway>),
        provider => Err(anyhow!("unsupported cloud provider {provider}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct RecordingNanoAdapter {
        requested_tokens: Mutex<Vec<Option<u32>>>,
    }

    impl NanoAdapter for RecordingNanoAdapter {
        fn snapshot(&self) -> NanoSnapshot {
            NanoSnapshot {
                availability: NanoAvailability::Available,
                detail: "Gemini Nano is ready.".to_owned(),
                provider: "gemini".to_owned(),
                model: "gemini-nano".to_owned(),
            }
        }

        fn run_prompt(
            &self,
            mode: NanoPromptMode,
            _prompt: &str,
            _temperature: Option<f32>,
            max_output_tokens: Option<u32>,
        ) -> Result<NanoPromptResponse> {
            self.requested_tokens
                .lock()
                .unwrap()
                .push(max_output_tokens);
            let text = match mode {
                NanoPromptMode::FirstBeat => "Sure.".to_owned(),
                _ => r#"{"assistant_reply":"More detail.","tool_request":null,"memory_candidates":[]}"#
                    .to_owned(),
            };
            Ok(NanoPromptResponse {
                text,
                snapshot: self.snapshot(),
                prompt: "prompt".to_owned(),
                latency_ms: 1,
                error_text: None,
                requested_max_output_tokens: max_output_tokens,
                effective_max_output_tokens: max_output_tokens,
            })
        }
    }

    #[test]
    fn normalize_nano_max_output_tokens_caps_above_mlkit_limit() {
        assert_eq!(normalize_nano_max_output_tokens(Some(512), 48), 256);
        assert_eq!(normalize_nano_max_output_tokens(Some(0), 48), 48);
        assert_eq!(normalize_nano_max_output_tokens(None, 48), 48);
    }

    #[test]
    fn runtime_full_reply_never_requests_more_than_256_tokens() {
        let nano = Arc::new(RecordingNanoAdapter::default());
        let runtime = DefaultModelRuntime::from_resolver(
            RuntimeConfigResolver {
                config: RuntimeModelConfig {
                    nano: NanoRuntimeConfig {
                        enabled: true,
                        provider: "gemini".to_owned(),
                        model: "gemini-nano".to_owned(),
                        max_output_tokens: Some(1024),
                        temperature: Some(0.4),
                    },
                    cloud: RuntimeCloudConfig {
                        selected_profile: None,
                        profiles: HashMap::new(),
                    },
                },
                invalid_profiles: Vec::new(),
            },
            nano.clone(),
        )
        .expect("runtime should build");

        let first_beat = runtime
            .run_first_beat(&FirstBeatRequest {
                prompt: CompiledNanoPrompt {
                    mode: PromptMode::FirstBeat,
                    prompt: "Explain gravity".to_owned(),
                    max_output_tokens: Some(256),
                },
            })
            .expect("first beat should succeed");
        assert_eq!(first_beat.text, "Sure.");

        runtime
            .run_nano_reply(&NanoReplyRequest {
                prompt: CompiledNanoPrompt {
                    mode: PromptMode::NanoReply,
                    prompt: "Explain gravity".to_owned(),
                    max_output_tokens: Some(256),
                },
            })
            .expect("nano follow-up should succeed");

        let requested_tokens = nano.requested_tokens.lock().unwrap().clone();
        assert_eq!(requested_tokens, vec![Some(256), Some(256)]);
    }

    #[test]
    fn runtime_marks_placeholder_cloud_profiles_unavailable_and_fails_locally() {
        let nano = Arc::new(RecordingNanoAdapter::default());
        let runtime = DefaultModelRuntime::from_resolver(
            RuntimeConfigResolver {
                config: RuntimeModelConfig {
                    nano: default_nano_config(),
                    cloud: RuntimeCloudConfig {
                        selected_profile: Some("gemini_flash_lite".to_owned()),
                        profiles: HashMap::from([(
                            "gemini_flash_lite".to_owned(),
                            ModelConfig {
                                provider: "gemini".to_owned(),
                                api_key: "YOUR_GEMINI_KEY".to_owned(),
                                model: "gemini-2.5-flash-lite".to_owned(),
                                label: Some("Gemini Flash Lite".to_owned()),
                                endpoint: String::new(),
                                temperature: Some(0.7),
                                timeout_ms: 45_000,
                                stream: false,
                                reasoning: None,
                                enabled: true,
                            },
                        )]),
                    },
                },
                invalid_profiles: Vec::new(),
            },
            nano,
        )
        .expect("runtime should build");

        let overview = runtime.overview();
        let selected = overview
            .cloud_profiles
            .iter()
            .find(|profile| profile.id == "gemini_flash_lite")
            .expect("missing profile summary");

        assert!(!selected.available);
        assert!(selected.selected);

        let error = runtime
            .run_cloud_reasoning(&CloudReasoningRequest {
                request: ModelRequest {
                    messages: Vec::new(),
                },
            })
            .expect_err("placeholder key should fail before provider call");

        assert!(error
            .to_string()
            .contains("Gemini Flash Lite is missing a valid API key."));
    }

    #[test]
    fn runtime_accepts_turns_when_cloud_is_available_even_without_nano() {
        let runtime = DefaultModelRuntime::from_resolver(
            RuntimeConfigResolver {
                config: RuntimeModelConfig {
                    nano: NanoRuntimeConfig {
                        enabled: false,
                        provider: "gemini".to_owned(),
                        model: "gemini-nano".to_owned(),
                        max_output_tokens: Some(48),
                        temperature: Some(0.4),
                    },
                    cloud: RuntimeCloudConfig {
                        selected_profile: Some("openai_mini".to_owned()),
                        profiles: HashMap::from([(
                            "openai_mini".to_owned(),
                            ModelConfig {
                                provider: "openai".to_owned(),
                                api_key: "test-key".to_owned(),
                                model: "gpt-5-mini".to_owned(),
                                label: Some("OpenAI Mini".to_owned()),
                                endpoint: "https://api.openai.com/v1/chat/completions".to_owned(),
                                temperature: Some(0.7),
                                timeout_ms: 45_000,
                                stream: false,
                                reasoning: None,
                                enabled: true,
                            },
                        )]),
                    },
                },
                invalid_profiles: Vec::new(),
            },
            Arc::new(DisabledNanoAdapter::new("Nano is unavailable.")),
        )
        .expect("runtime should build");

        assert!(runtime.can_accept_turns());
    }

    #[test]
    fn local_embedding_runtime_is_deterministic() {
        let runtime = LocalDeterministicEmbeddingRuntime::new(16);

        let first = runtime.embed("hello notes").expect("embedding");
        let second = runtime.embed("hello notes").expect("embedding");

        assert_eq!(runtime.runtime_id(), "local_deterministic_v1");
        assert_eq!(first.len(), 16);
        assert_eq!(first, second);
    }

    #[test]
    fn local_embedding_runtime_batches_without_network() {
        let runtime = LocalDeterministicEmbeddingRuntime::new(8);
        let inputs = vec!["first note".to_owned(), "second note".to_owned()];

        let batch = runtime.embed_batch(&inputs).expect("batch embedding");

        assert_eq!(batch.len(), 2);
        assert!(batch.iter().all(|vector| vector.len() == 8));
    }
}
