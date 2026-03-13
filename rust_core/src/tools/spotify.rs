use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::blocking::{Client, RequestBuilder};
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use time::format_description::well_known::Rfc3339;
use time::{Duration as TimeDuration, OffsetDateTime};

use crate::load_config;

use super::SpotifyState;

const DEFAULT_HTTP_TIMEOUT_MS: u64 = 10_000;
const TOKEN_REFRESH_BUFFER_SECONDS: i64 = 60;
const SPOTIFY_TOKEN_FILE: &str = "spotify_tokens.json";
const SPOTIFY_TOOL_NAME: &str = "spotify";
const SPOTIFY_AUTH_REQUIRED_MESSAGE: &str = "Spotify login required";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SpotifyConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    #[serde(default = "default_scopes")]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub authorization_code: Option<String>,
    #[serde(default = "default_accounts_base_url")]
    pub accounts_base_url: String,
    #[serde(default = "default_api_base_url")]
    pub api_base_url: String,
    #[serde(default = "default_http_timeout_ms")]
    pub timeout_ms: u64,
}

impl SpotifyConfig {
    fn is_configured(&self) -> bool {
        !self.client_id.trim().is_empty()
            && !self.client_secret.trim().is_empty()
            && self.client_id != "YOUR_CLIENT_ID"
            && self.client_secret != "YOUR_CLIENT_SECRET"
            && !self.redirect_uri.trim().is_empty()
    }
}

fn default_scopes() -> Vec<String> {
    vec![
        "user-read-playback-state".to_owned(),
        "user-modify-playback-state".to_owned(),
        "user-read-currently-playing".to_owned(),
    ]
}

fn default_accounts_base_url() -> String {
    "https://accounts.spotify.com".to_owned()
}

fn default_api_base_url() -> String {
    "https://api.spotify.com/v1".to_owned()
}

fn default_http_timeout_ms() -> u64 {
    DEFAULT_HTTP_TIMEOUT_MS
}

#[derive(Debug, Clone)]
pub(crate) struct SpotifyAdapter {
    client: Client,
    config: SpotifyConfig,
    token_path: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct SpotifyOutcome {
    pub result_json: Value,
    pub summary: String,
    pub state: SpotifyState,
}

impl SpotifyOutcome {
    pub(crate) fn error(action: &str, mut state: SpotifyState, message: impl Into<String>) -> Self {
        let message = message.into();
        state.last_action = Some(action.to_owned());
        state.last_error = Some(message.clone());
        Self {
            result_json: json!({
                "status": "error",
                "tool": SPOTIFY_TOOL_NAME,
                "action": normalize_auth_action(action),
                "message": message
            }),
            summary: message,
            state,
        }
    }

