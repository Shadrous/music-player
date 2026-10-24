//! QML-bound playback snapshot + radio refill page size. The engine itself
//! lives on the `Backend` singleton (`src/main.rs`): request/poll workers,
//! rodio via [`AudioBackend`](super::backend::AudioBackend), and the
//! [`Queue`](super::queue::Queue).

use super::PlaybackState;

/// QML-bound playback snapshot: plain data, no URLs, no secrets.
#[derive(Debug, Clone, Default)]
pub struct PlaybackSnapshot {
    pub state: PlaybackState,
    pub is_playing: bool,
    pub title: String,
    pub artist: String,
    pub position_secs: u32,
    pub duration_secs: u32,
    pub next_title: String,
    pub next_artist: String,
    pub queue_len: usize,
    pub upgrade_message: Option<String>,
}

/// Radio refill page size for `get_watch_playlist`.
pub const REFILL_LIMIT: usize = 25;
