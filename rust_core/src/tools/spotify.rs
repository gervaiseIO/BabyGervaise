use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::header::RETRY_AFTER;
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use time::format_description::well_known::Rfc3339;
use time::{Duration as TimeDuration, OffsetDateTime};

use crate::now_rfc3339;

const DEFAULT_HTTP_TIMEOUT_MS: u64 = 10_000;
const TOKEN_REFRESH_BUFFER_SECONDS: i64 = 60;
const SPOTIFY_TOKEN_FILE: &str = "spotify_tokens.json";
const SPOTIFY_TOOL_NAME: &str = "spotify";
const DEFAULT_SPOTIFY_API_BASE_URL: &str = "https://api.spotify.com/v1";
const DEFAULT_SPOTIFY_ACCOUNTS_BASE_URL: &str = "https://accounts.spotify.com";
const SPOTIFY_AUTH_REQUIRED_MESSAGE: &str =
    "Spotify authentication required. If you want, ask me to connect Spotify.";
const SPOTIFY_SCOPE_USER_READ_PRIVATE: &str = "user-read-private";
const SPOTIFY_SCOPE_USER_READ_EMAIL: &str = "user-read-email";
const SPOTIFY_SCOPE_USER_READ_PLAYBACK_STATE: &str = "user-read-playback-state";
const SPOTIFY_SCOPE_USER_MODIFY_PLAYBACK_STATE: &str = "user-modify-playback-state";
const SPOTIFY_SCOPE_USER_READ_CURRENTLY_PLAYING: &str = "user-read-currently-playing";
const MAX_SPOTIFY_API_DIAGNOSTICS: usize = 24;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct SpotifyState {
    availability: String,
    availability_reason: Option<String>,
    configured: bool,
    connected: bool,
    connection_status: String,
    token_status: String,
    capability_status: String,
    capability_summary: Option<String>,
    auth_connected: bool,
    auth_in_progress: bool,
    pending_auth_state: Option<String>,
    account_display_name: Option<String>,
    account_email: Option<String>,
    spotify_user_id: Option<String>,
    scopes: Vec<String>,
    missing_scopes: Vec<String>,
    last_authenticated_at: Option<String>,
    last_refresh_at: Option<String>,
    is_playing: bool,
    last_query: Option<String>,
    volume_percent: Option<u8>,
    track: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    track_uri: Option<String>,
    device_id: Option<String>,
    device_name: Option<String>,
    last_action: Option<String>,
    last_status: Option<String>,
    last_error: Option<String>,
    last_error_reason: Option<String>,
    available_devices: Vec<SpotifyDeviceSummary>,
    recent_api_diagnostics: Vec<SpotifyApiDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpotifyApiDiagnostic {
    created_at: String,
    operation: String,
    method: String,
    endpoint: String,
    status_code: Option<u16>,
    latency_ms: i64,
    device_id: Option<String>,
    result_category: String,
    message: String,
    success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SpotifyAccountProfile {
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    spotify_user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SpotifyDeviceSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    #[serde(rename = "type")]
    pub device_type: String,
    pub is_active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_percent: Option<u8>,
}

impl From<&SpotifyConnectDevice> for SpotifyDeviceSummary {
    fn from(value: &SpotifyConnectDevice) -> Self {
        Self {
            id: value.id.clone(),
            name: value.name.clone(),
            device_type: value.device_type.clone(),
            is_active: value.is_active,
            volume_percent: value.volume_percent,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SpotifyConfig {
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
    fn validate(&self) -> Result<(), SpotifyUnavailable> {
        if self.client_id.trim().is_empty()
            || self.client_secret.trim().is_empty()
            || self.redirect_uri.trim().is_empty()
            || self.client_id == "YOUR_CLIENT_ID"
            || self.client_secret == "YOUR_CLIENT_SECRET"
        {
            return Err(SpotifyUnavailable::missing_config(
                "Spotify is not configured. Fill client_id, client_secret, and redirect_uri in config/spotify_config.json.",
            ));
        }

        Url::parse(&self.redirect_uri).map_err(|error| {
            SpotifyUnavailable::bad_config(format!("Spotify redirect_uri is invalid: {error}"))
        })?;
        Url::parse(&self.accounts_base_url).map_err(|error| {
            SpotifyUnavailable::bad_config(format!("Spotify accounts_base_url is invalid: {error}"))
        })?;
        Url::parse(&self.api_base_url).map_err(|error| {
            SpotifyUnavailable::bad_config(format!("Spotify api_base_url is invalid: {error}"))
        })?;

        Ok(())
    }

    fn configured_scopes(&self) -> Vec<String> {
        let mut scopes = self
            .scopes
            .iter()
            .map(|scope| scope.trim())
            .filter(|scope| !scope.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();

        if scopes.is_empty() {
            scopes = default_scopes();
        }

        scopes
    }
}

fn default_scopes() -> Vec<String> {
    vec![
        SPOTIFY_SCOPE_USER_READ_PRIVATE.to_owned(),
        SPOTIFY_SCOPE_USER_READ_PLAYBACK_STATE.to_owned(),
        SPOTIFY_SCOPE_USER_MODIFY_PLAYBACK_STATE.to_owned(),
        SPOTIFY_SCOPE_USER_READ_CURRENTLY_PLAYING.to_owned(),
    ]
}

fn default_accounts_base_url() -> String {
    DEFAULT_SPOTIFY_ACCOUNTS_BASE_URL.to_owned()
}

fn default_api_base_url() -> String {
    DEFAULT_SPOTIFY_API_BASE_URL.to_owned()
}

fn default_http_timeout_ms() -> u64 {
    DEFAULT_HTTP_TIMEOUT_MS
}

#[derive(Debug, Clone)]
pub(crate) struct SpotifyAdapter {
    runtime: SpotifyRuntime,
    token_store: Option<SpotifyTokenStore>,
}

#[derive(Debug, Clone)]
enum SpotifyRuntime {
    Available(SpotifyToolContext),
    Unavailable(SpotifyUnavailable),
}

#[derive(Debug, Clone)]
struct SpotifyToolContext {
    config: SpotifyConfig,
    auth: SpotifyAuthManager,
    api: SpotifyApiClient,
}

#[derive(Debug, Clone)]
struct SpotifyUnavailable {
    reason: &'static str,
    message: String,
}

impl SpotifyUnavailable {
    fn missing_config(message: impl Into<String>) -> Self {
        Self {
            reason: "missing_config",
            message: message.into(),
        }
    }

    fn bad_config(message: impl Into<String>) -> Self {
        Self {
            reason: "bad_config",
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone)]
struct SpotifyConnectionSnapshot {
    status: &'static str,
    configured: bool,
    connected: bool,
    token_status: &'static str,
    capability_status: String,
    capability_summary: Option<String>,
    reason: Option<&'static str>,
    account: Option<SpotifyAccountProfile>,
    scopes: Vec<String>,
    missing_scopes: Vec<String>,
    required_scopes: Vec<String>,
    last_authenticated_at: Option<String>,
    last_refresh_at: Option<String>,
    last_error: Option<String>,
    last_error_reason: Option<String>,
}

impl SpotifyConnectionSnapshot {
    fn configured(required_scopes: Vec<String>) -> Self {
        Self {
            status: "disconnected",
            configured: true,
            connected: false,
            token_status: "missing",
            capability_status: "auth_required".to_owned(),
            capability_summary: Some(SPOTIFY_AUTH_REQUIRED_MESSAGE.to_owned()),
            reason: None,
            account: None,
            scopes: Vec::new(),
            missing_scopes: Vec::new(),
            required_scopes,
            last_authenticated_at: None,
            last_refresh_at: None,
            last_error: None,
            last_error_reason: None,
        }
    }

    fn from_unavailable(
        unavailable: &SpotifyUnavailable,
        current_state: &SpotifyState,
        required_scopes: Vec<String>,
    ) -> Self {
        let status = if unavailable.reason == "missing_config" {
            "unconfigured"
        } else {
            "error"
        };
        Self {
            status,
            configured: false,
            connected: false,
            token_status: if unavailable.reason == "missing_config" {
                "missing"
            } else {
                "invalid"
            },
            capability_status: if unavailable.reason == "missing_config" {
                "auth_required".to_owned()
            } else {
                "error".to_owned()
            },
            capability_summary: Some(unavailable.message.clone()),
            reason: Some(unavailable.reason),
            account: account_from_state(current_state),
            scopes: current_state.scopes.clone(),
            missing_scopes: current_state.missing_scopes.clone(),
            required_scopes,
            last_authenticated_at: current_state.last_authenticated_at.clone(),
            last_refresh_at: current_state.last_refresh_at.clone(),
            last_error: Some(unavailable.message.clone()),
            last_error_reason: Some(unavailable.reason.to_owned()),
        }
    }

    fn to_result_json(&self) -> Map<String, Value> {
        let mut result = object_fields(json!({
            "status": self.status,
            "tool": SPOTIFY_TOOL_NAME,
            "configured": self.configured,
            "connected": self.connected,
            "token_status": self.token_status,
            "capability_status": self.capability_status,
            "scopes": self.scopes,
            "missing_scopes": self.missing_scopes,
            "required_scopes": self.required_scopes
        }));

        if let Some(account) = self.account.as_ref() {
            if let Some(display_name) = account.display_name.as_ref() {
                result.insert(
                    "account_display_name".to_owned(),
                    Value::String(display_name.clone()),
                );
            }
            if let Some(email) = account.email.as_ref() {
                result.insert("account_email".to_owned(), Value::String(email.clone()));
            }
            if let Some(spotify_user_id) = account.spotify_user_id.as_ref() {
                result.insert(
                    "spotify_user_id".to_owned(),
                    Value::String(spotify_user_id.clone()),
                );
            }
        }

        if let Some(reason) = self.reason {
            result.insert("reason".to_owned(), Value::String(reason.to_owned()));
        }
        if let Some(capability_summary) = self.capability_summary.as_ref() {
            result.insert(
                "capability_summary".to_owned(),
                Value::String(capability_summary.clone()),
            );
        }
        if let Some(last_authenticated_at) = self.last_authenticated_at.as_ref() {
            result.insert(
                "last_authenticated_at".to_owned(),
                Value::String(last_authenticated_at.clone()),
            );
        }
        if let Some(last_refresh_at) = self.last_refresh_at.as_ref() {
            result.insert(
                "last_refresh_at".to_owned(),
                Value::String(last_refresh_at.clone()),
            );
        }
        if let Some(last_error) = self.last_error.as_ref() {
            result.insert("last_error".to_owned(), Value::String(last_error.clone()));
        }

        result
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SpotifyOutcome {
    pub result_json: Value,
    pub summary: String,
    pub state: SpotifyState,
}

#[derive(Debug, Clone)]
enum SpotifyToolError {
    Unavailable(SpotifyUnavailable),
    AuthRequired {
        message: String,
    },
    AuthExpired {
        message: String,
    },
    AuthError {
        message: String,
    },
    Forbidden {
        reason: &'static str,
        message: String,
        code: Option<u16>,
    },
    NoAvailableDevice {
        message: String,
        devices: Vec<SpotifyDeviceSummary>,
    },
    DeviceNotFound {
        message: String,
        devices: Vec<SpotifyDeviceSummary>,
    },
    PlaybackNotActive {
        message: String,
        devices: Vec<SpotifyDeviceSummary>,
    },
    RateLimited {
        message: String,
        retry_after_seconds: Option<u64>,
    },
    Api {
        message: String,
        code: u16,
    },
    Network {
        message: String,
    },
    BadRequest {
        message: String,
    },
    Unknown {
        message: String,
    },
}

impl SpotifyToolError {
    fn status(&self) -> &'static str {
        match self {
            Self::Unavailable(_) => "unavailable",
            Self::AuthRequired { .. } => "auth_required",
            Self::AuthExpired { .. } => "auth_expired",
            Self::NoAvailableDevice { .. } => "no_available_device",
            Self::DeviceNotFound { .. } => "device_not_found",
            Self::PlaybackNotActive { .. } => "playback_not_active",
            Self::RateLimited { .. } => "rate_limited",
            Self::Forbidden { .. } => "forbidden",
            Self::AuthError { .. }
            | Self::Api { .. }
            | Self::Network { .. }
            | Self::BadRequest { .. }
            | Self::Unknown { .. } => "error",
        }
    }

    fn reason(&self) -> Option<&'static str> {
        match self {
            Self::Unavailable(unavailable) => Some(unavailable.reason),
            Self::AuthRequired { .. } => Some("auth_required"),
            Self::AuthExpired { .. } => Some("auth_expired"),
            Self::AuthError { .. } => Some("auth_error"),
            Self::Forbidden { reason, .. } => Some(reason),
            Self::NoAvailableDevice { .. } => Some("no_available_device"),
            Self::DeviceNotFound { .. } => Some("device_not_found"),
            Self::PlaybackNotActive { .. } => Some("playback_not_active"),
            Self::RateLimited { .. } => Some("rate_limited"),
            Self::Api { .. } => Some("spotify_api_error"),
            Self::Network { .. } => Some("network_error"),
            Self::Unknown { .. } => Some("unknown_error"),
            Self::BadRequest { .. } => Some("bad_request"),
        }
    }

    fn code(&self) -> Option<u16> {
        match self {
            Self::Forbidden { code, .. } => *code,
            Self::Api { code, .. } => Some(*code),
            _ => None,
        }
    }

    fn message(&self) -> &str {
        match self {
            Self::Unavailable(unavailable) => &unavailable.message,
            Self::AuthRequired { message }
            | Self::AuthExpired { message }
            | Self::AuthError { message }
            | Self::Forbidden { message, .. }
            | Self::NoAvailableDevice { message, .. }
            | Self::DeviceNotFound { message, .. }
            | Self::PlaybackNotActive { message, .. }
            | Self::RateLimited { message, .. }
            | Self::Api { message, .. }
            | Self::Network { message }
            | Self::BadRequest { message }
            | Self::Unknown { message } => message,
        }
    }

    fn extra_fields(&self, result: &mut Map<String, Value>) {
        match self {
            Self::NoAvailableDevice { devices, .. }
            | Self::DeviceNotFound { devices, .. }
            | Self::PlaybackNotActive { devices, .. } => {
                result.insert("devices".to_owned(), devices_json_from_summaries(devices));
            }
            Self::RateLimited {
                retry_after_seconds,
                ..
            } => {
                if let Some(retry_after_seconds) = retry_after_seconds {
                    result.insert(
                        "retry_after_seconds".to_owned(),
                        Value::Number(serde_json::Number::from(*retry_after_seconds)),
                    );
                }
            }
            _ => {}
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
    #[serde(default)]
    granted_scopes: Vec<String>,
    #[serde(default)]
    account: Option<SpotifyAccountProfile>,
    #[serde(default)]
    last_authenticated_at: Option<String>,
    #[serde(default)]
    last_refresh_at: Option<String>,
}

#[derive(Debug, Clone)]
struct SpotifyTokenStore {
    path: PathBuf,
}

impl SpotifyTokenStore {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn load(&self) -> Result<SpotifyTokens, SpotifyToolError> {
        if !self.path.exists() {
            return Ok(SpotifyTokens::default());
        }

        let contents =
            fs::read_to_string(&self.path).map_err(|error| SpotifyToolError::AuthError {
                message: format!("Failed to read Spotify token cache: {error}"),
            })?;

        serde_json::from_str(&contents).map_err(|error| SpotifyToolError::AuthError {
            message: format!("Spotify token cache is invalid: {error}"),
        })
    }

    fn save(&self, tokens: &SpotifyTokens) -> Result<(), SpotifyToolError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| SpotifyToolError::Unknown {
                message: format!("Failed to create Spotify token directory: {error}"),
            })?;
        }

        let payload =
            serde_json::to_vec_pretty(tokens).map_err(|error| SpotifyToolError::Unknown {
                message: format!("Failed to encode Spotify token cache: {error}"),
            })?;
        fs::write(&self.path, payload).map_err(|error| SpotifyToolError::Unknown {
            message: format!("Failed to write Spotify token cache: {error}"),
        })?;
        apply_private_permissions(&self.path);
        Ok(())
    }

    fn clear(&self) -> Result<(), SpotifyToolError> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(SpotifyToolError::Unknown {
                message: format!("Failed to clear Spotify token cache: {error}"),
            }),
        }
    }
}

#[derive(Debug, Clone)]
struct SpotifyAuthManager {
    client: Client,
    config: SpotifyConfig,
    token_store: SpotifyTokenStore,
}

#[derive(Debug, Clone)]
struct AccessGrant {
    access_token: String,
    granted_scopes: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum TokenPersistenceKind {
    Authentication,
    Refresh,
}

#[derive(Debug, Clone)]
struct SpotifyApiClient {
    client: Client,
    config: SpotifyConfig,
}

#[derive(Debug, Clone)]
struct SpotifyTrack {
    name: String,
    artists: Vec<String>,
    album: String,
    uri: String,
}

impl SpotifyTrack {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "artists": self.artists,
            "album": self.album,
            "uri": self.uri
        })
    }

    fn display(&self) -> String {
        format!("{} by {}", self.name, self.artists.join(", "))
    }
}

#[derive(Debug, Clone)]
struct SpotifyCurrentUserProfile {
    display_name: Option<String>,
    email: Option<String>,
    spotify_user_id: String,
}

impl SpotifyCurrentUserProfile {
    fn to_account_profile(&self) -> SpotifyAccountProfile {
        SpotifyAccountProfile {
            display_name: self.display_name.clone(),
            email: self.email.clone(),
            spotify_user_id: Some(self.spotify_user_id.clone()),
        }
    }
}

#[derive(Debug, Clone)]
struct SpotifyAlbumMatch {
    name: String,
    artists: Vec<String>,
    uri: String,
}

impl SpotifyAlbumMatch {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "artists": self.artists,
            "uri": self.uri
        })
    }
}