    fn requires_auth(action: &str, mut state: SpotifyState) -> Self {
        state.last_action = Some(action.to_owned());
        state.last_error = Some(SPOTIFY_AUTH_REQUIRED_MESSAGE.to_owned());
        state.auth_connected = false;
        Self {
            result_json: json!({
                "status": "requires_auth",
                "tool": SPOTIFY_TOOL_NAME,
                "action": normalize_auth_action(action),
                "message": SPOTIFY_AUTH_REQUIRED_MESSAGE
            }),
            summary: "You need to sign in to Spotify first. Do you want to do that now?".to_owned(),
            state,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SpotifyTokens {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_at: Option<String>,
}

#[derive(Debug, Clone)]
struct TrackInfo {
    track: String,
    artist: String,
    album: String,
    uri: String,
}

#[derive(Debug, Clone)]
struct PlaybackInfo {
    track: Option<TrackInfo>,
    is_playing: bool,
    volume_percent: Option<u8>,
    device_name: Option<String>,
}

type ActionResult<T> = std::result::Result<T, String>;

impl SpotifyAdapter {
    pub(crate) fn new(app_files_dir: &Path, config_dir: &Path) -> Result<Self> {
        let config = load_config::<SpotifyConfig>(config_dir, "spotify_config.json")
            .context("failed to load spotify_config.json")?;
        let client = Client::builder()
            .timeout(Duration::from_millis(config.timeout_ms.max(1_000)))
            .build()
            .context("failed to build Spotify HTTP client")?;

        Ok(Self {
            client,
            config,
            token_path: app_files_dir.join(SPOTIFY_TOKEN_FILE),
        })
    }

    pub(crate) fn execute(
        &self,
        action: &str,
        arguments: &Value,
        current_state: &SpotifyState,
    ) -> SpotifyOutcome {
        match self.run_action(action, arguments, current_state) {
            Ok(outcome) => outcome,
            Err(message) if message == SPOTIFY_AUTH_REQUIRED_MESSAGE => {
                SpotifyOutcome::requires_auth(action, current_state.clone())
            }
            Err(message) => SpotifyOutcome::error(action, current_state.clone(), message),
        }
    }

    fn run_action(
        &self,
        action: &str,
        arguments: &Value,
        current_state: &SpotifyState,
    ) -> ActionResult<SpotifyOutcome> {
        if !self.config.is_configured() {
            return Err("Spotify credentials are not configured".to_owned());
        }

        match action {
            "auth_status" => self.auth_status(current_state),
            "start_auth" => self.start_auth(current_state),
            "handle_callback" => self.handle_callback(arguments, current_state),
            "exchange_code" => self.exchange_code_action(arguments, current_state),
            "refresh_token" => self.refresh_token_action(current_state),
            "play" => {
                let access_token = self.ensure_access_token()?;
                self.play(arguments, current_state, &access_token)
            }
            "pause" => {
                let access_token = self.ensure_access_token()?;
                self.pause(current_state, &access_token)
            }
            "next_track" => {
                let access_token = self.ensure_access_token()?;
                self.skip(
                    "next_track",
                    "/me/player/next",
                    current_state,
                    &access_token,
                )
            }
            "previous_track" => {
                let access_token = self.ensure_access_token()?;
                self.skip(
                    "previous_track",
                    "/me/player/previous",
                    current_state,
                    &access_token,
                )
            }
            "current_playback" => {
                let access_token = self.ensure_access_token()?;
                self.current_playback_action(current_state, &access_token)
            }
            "set_volume" => {
                let access_token = self.ensure_access_token()?;
                self.set_volume(arguments, current_state, &access_token)
            }
            "search_track" | "search" => {
                let access_token = self.ensure_access_token()?;
                self.search_track_action("search_track", arguments, current_state, &access_token)
            }
            other => Err(format!("Unsupported Spotify action: {other}")),
        }
    }

    fn auth_status(&self, current_state: &SpotifyState) -> ActionResult<SpotifyOutcome> {
        let mut state = next_state(current_state, "auth_status");

        match self.ensure_access_token() {
            Ok(_) => {
                state.auth_connected = true;
                state.auth_in_progress = false;
                state.pending_auth_state = None;
                Ok(SpotifyOutcome {
                    result_json: json!({
                        "status": "success",
                        "tool": SPOTIFY_TOOL_NAME,
                        "action": "auth_status",
                        "connected": true
                    }),
                    summary: "Spotify is already connected.".to_owned(),
                    state,
                })
            }
            Err(message) if message == SPOTIFY_AUTH_REQUIRED_MESSAGE => {
                state.auth_connected = false;
                Ok(SpotifyOutcome::requires_auth("auth_status", state))
            }
            Err(message) => Err(message),
        }
    }

    fn start_auth(&self, current_state: &SpotifyState) -> ActionResult<SpotifyOutcome> {
        let mut state = next_state(current_state, "start_auth");

        if self.ensure_access_token().is_ok() {
            state.auth_connected = true;
            state.auth_in_progress = false;
            state.pending_auth_state = None;
            return Ok(SpotifyOutcome {
                result_json: json!({
                    "status": "success",
                    "tool": SPOTIFY_TOOL_NAME,
                    "action": "auth_complete"
                }),
                summary: "Spotify is already connected.".to_owned(),
                state,
            });
        }

        let oauth_state = generate_oauth_state();
        let authorize_url = self.build_authorize_url(&oauth_state)?;
        state.auth_connected = false;
        state.auth_in_progress = true;
        state.pending_auth_state = Some(oauth_state);

        Ok(SpotifyOutcome {
            result_json: json!({
                "status": "auth_started",
                "tool": SPOTIFY_TOOL_NAME,
                "action": "start_auth",
                "authorize_url": authorize_url
            }),
            summary: "Opening Spotify sign-in now.".to_owned(),
            state,
        })
    }

    fn handle_callback(
        &self,
        arguments: &Value,
        current_state: &SpotifyState,
    ) -> ActionResult<SpotifyOutcome> {
        let callback_url = optional_string_argument(arguments, &["callback_url", "url"]);
        let mut state = next_state(current_state, "handle_callback");

        let callback = match callback_url {
            Some(url) => parse_callback_url(&url)?,
            None => CallbackPayload {
                code: optional_string_argument(arguments, &["code"]),
                oauth_state: optional_string_argument(arguments, &["state"]),
                error: optional_string_argument(arguments, &["error"]),
                error_description: optional_string_argument(arguments, &["error_description"]),
            },
        };

        if let Some(error) = callback.error {
            state.auth_connected = false;
            state.auth_in_progress = false;
            state.pending_auth_state = None;
            let message = callback
                .error_description
                .unwrap_or(error)
                .trim()
                .to_owned();
            return Ok(SpotifyOutcome::error(
                "auth_complete",
                state,
                if message.is_empty() {
                    "Spotify authentication failed".to_owned()
                } else {
                    message
                },
            ));
        }

        if let Some(expected_state) = current_state.pending_auth_state.as_ref() {
            if callback.oauth_state.as_deref() != Some(expected_state.as_str()) {
                state.auth_connected = false;
                state.auth_in_progress = false;
                state.pending_auth_state = None;
                return Ok(SpotifyOutcome::error(
                    "auth_complete",
                    state,
                    "Spotify authentication failed",
                ));
            }
        }

        let Some(code) = callback.code else {
            state.auth_connected = false;
            state.auth_in_progress = false;
            state.pending_auth_state = None;
            return Ok(SpotifyOutcome::error(
                "auth_complete",
                state,
                "Spotify authentication failed",
            ));
        };

        self.complete_exchange("auth_complete", code, state)
    }

    fn exchange_code_action(
        &self,
        arguments: &Value,
        current_state: &SpotifyState,
    ) -> ActionResult<SpotifyOutcome> {
        let code = optional_string_argument(arguments, &["code"])
            .or_else(|| self.config.authorization_code.clone())
            .ok_or_else(|| "Spotify action requires code".to_owned())?;
        let mut state = next_state(current_state, "exchange_code");
        if let Some(expected_state) = current_state.pending_auth_state.as_ref() {
            if let Some(received_state) = optional_string_argument(arguments, &["state"]) {
                if received_state != *expected_state {
                    state.auth_connected = false;
                    state.auth_in_progress = false;
                    state.pending_auth_state = None;
                    return Ok(SpotifyOutcome::error(
                        "auth_complete",
                        state,
                        "Spotify authentication failed",
                    ));
                }
            }
        }
        self.complete_exchange("auth_complete", code, state)
    }

    fn refresh_token_action(&self, current_state: &SpotifyState) -> ActionResult<SpotifyOutcome> {
        let mut state = next_state(current_state, "refresh_token");
        let refresh_token = self.load_effective_refresh_token()?;
        let tokens = self.refresh_access_token(&refresh_token)?;
        self.persist_token_response(tokens, Some(refresh_token))?;
        state.auth_connected = true;
        state.auth_in_progress = false;
        state.pending_auth_state = None;

        Ok(SpotifyOutcome {
            result_json: json!({
                "status": "success",
                "tool": SPOTIFY_TOOL_NAME,
                "action": "refresh_token"
            }),
            summary: "Spotify session refreshed.".to_owned(),
            state,
        })
    }

    fn play(
        &self,
        arguments: &Value,
        current_state: &SpotifyState,
        access_token: &str,
    ) -> ActionResult<SpotifyOutcome> {
        let query = optional_string_argument(arguments, &["query"]);
        let mut state = next_state(current_state, "play");

        let selected_track = if let Some(query) = &query {
            let track = self.search_top_track(access_token, query)?;
            state.last_query = Some(query.clone());
            self.put_json(
                access_token,
                "/me/player/play",
                Some(json!({ "uris": [track.uri.clone()] })),
            )?;
            Some(track)
        } else {
            self.put_json(access_token, "/me/player/play", None)?;
            None
        };

        let playback = self.best_effort_current_playback(access_token);
        let effective_track = playback
            .as_ref()
            .and_then(|item| item.track.clone())
            .or(selected_track);
        let volume_percent = playback.as_ref().and_then(|item| item.volume_percent);
        let device_name = playback.as_ref().and_then(|item| item.device_name.clone());

        state.auth_connected = true;
        state.auth_in_progress = false;
        state.pending_auth_state = None;
        state.is_playing = true;
        state.volume_percent = volume_percent.or(state.volume_percent);
        state.device_name = device_name.or(state.device_name.clone());
        if let Some(track) = &effective_track {
            apply_track(&mut state, track);
        }

        let result_json =
            success_track_result("play", effective_track.as_ref(), true, volume_percent);
        let summary = effective_track
            .as_ref()
            .map(|track| format!("Playing {} by {}.", track.track, track.artist))
            .unwrap_or_else(|| "Spotify playback started.".to_owned());

        Ok(SpotifyOutcome {
            result_json,
            summary,
            state,
        })
    }

    fn pause(
        &self,
        current_state: &SpotifyState,
        access_token: &str,
    ) -> ActionResult<SpotifyOutcome> {
        self.put_json(access_token, "/me/player/pause", None)?;
        let mut state = next_state(current_state, "pause");
        state.auth_connected = true;
        state.auth_in_progress = false;
        state.pending_auth_state = None;
        state.is_playing = false;

        Ok(SpotifyOutcome {
            result_json: json!({
                "status": "success",
                "action": "pause"
            }),
            summary: "Spotify playback paused.".to_owned(),
            state,
        })
    }

    fn skip(
        &self,
        action: &str,
        path: &str,
        current_state: &SpotifyState,
        access_token: &str,
    ) -> ActionResult<SpotifyOutcome> {
        self.post_no_body(access_token, path)?;
        let mut state = next_state(current_state, action);
        let playback = self.best_effort_current_playback(access_token);

        state.auth_connected = true;
        state.auth_in_progress = false;
        state.pending_auth_state = None;
        if let Some(playback) = &playback {
            state.is_playing = playback.is_playing;
            state.volume_percent = playback.volume_percent.or(state.volume_percent);
            state.device_name = playback.device_name.clone().or(state.device_name.clone());
            if let Some(track) = &playback.track {
                apply_track(&mut state, track);
            }
        }

        let result_json = success_track_result(
            action,
            playback.as_ref().and_then(|item| item.track.as_ref()),
            playback
                .as_ref()
                .map(|item| item.is_playing)
                .unwrap_or(true),
            playback.as_ref().and_then(|item| item.volume_percent),
        );
        let summary = playback
            .as_ref()
            .and_then(|item| item.track.as_ref())
            .map(|track| match action {
                "previous_track" => format!("Moved back to {} by {}.", track.track, track.artist),
                _ => format!("Skipped to {} by {}.", track.track, track.artist),
            })
            .unwrap_or_else(|| match action {
                "previous_track" => "Moved to the previous track.".to_owned(),
                _ => "Skipped to the next track.".to_owned(),
            });

        Ok(SpotifyOutcome {
            result_json,
            summary,
            state,
        })
    }

    fn current_playback_action(
        &self,
        current_state: &SpotifyState,
        access_token: &str,
    ) -> ActionResult<SpotifyOutcome> {
        let mut state = next_state(current_state, "current_playback");
        let playback = self.fetch_current_playback(access_token)?;
        state.auth_connected = true;
        state.auth_in_progress = false;
        state.pending_auth_state = None;

        match playback {
            Some(playback) => {
                state.is_playing = playback.is_playing;
                state.volume_percent = playback.volume_percent;
                state.device_name = playback.device_name.clone();
                if let Some(track) = &playback.track {
                    apply_track(&mut state, track);
                } else {
                    clear_track(&mut state);
                }

                let result_json = success_track_result(
                    "current_playback",
                    playback.track.as_ref(),
                    playback.is_playing,
                    playback.volume_percent,
                );
                let summary = describe_playback(&playback);

                Ok(SpotifyOutcome {
                    result_json,
                    summary,
                    state,
                })
            }
            None => {
                state.is_playing = false;
                clear_track(&mut state);

                Ok(SpotifyOutcome {
                    result_json: json!({
                        "status": "success",
                        "action": "current_playback",
                        "is_playing": false,
                        "message": "Nothing is currently playing"
                    }),
                    summary: "Nothing is currently playing.".to_owned(),
                    state,
                })
            }
        }
    }

    fn set_volume(
        &self,
        arguments: &Value,
        current_state: &SpotifyState,
        access_token: &str,
    ) -> ActionResult<SpotifyOutcome> {
        let volume_percent = parse_volume_percent(arguments)?;
        self.put_query(
            access_token,
            "/me/player/volume",
            &[("volume_percent", volume_percent.to_string())],
        )?;

        let mut state = next_state(current_state, "set_volume");
        state.auth_connected = true;
        state.auth_in_progress = false;
        state.pending_auth_state = None;
        state.volume_percent = Some(volume_percent);

        Ok(SpotifyOutcome {
            result_json: json!({
                "status": "success",
                "action": "set_volume",
                "volume_percent": volume_percent
            }),
            summary: format!("Spotify volume set to {volume_percent}."),
            state,
        })
    }

    fn search_track_action(
        &self,
        action: &str,
        arguments: &Value,
        current_state: &SpotifyState,
        access_token: &str,
    ) -> ActionResult<SpotifyOutcome> {
        let query = required_string_argument(arguments, &["query"])?;
        let track = self.search_top_track(access_token, &query)?;
        let mut state = next_state(current_state, action);
        let track_name = track.track.clone();
        let artist_name = track.artist.clone();
        let album_name = track.album.clone();
        state.auth_connected = true;
        state.auth_in_progress = false;
        state.pending_auth_state = None;
        state.last_query = Some(query.clone());
        apply_track(&mut state, &track);

        Ok(SpotifyOutcome {
            result_json: json!({
                "status": "success",
                "action": "search_track",
                "query": query,
                "track": track_name,
                "artist": artist_name,
                "album": album_name
            }),
            summary: format!("Found {} by {}.", track.track, track.artist),
            state,
        })
    }

    fn complete_exchange(
        &self,
        action: &str,
        code: String,
        mut state: SpotifyState,
    ) -> ActionResult<SpotifyOutcome> {
        let response = self.exchange_authorization_code(&code)?;
        self.persist_token_response(response, None)?;
        state.auth_connected = true;
        state.auth_in_progress = false;
        state.pending_auth_state = None;
        state.last_error = None;

        Ok(SpotifyOutcome {
            result_json: json!({
                "status": "success",
                "tool": SPOTIFY_TOOL_NAME,
                "action": action
            }),
            summary: "You're connected to Spotify now. What would you like to listen to?"
                .to_owned(),
            state,
        })
    }

    fn ensure_access_token(&self) -> ActionResult<String> {
        let tokens = self.load_effective_tokens()?;

        if let Some(access_token) = tokens.access_token.clone() {
            if token_is_valid(tokens.expires_at.as_deref()) {
                return Ok(access_token);
            }
        }

        if let Some(refresh_token) = tokens.refresh_token {
            let refreshed = self.refresh_access_token(&refresh_token)?;
            let access_token = refreshed.access_token.clone();
            self.persist_token_response(refreshed, Some(refresh_token))?;
            return Ok(access_token);
        }

        Err(SPOTIFY_AUTH_REQUIRED_MESSAGE.to_owned())
    }

    fn load_effective_tokens(&self) -> ActionResult<SpotifyTokens> {
        let mut tokens = self.load_tokens()?;
        if tokens.refresh_token.is_none() {
            tokens.refresh_token = self.config.refresh_token.clone();
        }
        Ok(tokens)
    }

    fn load_effective_refresh_token(&self) -> ActionResult<String> {
        self.load_effective_tokens()?
            .refresh_token
            .ok_or_else(|| SPOTIFY_AUTH_REQUIRED_MESSAGE.to_owned())
    }

    fn persist_token_response(
        &self,
        response: TokenResponse,
        existing_refresh_token: Option<String>,
    ) -> ActionResult<()> {
        let tokens = SpotifyTokens {
            access_token: Some(response.access_token),
            refresh_token: response.refresh_token.or(existing_refresh_token),
            expires_at: Some(expiry_timestamp(response.expires_in)),
        };
        self.save_tokens(&tokens)
    }

    fn build_authorize_url(&self, oauth_state: &str) -> ActionResult<String> {
        let mut url = Url::parse(&format!(
            "{}/authorize",
            self.config.accounts_base_url.trim_end_matches('/')
        ))
        .map_err(|error| format!("Spotify authorize URL is invalid: {error}"))?;
        url.query_pairs_mut()
            .append_pair("client_id", &self.config.client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &self.config.redirect_uri)
            .append_pair("scope", &self.config.scopes.join(" "))
            .append_pair("state", oauth_state)
            .append_pair("show_dialog", "true");
        Ok(url.to_string())
    }

    fn load_tokens(&self) -> ActionResult<SpotifyTokens> {
        if !self.token_path.exists() {
            return Ok(SpotifyTokens::default());
        }

        let contents = fs::read_to_string(&self.token_path)
            .map_err(|error| format!("Failed to read Spotify token cache: {error}"))?;
        serde_json::from_str(&contents)
            .map_err(|error| format!("Spotify token cache is invalid: {error}"))
    }

    fn save_tokens(&self, tokens: &SpotifyTokens) -> ActionResult<()> {
        if let Some(parent) = self.token_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Failed to create Spotify token directory: {error}"))?;
        }

        let payload = serde_json::to_vec_pretty(tokens)
            .map_err(|error| format!("Failed to encode Spotify token cache: {error}"))?;
        fs::write(&self.token_path, payload)
            .map_err(|error| format!("Failed to write Spotify token cache: {error}"))?;
        apply_private_permissions(&self.token_path);
        Ok(())
    }

    fn refresh_access_token(&self, refresh_token: &str) -> ActionResult<TokenResponse> {
        self.send_token_request(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
    }

    fn exchange_authorization_code(&self, code: &str) -> ActionResult<TokenResponse> {
        self.send_token_request(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", self.config.redirect_uri.as_str()),
        ])
    }

    fn send_token_request(&self, form: &[(&str, &str)]) -> ActionResult<TokenResponse> {
        let url = format!(
            "{}/api/token",
            self.config.accounts_base_url.trim_end_matches('/')
        );
        let response = self
            .client
            .post(url)
            .basic_auth(&self.config.client_id, Some(&self.config.client_secret))
            .form(form)
            .send()
            .map_err(|error| format!("Spotify authentication request failed: {error}"))?;

        let value = read_json_response(response)?;
        serde_json::from_value(value)
            .map_err(|error| format!("Spotify authentication response was invalid: {error}"))
    }

    fn search_top_track(&self, access_token: &str, query: &str) -> ActionResult<TrackInfo> {
        let url = format!("{}/search", self.config.api_base_url.trim_end_matches('/'));
        let response = self
            .client
            .get(url)
            .bearer_auth(access_token)
            .query(&[("q", query), ("type", "track"), ("limit", "1")])
            .send()
            .map_err(|error| format!("Spotify search request failed: {error}"))?;
        let value = read_json_response(response)?;
        let payload: SpotifySearchResponse = serde_json::from_value(value)
            .map_err(|error| format!("Spotify search response was invalid: {error}"))?;
        payload
            .tracks
            .items
            .into_iter()
            .next()
            .map(TrackInfo::from)
            .ok_or_else(|| format!("No Spotify track found for \"{query}\""))
    }

    fn fetch_current_playback(&self, access_token: &str) -> ActionResult<Option<PlaybackInfo>> {
        let url = format!(
            "{}/me/player",
            self.config.api_base_url.trim_end_matches('/')
        );
        let response = self
            .client
            .get(url)
            .bearer_auth(access_token)
            .send()
            .map_err(|error| format!("Spotify current playback request failed: {error}"))?;
        let value = read_optional_json_response(response)?;
        let Some(value) = value else {
            return Ok(None);
        };

        let payload: SpotifyPlaybackResponse = serde_json::from_value(value)
            .map_err(|error| format!("Spotify playback response was invalid: {error}"))?;
        let device = payload.device;
        Ok(Some(PlaybackInfo {
            track: payload.item.map(TrackInfo::from),
            is_playing: payload.is_playing,
            volume_percent: device.as_ref().and_then(|item| item.volume_percent),
            device_name: device.and_then(|item| item.name),
        }))
    }

    fn best_effort_current_playback(&self, access_token: &str) -> Option<PlaybackInfo> {
        self.fetch_current_playback(access_token).ok().flatten()
    }

    fn put_json(&self, access_token: &str, path: &str, body: Option<Value>) -> ActionResult<()> {
        let url = format!(
            "{}/{}",
            self.config.api_base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        let request = self.client.put(url).bearer_auth(access_token);
        let request = match body {
            Some(payload) => request.json(&payload),
            None => request,
        };
        self.send_empty(request)
    }

    fn put_query(
        &self,
        access_token: &str,
        path: &str,
        query: &[(&str, String)],
    ) -> ActionResult<()> {
        let url = format!(
            "{}/{}",
            self.config.api_base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        self.send_empty(self.client.put(url).bearer_auth(access_token).query(query))
    }

    fn post_no_body(&self, access_token: &str, path: &str) -> ActionResult<()> {
        let url = format!(
            "{}/{}",
            self.config.api_base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        self.send_empty(self.client.post(url).bearer_auth(access_token))
    }

    fn send_empty(&self, request: RequestBuilder) -> ActionResult<()> {
        let response = request
            .send()
            .map_err(|error| format!("Spotify playback request failed: {error}"))?;
        read_optional_json_response(response).map(|_| ())
    }
}

impl From<SpotifyTrackItem> for TrackInfo {
    fn from(value: SpotifyTrackItem) -> Self {
        let artist = value
            .artists
            .into_iter()
            .map(|artist| artist.name)
            .collect::<Vec<_>>()
            .join(", ");
        Self {
            track: value.name,
            artist,
            album: value.album.name,
            uri: value.uri,
        }
    }
}

#[derive(Debug)]
struct CallbackPayload {
    code: Option<String>,
    oauth_state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

fn parse_callback_url(callback_url: &str) -> ActionResult<CallbackPayload> {
    let url = Url::parse(callback_url)
        .map_err(|error| format!("Spotify callback URL is invalid: {error}"))?;
    let mut code = None;
    let mut oauth_state = None;
    let mut error = None;
    let mut error_description = None;

    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => oauth_state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            "error_description" => error_description = Some(value.into_owned()),
            _ => {}
        }
    }

    Ok(CallbackPayload {
        code,
        oauth_state,
        error,
        error_description,
    })
}

fn generate_oauth_state() -> String {
    format!(
        "spotify-{}",
        OffsetDateTime::now_utc().unix_timestamp_nanos()
    )
}

fn normalize_auth_action(action: &str) -> &str {
    match action {
        "handle_callback" | "exchange_code" => "auth_complete",
        other => other,
    }
}

fn next_state(current_state: &SpotifyState, action: &str) -> SpotifyState {
    let mut state = current_state.clone();
    state.last_action = Some(action.to_owned());
    state.last_error = None;
    state
}

fn apply_track(state: &mut SpotifyState, track: &TrackInfo) {
    state.track = Some(track.track.clone());
    state.artist = Some(track.artist.clone());
    state.album = Some(track.album.clone());
}

fn clear_track(state: &mut SpotifyState) {
    state.track = None;
    state.artist = None;
    state.album = None;
}

fn success_track_result(
    action: &str,
    track: Option<&TrackInfo>,
    is_playing: bool,
    volume_percent: Option<u8>,
) -> Value {
    let mut result = json!({
        "status": "success",
        "action": action,
        "is_playing": is_playing
    });

    if let Some(track) = track {
        if let Some(object) = result.as_object_mut() {
            object.insert("track".to_owned(), Value::String(track.track.clone()));
            object.insert("artist".to_owned(), Value::String(track.artist.clone()));
            object.insert("album".to_owned(), Value::String(track.album.clone()));
        }
    }

    if let Some(volume_percent) = volume_percent {
        if let Some(object) = result.as_object_mut() {
            object.insert(
                "volume_percent".to_owned(),
                Value::Number(serde_json::Number::from(volume_percent)),
            );
        }
    }

    result
}

fn describe_playback(playback: &PlaybackInfo) -> String {
    match &playback.track {
        Some(track) if playback.is_playing => {
            format!("{} by {} is playing.", track.track, track.artist)
        }
        Some(track) => format!("{} by {} is paused.", track.track, track.artist),
        None => "Spotify playback state updated.".to_owned(),
    }
}

fn required_string_argument(arguments: &Value, keys: &[&str]) -> ActionResult<String> {
    optional_string_argument(arguments, keys).ok_or_else(|| {
        let joined = keys.join(" or ");
        format!("Spotify action requires {joined}")
    })
}

fn optional_string_argument(arguments: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| arguments.get(*key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn parse_volume_percent(arguments: &Value) -> ActionResult<u8> {
    for key in ["volume_percent", "level"] {
        if let Some(value) = arguments.get(key) {
            if let Some(level) = value.as_u64() {
                return Ok(level.min(100) as u8);
            }
            if let Some(level) = value.as_i64() {
                return Ok(level.clamp(0, 100) as u8);
            }
            if let Some(level) = value.as_str() {
                let parsed = level.trim().parse::<u8>().map_err(|_| {
                    "Spotify set_volume requires a numeric volume_percent".to_owned()
                })?;
                return Ok(parsed.min(100));
            }
        }
    }

    Err("Spotify set_volume requires volume_percent".to_owned())
}

fn token_is_valid(expires_at: Option<&str>) -> bool {
    let Some(expires_at) = expires_at else {
        return false;
    };
    let Ok(expiry) = OffsetDateTime::parse(expires_at, &Rfc3339) else {
        return false;
    };
    let refresh_deadline =
        OffsetDateTime::now_utc() + TimeDuration::seconds(TOKEN_REFRESH_BUFFER_SECONDS);
    expiry > refresh_deadline
}

fn expiry_timestamp(expires_in: i64) -> String {
    (OffsetDateTime::now_utc() + TimeDuration::seconds(expires_in.max(1)))
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

fn read_json_response(response: reqwest::blocking::Response) -> ActionResult<Value> {
    read_optional_json_response(response)?.ok_or_else(|| "Spotify response was empty".to_owned())
}

fn read_optional_json_response(
    response: reqwest::blocking::Response,
) -> ActionResult<Option<Value>> {
    let status = response.status();
    let text = response
        .text()
        .map_err(|error| format!("Failed to read Spotify response: {error}"))?;

    if status == StatusCode::NO_CONTENT || text.trim().is_empty() {
        return if status.is_success() {
            Ok(None)
        } else {
            Err(normalize_spotify_error(status, &text))
        };
    }

    if !status.is_success() {
        return Err(normalize_spotify_error(status, &text));
    }

    serde_json::from_str(&text)
        .map(Some)
        .map_err(|error| format!("Spotify returned invalid JSON: {error}"))
}

fn normalize_spotify_error(status: StatusCode, body: &str) -> String {
    let fallback = format!("Spotify request failed with HTTP {}", status.as_u16());
    let mut message = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(Value::as_object)
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .or_else(|| {
                    value
                        .get("error_description")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .or_else(|| {
                    value
                        .get("error")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
        })
        .unwrap_or_else(|| fallback.clone());

    let normalized = message.to_ascii_lowercase();
    if normalized.contains("no active device") {
        return "Spotify device not available".to_owned();
    }
    if normalized.contains("premium required") || normalized.contains("only premium users") {
        return "Spotify Premium is required for playback control".to_owned();
    }
    if status == StatusCode::UNAUTHORIZED {
        return "Spotify authentication failed".to_owned();
    }

    if message.is_empty() {
        message = fallback;
    }
    message
}

#[cfg(unix)]
fn apply_private_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    if let Ok(metadata) = fs::metadata(path) {
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o600);
        let _ = fs::set_permissions(path, permissions);
    }
}

#[cfg(not(unix))]
fn apply_private_permissions(_path: &Path) {}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    expires_in: i64,
}

#[derive(Debug, Deserialize)]
struct SpotifySearchResponse {
    tracks: SpotifySearchTracks,
}

#[derive(Debug, Deserialize)]
struct SpotifySearchTracks {
    items: Vec<SpotifyTrackItem>,
}

#[derive(Debug, Deserialize)]
struct SpotifyPlaybackResponse {
    is_playing: bool,
    #[serde(default)]
    device: Option<SpotifyDevice>,
    #[serde(default)]
    item: Option<SpotifyTrackItem>,
}

#[derive(Debug, Deserialize)]
struct SpotifyDevice {
    #[serde(default)]
    volume_percent: Option<u8>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SpotifyTrackItem {
    name: String,
    uri: String,
    album: SpotifyAlbum,
    artists: Vec<SpotifyArtist>,
}

#[derive(Debug, Deserialize)]
struct SpotifyAlbum {
    name: String,
}

#[derive(Debug, Deserialize)]
struct SpotifyArtist {
    name: String,
}
