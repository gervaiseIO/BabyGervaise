use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::memory::MemoryStore;
use crate::now_rfc3339;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolName {
    Spotify,
    Hue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolRequest {
    pub tool: ToolName,
    pub action: String,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SpotifyState {
    is_playing: bool,
    last_query: Option<String>,
    volume: u8,
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
}

impl ToolExecutor {
    pub fn new(memory: MemoryStore) -> Self {
        Self { memory }
    }

    pub fn execute(&self, request: &ToolRequest) -> Result<ToolExecutionResult> {
        match request.tool {
            ToolName::Spotify => self.execute_spotify(request),
            ToolName::Hue => self.execute_hue(request),
        }
    }

    fn execute_spotify(&self, request: &ToolRequest) -> Result<ToolExecutionResult> {
        let mut state: SpotifyState = self
            .memory
            .get_tool_state("spotify")?
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or(SpotifyState {
                is_playing: false,
                last_query: None,
                volume: 50,
            });

        let result_json = match request.action.as_str() {
            "play" => {
                let query = request
                    .arguments
                    .get("query")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                if query.is_some() {
                    state.last_query = query.clone();
                }
                state.is_playing = true;
                json!({
                    "status": "ok",
                    "query": query,
                    "message": "Spotify playback started."
                })
            }
            "pause" => {
                state.is_playing = false;
                json!({
                    "status": "ok",
                    "message": "Spotify playback paused."
                })
            }
            "search" => {
                let query = request
                    .arguments
                    .get("query")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow!("spotify.search requires a query"))?;
                state.last_query = Some(query.to_owned());
                json!({
                    "status": "ok",
                    "query": query,
                    "results": [
                        { "title": format!("{query} — Demo Track"), "artist": "Baby Gervaise" },
                        { "title": format!("{query} — Live Version"), "artist": "Baby Gervaise" }
                    ]
                })
            }
            "set_volume" => {
                let level = request
                    .arguments
                    .get("level")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| anyhow!("spotify.set_volume requires a numeric level"))?;
                let level = level.min(100) as u8;
                state.volume = level;
                json!({
                    "status": "ok",
                    "level": level,
                    "message": format!("Spotify volume set to {level}.")
                })
            }
            action => return Err(anyhow!("unsupported Spotify action: {action}")),
        };

        let state_json = serde_json::to_value(&state)?;
        self.memory
            .set_tool_state("spotify", &state_json, &now_rfc3339())
            .context("failed to persist spotify state")?;

        Ok(ToolExecutionResult {
            tool: ToolName::Spotify,
            action: request.action.clone(),
            summary: result_json
                .get("message")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| "Spotify action completed.".to_owned()),
            state_json,
            result_json,
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
                    "status": "ok",
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
                    "status": "ok",
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
                    "status": "ok",
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
                    "status": "ok",
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