#[derive(Debug, Clone)]
struct SpotifyArtistMatch {
    name: String,
    uri: String,
}

impl SpotifyArtistMatch {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "uri": self.uri
        })
    }
}

#[derive(Debug, Clone)]
struct SpotifyPlaylistMatch {
    name: String,
    owner: String,
    uri: String,
}

impl SpotifyPlaylistMatch {
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "owner": self.owner,
            "uri": self.uri
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
struct SpotifyConnectDevice {
    #[serde(default)]
    id: Option<String>,
    name: String,
    #[serde(rename = "type")]
    device_type: String,
    #[serde(default)]
    is_active: bool,
    #[serde(default)]
    is_private_session: bool,
    #[serde(default)]
    is_restricted: bool,
    #[serde(default)]
    supports_volume: bool,
    #[serde(default)]
    volume_percent: Option<u8>,
}

#[derive(Debug, Clone)]
struct SpotifyPlaybackState {
    is_playing: bool,
    device: Option<SpotifyConnectDevice>,
    track: Option<SpotifyTrack>,
}

#[derive(Debug, Clone, Default)]
struct RequestedDevice {
    device_id: Option<String>,
    device_name: Option<String>,
}

impl RequestedDevice {
    fn from_arguments(arguments: &Value) -> Self {
        Self {
            device_id: optional_string_argument(arguments, &["device_id", "target_device_id"]),
            device_name: optional_string_argument(
                arguments,
                &["device_name", "target_device", "device"],
            ),
        }
    }

    fn is_explicit(&self) -> bool {
        self.device_id.is_some() || self.device_name.is_some()
    }

    fn label(&self) -> Option<&str> {
        self.device_name.as_deref().or(self.device_id.as_deref())
    }
}

#[derive(Debug, Clone)]
struct ResolvedDevice {
    device: SpotifyConnectDevice,
    selection_reason: &'static str,
}

struct SpotifyDeviceResolver;

impl SpotifyDeviceResolver {
    fn resolve_for_play(
        devices: &[SpotifyConnectDevice],
        request: &RequestedDevice,
        state: &SpotifyState,
    ) -> Result<ResolvedDevice, SpotifyToolError> {
        if let Some(device) = Self::resolve_explicit(devices, request)? {
            return Ok(ResolvedDevice {
                device,
                selection_reason: "explicit_device",
            });
        }

        Self::fallback_device(devices, state)
            .map(|device| ResolvedDevice {
                selection_reason: if device.is_active {
                    "active_device"
                } else if state.device_id.as_deref() == device.id.as_deref() {
                    "last_used_device"
                } else {
                    "best_available_device"
                },
                device,
            })
            .ok_or_else(|| no_available_device_error(devices))
    }

    fn resolve_for_transfer(
        devices: &[SpotifyConnectDevice],
        request: &RequestedDevice,
        state: &SpotifyState,
    ) -> Result<ResolvedDevice, SpotifyToolError> {
        if let Some(device) = Self::resolve_explicit(devices, request)? {
            return Ok(ResolvedDevice {
                device,
                selection_reason: "explicit_device",
            });
        }

        Self::fallback_device(devices, state)
            .map(|device| ResolvedDevice {
                device,
                selection_reason: "best_available_device",
            })
            .ok_or_else(|| no_available_device_error(devices))
    }

    fn resolve_for_active_control(
        devices: &[SpotifyConnectDevice],
        request: &RequestedDevice,
    ) -> Result<ResolvedDevice, SpotifyToolError> {
        if let Some(device) = Self::resolve_explicit(devices, request)? {
            return Ok(ResolvedDevice {
                device,
                selection_reason: "explicit_device",
            });
        }

        if let Some(device) = devices
            .iter()
            .find(|device| device.is_active && !device.is_restricted && !device.is_private_session)
            .cloned()
        {
            return Ok(ResolvedDevice {
                device,
                selection_reason: "active_device",
            });
        }

        if devices.is_empty() {
            return Err(no_available_device_error(devices));
        }

        Err(SpotifyToolError::PlaybackNotActive {
            message: "No Spotify device is actively playing right now.".to_owned(),
            devices: devices.iter().map(SpotifyDeviceSummary::from).collect(),
        })
    }

    fn resolve_for_volume(
        devices: &[SpotifyConnectDevice],
        request: &RequestedDevice,
        state: &SpotifyState,
    ) -> Result<ResolvedDevice, SpotifyToolError> {
        if let Some(device) = Self::resolve_explicit(devices, request)? {
            let resolved = ResolvedDevice {
                device,
                selection_reason: "explicit_device",
            };
            return ensure_volume_capable_device(resolved, devices);
        }

        if let Some(device) = devices
            .iter()
            .find(|device| device.is_active && !device.is_restricted && !device.is_private_session)
            .cloned()
        {
            return ensure_volume_capable_device(
                ResolvedDevice {
                    device,
                    selection_reason: "active_device",
                },
                devices,
            );
        }

        match Self::fallback_device(devices, state) {
            Some(device) => ensure_volume_capable_device(
                ResolvedDevice {
                    device,
                    selection_reason: "best_available_device",
                },
                devices,
            ),
            None => Err(no_available_device_error(devices)),
        }
    }

    fn resolve_explicit(
        devices: &[SpotifyConnectDevice],
        request: &RequestedDevice,
    ) -> Result<Option<SpotifyConnectDevice>, SpotifyToolError> {
        if !request.is_explicit() {
            return Ok(None);
        }

        let candidates = targetable_devices(devices);

        if let Some(device_id) = request.device_id.as_deref() {
            if let Some(device) = candidates
                .iter()
                .find(|device| device.id.as_deref() == Some(device_id))
                .cloned()
            {
                return Ok(Some(device));
            }
        }

        if let Some(device_name) = request.device_name.as_deref() {
            if let Some(device) = match_device_by_name(&candidates, device_name) {
                return Ok(Some(device));
            }
        }

        Err(SpotifyToolError::DeviceNotFound {
            message: format!(
                "I couldn't find a Spotify device matching {}.",
                request
                    .label()
                    .map(|label| format!("\"{label}\""))
                    .unwrap_or_else(|| "that target".to_owned())
            ),
            devices: devices.iter().map(SpotifyDeviceSummary::from).collect(),
        })
    }

    fn fallback_device(
        devices: &[SpotifyConnectDevice],
        state: &SpotifyState,
    ) -> Option<SpotifyConnectDevice> {
        let mut devices = targetable_devices(devices);
        devices.sort_by_key(|device| {
            (
                preference_rank(device, state),
                device.name.to_ascii_lowercase(),
                device.device_type.to_ascii_lowercase(),
            )
        });
        devices.into_iter().next()
    }
}

fn preference_rank(device: &SpotifyConnectDevice, state: &SpotifyState) -> (u8, u8, u8) {
    let last_used = if state.device_id.as_deref() == device.id.as_deref()
        || state.device_name.as_deref() == Some(device.name.as_str())
    {
        0
    } else {
        1
    };
    let active = if device.is_active { 0 } else { 1 };
    let type_rank = match device.device_type.to_ascii_lowercase().as_str() {
        "computer" => 0,
        "smartphone" => 1,
        "speaker" => 2,
        _ => 3,
    };
    (last_used, active, type_rank)
}

fn targetable_devices(devices: &[SpotifyConnectDevice]) -> Vec<SpotifyConnectDevice> {
    devices
        .iter()
        .filter(|device| !device.is_restricted && !device.is_private_session)
        .cloned()
        .collect()
}

fn match_device_by_name(
    devices: &[SpotifyConnectDevice],
    requested_name: &str,
) -> Option<SpotifyConnectDevice> {
    let normalized = requested_name.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }

    devices
        .iter()
        .find(|device| device.name.eq_ignore_ascii_case(requested_name))
        .cloned()
        .or_else(|| {
            devices
                .iter()
                .find(|device| device.name.to_ascii_lowercase().contains(&normalized))
                .cloned()
        })
        .or_else(|| {
            let hints = device_hint_terms(&normalized);
            devices
                .iter()
                .find(|device| {
                    let name = device.name.to_ascii_lowercase();
                    let device_type = device.device_type.to_ascii_lowercase();
                    hints
                        .iter()
                        .any(|hint| name.contains(hint) || device_type.contains(hint))
                })
                .cloned()
        })
}

fn device_hint_terms(value: &str) -> Vec<String> {
    let mut terms = vec![value.to_owned()];
    if value.contains("phone") {
        terms.extend(
            ["phone", "smartphone", "iphone", "pixel", "android"]
                .into_iter()
                .map(str::to_owned),
        );
    }
    if value.contains("computer") || value.contains("laptop") || value.contains("mac") {
        terms.extend(
            ["computer", "laptop", "desktop", "mac", "macbook"]
                .into_iter()
                .map(str::to_owned),
        );
    }
    if value.contains("speaker") || value.contains("tv") {
        terms.extend(["speaker", "tv", "soundbar"].into_iter().map(str::to_owned));
    }
    terms.sort();
    terms.dedup();
    terms
}

impl SpotifyAdapter {
    pub(crate) fn new(app_files_dir: &Path, config_dir: &Path) -> Self {
        let token_store = SpotifyTokenStore::new(app_files_dir.join(SPOTIFY_TOKEN_FILE));
        let runtime = match load_spotify_config(config_dir) {
            Ok(config) => {
                let client = match Client::builder()
                    .timeout(Duration::from_millis(config.timeout_ms.max(1_000)))
                    .build()
                {
                    Ok(client) => client,
                    Err(error) => {
                        let unavailable = SpotifyUnavailable::bad_config(format!(
                            "Failed to build the Spotify HTTP client: {error}"
                        ));
                        eprintln!(
                            "spotify unavailable [{}]: {}",
                            unavailable.reason, unavailable.message
                        );
                        return Self {
                            runtime: SpotifyRuntime::Unavailable(unavailable),
                            token_store: Some(token_store),
                        };
                    }
                };

                SpotifyRuntime::Available(SpotifyToolContext {
                    config: config.clone(),
                    auth: SpotifyAuthManager {
                        client: client.clone(),
                        config: config.clone(),
                        token_store: token_store.clone(),
                    },
                    api: SpotifyApiClient {
                        client,
                        config: config.clone(),
                    },
                })
            }
            Err(unavailable) => {
                eprintln!(
                    "spotify unavailable [{}]: {}",
                    unavailable.reason, unavailable.message
                );
                SpotifyRuntime::Unavailable(unavailable)
            }
        };

        Self {
            runtime,
            token_store: Some(token_store),
        }
    }

    pub(crate) fn disabled() -> Self {
        Self {
            runtime: SpotifyRuntime::Unavailable(SpotifyUnavailable::missing_config(
                "Spotify is not configured for this runtime.",
            )),
            token_store: None,
        }
    }

    pub(crate) fn execute(
        &self,
        action: &str,
        arguments: &Value,
        current_state: &SpotifyState,
    ) -> SpotifyOutcome {
        match &self.runtime {
            SpotifyRuntime::Available(context) => context.execute(action, arguments, current_state),
            SpotifyRuntime::Unavailable(unavailable) => {
                let state = next_state(current_state, action);
                match action {
                    "disconnect" | "clear_tokens" | "unlink_account" => {
                        if let Some(token_store) = self.token_store.as_ref() {
                            match token_store.clear() {
                                Ok(()) => {
                                    let mut state = state;
                                    state.clear_connection();
                                    let summary = match action {
                                        "clear_tokens" => "Spotify tokens cleared.",
                                        "unlink_account" => "Spotify account unlinked.",
                                        _ => {
                                            "Spotify has been disconnected. You can sign in again whenever you want."
                                        }
                                    };
                                    success_outcome(
                                        action,
                                        state,
                                        summary,
                                        object_fields(json!({
                                            "connection_status": "disconnected",
                                            "connected": false
                                        })),
                                    )
                                }
                                Err(error) => failure_outcome(action, state, error),
                            }
                        } else {
                            let mut state = state;
                            state.clear_connection();
                            success_outcome(
                                action,
                                state,
                                "Spotify has been disconnected. You can sign in again whenever you want.",
                                object_fields(json!({
                                    "connection_status": "disconnected",
                                    "connected": false
                                })),
                            )
                        }
                    }
                    "get_connection_state"
                    | "get_linked_account"
                    | "auth_status"
                    | "validate_connection"
                    | "validate_scopes" => {
                        let snapshot = unavailable_connection_snapshot(
                            unavailable,
                            self.token_store.as_ref(),
                            &state,
                        );
                        let summary = match action {
                            "get_linked_account" if snapshot.account.is_none() => {
                                Some("No Spotify account is currently linked.".to_owned())
                            }
                            "get_linked_account" => snapshot.account.as_ref().map(|account| {
                                format!("Spotify is linked to {}.", display_account_label(account))
                            }),
                            _ => None,
                        };
                        if action == "auth_status" && snapshot.status == "disconnected" {
                            auth_required_outcome(action, state, snapshot)
                        } else {
                            connection_state_outcome(action, state, snapshot, summary, Map::new())
                        }
                    }
                    _ => failure_outcome(
                        action,
                        state,
                        SpotifyToolError::Unavailable(unavailable.clone()),
                    ),
                }
            }
        }
    }

    pub(crate) fn passive_state_snapshot(&self, current_state: &SpotifyState) -> SpotifyState {
        let mut state = next_state(current_state, "passive_snapshot");
        let snapshot = match &self.runtime {
            SpotifyRuntime::Available(context) => context.load_connection_snapshot(&state),
            SpotifyRuntime::Unavailable(unavailable) => {
                unavailable_connection_snapshot(unavailable, self.token_store.as_ref(), &state)
            }
        };
        state.apply_connection_snapshot(&snapshot);
        state.last_action = current_state.last_action.clone();
        state.last_status = current_state.last_status.clone();
        state
    }
}

impl SpotifyToolContext {
    fn execute(
        &self,
        action: &str,
        arguments: &Value,
        current_state: &SpotifyState,
    ) -> SpotifyOutcome {
        match action {
            "get_connection_state" => self.get_connection_state_action(current_state),
            "get_linked_account" => self.get_linked_account_action(current_state),
            "auth_status" => self.auth_status_action(current_state),
            "validate_connection" => self.validate_connection_action(current_state),
            "validate_scopes" => self.validate_scopes_action(current_state),
            "start_auth" => self.start_auth_action(current_state),
            "complete_auth" => self.handle_callback_action(arguments, current_state),
            "handle_callback" => self.handle_callback_action(arguments, current_state),
            "exchange_code" => self.exchange_code_action(arguments, current_state),
            "refresh_token" => self.refresh_token_action(current_state),
            "refresh_token_if_needed" => self.refresh_token_if_needed_action(current_state),
            "disconnect" | "clear_tokens" | "unlink_account" => {
                self.disconnect_action(action, current_state)
            }
            "capability_status" => self.capability_status_action(current_state),
            "get_devices" => self.get_devices_action(current_state),
            "get_active_device" => self.get_active_device_action(current_state),
            "transfer_playback" => self.transfer_playback_action(arguments, current_state),
            "ensure_playback_target" => {
                self.ensure_playback_target_action(arguments, current_state)
            }
            "current_playback" | "playback_state" => {
                self.current_playback_action(action, current_state)
            }
            "currently_playing" => self.currently_playing_action(current_state),
            "play" | "resume_playback" => self.play_action(action, arguments, current_state),
            "pause" => self.pause_action(current_state, arguments),
            "next_track" => self.skip_action("next_track", current_state, arguments),
            "previous_track" => self.skip_action("previous_track", current_state, arguments),
            "set_volume" => self.set_volume_action(arguments, current_state),
            "search" | "search_track" => self.search_track_action(action, arguments, current_state),
            "search_album" => self.search_album_action(arguments, current_state),
            "search_artist" => self.search_artist_action(arguments, current_state),
            "search_playlist" => self.search_playlist_action(arguments, current_state),
            "resolve_track_uri_from_query" => {
                self.resolve_track_uri_from_query_action(arguments, current_state)
            }
            other => failure_outcome(
                other,
                next_state(current_state, other),
                SpotifyToolError::BadRequest {
                    message: format!("Unsupported Spotify action: {other}"),
                },
            ),
        }
    }

    fn required_scopes(&self) -> Vec<String> {
        self.config.configured_scopes()
    }

    fn missing_required_scopes(&self, granted_scopes: &[String]) -> Vec<String> {
        if granted_scopes.is_empty() {
            return Vec::new();
        }

        self.config
            .configured_scopes()
            .into_iter()
            .filter(|scope| !granted_scopes.iter().any(|granted| granted == scope))
            .collect()
    }

    fn load_valid_cached_grant(&self) -> Result<Option<AccessGrant>, SpotifyToolError> {
        let tokens = self.auth.load_cached_tokens()?;
        let Some(access_token) = tokens.access_token else {
            return Ok(None);
        };

        if !token_is_valid(tokens.expires_at.as_deref()) {
            return Ok(None);
        }

        Ok(Some(AccessGrant {
            access_token,
            granted_scopes: tokens.granted_scopes,
        }))
    }

    fn ensure_access_grant(
        &self,
        state: &mut SpotifyState,
    ) -> Result<AccessGrant, SpotifyToolError> {
        let tokens = self.auth.load_effective_tokens()?;

        if let Some(access_token) = tokens.access_token.clone() {
            if token_is_valid(tokens.expires_at.as_deref()) {
                return Ok(AccessGrant {
                    access_token,
                    granted_scopes: tokens.granted_scopes,
                });
            }
        }

        if tokens.refresh_token.is_some() {
            return self.refresh_session_with_diagnostics(state, tokens);
        }

        Err(SpotifyToolError::AuthRequired {
            message: SPOTIFY_AUTH_REQUIRED_MESSAGE.to_owned(),
        })
    }

