//! Playback URL fetching + state (P3).
//!
//! Handover notes:
//! - EVERY playback-URL fetch calls [`check_playback_gate`] first: only
//!   `Premium` proceeds. Blocked fetches return [`PlaybackError`] WITHOUT
//!   touching the network (fail-closed), and callers surface the
//!   [`PlaybackState::UpgradeRequired`] state + [`crate::legal`] message.
//! - Media URLs are SHORT-LIVED and IN-MEMORY only: [`StreamTicket`] holds
//!   one URL string, cleared on stop / error / track change. URLs are never
//!   logged, persisted, cached to disk, or exported — `Display` impls below
//!   never render them, and `reqwest` errors (which embed URLs) are mapped
//!   to static strings before crossing into logs or UI.
//! - googlevideo URLs expire: a ticket is fetched per play (prefetched
//!   tickets live 90 s in memory), so HTTP 403 ([`classify_stream_status`],
//!   [`PlaybackError::ExpiredUrl`]) surfaces as a fail-closed error.
//! - Legal: NO download / persist / export of media bytes, NO offline
//!   caching, NO ad-strip / ad-skip / ad-block logic — anywhere, every
//!   milestone (see `crate::legal`).
//!
//! Wiring: the `Backend` singleton (`src/main.rs`) runs [`LiveUrlSource`]
//! on worker threads and hands tickets to `backend::RodioBackend`.

pub mod backend;
pub mod player;
pub mod queue;

use crate::premium_gate::{GateStatus, PremiumGate};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Why playback did not start / continue. Carries NO URLs, secrets, or
/// media bytes — safe to log and to surface in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackError {
    /// Logged out — sign in first.
    LoggedOut,
    /// Authenticated but not Premium (or ambiguous probe) — upgrade required.
    UpgradeRequired,
    /// Gate passed but the InnerTube / stream call failed (transport only).
    Transport,
    /// Short-lived stream URL rejected (HTTP 403) — refresh once, then stop.
    ExpiredUrl,
    /// Nothing to play (empty queue / unplayable row).
    NoTrack,
    /// No audio output available on this machine.
    BackendUnavailable,
}

impl std::fmt::Display for PlaybackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LoggedOut => write!(f, "not logged in"),
            Self::UpgradeRequired => write!(f, "YouTube Music Premium is required"),
            Self::Transport => write!(f, "playback request failed"),
            Self::ExpiredUrl => write!(f, "stream expired"),
            Self::NoTrack => write!(f, "nothing to play"),
            Self::BackendUnavailable => write!(f, "no audio output available"),
        }
    }
}

impl std::error::Error for PlaybackError {}

/// QML-facing playback state. Serialized via [`PlaybackState::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlaybackState {
    #[default]
    Stopped,
    Loading,
    Playing,
    Paused,
    UpgradeRequired,
    Error,
}

impl PlaybackState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Loading => "loading",
            Self::Playing => "playing",
            Self::Paused => "paused",
            Self::UpgradeRequired => "upgrade",
            Self::Error => "error",
        }
    }

    /// Unknown input falls back to `Stopped` (fail-safe: no phantom state).
    pub fn from_str(s: &str) -> Self {
        match s {
            "stopped" => Self::Stopped,
            "loading" => Self::Loading,
            "playing" => Self::Playing,
            "paused" => Self::Paused,
            "upgrade" => Self::UpgradeRequired,
            "error" => Self::Error,
            _ => Self::Stopped,
        }
    }
}

/// Short-lived stream ticket: ONE googlevideo URL held IN MEMORY ONLY.
/// Cleared on stop / error / track change; never serialized, logged, or
/// written anywhere (see module docs).
#[derive(Debug, Clone)]
pub struct StreamTicket {
    video_id: String,
    url: String,
    /// The UA the URL was minted for. The media edge binds these together.
    user_agent: String,
    /// Track length from the player response (`videoDetails.lengthSeconds` or
    /// the first format's `approxDurationMs`), when the response carries it.
    duration_secs: Option<u32>,
    /// First ranged chunk fetched on the worker, so rodio's decoder probe
    /// never blocks the UI thread. Memory-only; dies with the ticket.
    prefetched: Vec<u8>,
}

