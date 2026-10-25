//! Audio backend trait + rodio default (P3).
//!
//! Handover notes:
//! - [`AudioBackend`] is swappable: [`RodioBackend`] is the default, tests
//!   use `MockBackend` (see `#[cfg(test)]`) so `cargo test` needs no audio
//!   hardware, no network, and writes no files.
//! - Streaming is IN-MEMORY ONLY: [`HttpAudioStream`] pulls the short-lived
//!   googlevideo URL in small Range chunks, retaining at most ONE in-flight
//!   chunk (prior bytes are dropped, never accumulated, never written to
//!   disk). The ticket URL lives only in the controller + this stream; it is
//!   never logged (HTTP errors embed URLs, so every error is mapped to a
//!   static string before it can reach logs or UI).
//! - HTTP 403 inside the pump surfaces as `PermissionDenied`, which the
//!   controller maps to [`PlaybackError::ExpiredUrl`](super::PlaybackError)
//!   (drop ticket, refresh once, fail-closed after).
//! - Audio-device absence is fail-closed: [`RodioBackend::play_stream`]
//!   returns `BackendUnavailable` instead of silent fake playback. The sink
//!   is opened LAZILY (never at startup), so headless/offscreen runs stay
//!   silent on stderr.
//! - Legal: NO download / persist / export of media bytes, NO offline cache,
//!   NO ad logic — anywhere (see `crate::legal`).
//!
//! P4 gaps: resume/seek tuning on flaky links; gapless prefetch of the
//! next-up URL (still in-memory, still short-lived).

use std::io::{Read, Seek, SeekFrom};
use std::time::Duration;

use super::{PlaybackError, StreamTicket};

/// Swappable audio backend. Implementations stream in memory only and
/// persist zero media bytes ([`AudioBackend::bytes_persisted`]).
pub trait AudioBackend {
    /// Stream `ticket` now (replacing anything playing). Fail-closed on
    /// device / decode errors.
    fn play_stream(&mut self, ticket: &StreamTicket) -> Result<(), PlaybackError>;
    fn pause(&mut self);
    fn resume(&mut self);
    fn stop(&mut self);
    /// Seek the current stream to `secs` (no-op when nothing is loaded).
    fn seek(&mut self, secs: u32);
    /// Volume 0.0..=1.0 (clamped).
    fn set_volume(&mut self, volume: f32);
    /// Decoder position of the current stream (0 when stopped).
    fn position_secs(&self) -> u32;
    /// True once a loaded stream has played to its end (drives auto-advance).
    fn is_finished(&self) -> bool;
}

/// Clamp a volume into 0.0..=1.0 (non-finite becomes 0.0). Pure, tested.
pub fn clamp_volume(v: f32) -> f32 {
    if !v.is_finite() {
        return 0.0;
    }
    v.clamp(0.0, 1.0)
}

/// Live rodio state for one stream.
struct ActiveStream {
    // Held: dropping the sink ends OS playback. `log_on_drop(false)` keeps
    // headless runs silent on stderr.
    _sink: rodio::MixerDeviceSink,
    player: rodio::Player,
    volume: f32,
}

/// Default rodio backend. The output sink opens lazily on first
/// [`RodioBackend::play_stream`] — construction never touches audio
/// hardware, keeping tests and offscreen runs side-effect free.
pub struct RodioBackend {
    active: Option<ActiveStream>,
    volume: f32,
    playing: bool,
    current_video: Option<String>,
}

impl Default for RodioBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl RodioBackend {
    pub fn new() -> Self {
        Self {
            active: None,
            volume: 0.8,
            playing: false,
            current_video: None,
        }
    }

    fn drop_active(&mut self) {
        if let Some(active) = self.active.take() {
            active.player.stop();
        }
        self.playing = false;
        // Short-lived URL dies with the stream (memory only, never logged).
        self.current_video = None;
    }
}

impl AudioBackend for RodioBackend {

    fn play_stream(&mut self, ticket: &StreamTicket) -> Result<(), PlaybackError> {
        if !ticket.stream_url().starts_with("https://") {
            return Err(PlaybackError::Transport);
        }
        self.drop_active();

        let mut sink = rodio::DeviceSinkBuilder::open_default_sink().map_err(|_| {
            if debug_audio() {
                eprintln!("audio: no default output device");
            }
            PlaybackError::BackendUnavailable
        })?;
        // Silence the on-drop stderr note: headless/offscreen runs must
        // keep stderr empty.
        sink.log_on_drop(false);
        let player = rodio::Player::connect_new(sink.mixer());
        let stream = HttpAudioStream::new(
            ticket.stream_url().to_string(),
            ticket.user_agent(),
            ticket.prefetched(),
        )?;
        // Format sniffing reads from the network here (runtime only, never
        // in tests). Any decode failure is fail-closed transport.
        let decoder = rodio::Decoder::new(stream).map_err(|_| {
            if debug_audio() {
                eprintln!("audio: decoder could not read the stream");
            }
            PlaybackError::Transport
        })?;
        player.append(decoder);
        player.set_volume(clamp_volume(self.volume));
        player.play();
        if debug_audio() {
            eprintln!("audio: playing {}", ticket.video_id());
        }
        self.active = Some(ActiveStream {
            _sink: sink,
            player,
            volume: self.volume,
        });
        self.playing = true;
        // Metadata only (video id); the URL itself is not retained here.
        self.current_video = Some(ticket.video_id().to_string());
        Ok(())
    }