    fn refresh_session_with_diagnostics(
        &self,
        state: &mut SpotifyState,
        tokens: SpotifyTokens,
    ) -> Result<AccessGrant, SpotifyToolError> {
        let started_at = Instant::now();
        let result = self.auth.refresh_session_from(tokens);
        self.record_api_diagnostic(
            state,
            "refresh_token",
            "POST",
            "/api/token",
            None,
            started_at,
            &result,
            Some(200),
        );
        result
    }

    fn record_api_diagnostic<T>(
        &self,
        state: &mut SpotifyState,
        operation: &str,
        method: &str,
        endpoint: &str,
        device_id: Option<&str>,
        started_at: Instant,
        result: &Result<T, SpotifyToolError>,
        success_status_code: Option<u16>,
    ) {
        let (success, status_code, result_category, message) = match result {
            Ok(_) => (
                true,
                success_status_code,
                "success".to_owned(),
                "Spotify request succeeded.".to_owned(),
            ),
            Err(error) => (
                false,
                status_code_for_error(error),
                error.reason().unwrap_or("spotify_api_error").to_owned(),
                error.message().to_owned(),
            ),
        };

        let latency_ms = started_at.elapsed().as_millis().min(i64::MAX as u128) as i64;
        state.push_api_diagnostic(SpotifyApiDiagnostic {
            created_at: now_rfc3339(),
            operation: operation.to_owned(),
            method: method.to_owned(),
            endpoint: endpoint.to_owned(),
            status_code,
            latency_ms,
            device_id: device_id.map(str::to_owned),
            result_category,
            message,
            success,
        });
    }

    fn get_profile_with_diagnostics(
        &self,
        state: &mut SpotifyState,
        access_token: &str,
        granted_scopes: &[String],
    ) -> Result<SpotifyCurrentUserProfile, SpotifyToolError> {
        let started_at = Instant::now();
        let result = self.api.get_current_user_profile(access_token);
        self.record_api_diagnostic(
            state,
            "get_profile",
            "GET",
            "/v1/me",
            None,
            started_at,
            &result,
            Some(200),
        );
        result.map(|mut profile| {
            if profile.email.is_some()
                && !granted_scopes.is_empty()
                && !granted_scopes
                    .iter()
                    .any(|scope| scope == SPOTIFY_SCOPE_USER_READ_EMAIL)
            {
                profile.email = None;
            }
            profile
        })
    }

    fn get_devices_with_diagnostics(
        &self,
        state: &mut SpotifyState,
        access_token: &str,
    ) -> Result<Vec<SpotifyConnectDevice>, SpotifyToolError> {
        let started_at = Instant::now();
        let result = self.api.get_devices(access_token);
        self.record_api_diagnostic(
            state,
            "get_devices",
            "GET",
            "/v1/me/player/devices",
            None,
            started_at,
            &result,
            Some(200),
        );
        result
    }

    fn get_current_playback_with_diagnostics(
        &self,
        state: &mut SpotifyState,
        access_token: &str,
    ) -> Result<Option<SpotifyPlaybackState>, SpotifyToolError> {
        let started_at = Instant::now();
        let result = self.api.get_current_playback(access_token);
        let success_status_code = match result {
            Ok(Some(_)) => Some(200),
            Ok(None) => Some(204),
            Err(_) => None,
        };
        self.record_api_diagnostic(
            state,
            "get_current_playback",
            "GET",
            "/v1/me/player",
            None,
            started_at,
            &result,
            success_status_code,
        );
        result
    }

    fn transfer_playback_with_diagnostics(
        &self,
        state: &mut SpotifyState,
        access_token: &str,
        device_id: &str,
        play: bool,
    ) -> Result<(), SpotifyToolError> {
        let started_at = Instant::now();
        let result = self
            .api
            .transfer_playback(access_token, device_id, play)
            .map_err(|error| self.normalize_playback_command_error(error));
        self.record_api_diagnostic(
            state,
            "transfer_playback",
            "PUT",
            "/v1/me/player",
            Some(device_id),
            started_at,
            &result,
            Some(204),
        );
        result
    }

    fn start_playback_with_diagnostics(
        &self,
        state: &mut SpotifyState,
        access_token: &str,
        device_id: Option<&str>,
        uris: Option<&[String]>,
    ) -> Result<(), SpotifyToolError> {
        let started_at = Instant::now();
        let result = self
            .api
            .start_or_resume_playback(access_token, device_id, uris)
            .map_err(|error| self.normalize_playback_command_error(error));
        self.record_api_diagnostic(
            state,
            "start_playback",
            "PUT",
            "/v1/me/player/play",
            device_id,
            started_at,
            &result,
            Some(204),
        );
        result
    }

    fn normalize_playback_command_error(&self, error: SpotifyToolError) -> SpotifyToolError {
        match error {
            SpotifyToolError::Api { code, .. } if code == 403 => SpotifyToolError::Forbidden {
                reason: "playback_forbidden",
                message: "Spotify refused playback control for this request. This can happen when Premium is required or playback permissions need to be refreshed.".to_owned(),
                code: Some(403),
            },
            other => other,
        }
    }

    fn bootstrap_connected_snapshot(
        &self,
        state: &mut SpotifyState,
        current_state: &SpotifyState,
        grant: &AccessGrant,
    ) -> SpotifyConnectionSnapshot {
        let mut snapshot = self.load_connection_snapshot(current_state);
        snapshot.status = "connected";
        snapshot.connected = true;
        snapshot.token_status = "valid";
        snapshot.reason = None;
        snapshot.last_error = None;
        snapshot.last_error_reason = None;
        snapshot.capability_status = "connected".to_owned();
        snapshot.capability_summary = None;
        if !grant.granted_scopes.is_empty() {
            snapshot.scopes = grant.granted_scopes.clone();
        }

        let existing_account = snapshot.account.clone();
        match self.get_profile_with_diagnostics(state, &grant.access_token, &snapshot.scopes) {
            Ok(profile) => {
                let account = profile.to_account_profile();
                let _ = self.auth.persist_account_profile(account.clone());
                snapshot.account = Some(account);
            }
            Err(SpotifyToolError::Forbidden {
                reason, message, ..
            }) if reason == "invalid_scope" => {
                snapshot.account = existing_account;
                snapshot.capability_status = "invalid_scope".to_owned();
                snapshot.capability_summary = Some(message.clone());
                snapshot.last_error = Some(message);
                snapshot.last_error_reason = Some(reason.to_owned());
                snapshot.missing_scopes = self.missing_required_scopes(&snapshot.scopes);
                return snapshot;
            }
            Err(SpotifyToolError::AuthRequired { message }) => {
                snapshot.status = "disconnected";
                snapshot.connected = false;
                snapshot.token_status = "missing";
                snapshot.capability_status = "auth_required".to_owned();
                snapshot.capability_summary = Some(message.clone());
                snapshot.last_error = Some(message);
                snapshot.last_error_reason = Some("auth_required".to_owned());
                return snapshot;
            }
            Err(SpotifyToolError::AuthExpired { message })
            | Err(SpotifyToolError::AuthError { message }) => {
                snapshot.status = "expired";
                snapshot.connected = false;
                snapshot.token_status = "expired";
                snapshot.capability_status = "auth_expired".to_owned();
                snapshot.capability_summary = Some(message.clone());
                snapshot.last_error = Some(message);
                snapshot.last_error_reason = Some("auth_expired".to_owned());
                return snapshot;
            }
            Err(error) => {
                snapshot.account = existing_account;
                snapshot.capability_status = "connected_but_profile_unavailable".to_owned();
                snapshot.capability_summary = Some(format!(
                    "Spotify is connected, but the linked account profile could not be refreshed. {}",
                    error.message()
                ));
                snapshot.last_error = Some(error.message().to_owned());
                snapshot.last_error_reason = error.reason().map(str::to_owned);
            }
        }

        snapshot.missing_scopes = self.missing_required_scopes(&snapshot.scopes);
        if !snapshot.missing_scopes.is_empty() {
            let message = format!(
                "Spotify access is missing required permissions: {}. Reconnect Spotify to continue.",
                snapshot.missing_scopes.join(", ")
            );
            snapshot.capability_status = "invalid_scope".to_owned();
            snapshot.capability_summary = Some(message.clone());
            snapshot.last_error = Some(message);
            snapshot.last_error_reason = Some("invalid_scope".to_owned());
            return snapshot;
        }

        let devices = match self.get_devices_with_diagnostics(state, &grant.access_token) {
            Ok(devices) => devices,
            Err(SpotifyToolError::AuthRequired { message }) => {
                snapshot.status = "disconnected";
                snapshot.connected = false;
                snapshot.token_status = "missing";
                snapshot.capability_status = "auth_required".to_owned();
                snapshot.capability_summary = Some(message.clone());
                snapshot.last_error = Some(message);
                snapshot.last_error_reason = Some("auth_required".to_owned());
                return snapshot;
            }
            Err(SpotifyToolError::AuthExpired { message })
            | Err(SpotifyToolError::AuthError { message }) => {
                snapshot.status = "expired";
                snapshot.connected = false;
                snapshot.token_status = "expired";
                snapshot.capability_status = "auth_expired".to_owned();
                snapshot.capability_summary = Some(message.clone());
                snapshot.last_error = Some(message);
                snapshot.last_error_reason = Some("auth_expired".to_owned());
                return snapshot;
            }
            Err(SpotifyToolError::Forbidden {
                reason, message, ..
            }) if reason == "invalid_scope" => {
                snapshot.capability_status = "invalid_scope".to_owned();
                snapshot.capability_summary = Some(message.clone());
                snapshot.last_error = Some(message);
                snapshot.last_error_reason = Some(reason.to_owned());
                return snapshot;
            }
            Err(error) => {
                snapshot.capability_status = "connected_but_playback_unavailable".to_owned();
                snapshot.capability_summary = Some(format!(
                    "Spotify is connected, but playback devices could not be refreshed. {}",
                    error.message()
                ));
                snapshot.last_error = Some(error.message().to_owned());
                snapshot.last_error_reason = error.reason().map(str::to_owned);
                return snapshot;
            }
        };
        state.record_devices(&devices);

        match self.get_current_playback_with_diagnostics(state, &grant.access_token) {
            Ok(playback) => {
                state.record_playback(playback.as_ref());
            }
            Err(SpotifyToolError::AuthRequired { message }) => {
                snapshot.status = "disconnected";
                snapshot.connected = false;
                snapshot.token_status = "missing";
                snapshot.capability_status = "auth_required".to_owned();
                snapshot.capability_summary = Some(message.clone());
                snapshot.last_error = Some(message);
                snapshot.last_error_reason = Some("auth_required".to_owned());
                return snapshot;
            }
            Err(SpotifyToolError::AuthExpired { message })
            | Err(SpotifyToolError::AuthError { message }) => {
                snapshot.status = "expired";
                snapshot.connected = false;
                snapshot.token_status = "expired";
                snapshot.capability_status = "auth_expired".to_owned();
                snapshot.capability_summary = Some(message.clone());
                snapshot.last_error = Some(message);
                snapshot.last_error_reason = Some("auth_expired".to_owned());
                return snapshot;
            }
            Err(SpotifyToolError::Forbidden {
                reason, message, ..
            }) if reason == "invalid_scope" => {
                snapshot.capability_status = "invalid_scope".to_owned();
                snapshot.capability_summary = Some(message.clone());
                snapshot.last_error = Some(message);
                snapshot.last_error_reason = Some(reason.to_owned());
                return snapshot;
            }
            Err(error) => {
                snapshot.capability_status = "connected_but_playback_unavailable".to_owned();
                snapshot.capability_summary = Some(format!(
                    "Spotify is connected, but current playback could not be checked. {}",
                    error.message()
                ));
                snapshot.last_error = Some(error.message().to_owned());
                snapshot.last_error_reason = error.reason().map(str::to_owned);
                return snapshot;
            }
        }

        let targetable = targetable_devices(&devices);
        snapshot.capability_status = if snapshot.account.is_none() {
            "connected_but_profile_unavailable".to_owned()
        } else if targetable.is_empty() {
            "no_available_device".to_owned()
        } else {
            "connected".to_owned()
        };

        snapshot.capability_summary = Some(match snapshot.capability_status.as_str() {
            "connected_but_profile_unavailable" => {
                "Spotify is connected, but the linked account profile is not available.".to_owned()
            }
            "no_available_device" => {
                "Spotify is connected, but no available playback device was found.".to_owned()
            }
            _ => {
                let active_device = devices.iter().find(|device| device.is_active);
                if let Some(account) = snapshot.account.as_ref() {
                    if let Some(active_device) = active_device {
                        format!(
                            "Spotify is connected as {}. Active device: {}.",
                            display_account_label(account),
                            active_device.name
                        )
                    } else if !devices.is_empty() {
                        format!(
                            "Spotify is connected as {}. {} playback devices are available.",
                            display_account_label(account),
                            devices.len()
                        )
                    } else {
                        format!(
                            "Spotify is connected as {}.",
                            display_account_label(account)
                        )
                    }
                } else {
                    "Spotify is connected.".to_owned()
                }
            }
        });

        snapshot
    }

    fn load_connection_snapshot(&self, current_state: &SpotifyState) -> SpotifyConnectionSnapshot {
        let required_scopes = self.required_scopes();
        let mut snapshot = SpotifyConnectionSnapshot::configured(required_scopes);
        snapshot.last_error = current_state.last_error.clone();
        snapshot.last_error_reason = current_state.last_error_reason.clone();
        snapshot.last_authenticated_at = current_state.last_authenticated_at.clone();
        snapshot.last_refresh_at = current_state.last_refresh_at.clone();
        snapshot.account = account_from_state(current_state);
        snapshot.scopes = current_state.scopes.clone();
        snapshot.missing_scopes = current_state.missing_scopes.clone();
        snapshot.capability_status = if current_state.capability_status.trim().is_empty() {
            "auth_required".to_owned()
        } else {
            current_state.capability_status.clone()
        };
        snapshot.capability_summary = current_state.capability_summary.clone();

        if current_state.auth_in_progress || current_state.pending_auth_state.is_some() {
            snapshot.status = "connecting";
            snapshot.token_status = match current_state.token_status.as_str() {
                "valid" => "valid",
                "expired" => "expired",
                "refresh_failed" => "refresh_failed",
                "invalid" => "invalid",
                _ => "missing",
            };
            return snapshot;
        }

        let tokens = match self.auth.load_cached_tokens() {
            Ok(tokens) => tokens,
            Err(error) => {
                snapshot.status = "error";
                snapshot.configured = true;
                snapshot.connected = false;
                snapshot.token_status = "invalid";
                snapshot.reason = canonical_reason(error.reason());
                snapshot.last_error = Some(error.message().to_owned());
                snapshot.last_error_reason = error.reason().map(str::to_owned);
                return snapshot;
            }
        };

        if tokens.account.is_some() {
            snapshot.account = tokens.account.clone();
        }
        if !tokens.granted_scopes.is_empty() {
            snapshot.scopes = tokens.granted_scopes.clone();
        }
        snapshot.missing_scopes = self.missing_required_scopes(&snapshot.scopes);
        if tokens.last_authenticated_at.is_some() {
            snapshot.last_authenticated_at = tokens.last_authenticated_at.clone();
        }
        if tokens.last_refresh_at.is_some() {
            snapshot.last_refresh_at = tokens.last_refresh_at.clone();
        }

        let has_any_token = tokens.access_token.is_some() || tokens.refresh_token.is_some();
        if !has_any_token {
            snapshot.status = "disconnected";
            snapshot.connected = false;
            snapshot.token_status = "missing";
            snapshot.capability_status = "auth_required".to_owned();
            snapshot.capability_summary = Some(SPOTIFY_AUTH_REQUIRED_MESSAGE.to_owned());
            return snapshot;
        }

        let access_token_valid = token_is_valid(tokens.expires_at.as_deref());
        if access_token_valid {
            snapshot.status = "connected";
            snapshot.connected = true;
            snapshot.token_status = "valid";
            snapshot.reason = canonical_reason(snapshot.last_error_reason.as_deref());
            if !snapshot.missing_scopes.is_empty() {
                let message = format!(
                    "Spotify access is missing required permissions: {}. Reconnect Spotify to continue.",
                    snapshot.missing_scopes.join(", ")
                );
                snapshot.capability_status = "invalid_scope".to_owned();
                snapshot.capability_summary = Some(message.clone());
                snapshot.last_error = Some(message);
                snapshot.last_error_reason = Some("invalid_scope".to_owned());
            } else if snapshot.capability_status.trim().is_empty()
                || snapshot.capability_status == "auth_required"
            {
                snapshot.capability_status = if snapshot.account.is_some() {
                    "connected".to_owned()
                } else {
                    "connected_but_profile_unavailable".to_owned()
                };
            }
            return snapshot;
        }

        snapshot.status = "expired";
        snapshot.connected = false;
        snapshot.token_status = if snapshot.last_error_reason.as_deref() == Some("auth_error") {
            "refresh_failed"
        } else {
            "expired"
        };
        snapshot.reason = canonical_reason(snapshot.last_error_reason.as_deref());
        snapshot.capability_status = "auth_expired".to_owned();
        snapshot.capability_summary = snapshot.last_error.clone();
        snapshot
    }