impl StreamTicket {
    /// Fail-closed constructor: only `https://` URLs are accepted.
    pub fn new(video_id: &str, url: &str) -> Option<Self> {
        Self::with_user_agent(video_id, url, ytmusicapi::USER_AGENT)
    }

    /// Same fail-closed checks, with the UA the player client used.
    pub fn with_user_agent(video_id: &str, url: &str, user_agent: &str) -> Option<Self> {
        if video_id.is_empty() || !url.starts_with("https://") || user_agent.trim().is_empty() {
            return None;
        }
        Some(Self {
            video_id: video_id.to_string(),
            url: url.to_string(),
            user_agent: user_agent.to_string(),
            duration_secs: None,
            prefetched: Vec::new(),
        })
    }

    /// Attach the player-reported length (no-op when it is zero/absent).
    pub fn with_duration(mut self, duration_secs: Option<u32>) -> Self {
        self.duration_secs = duration_secs.filter(|secs| *secs > 0);
        self
    }

    pub fn video_id(&self) -> &str {
        &self.video_id
    }

    /// The in-memory URL for the audio backend. Handle with care: never log.
    pub fn stream_url(&self) -> &str {
        &self.url
    }

    /// User agent for ranged media reads (not a secret).
    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    /// Player-reported track length, when available.
    pub fn duration_secs(&self) -> Option<u32> {
        self.duration_secs
    }

    fn set_prefetch(&mut self, bytes: Vec<u8>) {
        self.prefetched = bytes;
    }

    /// First chunk already fetched by the worker (empty when unavailable).
    pub fn prefetched(&self) -> &[u8] {
        &self.prefetched
    }
}

/// First ranged chunk size prefetched on the playback worker.
const PREFETCH_BYTES: u64 = 256 * 1024;

/// Fetch the first chunk into the ticket so `Decoder::new` reads memory, not
/// the network, on the UI thread. Failure is silent: playback falls back to
/// ranged reads exactly as before.
async fn prefetch_ticket(ticket: &mut StreamTicket) {
    let Ok(client) = reqwest::Client::builder()
        .user_agent(ticket.user_agent())
        .timeout(Duration::from_secs(10))
        .build()
    else {
        return;
    };
    let Ok(response) = client
        .get(ticket.stream_url())
        .header("Range", format!("bytes=0-{}", PREFETCH_BYTES - 1))
        .send()
        .await
    else {
        return;
    };
    if !matches!(response.status().as_u16(), 200 | 206) {
        return;
    }
    if let Ok(bytes) = response.bytes().await {
        if !bytes.is_empty() {
            ticket.set_prefetch(bytes.to_vec());
        }
    }
}

/// Short-lived ticket cache: makes Next/Previous instant when the queue's
/// neighbours were prefetched. URLs live in memory only and expire quickly;
/// a stale entry just fails closed at the media edge and is refetched.
const TICKET_TTL: Duration = Duration::from_secs(90);
const TICKET_CACHE_MAX: usize = 8;

static TICKET_CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<String, (Instant, StreamTicket)>>> =
    std::sync::OnceLock::new();

fn ticket_cache() -> &'static std::sync::Mutex<HashMap<String, (Instant, StreamTicket)>> {
    TICKET_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// A still-fresh cached ticket for `video_id`, if any.
pub fn cached_ticket(video_id: &str) -> Option<StreamTicket> {
    let mut cache = ticket_cache().lock().ok()?;
    match cache.get(video_id) {
        Some((at, ticket)) if at.elapsed() < TICKET_TTL => Some(ticket.clone()),
        Some(_) => {
            cache.remove(video_id);
            None
        }
        None => None,
    }
}

