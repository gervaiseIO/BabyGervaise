mod spotify;

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::memory::MemoryStore;
use crate::now_rfc3339;
use spotify::{SpotifyAdapter, SpotifyState};

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolActionAvailability {
    pub action_id: String,
    pub label: String,
    pub enabled: bool,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolDetailLine {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolOverviewEntry {
    pub tool_id: String,
    pub display_name: String,
    pub category: String,
    pub available: bool,
    pub integrated: bool,
    pub auth_state: String,
    pub health_state: String,
    pub next_step: String,
    pub summary: String,
    #[serde(default)]
    pub account_label: Option<String>,
    #[serde(default)]
    pub capability_summary: Option<String>,
    #[serde(default)]
    pub detail_lines: Vec<ToolDetailLine>,
    #[serde(default)]
    pub actions: Vec<ToolActionAvailability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsOverview {
    #[serde(default)]
    pub catalog: Vec<ToolOverviewEntry>,
    #[serde(default)]
    pub available_tools: Vec<String>,
    #[serde(default)]
    pub integrated_tools: Vec<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolIntentMatch {
    #[serde(default)]
    pub exact_request: Option<ToolRequest>,
    #[serde(default)]
    pub probable_tool: Option<ToolName>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionResult {
    pub tool: ToolName,
    pub action: String,
    pub summary: String,
    pub state_json: Value,
    pub result_json: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisibleToolCard {
    pub tool: String,
    pub action: String,
    pub status: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub supporting_lines: Vec<String>,
    pub tone: String,
    pub icon: String,
    pub comparison_text: String,
}

impl ToolExecutionResult {
    pub fn is_success(&self) -> bool {
        matches!(
            self.result_json.get("status").and_then(Value::as_str),
            Some(
                "success"
                    | "auth_started"
                    | "connected"
                    | "disconnected"
                    | "connecting"
                    | "unconfigured"
            )
        )
    }

    pub fn visible_card(&self) -> VisibleToolCard {
        let status = self
            .result_json
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        match self.tool {
            ToolName::Spotify => spotify_visible_card(self, status),
            ToolName::Hue => hue_visible_card(self, status),
        }
    }
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
    spotify: SpotifyAdapter,
}

impl ToolExecutor {
    pub fn new(memory: MemoryStore) -> Self {
        Self {
            memory,
            spotify: SpotifyAdapter::disabled(),
        }
    }

    pub fn with_spotify(memory: MemoryStore, app_files_dir: &Path, config_dir: &Path) -> Self {
        let spotify = SpotifyAdapter::new(app_files_dir, config_dir);
        let executor = Self { memory, spotify };
        executor.seed_passive_tool_state();
        executor
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

    pub fn begin_tool_auth(&self, tool: &str) -> Result<ToolExecutionResult> {
        match ToolName::parse(tool)? {
            ToolName::Spotify => self.execute(&ToolRequest {
                tool: ToolName::Spotify,
                action: "start_auth".to_owned(),
                arguments: json!({}),
            }),
            ToolName::Hue => Err(anyhow!("tool does not support authentication: {tool}")),
        }
    }

    pub fn disconnect_tool(&self, tool: &str) -> Result<ToolExecutionResult> {
        match ToolName::parse(tool)? {
            ToolName::Spotify => self.execute(&ToolRequest {
                tool: ToolName::Spotify,
                action: "disconnect".to_owned(),
                arguments: json!({}),
            }),
            ToolName::Hue => Err(anyhow!("tool does not support disconnect: {tool}")),
        }
    }

    pub fn refresh_tool_state(&self, tool: &str) -> Result<ToolExecutionResult> {
        match ToolName::parse(tool)? {
            ToolName::Spotify => self.execute(&ToolRequest {
                tool: ToolName::Spotify,
                action: "get_connection_state".to_owned(),
                arguments: json!({}),
            }),
            ToolName::Hue => self.hue_status_result(),
        }
    }

    pub fn complete_tool_auth_callback(
        &self,
        tool: &str,
        callback_url: &str,
    ) -> Result<ToolExecutionResult> {
        match ToolName::parse(tool)? {
            ToolName::Spotify => self.execute(&ToolRequest {
                tool: ToolName::Spotify,
                action: "handle_callback".to_owned(),
                arguments: json!({
                    "callback_url": callback_url,
                }),
            }),
            ToolName::Hue => Err(anyhow!("tool does not support auth callbacks: {tool}")),
        }
    }

    pub fn overview(&self) -> Result<ToolsOverview> {
        let catalog = vec![
            self.tool_status(ToolName::Hue)?,
            self.tool_status(ToolName::Spotify)?,
        ];
        let available_tools = catalog
            .iter()
            .filter(|entry| entry.available)
            .map(|entry| entry.tool_id.clone())
            .collect::<Vec<_>>();
        let integrated_tools = catalog
            .iter()
            .filter(|entry| entry.integrated)
            .map(|entry| entry.tool_id.clone())
            .collect::<Vec<_>>();

        Ok(ToolsOverview {
            catalog,
            available_tools,
            integrated_tools,
        })
    }

    pub fn tool_status(&self, tool: ToolName) -> Result<ToolOverviewEntry> {
        match tool {
            ToolName::Spotify => self.spotify_overview_entry(),
            ToolName::Hue => self.hue_overview_entry(),
        }
    }

    pub fn detect_tool_intent(&self, text: &str) -> ToolIntentMatch {
        let lower = text.to_lowercase();

        if lower.contains("connect spotify") || lower.contains("sign in to spotify") {
            return ToolIntentMatch {
                exact_request: Some(ToolRequest {
                    tool: ToolName::Spotify,
                    action: "start_auth".to_owned(),
                    arguments: json!({}),
                }),
                probable_tool: None,
            };
        }
        if lower.contains("disconnect spotify") {
            return ToolIntentMatch {
                exact_request: Some(ToolRequest {
                    tool: ToolName::Spotify,
                    action: "disconnect".to_owned(),
                    arguments: json!({}),
                }),
                probable_tool: None,
            };
        }
        if lower.contains("what is playing on spotify")
            || lower.contains("what's playing on spotify")
            || lower.contains("spotify current playback")
        {
            return ToolIntentMatch {
                exact_request: Some(ToolRequest {
                    tool: ToolName::Spotify,
                    action: "current_playback".to_owned(),
                    arguments: json!({}),
                }),
                probable_tool: None,
            };
        }
        if let Some(query) = extract_after_keyword(text, "play ") {
            let cleaned = query
                .replace(" on spotify", "")
                .replace(" using spotify", "")
                .trim()
                .to_owned();
            if !cleaned.is_empty()
                && !cleaned.contains("lights")
                && !cleaned.contains("lamp")
                && (lower.contains("spotify") || !lower.contains("light"))
            {
                return ToolIntentMatch {
                    exact_request: Some(ToolRequest {
                        tool: ToolName::Spotify,
                        action: "play".to_owned(),
                        arguments: json!({ "query": cleaned }),
                    }),
                    probable_tool: None,
                };
            }
        }
        if (lower.contains("pause spotify") || lower == "pause") && !lower.contains("light") {
            return ToolIntentMatch {
                exact_request: Some(ToolRequest {
                    tool: ToolName::Spotify,
                    action: "pause".to_owned(),
                    arguments: json!({}),
                }),
                probable_tool: None,
            };
        }
        if lower.contains("turn on") && lower.contains("light") {
            return ToolIntentMatch {
                exact_request: Some(ToolRequest {
                    tool: ToolName::Hue,
                    action: "set_power".to_owned(),
                    arguments: json!({ "on": true }),
                }),
                probable_tool: None,
            };
        }
        if lower.contains("turn off") && lower.contains("light") {
            return ToolIntentMatch {
                exact_request: Some(ToolRequest {
                    tool: ToolName::Hue,
                    action: "set_power".to_owned(),
                    arguments: json!({ "on": false }),
                }),
                probable_tool: None,
            };
        }
        if let Some(color) = extract_after_keyword(text, "set the lights to ") {
            let color = color.trim();
            if !color.is_empty() {
                return ToolIntentMatch {
                    exact_request: Some(ToolRequest {
                        tool: ToolName::Hue,
                        action: "set_color".to_owned(),
                        arguments: json!({ "color": color }),
                    }),
                    probable_tool: None,
                };
            }
        }

        let probable_tool = if lower.contains("spotify") || lower.contains("playing") {
            Some(ToolName::Spotify)
        } else if lower.contains("hue") || lower.contains("light") || lower.contains("lights") {
            Some(ToolName::Hue)
        } else {
            None
        };

        ToolIntentMatch {
            exact_request: None,
            probable_tool,
        }
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

        let outcome = self
            .spotify
            .execute(&request.action, &request.arguments, &state);

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

    fn seed_passive_tool_state(&self) {
        let existing_state = match self.memory.get_tool_state("spotify") {
            Ok(state) => state,
            Err(_) => return,
        };
        if existing_state.is_some() {
            return;
        }

        let seeded_state = self
            .spotify
            .passive_state_snapshot(&SpotifyState::default());
        let state_json = match serde_json::to_value(&seeded_state) {
            Ok(value) => value,
            Err(_) => return,
        };
        let _ = self
            .memory
            .set_tool_state("spotify", &state_json, &now_rfc3339());
    }

    fn spotify_overview_entry(&self) -> Result<ToolOverviewEntry> {
        let state = self.spotify_state_value()?;
        let available = value_bool(&state, "configured");
        let integrated = value_bool(&state, "connected");
        let connection_status = value_string(&state, "connection_status").unwrap_or("unknown");
        let capability_status = value_string(&state, "capability_status").unwrap_or("unknown");
        let token_status = value_string(&state, "token_status").unwrap_or("unknown");
        let auth_state = match (
            connection_status,
            token_status,
            value_bool(&state, "auth_in_progress"),
        ) {
            (_, _, true) => "auth_in_progress",
            ("connected", "valid", _) => "connected",
            ("expired", _, _) | (_, "expired", _) => "expired",
            ("error", _, _) | (_, "refresh_failed", _) => "error",
            _ if available => "required_not_started",
            _ => "unavailable",
        }
        .to_owned();
        let health_state = match capability_status {
            "connected" => "healthy",
            "no_available_device"
            | "connected_but_profile_unavailable"
            | "connected_but_playback_unavailable"
            | "playback_forbidden"
            | "premium_required" => "degraded",
            "auth_required" | "auth_expired" | "invalid_scope" => "degraded",
            _ if available => "unavailable",
            _ => "error",
        }
        .to_owned();
        let next_step = match capability_status {
            "connected" => "ready",
            "no_available_device" => "missing_target_device",
            "invalid_scope" => "reconnect_required",
            "auth_expired" => "reconnect_required",
            "premium_required" => "configuration_required",
            "playback_forbidden" => "temporary_error",
            _ if integrated => "ready",
            _ if available => "auth_required",
            _ => "configuration_required",
        }
        .to_owned();

        let account_label = value_string(&state, "account_display_name")
            .or_else(|| value_string(&state, "spotify_user_id"))
            .map(ToOwned::to_owned);
        let capability_summary = value_string(&state, "capability_summary").map(ToOwned::to_owned);

        let mut detail_lines = Vec::new();
        if let Some(device) = value_string(&state, "device_name") {
            detail_lines.push(ToolDetailLine {
                label: "Device".to_owned(),
                value: device.to_owned(),
            });
        }
        if let Some(last_auth) = value_string(&state, "last_authenticated_at") {
            detail_lines.push(ToolDetailLine {
                label: "Last auth".to_owned(),
                value: last_auth.to_owned(),
            });
        }
        if let Some(last_error) = value_string(&state, "last_error") {
            detail_lines.push(ToolDetailLine {
                label: "Last issue".to_owned(),
                value: last_error.to_owned(),
            });
        }

        Ok(ToolOverviewEntry {
            tool_id: "spotify".to_owned(),
            display_name: "Spotify".to_owned(),
            category: "media".to_owned(),
            available,
            integrated,
            auth_state,
            health_state,
            next_step,
            summary: capability_summary
                .clone()
                .or_else(|| {
                    account_label
                        .as_ref()
                        .map(|account| format!("Connected as {account}"))
                })
                .unwrap_or_else(|| {
                    if available {
                        "Spotify is available but not connected.".to_owned()
                    } else {
                        "Spotify is not configured in this build.".to_owned()
                    }
                }),
            account_label,
            capability_summary,
            detail_lines,
            actions: vec![
                ToolActionAvailability {
                    action_id: "begin_auth".to_owned(),
                    label: if integrated {
                        "Reconnect".to_owned()
                    } else {
                        "Connect".to_owned()
                    },
                    enabled: available,
                    reason: (!available).then_some("Spotify config is missing.".to_owned()),
                },
                ToolActionAvailability {
                    action_id: "refresh_state".to_owned(),
                    label: "Refresh".to_owned(),
                    enabled: available,
                    reason: None,
                },
                ToolActionAvailability {
                    action_id: "disconnect".to_owned(),
                    label: "Disconnect".to_owned(),
                    enabled: available,
                    reason: None,
                },
            ],
        })
    }

    fn hue_overview_entry(&self) -> Result<ToolOverviewEntry> {
        let state = self.hue_state_value()?;
        let power = state.get("power").and_then(Value::as_bool).unwrap_or(false);
        let brightness = state
            .get("brightness")
            .and_then(Value::as_u64)
            .unwrap_or(50);
        let color = state
            .get("color")
            .and_then(Value::as_str)
            .unwrap_or("warm-white");

        Ok(ToolOverviewEntry {
            tool_id: "hue".to_owned(),
            display_name: "Hue".to_owned(),
            category: "home".to_owned(),
            available: true,
            integrated: true,
            auth_state: "not_required".to_owned(),
            health_state: "healthy".to_owned(),
            next_step: "ready".to_owned(),
            summary: if power {
                format!("Hue lights are on at {brightness}% in {color}.")
            } else {
                "Hue lights are off.".to_owned()
            },
            account_label: None,
            capability_summary: Some("Local capability adapter".to_owned()),
            detail_lines: vec![
                ToolDetailLine {
                    label: "Power".to_owned(),
                    value: if power { "On" } else { "Off" }.to_owned(),
                },
                ToolDetailLine {
                    label: "Brightness".to_owned(),
                    value: format!("{brightness}%"),
                },
                ToolDetailLine {
                    label: "Color".to_owned(),
                    value: color.to_owned(),
                },
            ],
            actions: Vec::new(),
        })
    }

    fn spotify_state_value(&self) -> Result<Value> {
        let current_state: SpotifyState = self
            .memory
            .get_tool_state("spotify")?
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        Ok(serde_json::to_value(
            self.spotify.passive_state_snapshot(&current_state),
        )?)
    }

    fn hue_state_value(&self) -> Result<Value> {
        Ok(self.memory.get_tool_state("hue")?.unwrap_or_else(|| {
            json!({
                "power": false,
                "brightness": 50,
                "color": "warm-white",
            })
        }))
    }

    fn hue_status_result(&self) -> Result<ToolExecutionResult> {
        let state_json = self.hue_state_value()?;
        let power = state_json
            .get("power")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let brightness = state_json
            .get("brightness")
            .and_then(Value::as_u64)
            .unwrap_or(50);
        let color = state_json
            .get("color")
            .and_then(Value::as_str)
            .unwrap_or("warm-white")
            .to_owned();
        let summary = if power {
            format!("Hue lights are on at {brightness}% in {color}.")
        } else {
            "Hue lights are off.".to_owned()
        };
        Ok(ToolExecutionResult {
            tool: ToolName::Hue,
            action: "refresh_state".to_owned(),
            summary: summary.clone(),
            state_json,
            result_json: json!({
                "status": "success",
                "message": summary,
                "power": power,
                "brightness": brightness,
                "color": color,
            }),
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

fn spotify_visible_card(result: &ToolExecutionResult, status: &str) -> VisibleToolCard {
    let message = result
        .result_json
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or(result.summary.as_str());
    let account = result
        .result_json
        .get("account_display_name")
        .and_then(Value::as_str);
    let device_name = result
        .result_json
        .pointer("/target_device/name")
        .and_then(Value::as_str)
        .or_else(|| {
            result
                .result_json
                .get("device_name")
                .and_then(Value::as_str)
        });
    let track_name = result
        .result_json
        .pointer("/track/name")
        .and_then(Value::as_str);
    let track_artists = result
        .result_json
        .pointer("/track/artists")
        .and_then(Value::as_array)
        .map(|artists| {
            artists
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|artists| !artists.is_empty());

    let mut supporting_lines = Vec::new();
    if let Some(account) = account {
        supporting_lines.push(format!("Connected as {account}."));
    }
    if let Some(device_name) = device_name {
        supporting_lines.push(format!("Playback device: {device_name}"));
    }

    match status {
        "connected" => VisibleToolCard {
            tool: "spotify".to_owned(),
            action: result.action.clone(),
            status: status.to_owned(),
            title: "Spotify connection".to_owned(),
            body: message.to_owned(),
            supporting_lines,
            tone: "positive".to_owned(),
            icon: "spotify".to_owned(),
            comparison_text: message.to_owned(),
        },
        "auth_started" | "connecting" => VisibleToolCard {
            tool: "spotify".to_owned(),
            action: result.action.clone(),
            status: status.to_owned(),
            title: "Spotify sign-in".to_owned(),
            body: message.to_owned(),
            supporting_lines: vec![
                "A browser step is required before playback control is available.".to_owned(),
            ],
            tone: "progress".to_owned(),
            icon: "spotify".to_owned(),
            comparison_text: message.to_owned(),
        },
        "auth_required" | "auth_expired" | "unconfigured" => VisibleToolCard {
            tool: "spotify".to_owned(),
            action: result.action.clone(),
            status: status.to_owned(),
            title: "Spotify needs attention".to_owned(),
            body: message.to_owned(),
            supporting_lines,
            tone: "warning".to_owned(),
            icon: "warning".to_owned(),
            comparison_text: message.to_owned(),
        },
        "error"
        | "forbidden"
        | "rate_limited"
        | "no_available_device"
        | "device_not_found"
        | "playback_not_active" => VisibleToolCard {
            tool: "spotify".to_owned(),
            action: result.action.clone(),
            status: status.to_owned(),
            title: "Spotify couldn't complete that".to_owned(),
            body: message.to_owned(),
            supporting_lines,
            tone: "error".to_owned(),
            icon: "error".to_owned(),
            comparison_text: message.to_owned(),
        },
        _ => {
            let body = match result.action.as_str() {
                "play" => match (track_name, track_artists.as_deref(), device_name) {
                    (Some(track_name), Some(artists), Some(device_name)) => {
                        format!("Playing {track_name} by {artists} on {device_name}.")
                    }
                    (Some(track_name), Some(artists), None) => {
                        format!("Playing {track_name} by {artists}.")
                    }
                    _ => message.to_owned(),
                },
                "pause" => message.to_owned(),
                _ => message.to_owned(),
            };
            VisibleToolCard {
                tool: "spotify".to_owned(),
                action: result.action.clone(),
                status: status.to_owned(),
                title: match result.action.as_str() {
                    "play" | "pause" | "next_track" | "previous_track" | "resume_playback" => {
                        "Spotify playback".to_owned()
                    }
                    "set_volume" => "Spotify volume".to_owned(),
                    "disconnect" => "Spotify disconnected".to_owned(),
                    _ => "Spotify update".to_owned(),
                },
                body: body.clone(),
                supporting_lines,
                tone: "neutral".to_owned(),
                icon: "spotify".to_owned(),
                comparison_text: body,
            }
        }
    }
}

fn hue_visible_card(result: &ToolExecutionResult, status: &str) -> VisibleToolCard {
    let message = result
        .result_json
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or(result.summary.as_str())
        .to_owned();
    let mut supporting_lines = Vec::new();
    if let Some(level) = result.result_json.get("level").and_then(Value::as_u64) {
        supporting_lines.push(format!("Brightness: {level}%"));
    }
    if let Some(color) = result.result_json.get("color").and_then(Value::as_str) {
        supporting_lines.push(format!("Color: {color}"));
    }
    if let Some(scene) = result.result_json.get("scene").and_then(Value::as_str) {
        supporting_lines.push(format!("Scene: {scene}"));
    }
    VisibleToolCard {
        tool: "hue".to_owned(),
        action: result.action.clone(),
        status: status.to_owned(),
        title: match result.action.as_str() {
            "set_power" => "Hue lights".to_owned(),
            "set_brightness" => "Hue brightness".to_owned(),
            "set_color" => "Hue color".to_owned(),
            "activate_scene" => "Hue scene".to_owned(),
            _ => "Hue update".to_owned(),
        },
        body: message.clone(),
        supporting_lines,
        tone: if status == "error" {
            "error".to_owned()
        } else {
            "positive".to_owned()
        },
        icon: if status == "error" {
            "error".to_owned()
        } else {
            "hue".to_owned()
        },
        comparison_text: message,
    }
}

fn extract_after_keyword<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let lower = text.to_lowercase();
    let start = lower.find(keyword)?;
    Some(text.get(start + keyword.len()..)?.trim())
}

fn value_bool<'a>(value: &'a Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn value_string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}