    fn get_connection_state_action(&self, current_state: &SpotifyState) -> SpotifyOutcome {
        let mut state = next_state(current_state, "get_connection_state");
        let mut snapshot = self.load_connection_snapshot(&state);

        if snapshot.status == "connected"
            && snapshot.account.is_none()
            && snapshot.capability_status != "invalid_scope"
        {
            match self.load_valid_cached_grant() {
                Ok(Some(grant)) => {
                    let bootstrap_state = state.clone();
                    snapshot =
                        self.bootstrap_connected_snapshot(&mut state, &bootstrap_state, &grant);
                }
                Ok(None) => {}
                Err(error) => {
                    return connection_error_outcome(
                        "get_connection_state",
                        state,
                        snapshot,
                        error,
                    );
                }
            }
        }

        connection_state_outcome("get_connection_state", state, snapshot, None, Map::new())
    }

    fn get_linked_account_action(&self, current_state: &SpotifyState) -> SpotifyOutcome {
        let mut state = next_state(current_state, "get_linked_account");
        let mut snapshot = self.load_connection_snapshot(&state);

        if snapshot.status == "connected" {
            match self.ensure_access_grant(&mut state) {
                Ok(grant) => {
                    let bootstrap_state = state.clone();
                    snapshot =
                        self.bootstrap_connected_snapshot(&mut state, &bootstrap_state, &grant);
                }
                Err(error) => {
                    return connection_error_outcome("get_linked_account", state, snapshot, error);
                }
            }
        }

        let summary = match snapshot.account.as_ref() {
            Some(account) => Some(format!(
                "Spotify is linked to {}.",
                display_account_label(account)
            )),
            None if snapshot.capability_status == "invalid_scope" => {
                snapshot.capability_summary.clone()
            }
            None if snapshot.status == "connected" => {
                snapshot.capability_summary.clone().or_else(|| {
                    Some(
                        "Spotify is connected, but the linked account profile is not available."
                            .to_owned(),
                    )
                })
            }
            None if snapshot.status == "disconnected" => {
                Some("No Spotify account is currently linked.".to_owned())
            }
            None => None,
        };

        connection_state_outcome("get_linked_account", state, snapshot, summary, Map::new())
    }

    fn validate_connection_action(&self, current_state: &SpotifyState) -> SpotifyOutcome {
        let mut state = next_state(current_state, "validate_connection");
        let mut snapshot = self.load_connection_snapshot(&state);

        match snapshot.status {
            "unconfigured" | "error" | "disconnected" | "connecting" => {
                return connection_state_outcome(
                    "validate_connection",
                    state,
                    snapshot,
                    None,
                    Map::new(),
                );
            }
            _ => {}
        }

        match self.ensure_access_grant(&mut state) {
            Ok(grant) => {
                let bootstrap_state = state.clone();
                snapshot = self.bootstrap_connected_snapshot(&mut state, &bootstrap_state, &grant);
                let summary = snapshot.capability_summary.clone();
                connection_state_outcome(
                    "validate_connection",
                    state,
                    snapshot,
                    summary,
                    Map::new(),
                )
            }
            Err(error) => connection_error_outcome("validate_connection", state, snapshot, error),
        }
    }

    fn validate_scopes_action(&self, current_state: &SpotifyState) -> SpotifyOutcome {
        let mut state = next_state(current_state, "validate_scopes");
        let mut snapshot = self.load_connection_snapshot(&state);

        match snapshot.status {
            "unconfigured" | "error" | "disconnected" | "connecting" => {
                return connection_state_outcome(
                    "validate_scopes",
                    state,
                    snapshot,
                    None,
                    Map::new(),
                );
            }
            _ => {}
        }

        match self.ensure_authorized(&mut state) {
            Ok(grant) => {
                let bootstrap_state = state.clone();
                snapshot = self.bootstrap_connected_snapshot(&mut state, &bootstrap_state, &grant);
                let mut extra = Map::new();
                let scopes_unknown =
                    snapshot.scopes.is_empty() && snapshot.missing_scopes.is_empty();
                extra.insert(
                    "scope_status".to_owned(),
                    Value::String(
                        if !snapshot.missing_scopes.is_empty() {
                            "invalid"
                        } else if scopes_unknown {
                            "unknown"
                        } else {
                            "valid"
                        }
                        .to_owned(),
                    ),
                );
                let summary = if !snapshot.missing_scopes.is_empty() {
                    snapshot
                        .capability_summary
                        .clone()
                        .unwrap_or_else(|| "Spotify permissions need to be refreshed.".to_owned())
                } else if scopes_unknown {
                    "Spotify is connected, but the granted scopes are not cached yet.".to_owned()
                } else {
                    "Spotify playback scopes are valid.".to_owned()
                };
                connection_state_outcome("validate_scopes", state, snapshot, Some(summary), extra)
            }
            Err(error) => connection_error_outcome("validate_scopes", state, snapshot, error),
        }
    }

    fn ensure_authorized(&self, state: &mut SpotifyState) -> Result<AccessGrant, SpotifyToolError> {
        let grant = self.ensure_access_grant(state)?;
        self.ensure_required_scopes(&grant)?;
        Ok(grant)
    }

    fn ensure_required_scopes(&self, grant: &AccessGrant) -> Result<(), SpotifyToolError> {
        let missing = self.missing_required_scopes(&grant.granted_scopes);
        if missing.is_empty() {
            return Ok(());
        }

        Err(SpotifyToolError::Forbidden {
            reason: "invalid_scope",
            message: format!(
                "Spotify access is missing required permissions: {}. Reconnect Spotify to continue.",
                missing.join(", ")
            ),
            code: None,
        })
    }

    fn auth_status_action(&self, current_state: &SpotifyState) -> SpotifyOutcome {
        let mut state = next_state(current_state, "auth_status");
        let mut snapshot = self.load_connection_snapshot(&state);

        match snapshot.status {
            "unconfigured" | "error" | "connecting" => {
                return connection_state_outcome("auth_status", state, snapshot, None, Map::new());
            }
            "disconnected" => return auth_required_outcome("auth_status", state, snapshot),
            _ => {}
        }

        match self.ensure_access_grant(&mut state) {
            Ok(grant) => {
                let bootstrap_state = state.clone();
                snapshot = self.bootstrap_connected_snapshot(&mut state, &bootstrap_state, &grant);
                let summary = snapshot.capability_summary.clone();
                connection_state_outcome("auth_status", state, snapshot, summary, Map::new())
            }
            Err(error) => connection_error_outcome("auth_status", state, snapshot, error),
        }
    }

    fn start_auth_action(&self, current_state: &SpotifyState) -> SpotifyOutcome {
        let mut state = next_state(current_state, "start_auth");
        let snapshot = self.load_connection_snapshot(&state);
        state.apply_connection_snapshot(&snapshot);

        let oauth_state = generate_oauth_state();
        match self.auth.build_authorize_url(&oauth_state) {
            Ok(authorize_url) => {
                state.auth_in_progress = true;
                state.connection_status = "connecting".to_owned();
                state.pending_auth_state = Some(oauth_state);
                auth_started_outcome(
                    "start_auth",
                    state,
                    "Opening Spotify sign-in now.",
                    object_fields(json!({
                        "authorize_url": authorize_url,
                        "configured": snapshot.configured,
                        "connected": false,
                        "connection_status": "connecting",
                        "required_scopes": self.config.configured_scopes()
                    })),
                )
            }
            Err(error) => failure_outcome("start_auth", state, error),
        }
    }

    fn handle_callback_action(
        &self,
        arguments: &Value,
        current_state: &SpotifyState,
    ) -> SpotifyOutcome {
        let mut state = next_state(current_state, "handle_callback");
        let callback = match parse_callback_payload(arguments) {
            Ok(callback) => callback,
            Err(error) => return failure_outcome("handle_callback", state, error),
        };

        if let Some(error) = callback.error {
            state.auth_connected = false;
            state.auth_in_progress = false;
            state.pending_auth_state = None;
            return failure_outcome(
                "handle_callback",
                state,
                SpotifyToolError::AuthError {
                    message: callback
                        .error_description
                        .unwrap_or(error)
                        .trim()
                        .to_owned(),
                },
            );
        }

        if let Some(expected_state) = current_state.pending_auth_state.as_deref() {
            if callback.oauth_state.as_deref() != Some(expected_state) {
                state.auth_connected = false;
                state.auth_in_progress = false;
                state.pending_auth_state = None;
                return failure_outcome(
                    "handle_callback",
                    state,
                    SpotifyToolError::AuthError {
                        message:
                            "Spotify callback state did not match the pending sign-in request."
                                .to_owned(),
                    },
                );
            }
        }

        let Some(code) = callback.code else {
            state.auth_connected = false;
            state.auth_in_progress = false;
            state.pending_auth_state = None;
            return failure_outcome(
                "handle_callback",
                state,
                SpotifyToolError::AuthError {
                    message: "Spotify callback did not include an authorization code.".to_owned(),
                },
            );
        };

        match self.auth.exchange_authorization_code(&code) {
            Ok(grant) => {
                state.auth_in_progress = false;
                state.pending_auth_state = None;
                let bootstrap_state = state.clone();
                let snapshot =
                    self.bootstrap_connected_snapshot(&mut state, &bootstrap_state, &grant);
                let summary = if snapshot.capability_status == "connected" {
                    "Spotify is connected. What would you like to listen to?".to_owned()
                } else {
                    snapshot
                        .capability_summary
                        .clone()
                        .unwrap_or_else(|| "Spotify authentication completed.".to_owned())
                };
                connection_state_outcome(
                    "handle_callback",
                    state,
                    snapshot,
                    Some(summary),
                    Map::new(),
                )
            }
            Err(error) => failure_outcome("handle_callback", state, error),
        }
    }

    fn exchange_code_action(
        &self,
        arguments: &Value,
        current_state: &SpotifyState,
    ) -> SpotifyOutcome {
        let mut state = next_state(current_state, "exchange_code");
        let code = optional_string_argument(arguments, &["code"])
            .or_else(|| self.config.authorization_code.clone());
        let Some(code) = code else {
            return failure_outcome(
                "exchange_code",
                state,
                SpotifyToolError::BadRequest {
                    message: "Spotify exchange_code requires an authorization code.".to_owned(),
                },
            );
        };

        if let Some(expected_state) = current_state.pending_auth_state.as_deref() {
            if let Some(received_state) = optional_string_argument(arguments, &["state"]) {
                if received_state != expected_state {
                    state.auth_connected = false;
                    state.auth_in_progress = false;
                    state.pending_auth_state = None;
                    return failure_outcome(
                        "exchange_code",
                        state,
                        SpotifyToolError::AuthError {
                            message:
                                "Spotify authorization state did not match the pending sign-in request."
                                    .to_owned(),
                        },
                    );
                }
            }
        }

        match self.auth.exchange_authorization_code(&code) {
            Ok(grant) => {
                state.auth_in_progress = false;
                state.pending_auth_state = None;
                let bootstrap_state = state.clone();
                let snapshot =
                    self.bootstrap_connected_snapshot(&mut state, &bootstrap_state, &grant);
                let summary = if snapshot.capability_status == "connected" {
                    "Spotify is connected. What would you like to listen to?".to_owned()
                } else {
                    snapshot
                        .capability_summary
                        .clone()
                        .unwrap_or_else(|| "Spotify authentication completed.".to_owned())
                };
                connection_state_outcome(
                    "exchange_code",
                    state,
                    snapshot,
                    Some(summary),
                    Map::new(),
                )
            }
            Err(error) => failure_outcome("exchange_code", state, error),
        }
    }

    fn refresh_token_action(&self, current_state: &SpotifyState) -> SpotifyOutcome {
        let mut state = next_state(current_state, "refresh_token");
        let tokens = match self.auth.load_effective_tokens() {
            Ok(tokens) => tokens,
            Err(error) => return failure_outcome("refresh_token", state, error),
        };
        match self.refresh_session_with_diagnostics(&mut state, tokens) {
            Ok(grant) => {
                state.auth_in_progress = false;
                state.pending_auth_state = None;
                let bootstrap_state = state.clone();
                let snapshot =
                    self.bootstrap_connected_snapshot(&mut state, &bootstrap_state, &grant);
                state.apply_connection_snapshot(&snapshot);
                success_outcome(
                    "refresh_token",
                    state,
                    snapshot
                        .capability_summary
                        .clone()
                        .unwrap_or_else(|| "Spotify session refreshed.".to_owned()),
                    object_fields(json!({
                        "connected": true,
                        "granted_scopes": grant.granted_scopes,
                        "account_display_name": snapshot.account.as_ref().and_then(|account| account.display_name.clone()),
                        "spotify_user_id": snapshot.account.as_ref().and_then(|account| account.spotify_user_id.clone()),
                        "last_refresh_at": snapshot.last_refresh_at,
                        "capability_status": snapshot.capability_status,
                        "missing_scopes": snapshot.missing_scopes
                    })),
                )
            }
            Err(error) => failure_outcome("refresh_token", state, error),
        }
    }

    fn refresh_token_if_needed_action(&self, current_state: &SpotifyState) -> SpotifyOutcome {
        let mut state = next_state(current_state, "refresh_token_if_needed");
        let snapshot = self.load_connection_snapshot(&state);

        match snapshot.status {
            "unconfigured" | "error" | "disconnected" | "connecting" => {
                return connection_state_outcome(
                    "refresh_token_if_needed",
                    state,
                    snapshot,
                    None,
                    Map::new(),
                );
            }
            "connected" => {
                state.apply_connection_snapshot(&snapshot);
                return success_outcome(
                    "refresh_token_if_needed",
                    state,
                    "Spotify session is already valid.",
                    object_fields(json!({
                        "refreshed": false
                    })),
                );
            }
            _ => {}
        }

        let tokens = match self.auth.load_effective_tokens() {
            Ok(tokens) => tokens,
            Err(error) => {
                return connection_error_outcome("refresh_token_if_needed", state, snapshot, error);
            }
        };

        match self.refresh_session_with_diagnostics(&mut state, tokens) {
            Ok(grant) => {
                let bootstrap_state = state.clone();
                let refreshed =
                    self.bootstrap_connected_snapshot(&mut state, &bootstrap_state, &grant);
                state.apply_connection_snapshot(&refreshed);
                success_outcome(
                    "refresh_token_if_needed",
                    state,
                    refreshed
                        .capability_summary
                        .clone()
                        .unwrap_or_else(|| "Spotify session refreshed.".to_owned()),
                    object_fields(json!({
                        "refreshed": true,
                        "last_refresh_at": refreshed.last_refresh_at,
                        "capability_status": refreshed.capability_status,
                        "missing_scopes": refreshed.missing_scopes
                    })),
                )
            }
            Err(error) => {
                connection_error_outcome("refresh_token_if_needed", state, snapshot, error)
            }
        }
    }

    fn disconnect_action(&self, action: &str, current_state: &SpotifyState) -> SpotifyOutcome {
        let mut state = next_state(current_state, action);
        if let Err(error) = self.auth.clear_session() {
            return failure_outcome(action, state, error);
        }

        state.clear_connection();
        let summary = match action {
            "clear_tokens" => "Spotify tokens cleared.",
            "unlink_account" => "Spotify account unlinked.",
            _ => "Spotify has been disconnected. You can sign in again whenever you want.",
        };

        success_outcome(
            action,
            state,
            summary,
            object_fields(json!({
                "connection_status": "disconnected",
                "connected": false
            })),
        )
    }

    fn capability_status_action(&self, current_state: &SpotifyState) -> SpotifyOutcome {
        let mut state = next_state(current_state, "capability_status");
        let grant = match self.ensure_access_grant(&mut state) {
            Ok(grant) => grant,
            Err(error) => return failure_outcome("capability_status", state, error),
        };
        let bootstrap_state = state.clone();
        let snapshot = self.bootstrap_connected_snapshot(&mut state, &bootstrap_state, &grant);
        state.apply_connection_snapshot(&snapshot);

        let active_device = state
            .available_devices
            .iter()
            .find(|device| device.is_active)
            .cloned();
        let summary = snapshot
            .capability_summary
            .clone()
            .unwrap_or_else(|| "Spotify capability state updated.".to_owned());

        let mut fields = object_fields(json!({
            "available": true,
            "connected": snapshot.connected,
            "capability_status": snapshot.capability_status,
            "capability_summary": snapshot.capability_summary,
            "configured_scopes": self.config.configured_scopes(),
            "granted_scopes": grant.granted_scopes,
            "missing_scopes": snapshot.missing_scopes,
            "device_count": state.available_devices.len(),
            "devices": devices_json_from_summaries(&state.available_devices)
        }));
        if let Some(active_device) = active_device {
            fields.insert(
                "active_device".to_owned(),
                serde_json::to_value(active_device).unwrap_or(Value::Null),
            );
        }

        success_outcome("capability_status", state, summary, fields)
    }

    fn get_devices_action(&self, current_state: &SpotifyState) -> SpotifyOutcome {
        let mut state = next_state(current_state, "get_devices");
        let grant = match self.ensure_authorized(&mut state) {
            Ok(grant) => grant,
            Err(error) => return failure_outcome("get_devices", state, error),
        };
        let devices = match self.get_devices_with_diagnostics(&mut state, &grant.access_token) {
            Ok(devices) => devices,
            Err(error) => return failure_outcome("get_devices", state, error),
        };

        state.auth_connected = true;
        state.auth_in_progress = false;
        state.pending_auth_state = None;
        state.record_devices(&devices);

        let summary = if devices.is_empty() {
            "No Spotify devices are currently available.".to_owned()
        } else {
            format!(
                "You currently have {} Spotify devices available.",
                devices.len()
            )
        };
        state.capability_status = if targetable_devices(&devices).is_empty() {
            "no_available_device".to_owned()
        } else {
            "connected".to_owned()
        };
        state.capability_summary = Some(summary.clone());

        success_outcome(
            "get_devices",
            state,
            summary,
            object_fields(json!({
                "devices": devices_json(&devices)
            })),
        )
    }

