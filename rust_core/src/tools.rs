mod spotify;

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::memory::MemoryStore;
use crate::now_rfc3339;
use spotify::{SpotifyAdapter, SpotifyOutcome};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolName {
    Spotify,
    Hue,
}

impl ToolName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Spotify => "spotify",
            Self::Hue => "hue",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "spotify" => Ok(Self::Spotify),
            "hue" => Ok(Self::Hue),
            other => Err(anyhow!("unsupported tool: {other}")),
        }
    }
}

fn default_tool_arguments() -> Value {
    Value::Object(Map::new())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRequest {
    pub tool: ToolName,
    pub action: String,
    #[serde(default = "default_tool_arguments")]
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionResult {
    pub tool: ToolName,
    pub action: String,
    pub summary: String,
    pub state_json: Value,
    pub result_json: Value,
}

impl ToolExecutionResult {
    pub fn is_success(&self) -> bool {
        self.result_json
            .get("status")
            .and_then(Value::as_str)
            .map(|status| status != "error")
            .unwrap_or(true)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SpotifyState {
    auth_connected: bool,
    auth_in_progress: bool,
    pending_auth_state: Option<String>,
    is_playing: bool,
    last_query: Option<String>,
    volume_percent: Option<u8>,
    track: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    device_name: Option<String>,
    last_action: Option<String>,
    last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct HueState {
    power: bool,
    brightness: u8,
    color: String,
    last_scene: Option<String>,
}

#[derive(Clone)]
pub struct ToolExecutor {
    memory: MemoryStore,
    spotify: Option<SpotifyAdapter>,
}

impl ToolExecutor {
    pub fn new(memory: MemoryStore) -> Self {
        Self {
            memory,
            spotify: None,
        }
    }

    pub fn with_spotify(memory: MemoryStore, app_files_dir: &Path, config_dir: &Path) -> Self {
        let spotify = match SpotifyAdapter::new(app_files_dir, config_dir) {
            Ok(adapter) => Some(adapter),
            Err(error) => {
                eprintln!("spotify adapter disabled: {error}");
                None
            }
        };

        Self { memory, spotify }
    }

    pub fn execute_named(
        &self,
        tool: &str,
        action: &str,
        arguments: Value,
    ) -> Result<ToolExecutionResult> {
        self.execute(&ToolRequest {
            tool: ToolName::parse(tool)?,
            action: action.to_owned(),
            arguments,
        })
    }

    pub fn execute(&self, request: &ToolRequest) -> Result<ToolExecutionResult> {
        match request.tool {
            ToolName::Spotify => self.execute_spotify(request),
            ToolName::Hue => self.execute_hue(request),
        }
    }

    fn execute_spotify(&self, request: &ToolRequest) -> Result<ToolExecutionResult> {
        let state: SpotifyState = self
            .memory
            .get_tool_state("spotify")?
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();

        let outcome = match &self.spotify {
            Some(adapter) => adapter.execute(&request.action, &request.arguments, &state),
            None => SpotifyOutcome::error(
                &request.action,
                state.clone(),
                "Spotify is not configured for this runtime",
            ),
        };

        let state_json = serde_json::to_value(&outcome.state)?;
        self.memory
            .set_tool_state("spotify", &state_json, &now_rfc3339())
            .context("failed to persist spotify state")?;

        Ok(ToolExecutionResult {
            tool: ToolName::Spotify,
            action: request.action.clone(),
            summary: outcome.summary,
            state_json,
            result_json: outcome.result_json,
        })
    }

    fn execute_hue(&self, request: &ToolRequest) -> Result<ToolExecutionResult> {
        let mut state: HueState = self
            .memory
            .get_tool_state("hue")?
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or(HueState {
                power: false,
                brightness: 50,
                color: "warm-white".to_owned(),
                last_scene: None,
            });

        let result_json = match request.action.as_str() {
            "set_power" => {
                let on = request
                    .arguments
                    .get("on")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| anyhow!("hue.set_power requires a boolean on"))?;
                state.power = on;
                json!({
                    "status": "success",
                    "on": on,
                    "message": if on { "Hue lights turned on." } else { "Hue lights turned off." }
                })
            }
            "set_brightness" => {
                let level = request
                    .arguments
                    .get("level")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| anyhow!("hue.set_brightness requires a numeric level"))?;
                let level = level.min(100) as u8;
                state.brightness = level;
                json!({
                    "status": "success",
                    "level": level,
                    "message": format!("Hue brightness set to {level}.")
                })
            }
            "set_color" => {
                let color = request
                    .arguments
                    .get("color")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("hue.set_color requires a color"))?;
                state.color = color.to_owned();
                json!({
                    "status": "success",
                    "color": color,
                    "message": format!("Hue color changed to {color}.")
                })
            }
            "activate_scene" => {
                let scene = request
                    .arguments
                    .get("scene")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("hue.activate_scene requires a scene"))?;
                state.last_scene = Some(scene.to_owned());
                json!({
                    "status": "success",
                    "scene": scene,
                    "message": format!("Hue scene {scene} activated.")
                })
            }
            action => return Err(anyhow!("unsupported Hue action: {action}")),
        };

        let state_json = serde_json::to_value(&state)?;
        self.memory
            .set_tool_state("hue", &state_json, &now_rfc3339())
            .context("failed to persist hue state")?;

        Ok(ToolExecutionResult {
            tool: ToolName::Hue,
            action: request.action.clone(),
            summary: result_json
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Hue action completed.")
                .to_owned(),
            state_json,
            result_json,
        })
    }
}