/// Remember one ticket for a short window (memory only; never logged).
pub fn remember_ticket(ticket: &StreamTicket) {
    let Ok(mut cache) = ticket_cache().lock() else {
        return;
    };
    cache.retain(|_, (at, _)| at.elapsed() < TICKET_TTL);
    if cache.len() >= TICKET_CACHE_MAX {
        if let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, (at, _))| *at)
            .map(|(id, _)| id.clone())
        {
            cache.remove(&oldest);
        }
    }
    cache.insert(ticket.video_id().to_string(), (Instant::now(), ticket.clone()));
}

/// Track length from a `player` response. `videoDetails.lengthSeconds` is the
/// authoritative value; the first format's `approxDurationMs` is a fallback.
/// Pure; no URLs or media bytes are touched.
pub fn player_duration_secs(player: &serde_json::Value) -> Option<u32> {
    let detail = player
        .pointer("/videoDetails/lengthSeconds")
        .and_then(|v| v.as_str())
        .and_then(|s| s.trim().parse::<u64>().ok());
    let approx = player
        .pointer("/streamingData/adaptiveFormats/0/approxDurationMs")
        .or_else(|| player.pointer("/streamingData/formats/0/approxDurationMs"))
        .and_then(|v| v.as_str())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .map(|ms| ms / 1000);
    detail
        .or(approx)
        .map(|secs| secs.min(u64::from(u32::MAX)) as u32)
        .filter(|secs| *secs > 0)
}

/// THE playback gate: call before ANY playback-URL fetch. Maps the cached
/// [`GateStatus`] to `Ok` (Premium only) or a [`PlaybackError`].
pub fn check_playback_gate(gate: &PremiumGate) -> Result<(), PlaybackError> {
    if gate.can_fetch_playback_url() {
        return Ok(());
    }
    match gate.status() {
        GateStatus::Premium => Ok(()),
        GateStatus::LoggedOut => Err(PlaybackError::LoggedOut),
        GateStatus::UpgradeRequired => Err(PlaybackError::UpgradeRequired),
    }
}

/// Upgrade prompt for blocked playback (`None` for non-gate errors).
pub fn upgrade_message_for(error: PlaybackError) -> Option<String> {
    match error {
        PlaybackError::UpgradeRequired => Some(crate::legal::upgrade_message()),
        PlaybackError::LoggedOut => {
            Some("Sign in with YouTube Music Premium to enable playback.".to_string())
        }
        PlaybackError::Transport
        | PlaybackError::ExpiredUrl
        | PlaybackError::NoTrack
        | PlaybackError::BackendUnavailable => None,
    }
}

/// Classify an HTTP status from the stream edge: 200/206 flow, 403 means
/// the short-lived URL expired (refresh once), anything else is transport.
pub fn classify_stream_status(status: u16) -> Result<(), PlaybackError> {
    match status {
        200 | 206 => Ok(()),
        403 => Err(PlaybackError::ExpiredUrl),
        _ => Err(PlaybackError::Transport),
    }
}

/// Pick the best audio-only URL from a `player` (`get_song`) response,
/// reading `streamingData` IN MEMORY ONLY. The response value (and every
/// URL not picked) is dropped with the call — nothing is logged, persisted,
/// or cached. Prefers `audio/mp4` (rodio-decodable), then highest bitrate.
pub fn pick_audio_url(player_json: &serde_json::Value) -> Option<String> {
    let streaming = player_json.get("streamingData")?;
    let mut candidates: Vec<(&str, &str, u64)> = Vec::new();
    for key in ["formats", "adaptiveFormats"] {
        if let Some(list) = streaming.get(key).and_then(serde_json::Value::as_array) {
            for f in list {
                let mime = f
                    .get("mimeType")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                if !mime.contains("audio") {
                    continue;
                }
                let url = f
                    .get("url")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default();
                if url.is_empty() {
                    continue;
                }
                let bitrate = f
                    .get("bitrate")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                candidates.push((mime, url, bitrate));
            }
        }
    }
    candidates.sort_by(|a, b| {
        let mp4_a = u8::from(a.0.contains("mp4"));
        let mp4_b = u8::from(b.0.contains("mp4"));
        mp4_b.cmp(&mp4_a).then(b.2.cmp(&a.2))
    });
    candidates.into_iter().next().map(|(_, url, _)| url.to_string())
}