    fn get_active_device_action(&self, current_state: &SpotifyState) -> SpotifyOutcome {
        let mut state = next_state(current_state, "get_active_device");
        let grant = match self.ensure_authorized(&mut state) {
            Ok(grant) => grant,
            Err(error) => return failure_outcome("get_active_device", state, error),
        };
        let devices = match self.get_devices_with_diagnostics(&mut state, &grant.access_token) {
            Ok(devices) => devices,
            Err(error) => return failure_outcome("get_active_device", state, error),
        };
        state.record_devices(&devices);

        if devices.is_empty() {
            return failure_outcome(
                "get_active_device",
                state,
                no_available_device_error(&devices),
            );
        }

        match devices.iter().find(|device| device.is_active) {
            Some(device) => {
                state.auth_connected = true;
                state.auth_in_progress = false;
                state.pending_auth_state = None;
                state.record_device(device);
                state.capability_status = "connected".to_owned();
                state.capability_summary =
                    Some(format!("The active Spotify device is {}.", device.name));
                success_outcome(
                    "get_active_device",
                    state,
                    format!("The active Spotify device is {}.", device.name),
                    object_fields(json!({
                        "target_device": device_json(device)
                    })),
                )
            }
            None => failure_outcome(
                "get_active_device",
                state,
                SpotifyToolError::PlaybackNotActive {
                    message: "No Spotify device is currently active.".to_owned(),
                    devices: devices.iter().map(SpotifyDeviceSummary::from).collect(),
                },
            ),
        }
    }

    fn ensure_playback_target_action(
        &self,
        arguments: &Value,
        current_state: &SpotifyState,
    ) -> SpotifyOutcome {
        let mut state = next_state(current_state, "ensure_playback_target");
        let grant = match self.ensure_authorized(&mut state) {
            Ok(grant) => grant,
            Err(error) => return failure_outcome("ensure_playback_target", state, error),
        };
        let devices = match self.get_devices_with_diagnostics(&mut state, &grant.access_token) {
            Ok(devices) => devices,
            Err(error) => return failure_outcome("ensure_playback_target", state, error),
        };
        state.record_devices(&devices);

        let requested_device = RequestedDevice::from_arguments(arguments);
        match SpotifyDeviceResolver::resolve_for_play(&devices, &requested_device, &state) {
            Ok(resolved) => {
                state.auth_connected = true;
                state.auth_in_progress = false;
                state.pending_auth_state = None;
                state.record_device(&resolved.device);
                state.capability_status = "connected".to_owned();
                state.capability_summary =
                    Some(format!("Spotify will target {}.", resolved.device.name));
                success_outcome(
                    "ensure_playback_target",
                    state,
                    format!("Spotify will target {}.", resolved.device.name),
                    object_fields(json!({
                        "target_device": device_json(&resolved.device),
                        "selection_reason": resolved.selection_reason
                    })),
                )
            }
            Err(error) => failure_outcome("ensure_playback_target", state, error),
        }
    }

    fn transfer_playback_action(
        &self,
        arguments: &Value,
        current_state: &SpotifyState,
    ) -> SpotifyOutcome {
        let mut state = next_state(current_state, "transfer_playback");
        let grant = match self.ensure_authorized(&mut state) {
            Ok(grant) => grant,
            Err(error) => return failure_outcome("transfer_playback", state, error),
        };
        let devices = match self.get_devices_with_diagnostics(&mut state, &grant.access_token) {
            Ok(devices) => devices,
            Err(error) => return failure_outcome("transfer_playback", state, error),
        };
        state.record_devices(&devices);

        let requested_device = RequestedDevice::from_arguments(arguments);
        let resolved = match SpotifyDeviceResolver::resolve_for_transfer(
            &devices,
            &requested_device,
            &state,
        ) {
            Ok(resolved) => resolved,
            Err(error) => return failure_outcome("transfer_playback", state, error),
        };
        let Some(device_id) = resolved.device.id.as_deref() else {
            return failure_outcome(
                "transfer_playback",
                state,
                untargetable_device_error(&resolved.device, &devices),
            );
        };

        let play = optional_bool_argument(arguments, &["play", "resume"]).unwrap_or(false);
        if let Err(error) = self.transfer_playback_with_diagnostics(
            &mut state,
            &grant.access_token,
            device_id,
            play,
        ) {
            return failure_outcome("transfer_playback", state, error);
        }

        let playback = self
            .get_current_playback_with_diagnostics(&mut state, &grant.access_token)
            .ok()
            .flatten();
        state.auth_connected = true;
        state.auth_in_progress = false;
        state.pending_auth_state = None;
        if let Some(playback) = playback.as_ref() {
            state.record_playback(Some(playback));
        } else {
            state.record_device(&resolved.device);
            state.is_playing = play;
        }

        let summary = if play {
            format!(
                "Transferred Spotify playback to {} and resumed it.",
                resolved.device.name
            )
        } else {
            format!("Transferred Spotify playback to {}.", resolved.device.name)
        };
        state.capability_status = "connected".to_owned();
        state.capability_summary = Some(summary.clone());

        let mut fields = object_fields(json!({
            "target_device": device_json(&resolved.device),
            "selection_reason": resolved.selection_reason,
            "is_playing": playback
                .as_ref()
                .map(|playback| playback.is_playing)
                .unwrap_or(play)
        }));
        if let Some(track) = playback
            .as_ref()
            .and_then(|playback| playback.track.as_ref())
        {
            fields.insert("track".to_owned(), track.to_json());
        }

        success_outcome("transfer_playback", state, summary, fields)
    }

    fn play_action(
        &self,
        action: &str,
        arguments: &Value,
        current_state: &SpotifyState,
    ) -> SpotifyOutcome {
        let mut state = next_state(current_state, action);
        let grant = match self.ensure_authorized(&mut state) {
            Ok(grant) => grant,
            Err(error) => return failure_outcome(action, state, error),
        };

        let query = optional_string_argument(arguments, &["query"]);
        let selected_track = if let Some(query) = query.as_deref() {
            state.last_query = Some(query.to_owned());
            match self.api.search_top_track(&grant.access_token, query) {
                Ok(track) => Some(track),
                Err(error) => return failure_outcome(action, state, error),
            }
        } else {
            None
        };

        let devices = match self.get_devices_with_diagnostics(&mut state, &grant.access_token) {
            Ok(devices) => devices,
            Err(error) => return failure_outcome(action, state, error),
        };
        state.record_devices(&devices);

        let requested_device = RequestedDevice::from_arguments(arguments);
        let resolved =
            match SpotifyDeviceResolver::resolve_for_play(&devices, &requested_device, &state) {
                Ok(resolved) => resolved,
                Err(error) => return failure_outcome(action, state, error),
            };

        let device_id = if resolved.device.is_active {
            resolved.device.id.as_deref()
        } else {
            let Some(device_id) = resolved.device.id.as_deref() else {
                return failure_outcome(
                    action,
                    state,
                    untargetable_device_error(&resolved.device, &devices),
                );
            };

            // Activate the selected Spotify Connect device before issuing playback.
            if let Err(error) = self.transfer_playback_with_diagnostics(
                &mut state,
                &grant.access_token,
                device_id,
                false,
            ) {
                return failure_outcome(action, state, error);
            }

            Some(device_id)
        };

        let uris = selected_track.as_ref().map(|track| vec![track.uri.clone()]);
        if let Err(error) = self.start_playback_with_diagnostics(
            &mut state,
            &grant.access_token,
            device_id,
            uris.as_deref(),
        ) {
            return failure_outcome(action, state, error);
        }

        let playback = self
            .get_current_playback_with_diagnostics(&mut state, &grant.access_token)
            .ok()
            .flatten();
        let effective_track = playback
            .as_ref()
            .and_then(|playback| playback.track.clone())
            .or(selected_track.clone());
        let effective_device = playback
            .as_ref()
            .and_then(|playback| playback.device.clone())
            .unwrap_or_else(|| resolved.device.clone());

        state.auth_connected = true;
        state.auth_in_progress = false;
        state.pending_auth_state = None;
        state.record_device(&effective_device);
        state.is_playing = true;
        if let Some(playback) = playback.as_ref() {
            state.record_playback(Some(playback));
        }
        if let Some(track) = effective_track.as_ref() {
            state.record_track(track);
        }

        let summary = if let Some(track) = effective_track.as_ref() {
            format!("Playing {} on {}.", track.display(), effective_device.name)
        } else {
            format!("Spotify playback started on {}.", effective_device.name)
        };
        state.capability_status = "connected".to_owned();
        state.capability_summary = Some(summary.clone());

        let mut fields = object_fields(json!({
            "target_device": device_json(&effective_device),
            "selection_reason": resolved.selection_reason,
            "is_playing": true
        }));
        if let Some(track) = effective_track {
            fields.insert("track".to_owned(), track.to_json());
        }

        success_outcome(action, state, summary, fields)
    }

    fn pause_action(&self, current_state: &SpotifyState, arguments: &Value) -> SpotifyOutcome {
        let mut state = next_state(current_state, "pause");
        let grant = match self.ensure_authorized(&mut state) {
            Ok(grant) => grant,
            Err(error) => return failure_outcome("pause", state, error),
        };
        let devices = match self.api.get_devices(&grant.access_token) {
            Ok(devices) => devices,
            Err(error) => return failure_outcome("pause", state, error),
        };
        state.record_devices(&devices);

        let requested_device = RequestedDevice::from_arguments(arguments);
        let resolved =
            match SpotifyDeviceResolver::resolve_for_active_control(&devices, &requested_device) {
                Ok(resolved) => resolved,
                Err(error) => return failure_outcome("pause", state, error),
            };

        if !resolved.device.is_active && resolved.device.id.is_none() {
            return failure_outcome(
                "pause",
                state,
                untargetable_device_error(&resolved.device, &devices),
            );
        }

        if let Err(error) = self
            .api
            .pause_playback(&grant.access_token, resolved.device.id.as_deref())
        {
            return failure_outcome("pause", state, error);
        }

        let playback = self
            .api
            .get_current_playback(&grant.access_token)
            .ok()
            .flatten();
        state.auth_connected = true;
        state.auth_in_progress = false;
        state.pending_auth_state = None;
        state.record_device(&resolved.device);
        state.is_playing = false;
        if let Some(playback) = playback.as_ref() {
            state.record_playback(Some(playback));
        }

        success_outcome(
            "pause",
            state,
            format!("Paused Spotify on {}.", resolved.device.name),
            object_fields(json!({
                "target_device": device_json(&resolved.device),
                "is_playing": false
            })),
        )
    }

    fn skip_action(
        &self,
        action: &str,
        current_state: &SpotifyState,
        arguments: &Value,
    ) -> SpotifyOutcome {
        let mut state = next_state(current_state, action);
        let grant = match self.ensure_authorized(&mut state) {
            Ok(grant) => grant,
            Err(error) => return failure_outcome(action, state, error),
        };
        let devices = match self.api.get_devices(&grant.access_token) {
            Ok(devices) => devices,
            Err(error) => return failure_outcome(action, state, error),
        };
        state.record_devices(&devices);

        let requested_device = RequestedDevice::from_arguments(arguments);
        let resolved =
            match SpotifyDeviceResolver::resolve_for_active_control(&devices, &requested_device) {
                Ok(resolved) => resolved,
                Err(error) => return failure_outcome(action, state, error),
            };

        let result = match action {
            "previous_track" => self
                .api
                .skip_previous(&grant.access_token, resolved.device.id.as_deref()),
            _ => self
                .api
                .skip_next(&grant.access_token, resolved.device.id.as_deref()),
        };
        if let Err(error) = result {
            return failure_outcome(action, state, error);
        }

        let playback = self
            .api
            .get_current_playback(&grant.access_token)
            .ok()
            .flatten();
        state.auth_connected = true;
        state.auth_in_progress = false;
        state.pending_auth_state = None;
        state.record_device(&resolved.device);
        if let Some(playback) = playback.as_ref() {
            state.record_playback(Some(playback));
        }

        let summary = match playback
            .as_ref()
            .and_then(|playback| playback.track.as_ref())
        {
            Some(track) if action == "previous_track" => {
                format!("Moved back to {}.", track.display())
            }
            Some(track) => format!("Skipped to {}.", track.display()),
            None if action == "previous_track" => "Moved to the previous Spotify track.".to_owned(),
            None => "Skipped to the next Spotify track.".to_owned(),
        };

        let mut fields = object_fields(json!({
            "target_device": device_json(&resolved.device),
            "is_playing": playback
                .as_ref()
                .map(|playback| playback.is_playing)
                .unwrap_or(true)
        }));
        if let Some(track) = playback
            .as_ref()
            .and_then(|playback| playback.track.as_ref())
        {
            fields.insert("track".to_owned(), track.to_json());
        }

        success_outcome(action, state, summary, fields)
    }

    fn set_volume_action(&self, arguments: &Value, current_state: &SpotifyState) -> SpotifyOutcome {
        let mut state = next_state(current_state, "set_volume");
        let grant = match self.ensure_authorized(&mut state) {
            Ok(grant) => grant,
            Err(error) => return failure_outcome("set_volume", state, error),
        };

        let volume_percent = match parse_volume_percent(arguments) {
            Ok(volume_percent) => volume_percent,
            Err(error) => return failure_outcome("set_volume", state, error),
        };

        let devices = match self.api.get_devices(&grant.access_token) {
            Ok(devices) => devices,
            Err(error) => return failure_outcome("set_volume", state, error),
        };
        state.record_devices(&devices);

        let requested_device = RequestedDevice::from_arguments(arguments);
        let resolved =
            match SpotifyDeviceResolver::resolve_for_volume(&devices, &requested_device, &state) {
                Ok(resolved) => resolved,
                Err(error) => return failure_outcome("set_volume", state, error),
            };

        if !resolved.device.is_active && resolved.device.id.is_none() {
            return failure_outcome(
                "set_volume",
                state,
                untargetable_device_error(&resolved.device, &devices),
            );
        }

        if let Err(error) = self.api.set_volume(
            &grant.access_token,
            resolved.device.id.as_deref(),
            volume_percent,
        ) {
            return failure_outcome("set_volume", state, error);
        }

        state.auth_connected = true;
        state.auth_in_progress = false;
        state.pending_auth_state = None;
        state.record_device(&resolved.device);
        state.volume_percent = Some(volume_percent);

        success_outcome(
            "set_volume",
            state,
            format!(
                "Set Spotify volume to {} on {}.",
                volume_percent, resolved.device.name
            ),
            object_fields(json!({
                "target_device": device_json(&resolved.device),
                "volume_percent": volume_percent
            })),
        )
    }

    fn current_playback_action(
        &self,
        action: &str,
        current_state: &SpotifyState,
    ) -> SpotifyOutcome {
        let mut state = next_state(current_state, action);
        let grant = match self.ensure_authorized(&mut state) {
            Ok(grant) => grant,
            Err(error) => return failure_outcome(action, state, error),
        };

        match self.get_current_playback_with_diagnostics(&mut state, &grant.access_token) {
            Ok(Some(playback)) => {
                state.auth_connected = true;
                state.auth_in_progress = false;
                state.pending_auth_state = None;
                state.record_playback(Some(&playback));
                let summary = describe_playback(&playback);
                state.capability_status = "connected".to_owned();
                state.capability_summary = Some(summary.clone());
                success_outcome(action, state, summary, playback_fields(&playback))
            }
            Ok(None) => {
                state.auth_connected = true;
                state.auth_in_progress = false;
                state.pending_auth_state = None;
                state.is_playing = false;
                state.clear_track();
                state.capability_status = "connected".to_owned();
                state.capability_summary =
                    Some("Nothing is currently playing on Spotify.".to_owned());
                success_outcome(
                    action,
                    state,
                    "Nothing is currently playing on Spotify.".to_owned(),
                    object_fields(json!({
                        "is_playing": false
                    })),
                )
            }
            Err(error) => failure_outcome(action, state, error),
        }
    }

    fn currently_playing_action(&self, current_state: &SpotifyState) -> SpotifyOutcome {
        let mut state = next_state(current_state, "currently_playing");
        let grant = match self.ensure_authorized(&mut state) {
            Ok(grant) => grant,
            Err(error) => return failure_outcome("currently_playing", state, error),
        };

        match self.api.get_currently_playing(&grant.access_token) {
            Ok(Some(playback)) => {
                state.auth_connected = true;
                state.auth_in_progress = false;
                state.pending_auth_state = None;
                state.record_playback(Some(&playback));
                let summary = describe_playback(&playback);
                state.capability_status = "connected".to_owned();
                state.capability_summary = Some(summary.clone());
                success_outcome(
                    "currently_playing",
                    state,
                    summary,
                    playback_fields(&playback),
                )
            }
            Ok(None) => {
                state.auth_connected = true;
                state.auth_in_progress = false;
                state.pending_auth_state = None;
                state.is_playing = false;
                state.clear_track();
                state.capability_status = "connected".to_owned();
                state.capability_summary =
                    Some("Nothing is currently playing on Spotify.".to_owned());
                success_outcome(
                    "currently_playing",
                    state,
                    "Nothing is currently playing on Spotify.".to_owned(),
                    object_fields(json!({
                        "is_playing": false
                    })),
                )
            }
            Err(error) => failure_outcome("currently_playing", state, error),
        }
    }