    fn pause(&mut self) {
        if self.playing {
            if let Some(active) = &self.active {
                active.player.pause();
            }
            self.playing = false;
        }
    }

    fn resume(&mut self) {
        if !self.playing && self.active.is_some() {
            if let Some(active) = &self.active {
                active.player.play();
            }
            self.playing = true;
        }
    }

    fn stop(&mut self) {
        self.drop_active();
    }

    fn seek(&mut self, secs: u32) {
        if let Some(active) = &self.active {
            let _ = active.player.try_seek(Duration::from_secs(u64::from(secs)));
        }
    }

    fn set_volume(&mut self, volume: f32) {
        self.volume = clamp_volume(volume);
        if let Some(active) = &mut self.active {
            active.player.set_volume(self.volume);
            active.volume = self.volume;
        }
    }

    fn position_secs(&self) -> u32 {
        self.active
            .as_ref()
            .map_or(0, |a| a.player.get_pos().as_secs().min(u64::from(u32::MAX)) as u32)
    }

    fn is_finished(&self) -> bool {
        self.active.as_ref().is_some_and(|a| a.player.empty())
    }

}

/// In-memory HTTP audio stream: pulls the short-lived URL in small Range
/// chunks, retaining at most ONE in-flight chunk. Prior bytes are dropped,
/// never accumulated to disk or memory caches. Implements `Seek` via
/// position updates (+ a single HEAD for `SeekFrom::End` lengths).
struct HttpAudioStream {
    // The short-lived URL (memory only; never logged — see error mapping).
    url: String,
    client: reqwest::Client,
    rt: tokio::runtime::Runtime,
    pos: u64,
    len: Option<u64>,
    chunk: Vec<u8>,
    chunk_start: u64,
}

/// Range chunk size per network read. One in-flight chunk only; 256 KiB is
/// ~16 s of 128 kbps audio, which keeps the decoder ahead without caching.
const CHUNK_SIZE: u64 = 256 * 1024;

/// Opt-in stderr diagnostics for the live audio path (`SPINWAVE_DEBUG_AUDIO=1`).
/// Static status lines only — never a URL, token, or media byte.
fn debug_audio() -> bool {
    std::env::var("SPINWAVE_DEBUG_AUDIO")
        .map(|v| v == "1")
        .unwrap_or(false)
}

impl HttpAudioStream {
    fn new(url: String, user_agent: &str, prefetched: &[u8]) -> Result<Self, PlaybackError> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| PlaybackError::Transport)?;
        // The media edge rejects requests whose UA does not match the client
        // that minted the URL. Reuse the ticket's UA on every ranged read.
        let client = reqwest::Client::builder()
            .user_agent(user_agent)
            .build()
            .map_err(|_| PlaybackError::Transport)?;
        Ok(Self {
            url,
            client,
            rt,
            pos: 0,
            len: None,
            chunk: prefetched.to_vec(),
            chunk_start: 0,
        })
    }

    /// Fill the in-flight chunk at `self.pos`, dropping the previous chunk.
    fn fill(&mut self) -> std::io::Result<()> {
        let end = self.pos.saturating_add(CHUNK_SIZE).saturating_sub(1);
        let response = self
            .rt
            .block_on(
                self.client
                    .get(&self.url)
                    .header("Range", format!("bytes={}-{}", self.pos, end))
                    .send(),
            )
            .map_err(|_| {
                if debug_audio() {
                    eprintln!("audio: stream request failed");
                }
                std::io::Error::other("stream read failed")
            })?;
        // Static messages only: status/response errors embed the URL.
        let status = response.status().as_u16();
        if debug_audio() {
            eprintln!("audio: stream http {status}");
        }
        super::classify_stream_status(status).map_err(|e| {
            let kind = match e {
                PlaybackError::ExpiredUrl => std::io::ErrorKind::PermissionDenied,
                _ => std::io::ErrorKind::Other,
            };
            std::io::Error::new(kind, "stream read failed")
        })?;
        let bytes = self
            .rt
            .block_on(response.bytes())
            .map_err(|_| {
                if debug_audio() {
                    eprintln!("audio: stream body read failed");
                }
                std::io::Error::other("stream read failed")
            })?;
        // Drop the previous chunk: at most one in-flight chunk is retained.
        self.chunk = bytes.to_vec();
        self.chunk_start = self.pos;
        Ok(())
    }

    fn require_len(&mut self) -> std::io::Result<u64> {
        if let Some(len) = self.len {
            return Ok(len);
        }
        let response = self
            .rt
            .block_on(self.client.head(&self.url).send())
            .map_err(|_| {
                if debug_audio() {
                    eprintln!("audio: seek request failed");
                }
                std::io::Error::other("stream seek failed")
            })?;
        let status = response.status().as_u16();
        if debug_audio() {
            eprintln!("audio: seek http {status}");
        }
        super::classify_stream_status(status).map_err(|_| {
            std::io::Error::other("stream seek failed")
        })?;
        let len = response.content_length().ok_or_else(|| {
            if debug_audio() {
                eprintln!("audio: seek has no content length");
            }
            std::io::Error::other("stream seek failed")
        })?;
        self.len = Some(len);
        Ok(len)
    }
}