/// Source of playback URLs + radio refills. The live impl calls the gate
/// first (see [`LiveUrlSource`]).
pub trait PlaybackUrlSource {
    /// Fetch a short-lived stream ticket. Gate-checked, in-memory only.
    async fn fetch_url(&self, video_id: &str) -> Result<StreamTicket, PlaybackError>;
    /// Radio/autoplay refill rows for `video_id` (metadata only).
    async fn refill_tracks(
        &self,
        video_id: &str,
        limit: usize,
    ) -> Result<Vec<queue::QueueItem>, PlaybackError>;
}

/// Live InnerTube source: gate first, then `player` / `next` endpoints.
/// Untested live (no network in tests); parsing is covered through
/// [`pick_audio_url`].
pub struct LiveUrlSource<'a> {
    yt: &'a ytmusicapi::YTMusic,
    gate: &'a PremiumGate,
}

impl<'a> LiveUrlSource<'a> {
    pub fn new(yt: &'a ytmusicapi::YTMusic, gate: &'a PremiumGate) -> Self {
        Self { yt, gate }
    }
}

/// Real player `signatureTimestamp`, resolved once per process. The value
/// changes when YouTube ships a new player script; a long-lived app can keep
/// it for the session (a stale value fails closed, never opens playback).
static SIGNATURE_TIMESTAMP: std::sync::OnceLock<i64> = std::sync::OnceLock::new();

/// Resolve the player signature timestamp, caching it after the first
/// success. A failure falls back to `get_song`'s built-in default, which is
/// still gate-checked and fail-closed at the media edge.
async fn signature_timestamp(yt: &ytmusicapi::YTMusic) -> Option<i64> {
    if let Some(ts) = SIGNATURE_TIMESTAMP.get() {
        return Some(*ts);
    }
    match yt.get_signature_timestamp(None).await {
        Ok(ts) => {
            let _ = SIGNATURE_TIMESTAMP.set(ts);
            if std::env::var("MUSIC_PLAYER_DEBUG_AUDIO").as_deref() == Ok("1") {
                eprintln!("playback: signature timestamp resolved");
            }
            Some(ts)
        }
        Err(_) => {
            if std::env::var("MUSIC_PLAYER_DEBUG_AUDIO").as_deref() == Ok("1") {
                eprintln!("playback: signature timestamp unavailable; using default");
            }
            None
        }
    }
}