    fn search_track_action(
        &self,
        action: &str,
        arguments: &Value,
        current_state: &SpotifyState,
    ) -> SpotifyOutcome {
        let mut state = next_state(current_state, action);
        let grant = match self.ensure_authorized(&mut state) {
            Ok(grant) => grant,
            Err(error) => return failure_outcome(action, state, error),
        };
        let query = match required_string_argument(arguments, &["query"]) {
            Ok(query) => query,
            Err(error) => return failure_outcome(action, state, error),
        };
        match self.api.search_top_track(&grant.access_token, &query) {
            Ok(track) => {
                state.auth_connected = true;
                state.auth_in_progress = false;
                state.pending_auth_state = None;
                state.last_query = Some(query.clone());
                state.record_track(&track);
                success_outcome(
                    action,
                    state,
                    format!("Found {}.", track.display()),
                    object_fields(json!({
                        "query": query,
                        "track": track.to_json()
                    })),
                )
            }
            Err(error) => failure_outcome(action, state, error),
        }
    }

    fn search_album_action(
        &self,
        arguments: &Value,
        current_state: &SpotifyState,
    ) -> SpotifyOutcome {
        let mut state = next_state(current_state, "search_album");
        let grant = match self.ensure_authorized(&mut state) {
            Ok(grant) => grant,
            Err(error) => return failure_outcome("search_album", state, error),
        };
        let query = match required_string_argument(arguments, &["query"]) {
            Ok(query) => query,
            Err(error) => return failure_outcome("search_album", state, error),
        };
        match self.api.search_top_album(&grant.access_token, &query) {
            Ok(album) => {
                state.auth_connected = true;
                state.auth_in_progress = false;
                state.pending_auth_state = None;
                state.last_query = Some(query.clone());
                success_outcome(
                    "search_album",
                    state,
                    format!("Found the album {}.", album.name),
                    object_fields(json!({
                        "query": query,
                        "album": album.to_json()
                    })),
                )
            }
            Err(error) => failure_outcome("search_album", state, error),
        }
    }

    fn search_artist_action(
        &self,
        arguments: &Value,
        current_state: &SpotifyState,
    ) -> SpotifyOutcome {
        let mut state = next_state(current_state, "search_artist");
        let grant = match self.ensure_authorized(&mut state) {
            Ok(grant) => grant,
            Err(error) => return failure_outcome("search_artist", state, error),
        };
        let query = match required_string_argument(arguments, &["query"]) {
            Ok(query) => query,
            Err(error) => return failure_outcome("search_artist", state, error),
        };
        match self.api.search_top_artist(&grant.access_token, &query) {
            Ok(artist) => {
                state.auth_connected = true;
                state.auth_in_progress = false;
                state.pending_auth_state = None;
                state.last_query = Some(query.clone());
                success_outcome(
                    "search_artist",
                    state,
                    format!("Found the artist {}.", artist.name),
                    object_fields(json!({
                        "query": query,
                        "artist": artist.to_json()
                    })),
                )
            }
            Err(error) => failure_outcome("search_artist", state, error),
        }
    }

    fn search_playlist_action(
        &self,
        arguments: &Value,
        current_state: &SpotifyState,
    ) -> SpotifyOutcome {
        let mut state = next_state(current_state, "search_playlist");
        let grant = match self.ensure_authorized(&mut state) {
            Ok(grant) => grant,
            Err(error) => return failure_outcome("search_playlist", state, error),
        };
        let query = match required_string_argument(arguments, &["query"]) {
            Ok(query) => query,
            Err(error) => return failure_outcome("search_playlist", state, error),
        };
        match self.api.search_top_playlist(&grant.access_token, &query) {
            Ok(playlist) => {
                state.auth_connected = true;
                state.auth_in_progress = false;
                state.pending_auth_state = None;
                state.last_query = Some(query.clone());
                success_outcome(
                    "search_playlist",
                    state,
                    format!("Found the playlist {}.", playlist.name),
                    object_fields(json!({
                        "query": query,
                        "playlist": playlist.to_json()
                    })),
                )
            }
            Err(error) => failure_outcome("search_playlist", state, error),
        }
    }

    fn resolve_track_uri_from_query_action(
        &self,
        arguments: &Value,
        current_state: &SpotifyState,
    ) -> SpotifyOutcome {
        let mut state = next_state(current_state, "resolve_track_uri_from_query");
        let grant = match self.ensure_authorized(&mut state) {
            Ok(grant) => grant,
            Err(error) => return failure_outcome("resolve_track_uri_from_query", state, error),
        };
        let query = match required_string_argument(arguments, &["query"]) {
            Ok(query) => query,
            Err(error) => {
                return failure_outcome("resolve_track_uri_from_query", state, error);
            }
        };
        match self.api.search_top_track(&grant.access_token, &query) {
            Ok(track) => {
                state.auth_connected = true;
                state.auth_in_progress = false;
                state.pending_auth_state = None;
                state.last_query = Some(query.clone());
                state.record_track(&track);
                success_outcome(
                    "resolve_track_uri_from_query",
                    state,
                    format!("Resolved {}.", track.display()),
                    object_fields(json!({
                        "query": query,
                        "track": track.to_json()
                    })),
                )
            }
            Err(error) => failure_outcome("resolve_track_uri_from_query", state, error),
        }
    }
}

impl SpotifyState {
    fn clear_connection(&mut self) {
        self.configured = true;
        self.connected = false;
        self.connection_status = "disconnected".to_owned();
        self.token_status = "missing".to_owned();
        self.capability_status = "auth_required".to_owned();
        self.capability_summary = Some(SPOTIFY_AUTH_REQUIRED_MESSAGE.to_owned());
        self.auth_connected = false;
        self.auth_in_progress = false;
        self.pending_auth_state = None;
        self.account_display_name = None;
        self.account_email = None;
        self.spotify_user_id = None;
        self.scopes.clear();
        self.missing_scopes.clear();
        self.last_authenticated_at = None;
        self.last_refresh_at = None;
        self.last_error = None;
        self.last_error_reason = None;
        self.available_devices.clear();
        self.device_id = None;
        self.device_name = None;
        self.volume_percent = None;
        self.is_playing = false;
        self.clear_track();
    }

    fn apply_connection_snapshot(&mut self, snapshot: &SpotifyConnectionSnapshot) {
        self.configured = snapshot.configured;
        self.connected = snapshot.connected;
        self.connection_status = snapshot.status.to_owned();
        self.token_status = snapshot.token_status.to_owned();
        self.capability_status = snapshot.capability_status.clone();
        self.capability_summary = snapshot.capability_summary.clone();
        self.availability = if snapshot.configured {
            "available".to_owned()
        } else {
            "unavailable".to_owned()
        };
        self.availability_reason = snapshot.reason.map(str::to_owned);
        self.auth_connected = snapshot.connected;
        self.account_display_name = snapshot
            .account
            .as_ref()
            .and_then(|account| account.display_name.clone());
        self.account_email = snapshot
            .account
            .as_ref()
            .and_then(|account| account.email.clone());
        self.spotify_user_id = snapshot
            .account
            .as_ref()
            .and_then(|account| account.spotify_user_id.clone());
        self.scopes = snapshot.scopes.clone();
        self.missing_scopes = snapshot.missing_scopes.clone();
        self.last_authenticated_at = snapshot.last_authenticated_at.clone();
        self.last_refresh_at = snapshot.last_refresh_at.clone();
        self.last_error = snapshot.last_error.clone();
        self.last_error_reason = snapshot.last_error_reason.clone();
    }

    fn clear_track(&mut self) {
        self.track = None;
        self.artist = None;
        self.album = None;
        self.track_uri = None;
    }

    fn record_track(&mut self, track: &SpotifyTrack) {
        self.track = Some(track.name.clone());
        self.artist = Some(track.artists.join(", "));
        self.album = Some(track.album.clone());
        self.track_uri = Some(track.uri.clone());
    }

    fn record_device(&mut self, device: &SpotifyConnectDevice) {
        self.device_id = device.id.clone();
        self.device_name = Some(device.name.clone());
        self.volume_percent = device.volume_percent;
    }

    fn record_devices(&mut self, devices: &[SpotifyConnectDevice]) {
        self.available_devices = devices.iter().map(SpotifyDeviceSummary::from).collect();
    }

    fn record_playback(&mut self, playback: Option<&SpotifyPlaybackState>) {
        if let Some(playback) = playback {
            self.is_playing = playback.is_playing;
            if let Some(device) = playback.device.as_ref() {
                self.record_device(device);
            }
            if let Some(track) = playback.track.as_ref() {
                self.record_track(track);
            } else {
                self.clear_track();
            }
        }
    }

    fn push_api_diagnostic(&mut self, diagnostic: SpotifyApiDiagnostic) {
        self.recent_api_diagnostics.insert(0, diagnostic);
        self.recent_api_diagnostics
            .truncate(MAX_SPOTIFY_API_DIAGNOSTICS);
    }
}

impl SpotifyAuthManager {
    fn build_authorize_url(&self, oauth_state: &str) -> Result<String, SpotifyToolError> {
        let mut url = Url::parse(&format!(
            "{}/authorize",
            self.config.accounts_base_url.trim_end_matches('/')
        ))
        .map_err(|error| {
            SpotifyToolError::Unavailable(SpotifyUnavailable::bad_config(format!(
                "Spotify authorize URL is invalid: {error}"
            )))
        })?;
        url.query_pairs_mut()
            .append_pair("client_id", &self.config.client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &self.config.redirect_uri)
            .append_pair("scope", &self.config.configured_scopes().join(" "))
            .append_pair("state", oauth_state)
            .append_pair("show_dialog", "true");
        Ok(url.to_string())
    }

    fn refresh_session_from(&self, tokens: SpotifyTokens) -> Result<AccessGrant, SpotifyToolError> {
        let refresh_token = tokens
            .refresh_token
            .ok_or_else(|| SpotifyToolError::AuthRequired {
                message: SPOTIFY_AUTH_REQUIRED_MESSAGE.to_owned(),
            })?;
        let response = self.send_token_request(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.as_str()),
        ])?;
        let tokens = self.persist_token_response(
            response,
            Some(refresh_token),
            &tokens.granted_scopes,
            TokenPersistenceKind::Refresh,
            tokens.account.clone(),
            tokens.last_authenticated_at.clone(),
        )?;
        let access_token =
            tokens
                .access_token
                .clone()
                .ok_or_else(|| SpotifyToolError::AuthError {
                    message: "Spotify refresh response did not include an access token.".to_owned(),
                })?;
        Ok(AccessGrant {
            access_token,
            granted_scopes: tokens.granted_scopes,
        })
    }

    fn exchange_authorization_code(&self, code: &str) -> Result<AccessGrant, SpotifyToolError> {
        let existing = self.load_effective_tokens()?;
        let response = self.send_token_request(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", self.config.redirect_uri.as_str()),
        ])?;
        let tokens = self.persist_token_response(
            response,
            existing.refresh_token,
            &existing.granted_scopes,
            TokenPersistenceKind::Authentication,
            existing.account,
            existing.last_authenticated_at,
        )?;
        let access_token =
            tokens
                .access_token
                .clone()
                .ok_or_else(|| SpotifyToolError::AuthError {
                    message: "Spotify code exchange did not include an access token.".to_owned(),
                })?;
        Ok(AccessGrant {
            access_token,
            granted_scopes: tokens.granted_scopes,
        })
    }

    fn load_effective_tokens(&self) -> Result<SpotifyTokens, SpotifyToolError> {
        self.token_store.load()
    }

    fn load_cached_tokens(&self) -> Result<SpotifyTokens, SpotifyToolError> {
        self.token_store.load()
    }

    fn clear_session(&self) -> Result<(), SpotifyToolError> {
        self.token_store.clear()
    }

    fn persist_account_profile(
        &self,
        account: SpotifyAccountProfile,
    ) -> Result<SpotifyTokens, SpotifyToolError> {
        let mut tokens = self.load_effective_tokens()?;
        tokens.account = Some(account);
        self.token_store.save(&tokens)?;
        Ok(tokens)
    }

    fn persist_token_response(
        &self,
        response: TokenResponse,
        existing_refresh_token: Option<String>,
        existing_scopes: &[String],
        persistence_kind: TokenPersistenceKind,
        existing_account: Option<SpotifyAccountProfile>,
        existing_authenticated_at: Option<String>,
    ) -> Result<SpotifyTokens, SpotifyToolError> {
        let now = now_rfc3339();
        let granted_scopes = response
            .scope
            .as_deref()
            .map(parse_scope_string)
            .filter(|scopes| !scopes.is_empty())
            .unwrap_or_else(|| existing_scopes.to_vec());
        let tokens = SpotifyTokens {
            access_token: Some(response.access_token),
            refresh_token: response.refresh_token.or(existing_refresh_token),
            expires_at: Some(expiry_timestamp(response.expires_in)),
            granted_scopes,
            account: existing_account,
            last_authenticated_at: match persistence_kind {
                TokenPersistenceKind::Authentication => Some(now.clone()),
                TokenPersistenceKind::Refresh => existing_authenticated_at,
            },
            last_refresh_at: match persistence_kind {
                TokenPersistenceKind::Authentication => None,
                TokenPersistenceKind::Refresh => Some(now),
            },
        };
        self.token_store.save(&tokens)?;
        Ok(tokens)
    }

    fn send_token_request(&self, form: &[(&str, &str)]) -> Result<TokenResponse, SpotifyToolError> {
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
            .map_err(|error| SpotifyToolError::Network {
                message: format!("Spotify authentication request failed: {error}"),
            })?;

        parse_token_response(response)
    }
}

impl SpotifyApiClient {
    fn get_current_user_profile(
        &self,
        access_token: &str,
    ) -> Result<SpotifyCurrentUserProfile, SpotifyToolError> {
        let url = format!("{}/me", self.config.api_base_url.trim_end_matches('/'));
        let value = self.read_json(
            self.client.get(url).bearer_auth(access_token),
            "Spotify current user request failed",
        )?;
        let payload: SpotifyCurrentUserResponse =
            serde_json::from_value(value).map_err(|error| SpotifyToolError::Unknown {
                message: format!("Spotify current user response was invalid: {error}"),
            })?;
        Ok(SpotifyCurrentUserProfile {
            display_name: payload.display_name,
            email: payload.email,
            spotify_user_id: payload.id,
        })
    }

    fn get_devices(
        &self,
        access_token: &str,
    ) -> Result<Vec<SpotifyConnectDevice>, SpotifyToolError> {
        let url = format!(
            "{}/me/player/devices",
            self.config.api_base_url.trim_end_matches('/')
        );
        let value = self.read_json(
            self.client.get(url).bearer_auth(access_token),
            "Spotify devices request failed",
        )?;
        let payload: SpotifyDevicesResponse =
            serde_json::from_value(value).map_err(|error| SpotifyToolError::Unknown {
                message: format!("Spotify devices response was invalid: {error}"),
            })?;
        Ok(payload.devices)
    }

    fn get_current_playback(
        &self,
        access_token: &str,
    ) -> Result<Option<SpotifyPlaybackState>, SpotifyToolError> {
        let url = format!(
            "{}/me/player",
            self.config.api_base_url.trim_end_matches('/')
        );
        let value = self.read_optional_json(
            self.client.get(url).bearer_auth(access_token),
            "Spotify playback request failed",
        )?;
        let Some(value) = value else {
            return Ok(None);
        };
        parse_playback_response(value)
    }

    fn get_currently_playing(
        &self,
        access_token: &str,
    ) -> Result<Option<SpotifyPlaybackState>, SpotifyToolError> {
        let url = format!(
            "{}/me/player/currently-playing",
            self.config.api_base_url.trim_end_matches('/')
        );
        let value = self.read_optional_json(
            self.client.get(url).bearer_auth(access_token),
            "Spotify currently-playing request failed",
        )?;
        let Some(value) = value else {
            return Ok(None);
        };
        parse_currently_playing_response(value)
    }

    fn transfer_playback(
        &self,
        access_token: &str,
        device_id: &str,
        play: bool,
    ) -> Result<(), SpotifyToolError> {
        let url = format!(
            "{}/me/player",
            self.config.api_base_url.trim_end_matches('/')
        );
        self.send_empty(
            self.client.put(url).bearer_auth(access_token).json(&json!({
                "device_ids": [device_id],
                "play": play
            })),
            "Spotify transfer playback request failed",
        )
    }

    fn start_or_resume_playback(
        &self,
        access_token: &str,
        device_id: Option<&str>,
        uris: Option<&[String]>,
    ) -> Result<(), SpotifyToolError> {
        let url = format!(
            "{}/me/player/play",
            self.config.api_base_url.trim_end_matches('/')
        );
        let request = self.client.put(url).bearer_auth(access_token);
        let request = if let Some(device_id) = device_id {
            request.query(&[("device_id", device_id)])
        } else {
            request
        };
        let request = if let Some(uris) = uris {
            request.json(&json!({ "uris": uris }))
        } else {
            request
        };
        self.send_empty(request, "Spotify start playback request failed")
    }

    fn pause_playback(
        &self,
        access_token: &str,
        device_id: Option<&str>,
    ) -> Result<(), SpotifyToolError> {
        let url = format!(
            "{}/me/player/pause",
            self.config.api_base_url.trim_end_matches('/')
        );
        let request = self.client.put(url).bearer_auth(access_token);
        let request = if let Some(device_id) = device_id {
            request.query(&[("device_id", device_id)])
        } else {
            request
        };
        self.send_empty(request, "Spotify pause playback request failed")
    }

    fn skip_next(
        &self,
        access_token: &str,
        device_id: Option<&str>,
    ) -> Result<(), SpotifyToolError> {
        self.skip("next", access_token, device_id)
    }

    fn skip_previous(
        &self,
        access_token: &str,
        device_id: Option<&str>,
    ) -> Result<(), SpotifyToolError> {
        self.skip("previous", access_token, device_id)
    }

    fn skip(
        &self,
        direction: &str,
        access_token: &str,
        device_id: Option<&str>,
    ) -> Result<(), SpotifyToolError> {
        let url = format!(
            "{}/me/player/{}",
            self.config.api_base_url.trim_end_matches('/'),
            direction
        );
        let request = self.client.post(url).bearer_auth(access_token);
        let request = if let Some(device_id) = device_id {
            request.query(&[("device_id", device_id)])
        } else {
            request
        };
        self.send_empty(request, "Spotify playback command failed")
    }