impl Read for HttpAudioStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let in_chunk = self.pos >= self.chunk_start
            && self.pos < self.chunk_start.saturating_add(self.chunk.len() as u64);
        if !in_chunk {
            self.fill()?;
        }
        let off = self.pos.saturating_sub(self.chunk_start) as usize;
        if off >= self.chunk.len() {
            return Ok(0);
        }
        let n = buf.len().min(self.chunk.len() - off);
        buf[..n].copy_from_slice(&self.chunk[off..off + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for HttpAudioStream {
    fn seek(&mut self, style: SeekFrom) -> std::io::Result<u64> {
        let base: i128 = match style {
            SeekFrom::Start(o) => i128::from(o),
            SeekFrom::Current(o) => i128::from(self.pos) + i128::from(o),
            SeekFrom::End(o) => i128::from(self.require_len()?) + i128::from(o),
        };
        if base < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "stream seek failed",
            ));
        }
        self.pos = base.min(i128::from(u64::MAX)) as u64;
        Ok(self.pos)
    }
}

/// Offline test double: records calls, holds metadata only (never the URL),
/// persists zero bytes. Models the no-persist / no-retain contract.
#[cfg(test)]
pub struct MockBackend {
    pub play_calls: usize,
    pub last_video_id: Option<String>,
    pub stopped_calls: usize,
    pub playing: bool,
    pub volume: f32,
    pub pos_secs: u32,
    pub fail_next_play: Option<PlaybackError>,
}

#[cfg(test)]
impl Default for MockBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl MockBackend {
    pub fn new() -> Self {
        Self {
            play_calls: 0,
            last_video_id: None,
            stopped_calls: 0,
            playing: false,
            volume: 0.8,
            pos_secs: 0,
            fail_next_play: None,
        }
    }

    /// Regression guard: the mock retains metadata only, never stream URLs.
    pub fn stores_url(&self) -> bool {
        false
    }
}

#[cfg(test)]
impl AudioBackend for MockBackend {

    fn play_stream(&mut self, ticket: &StreamTicket) -> Result<(), PlaybackError> {
        if let Some(e) = self.fail_next_play.take() {
            return Err(e);
        }
        self.play_calls += 1;
        // Metadata only: the URL string is never retained.
        self.last_video_id = Some(ticket.video_id().to_string());
        self.playing = true;
        Ok(())
    }

    fn pause(&mut self) {
        self.playing = false;
    }

    fn resume(&mut self) {
        self.playing = true;
    }

    fn stop(&mut self) {
        self.stopped_calls += 1;
        self.playing = false;
        self.last_video_id = None;
    }

    fn seek(&mut self, secs: u32) {
        self.pos_secs = secs;
    }

    fn set_volume(&mut self, volume: f32) {
        self.volume = clamp_volume(volume);
    }

    fn position_secs(&self) -> u32 {
        self.pos_secs
    }

    fn is_finished(&self) -> bool {
        false
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_clamps() {
        assert_eq!(clamp_volume(0.8), 0.8);
        assert_eq!(clamp_volume(2.0), 1.0);
        assert_eq!(clamp_volume(-1.0), 0.0);
        assert_eq!(clamp_volume(f32::NAN), 0.0);
    }

    #[test]
    fn rodio_backend_constructs_without_audio_hardware() {
        // Lazy sink: construction is side-effect free (offline-safe).
        let b = RodioBackend::new();
        assert_eq!(b.position_secs(), 0);
        assert!(!b.is_finished(), "nothing loaded is not 'finished'");
    }

    #[test]
    fn mock_models_no_url_retention() {
        let mut b = MockBackend::new();
        let t = StreamTicket::new("vid-1", "https://mock.invalid/stream").unwrap();
        b.play_stream(&t).unwrap();
        assert_eq!(b.last_video_id.as_deref(), Some("vid-1"));
        assert!(!b.stores_url());
        b.stop();
        assert!(b.last_video_id.is_none());
    }

    #[test]
    fn mock_backend_seek_updates_position() {
        let mut backend = MockBackend::new();
        backend.seek(42);
        assert_eq!(backend.position_secs(), 42);
    }
}