impl PlaybackUrlSource for LiveUrlSource<'_> {
    async fn fetch_url(&self, video_id: &str) -> Result<StreamTicket, PlaybackError> {
        check_playback_gate(self.gate)?;
        if video_id.is_empty() {
            return Err(PlaybackError::NoTrack);
        }
        if let Some(ticket) = cached_ticket(video_id) {
            return Ok(ticket);
        }
        // Primary: the guest VISIONOS player returns already-addressable
        // audio URLs (no `n` ciphering, no cookies). The media edge is sent
        // the same UA the URL was minted for.
        if let Some(response) = crate::auth::http::visionos_guest_player(video_id).await {
            if let Some(url) = pick_audio_url(&response) {
                let duration = player_duration_secs(&response);
                if let Some(ticket) = StreamTicket::with_user_agent(
                    video_id,
                    &url,
                    crate::auth::http::VISIONOS_UA,
                ) {
                    let mut ticket = ticket.with_duration(duration);
                    prefetch_ticket(&mut ticket).await;
                    remember_ticket(&ticket);
                    return Ok(ticket);
                }
            }
        }
        // Fallback: signed WEB_REMIX player. Some tracks are not offered to
        // the guest client; this path stays gate-checked and fail-closed.
        let signature = signature_timestamp(self.yt).await;
        let player = self
            .yt
            .get_song(video_id, signature)
            .await
            .map_err(|_| PlaybackError::Transport)?;
        let url = pick_audio_url(&player).ok_or(PlaybackError::Transport)?;
        let duration = player_duration_secs(&player);
        if let Some(mut ticket) = StreamTicket::new(video_id, &url)
            .map(|ticket| ticket.with_duration(duration))
        {
            prefetch_ticket(&mut ticket).await;
            remember_ticket(&ticket);
            Ok(ticket)
        } else {
            Err(PlaybackError::Transport)
        }
    }

    async fn refill_tracks(
        &self,
        video_id: &str,
        limit: usize,
    ) -> Result<Vec<queue::QueueItem>, PlaybackError> {
        check_playback_gate(self.gate)?;
        if video_id.is_empty() {
            return Ok(Vec::new());
        }
        // Radio playlist of the current track (changes each call).
        let watch = self
            .yt
            .get_watch_playlist(Some(video_id), None, limit, true, false)
            .await
            .map_err(|_| PlaybackError::Transport)?;
        Ok(watch
            .get("tracks")
            .and_then(serde_json::Value::as_array)
            .map(|xs| xs.iter().filter_map(queue::QueueItem::from_json).collect())
            .unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::premium_gate::{Playability, PremiumSignals};
    use std::time::SystemTime;

    fn premium_gate() -> PremiumGate {
        let mut gate = PremiumGate::new_logged_out();
        gate.apply(
            &PremiumSignals {
                logged_in: true,
                home_ok: true,
                premium_badge: Some(true),
                explore_top_songs: None,
                ads_absent: None,
                topbar_premium: None,
                hires_audio: None,
                playability: Playability::Playable,
            },
            SystemTime::now(),
        );
        gate
    }

    #[test]
    fn gate_blocks_every_playback_fetch() {
        assert_eq!(
            check_playback_gate(&PremiumGate::new_logged_out()),
            Err(PlaybackError::LoggedOut)
        );
        let mut gate = PremiumGate::new_logged_out();
        gate.apply(
            &PremiumSignals {
                logged_in: true,
                home_ok: true,
                premium_badge: Some(false),
                explore_top_songs: None,
                ads_absent: None,
                topbar_premium: None,
                hires_audio: None,
                playability: Playability::Playable,
            },
            SystemTime::now(),
        );
        assert_eq!(
            check_playback_gate(&gate),
            Err(PlaybackError::UpgradeRequired)
        );
        assert_eq!(check_playback_gate(&premium_gate()), Ok(()));
    }

    #[test]
    fn upgrade_message_attaches_to_gate_errors_only() {
        let msg = upgrade_message_for(PlaybackError::UpgradeRequired).expect("upsell");
        assert!(msg.contains("Premium"));
        assert!(msg.contains(crate::legal::YOUTUBE_TERMS_URL));
        assert!(upgrade_message_for(PlaybackError::LoggedOut).is_some());
        assert_eq!(upgrade_message_for(PlaybackError::Transport), None);
        assert_eq!(upgrade_message_for(PlaybackError::ExpiredUrl), None);
    }

    #[test]
    fn stream_status_maps_403_to_refresh() {
        assert_eq!(classify_stream_status(200), Ok(()));
        assert_eq!(classify_stream_status(206), Ok(()));
        assert_eq!(classify_stream_status(403), Err(PlaybackError::ExpiredUrl));
        assert_eq!(classify_stream_status(500), Err(PlaybackError::Transport));
    }

    #[test]
    fn ticket_rejects_non_https() {
        assert!(StreamTicket::new("vid", "https://mock.invalid/a").is_some());
        assert!(StreamTicket::new("vid", "http://mock.invalid/a").is_none());
        assert!(StreamTicket::new("", "https://mock.invalid/a").is_none());
    }

    #[test]
    fn errors_carry_no_urls_or_secrets() {
        for e in [
            PlaybackError::LoggedOut,
            PlaybackError::UpgradeRequired,
            PlaybackError::Transport,
            PlaybackError::ExpiredUrl,
            PlaybackError::NoTrack,
            PlaybackError::BackendUnavailable,
        ] {
            let msg = e.to_string();
            assert!(!msg.contains("http"), "URL leaked in {e:?}");
            assert!(!msg.contains("cookie") && !msg.contains("SAPISID"));
        }
    }

    #[test]
    fn pick_audio_url_prefers_mp4_then_bitrate() {
        let player = serde_json::json!({
            "playabilityStatus": {"status": "OK"},
            "streamingData": {
                "adaptiveFormats": [
                    {"mimeType": "audio/webm; codecs=\"opus\"", "bitrate": 160000, "url": "https://mock.invalid/opus-high"},
                    {"mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 128000, "url": "https://mock.invalid/m4a"},
                    {"mimeType": "video/mp4", "bitrate": 999999, "url": "https://mock.invalid/video"},
                    {"mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "bitrate": 48000, "url": "https://mock.invalid/m4a-low"},
                ]
            }
        });
        // mp4 preferred over higher-bitrate opus (rodio-decodable first).
        assert_eq!(
            pick_audio_url(&player).as_deref(),
            Some("https://mock.invalid/m4a")
        );
        assert!(pick_audio_url(&serde_json::json!({})).is_none());
    }

    #[test]
    fn playback_state_strings_roundtrip() {
        assert_eq!(PlaybackState::Playing.as_str(), "playing");
        assert_eq!(PlaybackState::UpgradeRequired.as_str(), "upgrade");
        assert_eq!(PlaybackState::from_str("upgrade"), PlaybackState::UpgradeRequired);
        assert_eq!(PlaybackState::from_str("bogus"), PlaybackState::Stopped);
    }

    #[test]
    fn player_duration_prefers_video_details() {
        let both = serde_json::json!({
            "videoDetails": {"lengthSeconds": "215"},
            "streamingData": {"adaptiveFormats": [{"approxDurationMs": "215000"}]}
        });
        assert_eq!(player_duration_secs(&both), Some(215));
        let approx = serde_json::json!({
            "streamingData": {"adaptiveFormats": [{"approxDurationMs": "90000"}]}
        });
        assert_eq!(player_duration_secs(&approx), Some(90));
        assert_eq!(player_duration_secs(&serde_json::json!({})), None);
        assert_eq!(
            player_duration_secs(&serde_json::json!({"videoDetails": {"lengthSeconds": "0"}})),
            None
        );
    }

    #[test]
    fn stream_ticket_carries_optional_duration() {
        let ticket = StreamTicket::new("vid", "https://mock.invalid/a").expect("ticket");
        assert_eq!(ticket.duration_secs(), None);
        assert_eq!(ticket.clone().with_duration(Some(215)).duration_secs(), Some(215));
        assert_eq!(ticket.with_duration(None).duration_secs(), None);
    }

    #[test]
    fn ticket_cache_roundtrips_and_keeps_duration() {
        let ticket = StreamTicket::new("cache-test-vid", "https://mock.invalid/audio")
            .expect("ticket")
            .with_duration(Some(42));
        remember_ticket(&ticket);
        let cached = cached_ticket("cache-test-vid").expect("cached");
        assert_eq!(cached.duration_secs(), Some(42));
        assert_eq!(cached.user_agent(), ticket.user_agent());
    }
}