    fn set_volume(
        &self,
        access_token: &str,
        device_id: Option<&str>,
        volume_percent: u8,
    ) -> Result<(), SpotifyToolError> {
        let url = format!(
            "{}/me/player/volume",
            self.config.api_base_url.trim_end_matches('/')
        );
        let request = self
            .client
            .put(url)
            .bearer_auth(access_token)
            .query(&[("volume_percent", volume_percent.to_string())]);
        let request = if let Some(device_id) = device_id {
            request.query(&[("device_id", device_id)])
        } else {
            request
        };
        self.send_empty(request, "Spotify volume request failed")
    }

    fn search_top_track(
        &self,
        access_token: &str,
        query: &str,
    ) -> Result<SpotifyTrack, SpotifyToolError> {
        let value = self.search(access_token, query, "track")?;
        let payload: SpotifySearchResponse =
            serde_json::from_value(value).map_err(|error| SpotifyToolError::Unknown {
                message: format!("Spotify search response was invalid: {error}"),
            })?;
        payload
            .tracks
            .and_then(|tracks| tracks.items.into_iter().next())
            .map(SpotifyTrack::from)
            .ok_or_else(|| SpotifyToolError::BadRequest {
                message: format!("No Spotify track found for \"{query}\"."),
            })
    }

    fn search_top_album(
        &self,
        access_token: &str,
        query: &str,
    ) -> Result<SpotifyAlbumMatch, SpotifyToolError> {
        let value = self.search(access_token, query, "album")?;
        let payload: SpotifySearchResponse =
            serde_json::from_value(value).map_err(|error| SpotifyToolError::Unknown {
                message: format!("Spotify search response was invalid: {error}"),
            })?;
        payload
            .albums
            .and_then(|albums| albums.items.into_iter().next())
            .map(SpotifyAlbumMatch::from)
            .ok_or_else(|| SpotifyToolError::BadRequest {
                message: format!("No Spotify album found for \"{query}\"."),
            })
    }

    fn search_top_artist(
        &self,
        access_token: &str,
        query: &str,
    ) -> Result<SpotifyArtistMatch, SpotifyToolError> {
        let value = self.search(access_token, query, "artist")?;
        let payload: SpotifySearchResponse =
            serde_json::from_value(value).map_err(|error| SpotifyToolError::Unknown {
                message: format!("Spotify search response was invalid: {error}"),
            })?;
        payload
            .artists
            .and_then(|artists| artists.items.into_iter().next())
            .map(SpotifyArtistMatch::from)
            .ok_or_else(|| SpotifyToolError::BadRequest {
                message: format!("No Spotify artist found for \"{query}\"."),
            })
    }

    fn search_top_playlist(
        &self,
        access_token: &str,
        query: &str,
    ) -> Result<SpotifyPlaylistMatch, SpotifyToolError> {
        let value = self.search(access_token, query, "playlist")?;
        let payload: SpotifySearchResponse =
            serde_json::from_value(value).map_err(|error| SpotifyToolError::Unknown {
                message: format!("Spotify search response was invalid: {error}"),
            })?;
        payload
            .playlists
            .and_then(|playlists| playlists.items.into_iter().next())
            .map(SpotifyPlaylistMatch::from)
            .ok_or_else(|| SpotifyToolError::BadRequest {
                message: format!("No Spotify playlist found for \"{query}\"."),
            })
    }

    fn search(
        &self,
        access_token: &str,
        query: &str,
        search_type: &str,
    ) -> Result<Value, SpotifyToolError> {
        let url = format!("{}/search", self.config.api_base_url.trim_end_matches('/'));
        self.read_json(
            self.client.get(url).bearer_auth(access_token).query(&[
                ("q", query),
                ("type", search_type),
                ("limit", "1"),
            ]),
            "Spotify search request failed",
        )
    }

    fn read_json(
        &self,
        request: RequestBuilder,
        request_context: &str,
    ) -> Result<Value, SpotifyToolError> {
        self.read_optional_json(request, request_context)?
            .ok_or_else(|| SpotifyToolError::Unknown {
                message: "Spotify response was empty.".to_owned(),
            })
    }

    fn read_optional_json(
        &self,
        request: RequestBuilder,
        request_context: &str,
    ) -> Result<Option<Value>, SpotifyToolError> {
        let response = request.send().map_err(|error| SpotifyToolError::Network {
            message: format!("{request_context}: {error}"),
        })?;
        read_optional_json_response(response)
    }

    fn send_empty(
        &self,
        request: RequestBuilder,
        request_context: &str,
    ) -> Result<(), SpotifyToolError> {
        let response = request.send().map_err(|error| SpotifyToolError::Network {
            message: format!("{request_context}: {error}"),
        })?;
        read_optional_json_response(response).map(|_| ())
    }
}

fn load_spotify_config(config_dir: &Path) -> Result<SpotifyConfig, SpotifyUnavailable> {
    let base_path = config_dir.join("spotify_config.json");
    let local_path = config_dir.join("spotify_config.local.json");

    if !base_path.exists() && !local_path.exists() {
        return Err(SpotifyUnavailable::missing_config(
            "Spotify is not configured. config/spotify_config.json is missing.",
        ));
    }

    let base_value = if base_path.exists() {
        read_json_file(&base_path)?
    } else {
        Value::Object(Map::new())
    };

    let merged = if local_path.exists() {
        merge_json(base_value, read_json_file(&local_path)?)
    } else {
        base_value
    };

    let config: SpotifyConfig = serde_json::from_value(merged).map_err(|error| {
        SpotifyUnavailable::bad_config(format!("Spotify configuration payload is invalid: {error}"))
    })?;
    config.validate()?;
    Ok(config)
}

fn read_json_file(path: &Path) -> Result<Value, SpotifyUnavailable> {
    let contents = fs::read_to_string(path).map_err(|error| {
        SpotifyUnavailable::bad_config(format!(
            "Failed to read Spotify configuration {}: {error}",
            path.display()
        ))
    })?;
    serde_json::from_str(&contents).map_err(|error| {
        SpotifyUnavailable::bad_config(format!(
            "Invalid JSON in Spotify configuration {}: {error}",
            path.display()
        ))
    })
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

fn account_from_state(state: &SpotifyState) -> Option<SpotifyAccountProfile> {
    if state.account_display_name.is_none()
        && state.account_email.is_none()
        && state.spotify_user_id.is_none()
    {
        return None;
    }

    Some(SpotifyAccountProfile {
        display_name: state.account_display_name.clone(),
        email: state.account_email.clone(),
        spotify_user_id: state.spotify_user_id.clone(),
    })
}

#[derive(Debug)]
struct CallbackPayload {
    code: Option<String>,
    oauth_state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

fn parse_callback_payload(arguments: &Value) -> Result<CallbackPayload, SpotifyToolError> {
    if let Some(callback_url) = optional_string_argument(arguments, &["callback_url", "url"]) {
        return parse_callback_url(&callback_url);
    }

    Ok(CallbackPayload {
        code: optional_string_argument(arguments, &["code"]),
        oauth_state: optional_string_argument(arguments, &["state"]),
        error: optional_string_argument(arguments, &["error"]),
        error_description: optional_string_argument(arguments, &["error_description"]),
    })
}

fn parse_callback_url(callback_url: &str) -> Result<CallbackPayload, SpotifyToolError> {
    let url = Url::parse(callback_url).map_err(|error| SpotifyToolError::AuthError {
        message: format!("Spotify callback URL is invalid: {error}"),
    })?;
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

fn next_state(current_state: &SpotifyState, action: &str) -> SpotifyState {
    let mut state = current_state.clone();
    state.availability = "available".to_owned();
    state.availability_reason = None;
    if state.connection_status.is_empty() {
        state.connection_status = "disconnected".to_owned();
    }
    if state.token_status.is_empty() {
        state.token_status = "missing".to_owned();
    }
    state.last_action = Some(action.to_owned());
    state.last_status = None;
    state.last_error = None;
    state.last_error_reason = None;
    state
}

fn success_outcome(
    action: &str,
    mut state: SpotifyState,
    summary: impl Into<String>,
    mut fields: Map<String, Value>,
) -> SpotifyOutcome {
    let summary = summary.into();
    state.last_status = Some("success".to_owned());
    state.last_error = None;
    let mut result = base_result("success", action);
    fields.insert("message".to_owned(), Value::String(summary.clone()));
    result.extend(fields);
    SpotifyOutcome {
        result_json: Value::Object(result),
        summary,
        state,
    }
}

fn auth_started_outcome(
    action: &str,
    mut state: SpotifyState,
    summary: impl Into<String>,
    mut fields: Map<String, Value>,
) -> SpotifyOutcome {
    let summary = summary.into();
    state.last_status = Some("auth_started".to_owned());
    state.last_error = None;
    let mut result = base_result("auth_started", action);
    fields.insert("message".to_owned(), Value::String(summary.clone()));
    result.extend(fields);
    SpotifyOutcome {
        result_json: Value::Object(result),
        summary,
        state,
    }
}

fn failure_outcome(
    action: &str,
    mut state: SpotifyState,
    error: SpotifyToolError,
) -> SpotifyOutcome {
    let message = error.message().trim().to_owned();
    state.last_status = Some(error.status().to_owned());
    state.last_error = Some(message.clone());
    state.last_error_reason = error.reason().map(str::to_owned);
    state.capability_summary = Some(message.clone());
    if let Some(reason) = error.reason() {
        state.capability_status = reason.to_owned();
    }
    match &error {
        SpotifyToolError::Unavailable(unavailable) => {
            state.configured = false;
            state.connected = false;
            state.connection_status = if unavailable.reason == "missing_config" {
                "unconfigured".to_owned()
            } else {
                "error".to_owned()
            };
            state.token_status = if unavailable.reason == "missing_config" {
                "missing".to_owned()
            } else {
                "invalid".to_owned()
            };
            state.capability_status = if unavailable.reason == "missing_config" {
                "auth_required".to_owned()
            } else {
                "error".to_owned()
            };
            state.availability = "unavailable".to_owned();
            state.availability_reason = Some(unavailable.reason.to_owned());
            state.auth_connected = false;
            state.auth_in_progress = false;
            state.pending_auth_state = None;
        }
        SpotifyToolError::AuthRequired { .. } => {
            state.auth_connected = false;
            state.auth_in_progress = false;
            state.connected = false;
            state.connection_status = "disconnected".to_owned();
            state.token_status = if state.token_status.is_empty() {
                "missing".to_owned()
            } else {
                state.token_status.clone()
            };
            state.capability_status = "auth_required".to_owned();
        }
        SpotifyToolError::AuthExpired { .. } => {
            state.auth_connected = false;
            state.auth_in_progress = false;
            state.connected = false;
            state.connection_status = "expired".to_owned();
            state.token_status = "expired".to_owned();
            state.capability_status = "auth_expired".to_owned();
        }
        SpotifyToolError::AuthError { .. } => {
            state.auth_connected = false;
            state.auth_in_progress = false;
            state.connected = false;
            state.connection_status = "expired".to_owned();
            state.token_status = "refresh_failed".to_owned();
            state.capability_status = "auth_expired".to_owned();
        }
        SpotifyToolError::Forbidden { reason, .. } => {
            if state.token_status.is_empty() {
                state.token_status = "valid".to_owned();
            }
            if state.connection_status.is_empty() || state.connection_status == "error" {
                state.connection_status = "connected".to_owned();
            }
            if state.connection_status == "connected" || state.token_status == "valid" {
                state.connected = true;
                state.auth_connected = true;
            }
            state.capability_status = (*reason).to_owned();
            if *reason == "invalid_scope" {
                state.missing_scopes = default_scopes()
                    .into_iter()
                    .filter(|scope| !state.scopes.iter().any(|granted| granted == scope))
                    .collect();
            }
        }
        SpotifyToolError::NoAvailableDevice { devices, .. } => {
            state.available_devices = devices.clone();
            if state.token_status.is_empty() {
                state.token_status = "valid".to_owned();
            }
            state.connected = true;
            state.auth_connected = true;
            state.connection_status = "connected".to_owned();
            state.capability_status = "no_available_device".to_owned();
        }
        SpotifyToolError::DeviceNotFound { devices, .. }
        | SpotifyToolError::PlaybackNotActive { devices, .. } => {
            state.available_devices = devices.clone();
            if state.token_status.is_empty() {
                state.token_status = "valid".to_owned();
            }
            state.connected = true;
            state.auth_connected = true;
            state.connection_status = "connected".to_owned();
        }
        _ => {}
    }

    let mut result = base_result(error.status(), action);
    if let Some(reason) = error.reason() {
        result.insert("reason".to_owned(), Value::String(reason.to_owned()));
    }
    if let Some(code) = error.code() {
        result.insert(
            "code".to_owned(),
            Value::Number(serde_json::Number::from(code)),
        );
    }
    result.insert("message".to_owned(), Value::String(message.clone()));
    error.extra_fields(&mut result);

    SpotifyOutcome {
        result_json: Value::Object(result),
        summary: message,
        state,
    }
}

fn base_result(status: &str, action: &str) -> Map<String, Value> {
    object_fields(json!({
        "status": status,
        "tool": SPOTIFY_TOOL_NAME,
        "action": action
    }))
}

fn connection_state_outcome(
    _action: &str,
    mut state: SpotifyState,
    snapshot: SpotifyConnectionSnapshot,
    summary: Option<String>,
    extra_fields: Map<String, Value>,
) -> SpotifyOutcome {
    let summary = summary.unwrap_or_else(|| connection_summary(&snapshot));
    state.apply_connection_snapshot(&snapshot);
    state.last_status = Some(snapshot.status.to_owned());
    state.last_error = snapshot.last_error.clone();
    state.last_error_reason = snapshot.last_error_reason.clone();
    state.auth_in_progress = snapshot.status == "connecting";
    state.pending_auth_state = if snapshot.status == "connecting" {
        state.pending_auth_state.clone()
    } else {
        None
    };

    let mut result = snapshot.to_result_json();
    result.insert("message".to_owned(), Value::String(summary.clone()));
    result.extend(extra_fields);

    SpotifyOutcome {
        result_json: Value::Object(result),
        summary,
        state,
    }
}

fn auth_required_outcome(
    _action: &str,
    mut state: SpotifyState,
    mut snapshot: SpotifyConnectionSnapshot,
) -> SpotifyOutcome {
    snapshot.connected = false;
    snapshot.capability_status = "auth_required".to_owned();
    snapshot.capability_summary = Some(SPOTIFY_AUTH_REQUIRED_MESSAGE.to_owned());
    snapshot.last_error = Some(SPOTIFY_AUTH_REQUIRED_MESSAGE.to_owned());
    snapshot.last_error_reason = Some("auth_required".to_owned());
    state.apply_connection_snapshot(&snapshot);
    state.last_status = Some("auth_required".to_owned());
    state.last_error = snapshot.last_error.clone();
    state.last_error_reason = snapshot.last_error_reason.clone();
    state.auth_in_progress = false;
    state.pending_auth_state = None;

    let mut result = snapshot.to_result_json();
    result.insert(
        "status".to_owned(),
        Value::String("auth_required".to_owned()),
    );
    result.insert(
        "message".to_owned(),
        Value::String(SPOTIFY_AUTH_REQUIRED_MESSAGE.to_owned()),
    );

    SpotifyOutcome {
        result_json: Value::Object(result),
        summary: SPOTIFY_AUTH_REQUIRED_MESSAGE.to_owned(),
        state,
    }
}

fn auth_expired_outcome(
    _action: &str,
    mut state: SpotifyState,
    mut snapshot: SpotifyConnectionSnapshot,
    reason: &'static str,
    message: String,
) -> SpotifyOutcome {
    snapshot.status = "expired";
    snapshot.connected = false;
    snapshot.token_status = if reason == "auth_error" {
        "refresh_failed"
    } else {
        "expired"
    };
    snapshot.capability_status = reason.to_owned();
    snapshot.capability_summary = Some(message.clone());
    snapshot.reason = Some(reason);
    snapshot.last_error = Some(message.clone());
    snapshot.last_error_reason = Some(reason.to_owned());
    state.apply_connection_snapshot(&snapshot);
    state.last_status = Some("auth_expired".to_owned());
    state.last_error = snapshot.last_error.clone();
    state.last_error_reason = snapshot.last_error_reason.clone();
    state.auth_in_progress = false;
    state.pending_auth_state = None;

    let mut result = snapshot.to_result_json();
    result.insert(
        "status".to_owned(),
        Value::String("auth_expired".to_owned()),
    );
    result.insert("reason".to_owned(), Value::String(reason.to_owned()));
    result.insert("message".to_owned(), Value::String(message.clone()));

    SpotifyOutcome {
        result_json: Value::Object(result),
        summary: message,
        state,
    }
}

fn connection_error_outcome(
    action: &str,
    state: SpotifyState,
    mut snapshot: SpotifyConnectionSnapshot,
    error: SpotifyToolError,
) -> SpotifyOutcome {
    match error {
        SpotifyToolError::AuthRequired { .. } => auth_required_outcome(action, state, snapshot),
        SpotifyToolError::AuthExpired { message } => {
            auth_expired_outcome(action, state, snapshot, "auth_expired", message)
        }
        SpotifyToolError::AuthError { message } => {
            auth_expired_outcome(action, state, snapshot, "auth_error", message)
        }
        SpotifyToolError::Unavailable(unavailable) => {
            snapshot = SpotifyConnectionSnapshot::from_unavailable(
                &unavailable,
                &state,
                snapshot.required_scopes.clone(),
            );
            connection_state_outcome(action, state, snapshot, None, Map::new())
        }
        other => failure_outcome(action, state, other),
    }
}

fn connection_summary(snapshot: &SpotifyConnectionSnapshot) -> String {
    if let Some(capability_summary) = snapshot.capability_summary.as_ref() {
        match snapshot.capability_status.as_str() {
            "connected"
            | "connected_but_profile_unavailable"
            | "connected_but_playback_unavailable"
            | "invalid_scope"
            | "premium_required"
            | "playback_forbidden"
            | "no_available_device" => return capability_summary.clone(),
            _ => {}
        }
    }

    match snapshot.status {
        "connected" => snapshot
            .account
            .as_ref()
            .map(|account| {
                format!(
                    "Spotify is connected as {}.",
                    display_account_label(account)
                )
            })
            .unwrap_or_else(|| "Spotify is connected.".to_owned()),
        "connecting" => "Spotify sign-in is in progress.".to_owned(),
        "expired" => "Spotify authentication expired.".to_owned(),
        "unconfigured" => "Spotify is not configured.".to_owned(),
        "disconnected" => "Spotify is disconnected.".to_owned(),
        _ => snapshot
            .last_error
            .clone()
            .unwrap_or_else(|| "Spotify connection state updated.".to_owned()),
    }
}

fn display_account_label(account: &SpotifyAccountProfile) -> String {
    account
        .display_name
        .clone()
        .or_else(|| account.email.clone())
        .or_else(|| account.spotify_user_id.clone())
        .unwrap_or_else(|| "your Spotify account".to_owned())
}

fn canonical_reason(reason: Option<&str>) -> Option<&'static str> {
    match reason {
        Some("missing_config") => Some("missing_config"),
        Some("bad_config") => Some("bad_config"),
        Some("auth_required") => Some("auth_required"),
        Some("auth_expired") => Some("auth_expired"),
        Some("auth_error") => Some("auth_error"),
        Some("invalid_scope") => Some("invalid_scope"),
        Some("premium_required") => Some("premium_required"),
        Some("playback_forbidden") => Some("playback_forbidden"),
        Some("no_available_device") => Some("no_available_device"),
        Some("device_not_found") => Some("device_not_found"),
        Some("playback_not_active") => Some("playback_not_active"),
        Some("rate_limited") => Some("rate_limited"),
        Some("network_error") => Some("network_error"),
        Some("spotify_api_error") => Some("spotify_api_error"),
        Some("unknown_error") => Some("unknown_error"),
        _ => None,
    }
}

fn status_code_for_error(error: &SpotifyToolError) -> Option<u16> {
    match error {
        SpotifyToolError::Forbidden { code, .. } => *code,
        SpotifyToolError::Api { code, .. } => Some(*code),
        SpotifyToolError::AuthExpired { .. } => Some(401),
        _ => None,
    }
}

fn unavailable_connection_snapshot(
    unavailable: &SpotifyUnavailable,
    token_store: Option<&SpotifyTokenStore>,
    current_state: &SpotifyState,
) -> SpotifyConnectionSnapshot {
    let mut snapshot = SpotifyConnectionSnapshot::from_unavailable(
        unavailable,
        current_state,
        if current_state.scopes.is_empty() {
            default_scopes()
        } else {
            current_state.scopes.clone()
        },
    );

    if let Some(token_store) = token_store {
        if let Ok(tokens) = token_store.load() {
            if tokens.account.is_some() {
                snapshot.account = tokens.account;
            }
            if !tokens.granted_scopes.is_empty() {
                snapshot.scopes = tokens.granted_scopes;
            }
            if tokens.last_authenticated_at.is_some() {
                snapshot.last_authenticated_at = tokens.last_authenticated_at;
            }
            if tokens.last_refresh_at.is_some() {
                snapshot.last_refresh_at = tokens.last_refresh_at;
            }
        }
    }

    snapshot
}

fn playback_fields(playback: &SpotifyPlaybackState) -> Map<String, Value> {
    let mut fields = object_fields(json!({
        "is_playing": playback.is_playing
    }));
    if let Some(track) = playback.track.as_ref() {
        fields.insert("track".to_owned(), track.to_json());
    }
    if let Some(device) = playback.device.as_ref() {
        fields.insert("target_device".to_owned(), device_json(device));
    }
    fields
}

fn describe_playback(playback: &SpotifyPlaybackState) -> String {
    match playback.track.as_ref() {
        Some(track) if playback.is_playing => format!("{} is playing.", track.display()),
        Some(track) => format!("{} is paused.", track.display()),
        None => "Spotify playback state updated.".to_owned(),
    }
}

fn no_available_device_error(devices: &[SpotifyConnectDevice]) -> SpotifyToolError {
    SpotifyToolError::NoAvailableDevice {
        message: "No Spotify playback device is available right now.".to_owned(),
        devices: devices.iter().map(SpotifyDeviceSummary::from).collect(),
    }
}

fn untargetable_device_error(
    device: &SpotifyConnectDevice,
    devices: &[SpotifyConnectDevice],
) -> SpotifyToolError {
    SpotifyToolError::DeviceNotFound {
        message: format!(
            "Spotify can't target {} right now because it has no device id.",
            device.name
        ),
        devices: devices.iter().map(SpotifyDeviceSummary::from).collect(),
    }
}

fn ensure_volume_capable_device(
    resolved: ResolvedDevice,
    devices: &[SpotifyConnectDevice],
) -> Result<ResolvedDevice, SpotifyToolError> {
    if resolved.device.supports_volume {
        return Ok(resolved);
    }

    Err(SpotifyToolError::DeviceNotFound {
        message: format!(
            "Spotify volume control is not available on {}.",
            resolved.device.name
        ),
        devices: devices.iter().map(SpotifyDeviceSummary::from).collect(),
    })
}

fn parse_volume_percent(arguments: &Value) -> Result<u8, SpotifyToolError> {
    for key in ["volume_percent", "level"] {
        if let Some(value) = arguments.get(key) {
            if let Some(level) = value.as_u64() {
                return Ok(level.min(100) as u8);
            }
            if let Some(level) = value.as_i64() {
                return Ok(level.clamp(0, 100) as u8);
            }
            if let Some(level) = value.as_str() {
                let parsed =
                    level
                        .trim()
                        .parse::<u8>()
                        .map_err(|_| SpotifyToolError::BadRequest {
                            message: "Spotify set_volume requires a numeric volume_percent."
                                .to_owned(),
                        })?;
                return Ok(parsed.min(100));
            }
        }
    }

    Err(SpotifyToolError::BadRequest {
        message: "Spotify set_volume requires volume_percent.".to_owned(),
    })
}

fn required_string_argument(arguments: &Value, keys: &[&str]) -> Result<String, SpotifyToolError> {
    optional_string_argument(arguments, keys).ok_or_else(|| SpotifyToolError::BadRequest {
        message: format!("Spotify action requires {}.", keys.join(" or ")),
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

fn optional_bool_argument(arguments: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| arguments.get(*key))
        .and_then(|value| {
            value.as_bool().or_else(|| {
                value
                    .as_str()
                    .map(|raw| matches!(raw.trim(), "true" | "yes" | "1"))
            })
        })
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

fn parse_scope_string(scope: &str) -> Vec<String> {
    scope
        .split_whitespace()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn parse_token_response(response: Response) -> Result<TokenResponse, SpotifyToolError> {
    let status = response.status();
    let retry_after = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let body = response.text().map_err(|error| SpotifyToolError::Network {
        message: format!("Failed to read Spotify authentication response: {error}"),
    })?;

    if !status.is_success() {
        return Err(normalize_token_error(status, &body, retry_after));
    }

    serde_json::from_str(&body).map_err(|error| SpotifyToolError::AuthError {
        message: format!("Spotify authentication response was invalid: {error}"),
    })
}

fn read_optional_json_response(response: Response) -> Result<Option<Value>, SpotifyToolError> {
    let status = response.status();
    let retry_after = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let text = response.text().map_err(|error| SpotifyToolError::Network {
        message: format!("Failed to read Spotify response: {error}"),
    })?;

    if status == StatusCode::NO_CONTENT || text.trim().is_empty() {
        return if status.is_success() {
            Ok(None)
        } else {
            Err(normalize_api_error(status, &text, retry_after))
        };
    }

    if !status.is_success() {
        return Err(normalize_api_error(status, &text, retry_after));
    }

    serde_json::from_str(&text)
        .map(Some)
        .map_err(|error| SpotifyToolError::Unknown {
            message: format!("Spotify returned invalid JSON: {error}"),
        })
}

fn normalize_api_error(
    status: StatusCode,
    body: &str,
    retry_after_seconds: Option<u64>,
) -> SpotifyToolError {
    let body_value = serde_json::from_str::<Value>(body).ok();
    let message = extract_error_message(body_value.as_ref())
        .filter(|message| !message.trim().is_empty())
        .unwrap_or_else(|| format!("Spotify returned HTTP {}.", status.as_u16()));
    let normalized = message.to_ascii_lowercase();

    if status == StatusCode::TOO_MANY_REQUESTS {
        return SpotifyToolError::RateLimited {
            message: "Spotify rate limited the request. Try again shortly.".to_owned(),
            retry_after_seconds,
        };
    }

    if normalized.contains("no active device") || normalized.contains("no currently active device")
    {
        return SpotifyToolError::NoAvailableDevice {
            message: "No Spotify playback device is available right now.".to_owned(),
            devices: Vec::new(),
        };
    }

    if normalized.contains("premium required") || normalized.contains("only premium users") {
        return SpotifyToolError::Forbidden {
            reason: "premium_required",
            message: "Spotify Premium is required for playback control.".to_owned(),
            code: Some(status.as_u16()),
        };
    }

    if normalized.contains("insufficient client scope")
        || normalized.contains("missing required scope")
        || normalized.contains("insufficient scope")
    {
        return SpotifyToolError::Forbidden {
            reason: "invalid_scope",
            message:
                "Spotify access is missing required permissions. Reconnect Spotify to continue."
                    .to_owned(),
            code: Some(status.as_u16()),
        };
    }

    if normalized.contains("device not found") {
        return SpotifyToolError::DeviceNotFound {
            message: "The requested Spotify device was not found.".to_owned(),
            devices: Vec::new(),
        };
    }

    if status == StatusCode::UNAUTHORIZED {
        return SpotifyToolError::AuthExpired {
            message: "Spotify authentication expired. Please sign in again.".to_owned(),
        };
    }

    SpotifyToolError::Api {
        message,
        code: status.as_u16(),
    }
}

fn normalize_token_error(
    status: StatusCode,
    body: &str,
    retry_after_seconds: Option<u64>,
) -> SpotifyToolError {
    let body_value = serde_json::from_str::<Value>(body).ok();
    let error_code = body_value
        .as_ref()
        .and_then(|value| value.get("error"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let message = extract_error_message(body_value.as_ref())
        .filter(|message| !message.trim().is_empty())
        .unwrap_or_else(|| {
            format!(
                "Spotify authentication failed with HTTP {}.",
                status.as_u16()
            )
        });

    if status == StatusCode::TOO_MANY_REQUESTS {
        return SpotifyToolError::RateLimited {
            message: "Spotify rate limited the authentication request. Try again shortly."
                .to_owned(),
            retry_after_seconds,
        };
    }

    if error_code == "invalid_grant" {
        return SpotifyToolError::AuthExpired {
            message: "Spotify authorization expired. Please sign in again.".to_owned(),
        };
    }

    if error_code == "invalid_client" || error_code == "unauthorized_client" {
        return SpotifyToolError::Unavailable(SpotifyUnavailable::bad_config(
            "Spotify rejected the configured client credentials. Check client_id and client_secret.",
        ));
    }

    if error_code == "invalid_scope" {
        return SpotifyToolError::Forbidden {
            reason: "invalid_scope",
            message:
                "Spotify access is missing required permissions. Reconnect Spotify to continue."
                    .to_owned(),
            code: Some(status.as_u16()),
        };
    }

    if status == StatusCode::UNAUTHORIZED {
        return SpotifyToolError::Unavailable(SpotifyUnavailable::bad_config(
            "Spotify rejected the configured client credentials. Check client_id and client_secret.",
        ));
    }

    SpotifyToolError::AuthError { message }
}

fn extract_error_message(value: Option<&Value>) -> Option<String> {
    value.and_then(|value| {
        value
            .get("error_description")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| {
                value
                    .get("error")
                    .and_then(Value::as_object)
                    .and_then(|error| error.get("message"))
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
}

fn device_json(device: &SpotifyConnectDevice) -> Value {
    serde_json::to_value(SpotifyDeviceSummary::from(device)).unwrap_or_else(|_| {
        json!({
            "id": device.id,
            "name": device.name,
            "type": device.device_type,
            "is_active": device.is_active,
            "volume_percent": device.volume_percent
        })
    })
}

fn devices_json(devices: &[SpotifyConnectDevice]) -> Value {
    devices_json_from_summaries(
        &devices
            .iter()
            .map(SpotifyDeviceSummary::from)
            .collect::<Vec<_>>(),
    )
}

fn devices_json_from_summaries(devices: &[SpotifyDeviceSummary]) -> Value {
    serde_json::to_value(devices).unwrap_or_else(|_| Value::Array(Vec::new()))
}

fn object_fields(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

fn parse_playback_response(value: Value) -> Result<Option<SpotifyPlaybackState>, SpotifyToolError> {
    let payload: SpotifyPlaybackResponse =
        serde_json::from_value(value).map_err(|error| SpotifyToolError::Unknown {
            message: format!("Spotify playback response was invalid: {error}"),
        })?;
    Ok(Some(SpotifyPlaybackState {
        is_playing: payload.is_playing,
        device: payload.device,
        track: payload.item.map(SpotifyTrack::from),
    }))
}

fn parse_currently_playing_response(
    value: Value,
) -> Result<Option<SpotifyPlaybackState>, SpotifyToolError> {
    let payload: SpotifyCurrentlyPlayingResponse =
        serde_json::from_value(value).map_err(|error| SpotifyToolError::Unknown {
            message: format!("Spotify currently-playing response was invalid: {error}"),
        })?;
    Ok(Some(SpotifyPlaybackState {
        is_playing: payload.is_playing,
        device: payload.device,
        track: payload.item.map(SpotifyTrack::from),
    }))
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
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SpotifyDevicesResponse {
    #[serde(default)]
    devices: Vec<SpotifyConnectDevice>,
}

#[derive(Debug, Deserialize)]
struct SpotifyCurrentUserResponse {
    id: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SpotifyPlaybackResponse {
    is_playing: bool,
    #[serde(default)]
    device: Option<SpotifyConnectDevice>,
    #[serde(default)]
    item: Option<SpotifyTrackItem>,
}

#[derive(Debug, Deserialize)]
struct SpotifyCurrentlyPlayingResponse {
    is_playing: bool,
    #[serde(default)]
    device: Option<SpotifyConnectDevice>,
    #[serde(default)]
    item: Option<SpotifyTrackItem>,
}

#[derive(Debug, Deserialize)]
struct SpotifySearchResponse {
    #[serde(default)]
    tracks: Option<SpotifyPage<SpotifyTrackItem>>,
    #[serde(default)]
    albums: Option<SpotifyPage<SpotifyAlbumItem>>,
    #[serde(default)]
    artists: Option<SpotifyPage<SpotifyArtistItem>>,
    #[serde(default)]
    playlists: Option<SpotifyPage<SpotifyPlaylistItem>>,
}

#[derive(Debug, Deserialize)]
struct SpotifyPage<T> {
    #[serde(default)]
    items: Vec<T>,
}

#[derive(Debug, Deserialize, Default)]
struct SpotifyTrackItem {
    name: String,
    uri: String,
    album: SpotifyAlbum,
    artists: Vec<SpotifyArtist>,
}

#[derive(Debug, Deserialize, Default)]
struct SpotifyAlbumItem {
    name: String,
    uri: String,
    #[serde(default)]
    artists: Vec<SpotifyArtist>,
}

#[derive(Debug, Deserialize, Default)]
struct SpotifyArtistItem {
    name: String,
    uri: String,
}

#[derive(Debug, Deserialize, Default)]
struct SpotifyPlaylistItem {
    name: String,
    uri: String,
    owner: SpotifyOwner,
}

#[derive(Debug, Deserialize, Default)]
struct SpotifyAlbum {
    name: String,
}

#[derive(Debug, Deserialize, Default)]
struct SpotifyArtist {
    name: String,
}

#[derive(Debug, Deserialize, Default)]
struct SpotifyOwner {
    #[serde(default)]
    display_name: Option<String>,
    id: String,
}

impl From<SpotifyTrackItem> for SpotifyTrack {
    fn from(value: SpotifyTrackItem) -> Self {
        Self {
            name: value.name,
            artists: value
                .artists
                .into_iter()
                .map(|artist| artist.name)
                .collect(),
            album: value.album.name,
            uri: value.uri,
        }
    }
}

impl From<SpotifyAlbumItem> for SpotifyAlbumMatch {
    fn from(value: SpotifyAlbumItem) -> Self {
        Self {
            name: value.name,
            artists: value
                .artists
                .into_iter()
                .map(|artist| artist.name)
                .collect(),
            uri: value.uri,
        }
    }
}

impl From<SpotifyArtistItem> for SpotifyArtistMatch {
    fn from(value: SpotifyArtistItem) -> Self {
        Self {
            name: value.name,
            uri: value.uri,
        }
    }
}

impl From<SpotifyPlaylistItem> for SpotifyPlaylistMatch {
    fn from(value: SpotifyPlaylistItem) -> Self {
        Self {
            name: value.name,
            owner: value.owner.display_name.unwrap_or(value.owner.id),
            uri: value.uri,
        }
    }
}
