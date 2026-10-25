use qtbridge::{QApp, qobject};

mod auth;
mod disk_cache;
mod legal;
mod models;
mod playback;
mod prefs;
mod premium_gate;
mod theme;
mod ytmusic;

use models::{AlbumModel, ArtistModel, PlaylistModel, TrackModel};
use playback::PlaybackState;
use playback::PlaybackUrlSource;
use playback::backend::{AudioBackend, RodioBackend};
use playback::player::PlaybackSnapshot;
use playback::queue::{Queue, QueueItem};

/// Player + session state for the QML shell. Thin: QML binds layout only;
/// all decisions live here in pure helpers + slot mutations.
pub struct Backend {
    is_playing: bool,
    // Video id of the current track (row highlight + like target).
    video_id: String,
    title: String,
    artist: String,
    position: i32,
    duration: i32,
    shuffle: bool,
    loop_mode: String,
    // `liked` mirrors `liked_ids` for the current track. `liked_ids` is seeded
    // from the account's Liked Music playlist after sign-in/restore.
    liked: bool,
    liked_ids: std::collections::HashSet<String>,
    // Hearts tapped before the Liked Music load lands win over it.
    liked_overrides: std::collections::HashMap<String, bool>,
    // (generation, ids) from the worker; a stale generation is dropped.
    liked_load: std::sync::Arc<std::sync::Mutex<Option<LikedLoad>>>,
    liked_gen: u64,
    next_title: String,
    next_artist: String,
    // 1-based position of the current track in the queue (0 = none).
    queue_position: i32,
    // Session surface: live login state for Sidebar/Library/LoginView.
    // `gate` is the cached fail-closed decision (never playback unless
    // Premium); the QML props below mirror it + identity. The long-lived
    // secret stays in the OS keyring; `YTMusic` clients are ephemeral per
    // probe (rebuilt from the keyring, dropped after).
    account_name: String,
    account_label: String,
    is_logged_in: bool,
    gate_status: String,
    gate: crate::premium_gate::PremiumGate,
    // The optional native sign-in child (WebKitGTK helper). Its stdout pipe
    // carries the cookie header once, directly into memory; never a file.
    browser_login_child: Option<std::process::Child>,
    // Header read from the child, held just long enough for QML to paint a
    // "completing sign-in" state before the blocking gate probe runs. Cleared
    // on success, cancel, and the next start; never logged.
    browser_login_header: Option<String>,
    // Launch restore runs on a worker (the probe takes seconds); QML polls
    // `poll_session` while `session_loading` is true.
    session_loading: bool,
    session_load: std::sync::Arc<std::sync::Mutex<Option<SessionLoad>>>,
    // Launch applied the saved gate result; the live probe is rechecking it.
    session_rechecking: bool,
    // Playback engine surface (see `crate::playback`): engine state for
    // the `playbackState` prop, upcoming count, volume, radio/autoplay.
    // The engine itself lives in `playback::player`; `apply_snapshot`
    // bridges it here signal-free (unit-tested), slots emit to QML.
    playback_state: String,
    queue_count: i32,
    volume: f64,
    radio_on: bool,
    autoplay_on: bool,
    // Settings: reopen the last queue on launch / at the exact position.
    resume_enabled: bool,
    resume_position: bool,
    // Settings: low-performance mode (see `prefs::Prefs::low_performance`).
    low_performance: bool,
    // Restored track not started yet: (video id, second to seek to on Play).
    resume_at: Option<(String, u32)>,
    // Queue model (see `crate::playback::queue`): transport slots drive
    // this instead of ad-hoc fields. Starts EMPTY (explicit empty state —
    // never a seeded demo track); the first live fill (row tap via
    // `play_queue`, radio/autoplay refill) seeds current/next from real
    // data only. Mock rows stay offline-test-only (see `crate::models`).
    queue: Queue,
    // Live audio engine: rodio default (lazy sink — construction is
    // side-effect free, see `crate::playback::backend`). `play_queue` /
    // `next` / `prev` start short-lived stream tickets here (in-memory
    // only, never logged/persisted); `toggle_play` pauses/resumes.
    audio: RodioBackend,
    // Async playback request: URL fetch + radio refill run on a worker
    // thread (no UI freeze); QML polls `poll_playback` and the ticket is
    // handed to rodio on this thread. `playback_token` supersedes stale
    // workers; the mutex slot carries no secrets beyond the short-lived
    // in-memory ticket (never logged/persisted).
    playback_token: u64,
    pending_playback: Option<PendingPlayback>,
    playback_load: std::sync::Arc<std::sync::Mutex<Vec<PlaybackLoad>>>,
    // Explicit live-failure line for the QML error badge (`""` = no error).
    // Set on transport/audio failures; the `upgrade` state uses the legal
    // upsell instead (see `upgrade_message`). Never carries URLs/secrets.
    error_message: String,
    // Last submitted search query (QML search pill writes via
    // `set_search_query`; browse views react and run live searches).
    search_query: String,
    // Cover art for the now-playing surfaces, set from the row/queue item
    // that started playback. QML mirrors it into Theme's cover source.
    cover_source: String,
    // Local recent plays (metadata only) for the History tab; remote history
    // is loaded alongside the library and merged on read.
    recent_plays: Vec<QueueItem>,
}

/// Live probe result computed off the UI thread, applied by `apply_probe`.
/// Identity + log-safe report only; never secrets.
struct ProbeOutcome {
    session_logged_in: bool,
    session_name: String,
    // `None` when no tokio runtime could be built.
    probed: Option<(crate::premium_gate::ProbeReport, Option<String>)>,
}

/// Launch-restore worker result.
enum SessionLoad {
    Failed(String),
    NotSignedIn,
    Probed(ProbeOutcome),
}

/// Keyring session + client + runtime for best-effort background work
/// (prefetch, like sync). `None` when signed out or no runtime.
/// (generation, video ids) handed from the Liked Music worker.
type LikedLoad = (u64, std::collections::HashSet<String>);

// shortcut: only the newest LIKED_LIMIT liked songs are seeded; older likes
// read unliked until toggled. Raise it (one request per 100) if that bites.
const LIKED_LIMIT: usize = 5000;

/// Video ids in a `get_playlist`/`get_liked_songs` response.
fn liked_ids_from_playlist(
    playlist: &serde_json::Map<String, serde_json::Value>,
) -> std::collections::HashSet<String> {
    playlist
        .get("tracks")
        .and_then(|t| t.as_array())
        .into_iter()
        .flatten()
        .filter_map(|t| t.get("videoId")?.as_str())
        .map(str::to_string)
        .collect()
}

/// Rows the Up next list shows; the queue itself is unbounded.
const UP_NEXT_ROWS: usize = 100;

/// How long a saved gate result may stand in for the launch probe (the
/// probe still re-runs in the background every launch).
const SESSION_CACHE_MAX_AGE_SECS: u64 = 3 * 24 * 3600;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Saved `(gate status, account name)` when recent and well-formed.
fn saved_session(saved: &serde_json::Value, now: u64) -> Option<(String, String)> {
    let at = saved.get("saved")?.as_u64()?;
    if now.saturating_sub(at) > SESSION_CACHE_MAX_AGE_SECS {
        return None;
    }
    let gate = saved.get("gate")?.as_str()?;
    if !["premium", "upgradeRequired"].contains(&gate) {
        return None;
    }
    let name = saved.get("name")?.as_str().unwrap_or_default();
    Some((gate.to_string(), name.to_string()))
}

fn restored_client() -> Option<(ytmusicapi::YTMusic, tokio::runtime::Runtime)> {
    let (_session, yt) = crate::auth::Session::restore(&crate::auth::KeyringStore).ok()??;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    Some((yt, runtime))
}

/// Run the live probe + account-name fetch. Blocking network; launch calls
/// it off the UI thread. No `Backend` state.
fn run_probe(session: &crate::auth::Session, yt: &ytmusicapi::YTMusic) -> ProbeOutcome {
    let session_logged_in = session.is_logged_in();
    let session_name = session.account_name().unwrap_or("").to_string();
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return ProbeOutcome { session_logged_in, session_name, probed: None };
    };
    let report = runtime.block_on(crate::auth::probe_report(
        yt,
        crate::premium_gate::PROBE_VIDEO_ID,
    ));
    // Best-effort identity with a short timeout; failure keeps "Signed in".
    let live_name = runtime.block_on(async {
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            crate::auth::fetch_account_name(yt),
        )
        .await
        .ok()
        .flatten()
    });
    ProbeOutcome { session_logged_in, session_name, probed: Some((report, live_name)) }
}

/// UI-side metadata for the in-flight playback request.
struct PendingPlayback {
    token: u64,
    video_id: String,
    title: String,
    artist: String,
    row_duration: u32,
}

/// Worker result: a short-lived ticket + radio refill, or a fail-closed error.
/// The ticket lives only in memory and is dropped after `play_stream`.
struct PlaybackLoad {
    token: u64,
    ticket: Option<crate::playback::StreamTicket>,
    refill: Vec<QueueItem>,
    error: Option<crate::playback::PlaybackError>,
}

impl Default for Backend {
    fn default() -> Self {
        Self {
            // Explicit empty state: nothing playing, no fake track. The
            // header shows "Browse", the player lane shows 0:00, and the
            // first live fill seeds current/next from real data only.
            is_playing: false,
            video_id: String::new(),
            title: String::new(),
            artist: String::new(),
            position: 0,
            duration: 0,
            shuffle: false,
            loop_mode: "off".to_string(),
            liked: false,
            liked_ids: std::collections::HashSet::new(),
            liked_overrides: std::collections::HashMap::new(),
            liked_load: std::sync::Arc::new(std::sync::Mutex::new(None)),
            liked_gen: 0,
            next_title: String::new(),
            next_artist: String::new(),
            queue_position: 0,
            playback_state: PlaybackState::Stopped.as_str().to_string(),
            queue_count: 0,
            volume: 0.8,
            radio_on: false,
            autoplay_on: true,
            resume_enabled: crate::prefs::load().resume,
            resume_position: crate::prefs::load().resume_position,
            low_performance: crate::prefs::load().low_performance,
            resume_at: None,
            queue: Queue::new(),
            audio: RodioBackend::new(),
            playback_token: 0,
            pending_playback: None,
            playback_load: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            error_message: String::new(),
            search_query: String::new(),
            cover_source: String::new(),
            recent_plays: Vec::new(),
            account_name: String::new(),
            account_label: account_label_for(
                false,
                "",
                crate::premium_gate::GateStatus::LoggedOut,
            ),
            is_logged_in: false,
            gate_status: gate_status_str(crate::premium_gate::GateStatus::LoggedOut).to_string(),
            gate: crate::premium_gate::PremiumGate::new_logged_out(),
            browser_login_child: None,
            browser_login_header: None,
            session_loading: false,
            session_load: std::sync::Arc::new(std::sync::Mutex::new(None)),
            session_rechecking: false,
        }
    }
}

/// QML string for a gate decision (camelCase to match `ConvertToCamelCase`
/// props): `loggedOut | premium | upgradeRequired`. Pure, unit-tested.
pub fn gate_status_str(status: crate::premium_gate::GateStatus) -> &'static str {
    match status {
        crate::premium_gate::GateStatus::LoggedOut => "loggedOut",
        crate::premium_gate::GateStatus::Premium => "premium",
        crate::premium_gate::GateStatus::UpgradeRequired => "upgradeRequired",
    }
}

/// Display label for the sidebar chip + login surfaces. Identity only
/// (account name when known, else "Signed in"), plus the gate tier. Never
/// carries secrets — inputs are bools/names/enums only. Pure, unit-tested.
pub fn account_label_for(
    logged_in: bool,
    account_name: &str,
    status: crate::premium_gate::GateStatus,
) -> String {
    if !logged_in {
        return "Not signed in".to_string();
    }
    let who = {
        let trimmed = account_name.trim();
        if trimmed.is_empty() {
            "Signed in".to_string()
        } else {
            trimmed.to_string()
        }
    };
    match status {
        crate::premium_gate::GateStatus::Premium => format!("{who} • Premium"),
        crate::premium_gate::GateStatus::UpgradeRequired => {
            format!("{who} • Upgrade required")
        }
        crate::premium_gate::GateStatus::LoggedOut => "Not signed in".to_string(),
    }
}

/// Format seconds as `m:ss` (clamped at 0). Pure, unit-tested; QML mirrors
/// this in JS for display (see PlayerCard.qml `fmt`).
pub fn format_time(secs: i32) -> String {
    let s = secs.max(0);
    format!("{}:{:02}", s / 60, s % 60)
}

/// Remaining label as `-m:ss`. Pure, unit-tested.
pub fn remaining_label(position: i32, duration: i32) -> String {
    format!("-{}", format_time((duration - position).max(0)))
}

/// Progress 0..1, clamped. Pure, unit-tested.
pub fn progress01(position: i32, duration: i32) -> f64 {
    if duration <= 0 {
        return 0.0;
    }
    ((position as f64) / (duration as f64)).clamp(0.0, 1.0)
}

/// Clamp a QML volume (0.0..=1.0, non-finite becomes 0.0). Pure, tested.
pub fn clamp_volume64(v: f64) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    v.clamp(0.0, 1.0)
}

/// One position tick (called per second from QML): advances only while
/// playing, clamped to duration. Pure, unit-tested.
pub fn advance_position(position: i32, duration: i32, playing: bool) -> i32 {
    if !playing {
        return position;
    }
    (position + 1).min(duration.max(0))
}

impl Backend {
    /// Bridge the playback engine into QML-bound fields WITHOUT emitting
    /// signals (unit-testable offline; slots emit after calling this).
    pub fn apply_snapshot(&mut self, snap: &PlaybackSnapshot) {
        self.is_playing = snap.is_playing;
        self.title = snap.title.clone();
        self.artist = snap.artist.clone();
        self.position = snap.position_secs.min(i32::MAX as u32) as i32;
        self.duration = snap.duration_secs.min(i32::MAX as u32) as i32;
        self.next_title = snap.next_title.clone();
        self.next_artist = snap.next_artist.clone();
        self.playback_state = snap.state.as_str().to_string();
        self.queue_count = snap.queue_len.min(i32::MAX as usize) as i32;
    }

    /// Refresh the next-up preview + count from the queue WITHOUT emitting
    /// signals (unit-testable offline; slots emit after calling this).
    fn refresh_queue_view(&mut self) {
        match self.queue.next_up() {
            Some(next) => {
                self.next_title = next.title.clone();
                self.next_artist = next.artist.clone();
            }
            None => {
                self.next_title.clear();
                self.next_artist.clear();
            }
        }
        self.queue_count = self.queue.len().min(i32::MAX as usize) as i32;
        self.queue_position = self.queue.position().min(i32::MAX as usize) as i32;
    }

    /// Adopt a queue item into the now-playing fields (keeps the old
    /// duration when the item carries none). Test helper: runtime playback
    /// goes through `request_playback`/`poll_playback`.
    #[cfg(test)]
    fn adopt_queue_item(&mut self, item: QueueItem) {
        self.title = item.title;
        self.artist = item.artist;
        if item.duration_secs > 0 {
            self.duration = item.duration_secs.min(i32::MAX as u32) as i32;
        }
        self.position = 0;
    }

    /// Start an already-current queue item (next/prev). The queue already
    /// moved, so this only requests the stream.
    fn play_current_live(
        &mut self,
        video_id: &str,
        title: &str,
        artist: &str,
        duration_label: &str,
        thumbnail_url: &str,
    ) {
        self.request_playback(
            video_id,
            title,
            artist,
            duration_label,
            false,
            Some(thumbnail_url),
        );
    }

    /// Fold a gate block into the upgrade/error snapshot WITHOUT emitting.
    /// Returns true when the gate blocked the request; `request_playback`
    /// emits afterwards. Signal-free so the unit test can prove the
    /// fail-closed ordering without a QML proxy.
    fn apply_gate_block(&mut self, title: &str, artist: &str) -> bool {
        let Err(error) = crate::playback::check_playback_gate(&self.gate) else {
            return false;
        };
        match error {
            crate::playback::PlaybackError::LoggedOut
            | crate::playback::PlaybackError::UpgradeRequired => {
                self.apply_upgrade_block(title, artist);
            }
            _ => {
                let msg = playback_error_message(error);
                self.apply_error_block(title, artist, &msg);
            }
        }
        true
    }

    /// Request playback WITHOUT blocking the UI. The URL fetch and radio
    /// refill run on a worker thread; QML polls [`Backend::poll_playback`],
    /// and rodio receives the ticket on the UI thread. Stale workers are
    /// superseded by the token and their tickets are dropped unread.
    fn request_playback(
        &mut self,
        video_id: &str,
        title: &str,
        artist: &str,
        duration_label: &str,
        set_queue: bool,
        thumbnail_url: Option<&str>,
    ) {
        let video_id = video_id.trim();
        if self.apply_gate_block(title, artist) {
            self.emit_playback_props();
            return;
        }
        if video_id.is_empty() {
            let msg = crate::playback::PlaybackError::NoTrack.to_string();
            self.apply_error_block(title, artist, &msg);
            self.emit_playback_props();
            return;
        }
        let row_duration = crate::playback::queue::parse_duration_label(duration_label);
        self.playback_token = self.playback_token.wrapping_add(1);
        let token = self.playback_token;
        self.audio.stop();
        self.pending_playback = Some(PendingPlayback {
            token,
            video_id: video_id.to_string(),
            title: title.to_string(),
            artist: artist.to_string(),
            row_duration,
        });
        if set_queue {
            let mut item = QueueItem::new(video_id, title, artist);
            item.duration_secs = row_duration;
            self.queue.set_current(item);
        }
        self.title = title.to_string();
        self.artist = artist.to_string();
        self.set_now_playing_id(video_id);
        self.cover_source = thumbnail_url.unwrap_or("").to_string();
        self.position = 0;
        self.duration = row_duration.min(i32::MAX as u32) as i32;
        self.is_playing = false;
        self.playback_state = PlaybackState::Loading.as_str().to_string();
        self.error_message.clear();
        self.refresh_queue_view();
        if let Ok(mut slot) = self.playback_load.lock() {
            slot.clear();
        }
        let slot = std::sync::Arc::clone(&self.playback_load);
        let gate = self.gate.clone();
        // Radio refill is only for an explicit radio lane or a queue that
        // has nothing after the current row. A clicked list already IS the
        // queue; inferring a 49-track radio from it produced unrelated next
        // tracks and bloated the queue.
        let need_refill = self.radio_on || (self.autoplay_on && self.queue.next_up().is_none());
        let vid = video_id.to_string();
        let spawned = std::thread::Builder::new()
            .name("playback-load".to_string())
            .spawn(move || {
                let outcome = (|| {
                    let store = crate::auth::KeyringStore;
                    let (_session, yt) = match crate::auth::Session::restore(&store) {
                        Ok(Some(pair)) => pair,
                        Ok(None) | Err(_) => {
                            return Err(crate::playback::PlaybackError::LoggedOut);
                        }
                    };
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| crate::playback::PlaybackError::Transport)?;
                    let source = crate::playback::LiveUrlSource::new(&yt, &gate);
                    let ticket = runtime.block_on(source.fetch_url(&vid))?;
                    let refill = if need_refill {
                        runtime
                            .block_on(source.refill_tracks(
                                &vid,
                                crate::playback::player::REFILL_LIMIT,
                            ))
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    };
                    Ok((ticket, refill))
                })();
                if let Ok(mut slot) = slot.lock() {
                    slot.push(match outcome {
                        Ok((ticket, refill)) => PlaybackLoad {
                            token,
                            ticket: Some(ticket),
                            refill,
                            error: None,
                        },
                        Err(error) => PlaybackLoad {
                            token,
                            ticket: None,
                            refill: Vec::new(),
                            error: Some(error),
                        },
                    });
                }
            });
        if spawned.is_err() {
            self.pending_playback = None;
            let msg = crate::playback::PlaybackError::Transport.to_string();
            self.apply_error_block(title, artist, &msg);
        }
        self.emit_playback_props();
    }

    /// Emit every playback-related QML prop after a request/poll transition.
    fn save_prefs(&self) {
        crate::prefs::save(crate::prefs::Prefs {
            resume: self.resume_enabled,
            resume_position: self.resume_position,
            low_performance: self.low_performance,
        });
    }

    /// What `save_playback_session` writes (`None` = resume off / no queue).
    fn playback_session_json(&self) -> Option<serde_json::Value> {
        if !self.resume_enabled {
            return None;
        }
        let (items, index) = self.queue.snapshot()?;
        let rows: Vec<serde_json::Value> =
            items.iter().map(crate::playback::queue::Queue::to_json).collect();
        let position = if self.resume_position { self.position.max(0) } else { 0 };
        Some(serde_json::json!({
            "rows": rows,
            "index": index,
            "position": position,
            "shuffle": self.shuffle,
            "loop": self.loop_mode,
            "volume": self.volume,
        }))
    }

    /// Fold a saved session into queue + props WITHOUT signals. Restored
    /// paused with nothing streaming; `false` when the file is unusable.
    fn apply_saved_playback(&mut self, saved: &serde_json::Value) -> bool {
        let items: Vec<QueueItem> = saved
            .get("rows")
            .and_then(|r| r.as_array())
            .into_iter()
            .flatten()
            .filter_map(QueueItem::from_json)
            .collect();
        let index = saved.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
        let Some(item) = items.get(index).cloned() else {
            return false;
        };
        self.queue.set_from_list(items, index);
        self.shuffle = saved.get("shuffle").and_then(|v| v.as_bool()).unwrap_or(false);
        self.queue.set_shuffle(self.shuffle);
        let mode = saved
            .get("loop")
            .and_then(|v| v.as_str())
            .and_then(crate::playback::queue::LoopMode::from_label)
            .unwrap_or_default();
        self.queue.set_loop_mode(mode);
        self.loop_mode = mode.as_str().to_string();
        if let Some(volume) = saved.get("volume").and_then(|v| v.as_f64()) {
            self.volume = clamp_volume64(volume);
        }
        let duration = item.duration_secs.min(i32::MAX as u32) as i32;
        let mut position = saved.get("position").and_then(|v| v.as_i64()).unwrap_or(0).max(0);
        if duration > 0 {
            position = position.min(i64::from(duration));
        }
        self.title = item.title.clone();
        self.artist = item.artist.clone();
        self.cover_source = item.thumbnail_url.clone();
        self.duration = duration;
        self.position = position as i32;
        self.is_playing = false;
        self.playback_state = PlaybackState::Paused.as_str().to_string();
        self.set_now_playing_id(&item.video_id);
        self.resume_at = Some((item.video_id, position as u32));
        self.refresh_queue_view();
        true
    }

    fn emit_playback_props(&mut self) {
        self.title_changed();
        self.artist_changed();
        self.position_changed();
        self.duration_changed();
        self.is_playing_changed();
        self.playback_state_changed();
        self.error_message_changed();
        self.next_title_changed();
        self.next_artist_changed();
        self.queue_count_changed();
        self.queue_position_changed();
        self.cover_source_changed();
        self.video_id_changed();
        self.liked_changed();
    }

    /// Fold a session + gate decision into the QML session props WITHOUT
    /// emitting signals (unit-testable offline with mocked
    /// [`PremiumSignals`](crate::premium_gate::PremiumSignals); slots emit
    /// after calling this). `account_name` is identity only — the live name
    /// from [`fetch_account_name`](crate::auth::fetch_account_name), or
    /// empty (generic "Signed in" label, never a fake name); never secrets.
    pub fn apply_session_state(
        &mut self,
        logged_in: bool,
        account_name: &str,
        status: crate::premium_gate::GateStatus,
    ) {
        // Keep the cached gate coherent with the mirrored props so
        // `play_queue` stays fail-closed off the same decision.
        // `PremiumGate` has no setter from status alone; callers that hold
        // mocked signals should `gate.apply(signals)` first and pass
        // `gate.status()` here — this mirrors that status into props.
        self.is_logged_in = logged_in;
        self.account_name = account_name.trim().to_string();
        self.gate_status = gate_status_str(status).to_string();
        self.account_label = account_label_for(logged_in, account_name, status);
    }

    /// Reset to logged-out WITHOUT emitting signals (offline; slots emit).
    /// Used by logout + failed restore (fail-closed default).
    pub fn apply_logged_out(&mut self) {
        self.gate = crate::premium_gate::PremiumGate::new_logged_out();
        self.apply_session_state(
            false,
            "",
            crate::premium_gate::GateStatus::LoggedOut,
        );
    }

    /// Offline restore fold (no network, no keyring type pinned): try
    /// [`Session::restore`](crate::auth::Session::restore) against `store`;
    /// success marks logged-in with a fail-closed `UpgradeRequired`
    /// placeholder until the live probe refines it to Premium; any failure
    /// (missing/invalid/transport) stays logged-out. Returns whether a
    /// session was found. Signal-free (slots emit).
    pub fn apply_restore_offline(&mut self, store: &impl crate::auth::SecretStore) -> bool {
        match crate::auth::Session::restore(store) {
            Ok(Some((session, _))) => {
                let name = session.account_name().unwrap_or("").to_string();
                // Placeholder gate: authenticated but unprobed is ambiguous,
                // so fail-closed UpgradeRequired (never LoggedOut, never Premium).
                self.gate = {
                    let mut g = crate::premium_gate::PremiumGate::new_logged_out();
                    g.apply(
                        &crate::premium_gate::PremiumSignals {
                            logged_in: true,
                            home_ok: false,
                            premium_badge: None,
                            explore_top_songs: None,
                            ads_absent: None,
                            topbar_premium: None,
                            hires_audio: None,
                            playability: crate::premium_gate::Playability::Unknown,
                        },
                        std::time::SystemTime::now(),
                    );
                    g
                };
                let status = self.gate.status();
                self.apply_session_state(true, &name, status);
                true
            }
            Ok(None) | Err(_) => {
                self.apply_logged_out();
                false
            }
        }
    }

    /// Fold a probe outcome into the cached gate + QML props WITHOUT
    /// emitting signals (slots emit). Returns the log-safe report. Any
    /// runtime/probe failure stays fail-closed: logged-in + `UpgradeRequired`,
    /// never Premium, never playback.
    fn apply_probe(&mut self, outcome: ProbeOutcome) -> String {
        let Some((report, live_name)) = outcome.probed else {
            self.gate.apply(
                &crate::premium_gate::PremiumSignals {
                    logged_in: true,
                    home_ok: false,
                    premium_badge: None,
                    explore_top_songs: None,
                    ads_absent: None,
                    topbar_premium: None,
                    hires_audio: None,
                    playability: crate::premium_gate::Playability::Unknown,
                },
                std::time::SystemTime::now(),
            );
            let status = self.gate.status();
            // Runtime failure is still an authenticated session, so keep
            // the login flag and block fail-closed.
            self.apply_session_state(true, &outcome.session_name, status);
            return "sign-in stored; probe runtime unavailable".to_string();
        };
        // Re-apply the reported signals into the cached gate (the probe ran
        // on a temp gate inside `probe_report`; the report is the only
        // log-safe carrier).
        let signals = crate::premium_gate::PremiumSignals {
            logged_in: report.logged_in,
            home_ok: report.home_ok,
            premium_badge: report.premium_badge,
            explore_top_songs: report.explore_top_songs,
            ads_absent: report.ads_absent,
            topbar_premium: report.topbar_premium,
            hires_audio: report.hires_audio,
            playability: report.playability,
        };
        self.gate.apply(&signals, std::time::SystemTime::now());
        let status = self.gate.status();
        // Fail-closed login flag: the session AND the probe must agree the
        // client is authenticated; otherwise show logged-out.
        let logged_in = outcome.session_logged_in && report.logged_in;
        // Live name wins; empty/failed falls back to the session name.
        let name = live_name
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(outcome.session_name);
        if !logged_in {
            self.apply_logged_out();
        } else {
            self.apply_session_state(true, &name, status);
        }
        report.render()
    }

    /// Remember the gate result for the next launch (logged-out forgets it).
    fn save_session_cache(&self) {
        if self.is_logged_in {
            crate::disk_cache::write(
                "session",
                &serde_json::json!({
                    "gate": self.gate_status,
                    "name": self.account_name,
                    "saved": now_secs(),
                }),
            );
        } else {
            crate::disk_cache::remove("session");
        }
    }

    fn emit_session_props(&mut self) {
        self.account_name_changed();
        self.account_label_changed();
        self.is_logged_in_changed();
        self.gate_status_changed();
    }

    /// Point the now-playing fields at `video_id` (row highlight + like).
    fn set_now_playing_id(&mut self, video_id: &str) {
        self.video_id = video_id.to_string();
        self.liked = self.liked_ids.contains(video_id);
    }

    /// Forget this account's likes and invalidate any in-flight load.
    fn reset_likes(&mut self) {
        self.liked_gen += 1;
        self.liked_ids.clear();
        self.liked_overrides.clear();
    }

    /// Fetch the account's Liked Music on a worker; `merge_liked_load`
    /// folds it in. Metadata only (video ids), memory only.
    fn start_liked_load(&mut self) {
        if !self.is_logged_in {
            return;
        }
        self.liked_gen += 1;
        let generation = self.liked_gen;
        let slot = self.liked_load.clone();
        std::thread::spawn(move || {
            let Some((yt, runtime)) = restored_client() else {
                return;
            };
            match runtime.block_on(yt.get_liked_songs(Some(LIKED_LIMIT))) {
                Ok(playlist) => {
                    if let Ok(mut s) = slot.lock() {
                        *s = Some((generation, liked_ids_from_playlist(&playlist)));
                    }
                }
                Err(_) => eprintln!("liked songs load failed"),
            }
        });
    }

    /// Apply a finished Liked Music load (no-op until one lands).
    fn merge_liked_load(&mut self) {
        let Some((generation, ids)) = self.liked_load.lock().ok().and_then(|mut s| s.take()) else {
            return;
        };
        if generation != self.liked_gen {
            return;
        }
        self.liked_ids = ids;
        for (id, on) in self.liked_overrides.drain() {
            if on {
                self.liked_ids.insert(id);
            } else {
                self.liked_ids.remove(&id);
            }
        }
        let liked = self.liked_ids.contains(&self.video_id);
        if liked != self.liked {
            self.liked = liked;
            self.liked_changed();
        }
    }

    /// Fold a gate-blocked play request into the QML props WITHOUT emitting
    /// signals (unit-testable offline; slots emit). Shows the requested
    /// title/artist with the `upgrade` state + legal upsell (fail-closed).
    fn apply_upgrade_block(&mut self, title: &str, artist: &str) {
        let snap = PlaybackSnapshot {
            state: PlaybackState::UpgradeRequired,
            is_playing: false,
            title: title.to_string(),
            artist: artist.to_string(),
            position_secs: 0,
            duration_secs: 0,
            next_title: String::new(),
            next_artist: String::new(),
            queue_len: 1,
            upgrade_message: Some(crate::legal::upgrade_message()),
        };
        self.apply_snapshot(&snap);
        self.error_message.clear();
    }

    /// Fold a failed live play into the QML props WITHOUT emitting signals
    /// (unit-testable offline; slots emit). Shows the requested title/artist
    /// with the `error` state + the explicit message (never silent mocks,
    /// never URLs/secrets — see [`playback_error_message`]).
    fn apply_error_block(&mut self, title: &str, artist: &str, message: &str) {
        let snap = PlaybackSnapshot {
            state: PlaybackState::Error,
            is_playing: false,
            title: title.to_string(),
            artist: artist.to_string(),
            position_secs: 0,
            duration_secs: 0,
            next_title: String::new(),
            next_artist: String::new(),
            queue_len: self.queue.len(),
            upgrade_message: None,
        };
        self.apply_snapshot(&snap);
        self.error_message = message.to_string();
    }

    /// Record the submitted search query WITHOUT emitting signals (slots emit).
    fn apply_search_query(&mut self, query: &str) {
        self.search_query = query.to_string();
    }

}

/// Display-ready message for a playback failure. Gate blocks surface the
/// legal/sign-in lines; everything else surfaces the static transport
/// message. Never carries URLs or secrets. Pure, unit-tested.
fn playback_error_message(error: crate::playback::PlaybackError) -> String {
    match crate::playback::upgrade_message_for(error) {
        Some(msg) => msg,
        None => error.to_string(),
    }
}

#[qobject(Singleton, LinkMe, ConvertToCamelCase)]
impl Backend {
    qproperty!("isPlaying", Member = is_playing, Notify = is_playing_changed);
    qproperty!("videoId", Member = video_id, Notify = video_id_changed);
    qproperty!("title", Member = title, Notify = title_changed);
    qproperty!("artist", Member = artist, Notify = artist_changed);
    qproperty!("position", Member = position, Notify = position_changed);
    qproperty!("duration", Member = duration, Notify = duration_changed);
    qproperty!("shuffle", Member = shuffle, Notify = shuffle_changed);
    qproperty!("loopMode", Member = loop_mode, Notify = loop_mode_changed);
    qproperty!("liked", Member = liked, Notify = liked_changed);
    qproperty!("nextTitle", Member = next_title, Notify = next_title_changed);
    qproperty!("nextArtist", Member = next_artist, Notify = next_artist_changed);
    qproperty!("queuePosition", Member = queue_position, Notify = queue_position_changed);
    qproperty!("sessionLoading", Member = session_loading, Notify = session_loading_changed);
    qproperty!("playbackState", Member = playback_state, Notify = playback_state_changed);
    qproperty!("queueCount", Member = queue_count, Notify = queue_count_changed);
    qproperty!("volume", Member = volume, Notify = volume_changed);
    qproperty!("radioOn", Member = radio_on, Notify = radio_on_changed);
    qproperty!("autoplayOn", Member = autoplay_on, Notify = autoplay_on_changed);
    qproperty!("resumeEnabled", Member = resume_enabled, Notify = resume_enabled_changed);
    qproperty!("resumePosition", Member = resume_position, Notify = resume_position_changed);
    qproperty!("lowPerformance", Member = low_performance, Notify = low_performance_changed);
    qproperty!("accountName", Member = account_name, Notify = account_name_changed);
    qproperty!("accountLabel", Member = account_label, Notify = account_label_changed);
    qproperty!("isLoggedIn", Member = is_logged_in, Notify = is_logged_in_changed);
    qproperty!("gateStatus", Member = gate_status, Notify = gate_status_changed);
    qproperty!("errorMessage", Member = error_message, Notify = error_message_changed);
    qproperty!("searchQuery", Member = search_query, Notify = search_query_changed);
    qproperty!("coverSource", Member = cover_source, Notify = cover_source_changed);

    #[qsignal]
    fn is_playing_changed(&mut self);
    #[qsignal]
    fn video_id_changed(&mut self);
    #[qsignal]
    fn title_changed(&mut self);
    #[qsignal]
    fn artist_changed(&mut self);
    #[qsignal]
    fn position_changed(&mut self);
    #[qsignal]
    fn duration_changed(&mut self);
    #[qsignal]
    fn shuffle_changed(&mut self);
    #[qsignal]
    fn loop_mode_changed(&mut self);
    #[qsignal]
    fn liked_changed(&mut self);
    #[qsignal]
    fn next_title_changed(&mut self);
    #[qsignal]
    fn next_artist_changed(&mut self);
    #[qsignal]
    fn queue_position_changed(&mut self);
    #[qsignal]
    fn session_loading_changed(&mut self);
    #[qsignal]
    fn playback_state_changed(&mut self);
    #[qsignal]
    fn queue_count_changed(&mut self);
    #[qsignal]
    fn volume_changed(&mut self);
    #[qsignal]
    fn radio_on_changed(&mut self);
    #[qsignal]
    fn autoplay_on_changed(&mut self);
    #[qsignal]
    fn account_name_changed(&mut self);
    #[qsignal]
    fn account_label_changed(&mut self);
    #[qsignal]
    fn is_logged_in_changed(&mut self);
    #[qsignal]
    fn gate_status_changed(&mut self);
    #[qsignal]
    fn error_message_changed(&mut self);
    #[qsignal]
    fn search_query_changed(&mut self);
    #[qsignal]
    fn cover_source_changed(&mut self);
    #[qsignal]
    fn resume_enabled_changed(&mut self);
    #[qsignal]
    fn resume_position_changed(&mut self);
    #[qsignal]
    fn low_performance_changed(&mut self);

    #[qslot]
    fn set_low_performance(&mut self, on: bool) {
        if on != self.low_performance {
            self.low_performance = on;
            self.save_prefs();
            self.low_performance_changed();
        }
    }

    #[qslot]
    fn set_resume_enabled(&mut self, on: bool) {
        if on != self.resume_enabled {
            self.resume_enabled = on;
            self.save_prefs();
            self.resume_enabled_changed();
        }
    }

    #[qslot]
    fn set_resume_position(&mut self, on: bool) {
        if on != self.resume_position {
            self.resume_position = on;
            self.save_prefs();
            self.resume_position_changed();
        }
    }

    /// Window closing: remember the queue (and position) for next launch,
    /// or forget it when resume is off. Metadata only.
    #[qslot]
    fn save_playback_session(&mut self) {
        match self.playback_session_json() {
            Some(saved) => crate::disk_cache::write("playback", &saved),
            None => crate::disk_cache::remove("playback"),
        }
    }

    /// Launch: reopen the saved queue paused on its track (Play starts it,
    /// at the saved second when that setting is on).
    #[qslot]
    fn restore_playback_session(&mut self) {
        if !self.resume_enabled || self.queue.current().is_some() {
            return;
        }
        let Some(saved) = crate::disk_cache::read("playback") else {
            return;
        };
        if self.apply_saved_playback(&saved) {
            self.audio.set_volume(self.volume as f32);
            self.emit_playback_props();
            self.volume_changed();
            self.shuffle_changed();
            self.loop_mode_changed();
        }
    }

    #[qslot]
    fn toggle_play(&mut self) {
        // The stream ticket is still loading; transport is intentionally
        // inert until rodio is holding a playable stream.
        if self.playback_state == PlaybackState::Loading.as_str() {
            return;
        }
        // Nothing loaded (stopped/error/ended): (re)start the current queue
        // item instead of flipping a "playing" flag over silence.
        if self.playback_state != PlaybackState::Playing.as_str() {
            // After the lane ended the last track sits in history.
            let item = self.queue.current().cloned().or_else(|| self.queue.prev());
            if let Some(item) = item {
                let label = crate::models::format_duration_secs(item.duration_secs);
                self.play_current_live(&item.video_id, &item.title, &item.artist, &label, &item.thumbnail_url);
            }
            return;
        }
        self.is_playing = !self.is_playing;
        // Drive the live audio engine with the display flag (both are
        // no-ops when no stream is loaded).
        if self.is_playing {
            self.audio.resume();
        } else {
            self.audio.pause();
        }
        self.is_playing_changed();
    }

    /// Previous: past the first 3 s restart the current track; otherwise
    /// step back through queue history (no history restarts too).
    #[qslot]
    fn prev(&mut self) {
        if self.position > 3 {
            self.seek_to(0);
            return;
        }
        match self.queue.prev() {
            Some(item) => self.play_or_stop(Some(item)),
            None => self.seek_to(0),
        }
    }

    /// Manual Next (never replays under repeat-one).
    #[qslot]
    fn next(&mut self) {
        let item = self.queue.skip();
        self.play_or_stop(item);
    }

    #[qslot]
    fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        self.queue.set_shuffle(self.shuffle);
        self.shuffle_changed();
    }

    #[qslot]
    fn cycle_loop(&mut self) {
        self.loop_mode = self.queue.cycle_loop().as_str().to_string();
        self.loop_mode_changed();
    }

    /// Like/unlike the current track on YouTube Music. Optimistic: the
    /// heart flips now, `rate_song` runs on a worker (best-effort, metadata
    /// only). Signed-out sessions can't rate, so the heart stays put.
    #[qslot]
    fn toggle_heart(&mut self) {
        if self.video_id.is_empty() || !self.is_logged_in {
            return;
        }
        let vid = self.video_id.clone();
        self.liked = !self.liked;
        if self.liked {
            self.liked_ids.insert(vid.clone());
        } else {
            self.liked_ids.remove(&vid);
        }
        self.liked_overrides.insert(vid.clone(), self.liked);
        let rating = if self.liked { "LIKE" } else { "INDIFFERENT" };
        std::thread::spawn(move || {
            if let Some((yt, runtime)) = restored_client() {
                if runtime.block_on(yt.rate_song(&vid, rating)).is_err() {
                    eprintln!("like sync failed");
                }
            }
        });
        self.liked_changed();
    }

    /// Upgrade prompt for the QML upgrade dialog (see `crate::legal`).
    /// Plain getter (no signals) so it stays unit-testable offline.
    #[qslot]
    fn upgrade_message(&self) -> String {
        crate::legal::upgrade_message()
    }

    /// YouTube Terms of Service URL for the QML upgrade dialog.
    #[qslot]
    fn terms_url(&self) -> String {
        crate::legal::YOUTUBE_TERMS_URL.to_string()
    }

    /// Google Privacy Policy URL for sign-in / upgrade surfaces. Every
    /// sign-in/upgrade surface links ToS + privacy via `legal::`.
    #[qslot]
    fn privacy_url(&self) -> String {
        crate::legal::GOOGLE_PRIVACY_URL.to_string()
    }

    /// Start the native sign-in window (ephemeral WebKitGTK helper) and
    /// return `"started"` or a short user-facing failure. The helper prints
    /// the Cookie header on stdout exactly once; [`Self::poll_browser_signin`]
    /// reads it after exit — no credential file, no shared browser profile.
    #[qslot]
    fn start_browser_signin(&mut self) -> String {
        self.cancel_browser_signin();
        let python = std::env::var("SPINWAVE_PYTHON").unwrap_or_else(|_| "python3".to_string());
        let child = std::process::Command::new(&python)
            .arg("-c")
            .arg(crate::auth::BROWSER_LOGIN_HELPER)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn();
        match child {
            Ok(child) => {
                self.browser_login_child = Some(child);
                eprintln!("browser sign-in: helper started");
                "started".to_string()
            }
            Err(_) => {
                eprintln!("browser sign-in: helper could not start");
                "Browser sign-in is unavailable on this system. Use the cookie option instead.".to_string()
            }
        }
    }

    /// Poll the native sign-in child without blocking. `"pending"` while the
    /// window is open; `"finishing"` once the cookie header has been read;
    /// otherwise a short failure line (window closed early, or helper
    /// unavailable). The header is held in memory, never logged.
    #[qslot]
    fn poll_browser_signin(&mut self) -> String {
        let Some(mut child) = self.browser_login_child.take() else {
            return "idle".to_string();
        };
        match child.try_wait() {
            Ok(None) => {
                self.browser_login_child = Some(child);
                "pending".to_string()
            }
            Ok(Some(_status)) => {
                let output = match child.wait_with_output() {
                    Ok(output) => output,
                    Err(e) => return format!("Browser sign-in failed: {e}"),
                };
                let header = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if output.status.success() && !header.is_empty() {
                    eprintln!("browser sign-in: cookie header received");
                    self.browser_login_header = Some(header);
                    "finishing".to_string()
                } else if output.status.code() == Some(2) {
                    eprintln!("browser sign-in: window closed before completion");
                    "Sign-in window closed before completion.".to_string()
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let reason = stderr.lines().last().unwrap_or("").trim();
                    let reason = if reason.is_empty() {
                        "no diagnostic".to_string()
                    } else {
                        reason.to_string()
                    };
                    eprintln!("browser sign-in: helper failed: {reason}");
                    format!("Browser sign-in did not complete: {reason}")
                }
            }
            Err(e) => {
                let _ = child.kill();
                format!("Browser sign-in failed: {e}")
            }
        }
    }

    /// Finish a sign-in whose header was read by [`Self::poll_browser_signin`].
    /// Separated from the poll so QML can paint a "completing" state before
    /// this call blocks on the live gate probe. Returns `"ok"` or a short
    /// failure line.
    #[qslot]
    fn finish_browser_signin(&mut self) -> String {
        match self.browser_login_header.take() {
            Some(header) => self.finish_cookie_signin(&header),
            None => "Sign-in did not complete. Try again.".to_string(),
        }
    }

    /// Close the native sign-in window and drop the child (cancel path).
    #[qslot]
    fn cancel_browser_signin(&mut self) {
        if let Some(mut child) = self.browser_login_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.browser_login_header = None;
    }

    /// Fallback sign-in: the user pastes the `Cookie` request-header value
    /// from their own browser. Same validation, keyring, and probe lane as
    /// the embedded window; returns `"ok"` or a short failure line.
    #[qslot]
    fn connect_with_cookie(&mut self, header: String) -> String {
        self.finish_cookie_signin(&header)
    }

    /// Shared cookie-login core: normalize + keyring persist + live probe +
    /// prop signals. On failure nothing is persisted and no props change.
    fn finish_cookie_signin(&mut self, header: &str) -> String {
        let mut store = crate::auth::KeyringStore;
        let (session, yt) = match crate::auth::login_from_cookie_header(&mut store, header) {
            Ok(pair) => pair,
            Err(e) => return format!("Sign-in failed: {e}"),
        };
        // A different account may be signed in; never show the previous
        // account's cached shelves.
        crate::ytmusic::home::clear_home_cache();
        crate::ytmusic::search::clear_search_cache();
        crate::ytmusic::library::clear_library_cache();
        self.recent_plays.clear();
        self.reset_likes();
        let report = self.apply_probe(run_probe(&session, &yt));
        eprintln!("browser sign-in: {report}");
        self.save_session_cache();
        self.start_liked_load();
        self.emit_session_props();
        if self.is_logged_in {
            "ok".to_string()
        } else {
            "Sign-in failed: YouTube did not accept this browser session. \
             Make sure you are signed in at music.youtube.com, then try again."
                .to_string()
        }
    }

    /// YouTube Music web app URL for the sign-in guide's "open browser" action.
    #[qslot]
    fn ytmusic_url(&self) -> String {
        crate::legal::YTMUSIC_URL.to_string()
    }

    /// Restore the keyring session on launch. The keyring read + live
    /// fail-closed probe run on a worker (several seconds of network); QML
    /// polls [`Self::poll_session`] while `sessionLoading` is true. Never
    /// logs secrets.
    #[qslot]
    fn restore_session(&mut self) {
        if self.session_loading || self.session_rechecking {
            return;
        }
        // Instant start: apply the last gate result now; the probe below
        // rechecks it and `tick_position` folds the answer in.
        let saved = crate::disk_cache::read("session")
            .and_then(|v| saved_session(&v, now_secs()));
        if let Some((gate, name)) = saved {
            self.gate = crate::ytmusic::gate_from_status_str(&gate);
            let status = self.gate.status();
            self.apply_session_state(true, &name, status);
            self.emit_session_props();
            self.start_liked_load();
            self.session_rechecking = true;
        }
        let slot = std::sync::Arc::clone(&self.session_load);
        let spawned = std::thread::Builder::new()
            .name("session-restore".to_string())
            .spawn(move || {
                let load = match crate::auth::Session::restore(&crate::auth::KeyringStore) {
                    Err(e) => SessionLoad::Failed(format!("session restore failed: {e}")),
                    Ok(None) => SessionLoad::NotSignedIn,
                    Ok(Some((session, yt))) => SessionLoad::Probed(run_probe(&session, &yt)),
                };
                if let Ok(mut slot) = slot.lock() {
                    *slot = Some(load);
                }
            });
        if spawned.is_err() {
            self.session_rechecking = false;
        } else if !self.session_rechecking {
            self.session_loading = true;
            self.session_loading_changed();
        }
    }

    /// Apply the launch-restore result once the worker lands:
    /// `loading | done`.
    #[qslot]
    fn poll_session(&mut self) -> String {
        let Some(load) = self.session_load.lock().ok().and_then(|mut slot| slot.take()) else {
            return "loading".to_string();
        };
        let rechecking = std::mem::take(&mut self.session_rechecking);
        let before = (self.is_logged_in, self.account_name.clone(), self.gate_status.clone());
        match load {
            SessionLoad::Failed(msg) => {
                self.apply_logged_out();
                eprintln!("{msg}");
            }
            SessionLoad::NotSignedIn => {
                self.apply_logged_out();
                eprintln!("session restore: no stored session");
            }
            SessionLoad::Probed(outcome) => {
                let report = self.apply_probe(outcome);
                eprintln!("session restore: {report}");
                // A confirmed recheck already started the likes load.
                if !(rechecking && before.0 && self.is_logged_in) {
                    self.start_liked_load();
                }
            }
        }
        self.save_session_cache();
        if self.session_loading {
            self.session_loading = false;
            self.session_loading_changed();
        }
        let after = (self.is_logged_in, self.account_name.clone(), self.gate_status.clone());
        // A recheck that agrees with the saved result changes nothing on screen.
        if !rechecking || before != after {
            self.emit_session_props();
        }
        "done".to_string()
    }

    /// Sign out: clears the OS keyring entry and resets the session props
    /// to logged-out. Missing entry counts as success. Never logs secrets.
    #[qslot]
    fn logout(&mut self) -> String {
        let mut store = crate::auth::KeyringStore;
        match crate::auth::Session::logout(&mut store) {
            Err(e) => format!("sign-out failed: {e}"),
            Ok(_) => {
                crate::ytmusic::home::clear_home_cache();
                crate::ytmusic::search::clear_search_cache();
                crate::ytmusic::library::clear_library_cache();
                self.recent_plays.clear();
                self.reset_likes();
                self.apply_logged_out();
                self.save_session_cache();
                eprintln!("signed out");
                self.emit_session_props();
                "signed out".to_string()
            }
        }
    }

    /// Play a row from the visible list. That list becomes the queue:
    /// history = rows before it, upcoming = rows after it. This is what
    /// makes Next/Previous follow the list the user is looking at instead of
    /// an inferred radio.
    #[qslot]
    fn play_queue(&mut self, rows_json: String, index: i32) {
        let values: Vec<serde_json::Value> =
            serde_json::from_str(&rows_json).unwrap_or_default();
        let items: Vec<QueueItem> = values.iter().filter_map(QueueItem::from_json).collect();
        if items.is_empty() {
            let msg = crate::playback::PlaybackError::NoTrack.to_string();
            self.apply_error_block("", "", &msg);
            self.emit_playback_props();
            return;
        }
        self.queue.set_from_list(items, index.max(0) as usize);
        let Some(item) = self.queue.current().cloned() else {
            return;
        };
        let label = crate::models::format_duration_secs(item.duration_secs);
        self.request_playback(
            &item.video_id,
            &item.title,
            &item.artist,
            &label,
            false,
            Some(&item.thumbnail_url),
        );
    }

    /// Metadata-only rows for the Up next list: the next
    /// [`UP_NEXT_ROWS`] only (a whole-playlist queue can be thousands).
    #[qslot]
    fn queue_rows_json(&self) -> String {
        let rows: Vec<serde_json::Value> = self
            .queue
            .upcoming_list()
            .iter()
            .take(UP_NEXT_ROWS)
            .map(crate::playback::queue::Queue::to_json)
            .collect();
        serde_json::to_string(&rows).unwrap_or_default()
    }

    /// Play one of the queue popup's upcoming rows: move it to current and
    /// stream it. The rest of the lane stays intact.
    #[qslot]
    fn play_queue_row(&mut self, video_id: String) {
        let Some(item) = self.queue.take_upcoming(&video_id) else {
            return;
        };
        self.queue.set_current(item.clone());
        self.refresh_queue_view();
        let label = crate::models::format_duration_secs(item.duration_secs);
        self.request_playback(
            &item.video_id,
            &item.title,
            &item.artist,
            &label,
            false,
            Some(&item.thumbnail_url),
        );
    }

    /// Output volume 0.0..=1.0 (clamped, drives the live audio engine too).
    #[qslot]
    fn set_volume(&mut self, value: f64) {
        let clamped = clamp_volume64(value);
        if (clamped - self.volume).abs() > f64::EPSILON {
            self.volume = clamped;
            self.audio.set_volume(clamped as f32);
            self.volume_changed();
        }
    }

    /// Search query submitted from the QML search pill (thin glue: stores the
    /// string; browse views react and run the live search in Rust).
    #[qslot]
    fn set_search_query(&mut self, query: String) {
        if self.search_query != query {
            self.apply_search_query(&query);
            self.search_query_changed();
        }
    }

    /// Start a background home fetch for the given gate state. Returns a
    /// token; QML polls [`Self::home_load_status`] and then fills the models
    /// from the shared cache. The UI thread never waits on the network.
    #[qslot]
    fn start_home_load(&self, gate_status: String) -> i32 {
        crate::ytmusic::home::start_home_load(&gate_status) as i32
    }

    /// Poll a background home fetch: `loading | ready | error:<msg> | stale`.
    #[qslot]
    fn home_load_status(&self, token: i32) -> String {
        crate::ytmusic::home::home_load_status(token.max(0) as u64)
    }

    /// Start a background search; returns a poll token (no UI-thread network).
    #[qslot]
    fn start_search_load(&self, query: String, gate_status: String) -> i32 {
        crate::ytmusic::search::start_search_load(&query, &gate_status) as i32
    }

    /// Poll a background search: `loading | ready | error:<msg> | stale`.
    #[qslot]
    fn search_load_status(&self, token: i32) -> String {
        crate::ytmusic::search::search_load_status(token.max(0) as u64)
    }

    /// Start a background library load (playlists + remote history).
    #[qslot]
    fn start_library_load(&self, gate_status: String) -> i32 {
        crate::ytmusic::library::start_library_load(&gate_status) as i32
    }

    /// Poll a background library load.
    #[qslot]
    fn library_load_status(&self, token: i32) -> String {
        crate::ytmusic::library::library_load_status(token.max(0) as u64)
    }

    /// Playlist cards JSON (empty before a load).
    #[qslot]
    fn playlists_json(&self) -> String {
        crate::ytmusic::library::playlists_json()
    }

    /// Start a background playlist-detail load.
    #[qslot]
    fn start_playlist_load(&self, playlist_id: String, gate_status: String) -> i32 {
        crate::ytmusic::library::start_playlist_load(&playlist_id, &gate_status) as i32
    }

    /// Poll a background playlist-detail load.
    #[qslot]
    fn playlist_load_status(&self, token: i32) -> String {
        crate::ytmusic::library::playlist_load_status(token.max(0) as u64)
    }

    /// Playlist rows JSON (empty before a load).
    #[qslot]
    fn playlist_rows_json(&self, playlist_id: String) -> String {
        crate::ytmusic::library::playlist_rows_json(&playlist_id)
    }

    /// Lyrics for `video_id` (see `ytmusic::lyrics`): JSON, or "" while
    /// loading (QML polls). Metadata only.
    #[qslot]
    fn lyrics_json(&self, video_id: String) -> String {
        crate::ytmusic::lyrics::lyrics_json(&video_id, &self.gate_status)
    }

    /// Whether the open playlist has another page (load on scroll).
    #[qslot]
    fn playlist_has_more(&self, playlist_id: String) -> bool {
        crate::ytmusic::library::playlist_has_more(&playlist_id)
    }

    /// Fetch the next page in the background; poll with
    /// `playlistLoadStatus`. 0 = nothing more.
    #[qslot]
    fn start_playlist_more(&self, playlist_id: String, gate_status: String) -> i32 {
        crate::ytmusic::library::start_playlist_more(&playlist_id, &gate_status) as i32
    }

    /// Rows from `offset` on, for appending a fetched page.
    #[qslot]
    fn playlist_rows_after(&self, playlist_id: String, offset: i32) -> String {
        crate::ytmusic::library::playlist_rows_after(&playlist_id, offset.max(0) as usize)
    }

    /// History rows: local recent plays first, then the account's remote
    /// history, deduplicated by video id. Metadata only.
    #[qslot]
    fn history_rows_json(&self) -> String {
        let mut rows: Vec<serde_json::Value> = self
            .recent_plays
            .iter()
            .map(crate::playback::queue::Queue::to_json)
            .collect();
        let remote: Vec<serde_json::Value> =
            serde_json::from_str(&crate::ytmusic::library::history_json()).unwrap_or_default();
        for row in remote {
            let id = row.get("video_id").and_then(|v| v.as_str());
            let seen = rows.iter().any(|existing| {
                existing.get("video_id").and_then(|v| v.as_str()) == id
            });
            if !seen {
                rows.push(row);
            }
        }
        serde_json::to_string(&rows).unwrap_or_default()
    }

    /// Radio lane toggle (feeds `refill_radio` in the engine).
    #[qslot]
    fn toggle_radio(&mut self) {
        self.radio_on = !self.radio_on;
        self.radio_on_changed();
    }

    /// Autoplay toggle (feeds `refill_radio` in the engine).
    #[qslot]
    fn toggle_autoplay(&mut self) {
        self.autoplay_on = !self.autoplay_on;
        self.autoplay_on_changed();
    }

    /// Seek the progress lane (clamped to duration). Drives the decoder's
    /// ranged stream, not just the label.
    #[qslot]
    fn seek_to(&mut self, pos: i32) {
        if self.playback_state == PlaybackState::Loading.as_str() {
            return;
        }
        let clamped = pos.clamp(0, self.duration.max(0));
        if let Some((_, at)) = self.resume_at.as_mut() {
            *at = clamped as u32;
            self.position = clamped;
            self.position_changed();
            return;
        }
        if clamped != self.position {
            self.audio.seek(clamped as u32);
            self.position = clamped;
            self.position_changed();
        }
    }

    /// One-second position tick (QML `Timer` glue): mirrors the decoder
    /// position and auto-advances when the stream ends (repeat-one replays,
    /// loop-off stops at the end of the lane).
    #[qslot]
    fn tick_position(&mut self) {
        self.merge_liked_load();
        if self.session_rechecking {
            self.poll_session();
        }
        if self.is_playing && self.audio.is_finished() {
            let item = self.queue.advance();
            self.play_or_stop(item);
            return;
        }
        // Restored and not started: nothing is streaming, keep the saved second.
        if self.resume_at.is_some() {
            return;
        }
        let measured = self.audio.position_secs().min(i32::MAX as u32) as i32;
        let next = if self.duration > 0 { measured.min(self.duration) } else { measured };
        if next != self.position {
            self.position = next;
            self.position_changed();
        }
    }

    /// Start `item`, or stop at the end of the lane.
    fn play_or_stop(&mut self, item: Option<QueueItem>) {
        if let Some(item) = item {
            let label = crate::models::format_duration_secs(item.duration_secs);
            self.play_current_live(&item.video_id, &item.title, &item.artist, &label, &item.thumbnail_url);
            return;
        }
        self.position = 0;
        self.audio.stop();
        self.is_playing = false;
        self.playback_state = PlaybackState::Stopped.as_str().to_string();
        self.refresh_queue_view();
        self.position_changed();
        self.is_playing_changed();
        self.playback_state_changed();
        self.next_title_changed();
        self.next_artist_changed();
        self.queue_count_changed();
        self.queue_position_changed();
    }

    /// Poll the background playback load. `"loading"` while the worker runs,
    /// `"ok"` once rodio is playing, `"error"` after a fail-closed failure,
    /// `"idle"` when nothing is pending. QML polls this while
    /// `playbackState === "loading"`.
    #[qslot]
    fn poll_playback(&mut self) -> String {
        let Some(pending) = self.pending_playback.as_ref() else {
            return "idle".to_string();
        };
        let token = pending.token;
        let load = self.playback_load.lock().ok().and_then(|mut slot| {
            slot.iter()
                .position(|load| load.token == token)
                .map(|index| slot.remove(index))
        });
        let Some(load) = load else {
            return "loading".to_string();
        };
        let pending = match self.pending_playback.take() {
            Some(pending) => pending,
            None => return "idle".to_string(),
        };
        if let Some(error) = load.error {
            match error {
                crate::playback::PlaybackError::LoggedOut
                | crate::playback::PlaybackError::UpgradeRequired => {
                    self.apply_upgrade_block(&pending.title, &pending.artist);
                }
                _ => {
                    let msg = playback_error_message(error);
                    self.apply_error_block(&pending.title, &pending.artist, &msg);
                }
            }
            self.emit_playback_props();
            return "error".to_string();
        }
        let Some(ticket) = load.ticket else {
            let msg = crate::playback::PlaybackError::Transport.to_string();
            self.apply_error_block(&pending.title, &pending.artist, &msg);
            self.emit_playback_props();
            return "error".to_string();
        };
        // A logout/gate change while the worker ran must still fail closed.
        if self.apply_gate_block(&pending.title, &pending.artist) {
            self.emit_playback_props();
            return "error".to_string();
        }
        if let Err(error) = self.audio.play_stream(&ticket) {
            let msg = playback_error_message(error);
            self.apply_error_block(&pending.title, &pending.artist, &msg);
            self.emit_playback_props();
            return "error".to_string();
        }
        if std::env::var("SPINWAVE_DEBUG_AUDIO").as_deref() == Ok("1") {
            eprintln!("playback: started {}", pending.video_id);
        }
        let duration = ticket.duration_secs().unwrap_or(pending.row_duration);
        if duration > 0 {
            self.duration = duration.min(i32::MAX as u32) as i32;
        }
        self.position = 0;
        if let Some((id, at)) = self.resume_at.take() {
            if id == pending.video_id && at > 0 {
                self.audio.seek(at);
                self.position = at.min(i32::MAX as u32) as i32;
            }
        }
        self.is_playing = true;
        self.playback_state = PlaybackState::Playing.as_str().to_string();
        self.error_message.clear();
        self.queue.refill(load.refill);
        if let Some(item) = self.queue.current().cloned() {
            if self
                .recent_plays
                .first()
                .map(|first| first.video_id != item.video_id)
                .unwrap_or(true)
            {
                self.recent_plays.insert(0, item);
                self.recent_plays.truncate(50);
            }
        }
        self.refresh_queue_view();
        // Prefetch the next row's ticket on a worker so Next is instant when
        // the queue is list-seeded. Cache is 90 s, memory-only.
        if let Some(next) = self.queue.next_up().cloned() {
            if !next.video_id.is_empty()
                && !self.low_performance
                && crate::playback::cached_ticket(&next.video_id).is_none()
            {
                let gate = self.gate.clone();
                let vid = next.video_id;
                std::thread::spawn(move || {
                    let Some((yt, runtime)) = restored_client() else {
                        return;
                    };
                    let source = crate::playback::LiveUrlSource::new(&yt, &gate);
                    let _ = runtime.block_on(source.fetch_url(&vid));
                });
            }
        }
        self.emit_playback_props();
        "ok".to_string()
    }
}

/// Silent keyring-restore log at launch (redacted): reports presence only,
/// never blob bytes. The QML `restoreSession` slot performs the live probe
/// and drives the session props; this line is the stderr counterpart so the
/// journal shows the launch decision without secrets.
fn maybe_restore_keyring_at_startup_log() {
    let store = crate::auth::KeyringStore;
    match crate::auth::Session::restore(&store) {
        Ok(Some(_)) => eprintln!("session restore: stored session found; probing…"),
        Ok(None) => eprintln!("session restore: no stored session"),
        Err(e) => eprintln!("session restore failed: {e}"),
    }
}

/// Env override for the QML entry point (resolution step (a)).
const QML_DIR_ENV_VAR: &str = "SPINWAVE_QML_DIR";

/// Resolve the QML entry point from disk (never `qrc:`).
///
/// Resolution order:
/// (a) `$SPINWAVE_QML_DIR/main.qml` when the env var is set,
/// (b) executable-relative walk-up — `<exe-dir>/qml`, `<exe-dir>/../qml`,
///     `<exe-dir>/../../qml` (covers installed and `target/debug` layouts),
/// (c) compile-time `CARGO_MANIFEST_DIR/qml/main.qml` (source-tree fallback —
///     a checked-out tree; never relied on in installed builds).
///
/// Fail-fast: panics with the full searched-path list instead of a bare
/// `qrc:/qml/main.qml: No such file or directory` ghost.
fn resolve_qml_file() -> std::path::PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let env_dir = std::env::var(QML_DIR_ENV_VAR)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(std::path::PathBuf::from);
    match find_qml_file(exe_dir.as_deref(), env_dir.as_deref(), env!("CARGO_MANIFEST_DIR")) {
        Ok(path) => path,
        Err(msg) => panic!("{msg}"),
    }
}

/// Pure core of [`resolve_qml_file`]: first existing candidate wins, else a
/// `Result::Err` naming every searched path. Takes its inputs as parameters
/// (instead of reading env/process state) so unit tests run with temp dirs
/// and no Qt.
fn find_qml_file(
    exe_dir: Option<&std::path::Path>,
    env_dir: Option<&std::path::Path>,
    manifest_dir: &str,
) -> Result<std::path::PathBuf, String> {
    let mut searched: Vec<std::path::PathBuf> = Vec::new();
    if let Some(dir) = env_dir {
        searched.push(dir.join("main.qml"));
    }
    if let Some(exe) = exe_dir {
        searched.push(exe.join("qml/main.qml"));
        searched.push(exe.join("../qml/main.qml"));
        searched.push(exe.join("../../qml/main.qml"));
    }
    searched.push(std::path::Path::new(manifest_dir).join("qml/main.qml"));
    for candidate in &searched {
        if candidate.is_file() {
            // Canonicalize so `..` walk-up candidates become clean absolute
            // paths (also safe for the `file://` URL built in `main`).
            let resolved = std::fs::canonicalize(candidate).unwrap_or_else(|_| candidate.clone());
            return Ok(resolved);
        }
    }
    let list = searched
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!(
        "QML main.qml not found; searched: [{list}]. \
Set ${QML_DIR_ENV_VAR} to the qml/ directory or run an installed binary."
    ))
}

/// One-time move of `~/.config/music-player` / `~/.cache/music-player`
/// (pre-rename) to the `spinwave` names, keeping theme, prefs and cache.
fn migrate_legacy_dirs() {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    for (var, fallback) in [("XDG_CONFIG_HOME", ".config"), ("XDG_CACHE_HOME", ".cache")] {
        let base = std::env::var_os(var)
            .map(std::path::PathBuf::from)
            .or_else(|| home.as_ref().map(|h| h.join(fallback)));
        if let Some(base) = base {
            let (old, new) = (base.join("music-player"), base.join("spinwave"));
            if old.is_dir() && !new.exists() {
                let _ = std::fs::rename(old, new);
            }
        }
    }
}

fn main() {
    migrate_legacy_dirs();
    // Low-performance mode: Qt's software renderer instead of the GPU. Must
    // be set before Qt starts, hence "from the next launch". An explicit
    // QT_QUICK_BACKEND from the user wins.
    if crate::prefs::load().low_performance && std::env::var_os("QT_QUICK_BACKEND").is_none() {
        std::env::set_var("QT_QUICK_BACKEND", "software");
    }
    // Silent keyring-restore log (redacted presence only). The live probe +
    // prop updates happen in QML via `Backend.restoreSession()` on startup.
    maybe_restore_keyring_at_startup_log();
    // File-based QML loading (no `qrc:`): the entry point resolves from
    // disk via `resolve_qml_file`, and its directory is added as an import
    // path so SIBLING components (Sidebar, CoverPanel, PlayerCard, …)
    // resolve from the same directory.
    let qml_file = resolve_qml_file();
    let qml_dir = qml_file
        .parent()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    let qml_url = format!("file://{}", qml_file.display());
    // Singleton backends (Backend, Theme) auto-register via LinkMe (see
    // qtbridge QML_REGISTER_CALLBACKS); list models register explicitly.
    QApp::new()
        .register::<TrackModel>()
        .register::<AlbumModel>()
        .register::<ArtistModel>()
        .register::<PlaylistModel>()
        .add_import_path(&qml_dir)
        .load_qml_from_file(&qml_url)
        .run();
}

#[cfg(test)]
mod tests {
    use super::{
        Backend, QueueItem, advance_position, clamp_volume64, format_time, liked_ids_from_playlist,
        progress01, remaining_label,
    };

    #[test]
    fn playback_session_round_trips_paused_at_saved_second() {
        let mut b = Backend::default();
        let rows: Vec<QueueItem> = ["a", "b", "c"]
            .iter()
            .map(|id| {
                let mut item = QueueItem::new(*id, format!("T{id}"), "Art");
                item.duration_secs = 200;
                item
            })
            .collect();
        b.queue.set_from_list(rows, 1);
        b.position = 95;
        b.shuffle = true;
        let saved = b.playback_session_json().expect("resume on by default");

        let mut r = Backend::default();
        assert!(r.apply_saved_playback(&saved));
        assert_eq!((r.title.as_str(), r.position, r.duration), ("Tb", 95, 200));
        assert_eq!(r.playback_state, "paused");
        assert!(r.shuffle && !r.is_playing);
        assert_eq!(r.resume_at, Some(("b".to_string(), 95)));
        assert_eq!(r.queue.position(), 2, "history kept");

        b.resume_position = false;
        assert_eq!(b.playback_session_json().unwrap()["position"], 0);
        b.resume_enabled = false;
        assert!(b.playback_session_json().is_none());
    }

    #[test]
    fn saved_session_needs_recent_known_gate() {
        let now = 10_000_000;
        let ok = serde_json::json!({"gate": "premium", "name": "A", "saved": now - 60});
        assert_eq!(super::saved_session(&ok, now), Some(("premium".into(), "A".into())));
        let old = serde_json::json!({"gate": "premium", "name": "A", "saved": now - 4 * 24 * 3600});
        assert_eq!(super::saved_session(&old, now), None);
        let odd = serde_json::json!({"gate": "loggedOut", "name": "A", "saved": now});
        assert_eq!(super::saved_session(&odd, now), None);
    }

    #[test]
    fn liked_load_seeds_ids_and_keeps_early_toggles() {
        let playlist = serde_json::json!({
            "tracks": [{"videoId": "a"}, {"videoId": "b"}, {"title": "no id"}]
        });
        let ids = liked_ids_from_playlist(playlist.as_object().unwrap());
        assert_eq!(ids.len(), 2);

        let mut b = Backend::default();
        b.liked_gen = 1;
        b.liked_overrides.insert("b".into(), false); // unliked before load
        b.liked_overrides.insert("c".into(), true); // liked before load
        *b.liked_load.lock().unwrap() = Some((1, ids.clone()));
        b.merge_liked_load();
        assert!(b.liked_ids.contains("a") && b.liked_ids.contains("c"));
        assert!(!b.liked_ids.contains("b"));

        // A load from a previous account/session is dropped.
        b.reset_likes();
        *b.liked_load.lock().unwrap() = Some((1, ids));
        b.merge_liked_load();
        assert!(b.liked_ids.is_empty());
    }

    #[test]
    fn defaults_are_explicit_empty_state() {
        // No stale seed: before any live fill the player shows an explicit
        // empty state (empty titles, 0:00, stopped, empty queue) — never a
        // fake track. Current/next seed exclusively from real live rows
        // (row tap via `play_queue`, radio/autoplay refill).
        let b = Backend::default();
        assert!(!b.is_playing, "nothing playing before any live fill");
        assert_eq!(b.title, "");
        assert_eq!(b.artist, "");
        assert_eq!((b.position, b.duration), (0, 0));
        assert!(!b.shuffle, "shuffle off until toggled");
        assert_eq!(b.loop_mode, "off");
        assert!(!b.liked, "no liked state before any live fill");
        assert_eq!(b.next_title, "");
        assert_eq!(b.next_artist, "");
        assert!(b.queue.current().is_none() && b.queue.next_up().is_none());
        assert_eq!(b.queue_count, 0);
        assert_eq!(b.playback_state, "stopped");
    }

    #[test]
    fn live_rows_seed_current_without_mock_data() {
        // Live-shaped rows (fixture mirrors `get_home` shelves) adopt into
        // now-playing + up-next with zero mock ids/titles in the live path.
        let live = crate::models::Track {
            title: "Külüstür Hafıza".to_string(),
            artist: "Deniz".to_string(),
            album: "Nostalji".to_string(),
            video_id: "vid-live-1".to_string(),
            duration: "3:24".to_string(),
            thumbnail_url: "https://i.ytimg.com/vi/vid-live-1/big.jpg".to_string(),
        };
        let mut b = Backend::default();
        let item = QueueItem::from(&live);
        b.queue.set_current(item.clone());
        b.adopt_queue_item(item);
        b.refresh_queue_view();
        assert_eq!((b.title.as_str(), b.artist.as_str()), ("Külüstür Hafıza", "Deniz"));
        assert_eq!(b.duration, 204);
        for s in [&b.title, &b.artist, &b.next_title, &b.next_artist] {
            assert!(!s.contains("Starlight") && !s.contains("Muse"));
            assert!(!s.contains("mock-vid"));
        }
    }

    #[test]
    fn loop_mode_cycles_all_one_off() {
        // Loop cycling now lives in the queue model (`LoopMode::cycle`);
        // the singleton delegates (see `cycle_loop`).
        assert_eq!(crate::playback::queue::LoopMode::All.cycle().as_str(), "one");
        assert_eq!(crate::playback::queue::LoopMode::One.cycle().as_str(), "off");
        assert_eq!(crate::playback::queue::LoopMode::Off.cycle().as_str(), "all");
    }

    #[test]
    fn time_labels_format() {
        assert_eq!(format_time(93), "1:33");
        assert_eq!(format_time(0), "0:00");
        assert_eq!(format_time(-5), "0:00");
        assert_eq!(remaining_label(93, 213), "-2:00");
        assert_eq!(remaining_label(213, 213), "-0:00");
        assert!((progress01(93, 213) - 93.0 / 213.0).abs() < 1e-9);
        assert_eq!(progress01(0, 0), 0.0);
        assert_eq!(progress01(999, 213), 1.0);
    }

    #[test]
    fn gate_blocks_fetch_when_logged_out() {
        // Fail-closed gate smoke: no playback URL without Premium.
        // offline smoke (no playback URL without Premium).
        let gate = crate::premium_gate::PremiumGate::new_logged_out();
        assert!(!gate.can_fetch_playback_url());
    }

    #[test]
    fn blocked_play_never_reaches_session_or_network_offline() {
        // Fail-closed ordering for the play path: logged-out AND
        // upgrade-required gates surface `upgrade` WITHOUT touching the
        // keyring or network (the gate check returns before any worker
        // spawn / `Session::restore`).
        for gate in [
            crate::premium_gate::PremiumGate::new_logged_out(),
            {
                use crate::premium_gate::{Playability, PremiumSignals};
                use std::time::SystemTime;
                let mut g = crate::premium_gate::PremiumGate::new_logged_out();
                g.apply(
                    &PremiumSignals {
                        logged_in: true,
                        home_ok: false,
                        premium_badge: None,
                        explore_top_songs: None,
                        ads_absent: None,
                        topbar_premium: None,
                        hires_audio: None,
                        playability: Playability::Unknown,
                    },
                    SystemTime::now(),
                );
                g
            },
        ] {
            let mut b = Backend::default();
            b.gate = gate;
            assert!(
                b.apply_gate_block("T", "A"),
                "blocked gate must be detected before any worker"
            );
            assert!(
                b.pending_playback.is_none(),
                "blocked gate must not spawn a worker"
            );
            assert_eq!(b.playback_state, "upgrade");
            assert!(!b.is_playing);
            assert_eq!(
                b.error_message, "",
                "upgrade badge covers blocked (no error line)"
            );
        }
    }

    #[test]
    fn upgrade_and_error_blocks_bridge_explicit_states() {
        let mut b = Backend::default();
        b.apply_upgrade_block("T", "A");
        assert_eq!(b.playback_state, "upgrade");
        assert!(!b.is_playing);
        assert_eq!((b.title.as_str(), b.artist.as_str()), ("T", "A"));
        assert_eq!(b.error_message, "");

        b.apply_error_block("T", "A", "playback request failed");
        assert_eq!(b.playback_state, "error");
        assert!(!b.is_playing);
        assert_eq!(b.error_message, "playback request failed");
    }

    #[test]
    fn playback_error_messages_carry_no_urls_or_secrets() {
        use crate::playback::PlaybackError;
        for e in [
            PlaybackError::LoggedOut,
            PlaybackError::UpgradeRequired,
            PlaybackError::Transport,
            PlaybackError::ExpiredUrl,
            PlaybackError::NoTrack,
            PlaybackError::BackendUnavailable,
        ] {
            let msg = super::playback_error_message(e);
            // No media/stream URLs or secrets — the legal ToS/privacy links
            // are REQUIRED on gate blocks (see below), so only stream-shaped
            // URLs and secret keys are banned here.
            assert!(!msg.contains("googlevideo"), "stream URL leaked in {e:?}");
            assert!(!msg.contains("watch?") && !msg.contains("mock.invalid"));
            assert!(!msg.contains("cookie") && !msg.contains("SAPISID"));
            assert!(!msg.to_ascii_lowercase().contains("token"));
            assert!(!msg.is_empty());
        }
        // Gate blocks name Premium + ToS (explicit, actionable).
        let msg = super::playback_error_message(PlaybackError::UpgradeRequired);
        assert!(msg.contains("Premium") && msg.contains(crate::legal::YOUTUBE_TERMS_URL));
    }

    #[test]
    fn search_query_and_error_defaults_are_empty() {
        let b = Backend::default();
        assert_eq!(b.search_query, "");
        assert_eq!(b.error_message, "");
        let mut b = b;
        b.apply_search_query("deniz");
        assert_eq!(b.search_query, "deniz");
    }

    #[test]
    fn legal_surfaces_mention_premium_and_tos() {
        let b = Backend::default();
        let msg = b.upgrade_message();
        assert!(msg.contains("Premium"), "upsell must name Premium");
        assert!(msg.contains(crate::legal::YOUTUBE_TERMS_URL));
        assert_eq!(b.terms_url(), crate::legal::YOUTUBE_TERMS_URL);
        // Privacy link + ToS on every surface.
        assert_eq!(b.privacy_url(), crate::legal::GOOGLE_PRIVACY_URL);
    }

    #[test]
    fn playback_props_default_sane() {
        let b = Backend::default();
        assert_eq!(b.playback_state, "stopped");
        assert_eq!(b.queue_count, 0);
        assert!((b.volume - 0.8).abs() < f64::EPSILON);
        assert!(!b.radio_on);
        assert!(b.autoplay_on);
    }

    #[test]
    fn volume_clamps64() {
        assert_eq!(clamp_volume64(0.8), 0.8);
        assert_eq!(clamp_volume64(9.0), 1.0);
        assert_eq!(clamp_volume64(-2.0), 0.0);
        assert_eq!(clamp_volume64(f64::NAN), 0.0);
    }

    #[test]
    fn position_ticks_only_while_playing() {
        assert_eq!(advance_position(10, 100, true), 11);
        assert_eq!(advance_position(100, 100, true), 100, "clamped at duration");
        assert_eq!(advance_position(10, 100, false), 10, "paused holds");
    }

    #[test]
    fn apply_snapshot_bridges_engine_state() {
        use crate::playback::PlaybackState;
        use crate::playback::player::PlaybackSnapshot;
        let mut b = Backend::default();
        b.apply_snapshot(&PlaybackSnapshot {
            state: PlaybackState::Playing,
            is_playing: true,
            title: "Low Orbit".to_string(),
            artist: "Vela".to_string(),
            position_secs: 12,
            duration_secs: 227,
            next_title: "Honey & Smoke".to_string(),
            next_artist: "June Calloway".to_string(),
            queue_len: 4,
            upgrade_message: None,
        });
        assert!(b.is_playing);
        assert_eq!(b.title, "Low Orbit");
        assert_eq!((b.position, b.duration), (12, 227));
        assert_eq!(b.next_title, "Honey & Smoke");
        assert_eq!(b.playback_state, "playing");
        assert_eq!(b.queue_count, 4);
    }

    #[test]
    fn queue_view_refresh_reveals_following() {
        // Signal-free queue view over explicit live items (no seed):
        // advancing reveals the following item (QML binds
        // `nextTitle`/`queueCount`/`queuePosition`).
        let mut b = Backend::default();
        b.refresh_queue_view();
        assert_eq!(b.next_title, "", "explicit empty before any live fill");
        assert_eq!(b.queue_count, 0, "current + upcoming");

        b.queue.enqueue(QueueItem::new("vid-live-1", "Külüstür Hafıza", "Deniz"));
        b.queue.enqueue(QueueItem::new("vid-live-2", "Neon Tide", "Glass Coast"));
        b.refresh_queue_view();
        assert_eq!(b.next_title, "Külüstür Hafıza");
        assert_eq!(b.next_artist, "Deniz");
        assert_eq!(b.queue_count, 2, "two upcoming");

        b.queue.skip();
        b.refresh_queue_view();
        assert_eq!(b.next_title, "Neon Tide", "following revealed");
        assert_eq!(b.next_artist, "Glass Coast");
        assert_eq!(b.queue_position, 1, "first track of the lane is current");
    }

    #[test]
    fn browser_signin_rejects_invalid_without_secrets() {
        // Invalid input never touches the keyring (validation first) and the
        // rejection carries no secret bytes.
        let mut b = Backend::default();
        for bad in ["", "   ", "not-a-cookie", "SAPISID=old"] {
            let out = b.connect_with_cookie(bad.to_string());
            assert!(out.starts_with("Sign-in failed:"), "rejects: {bad:?}");
            assert!(!b.is_logged_in, "rejected cookie must not log in");
            assert_eq!(b.gate_status, "loggedOut");
        }
        // No native child: polling is a clean idle, not a keyring write.
        assert_eq!(b.poll_browser_signin(), "idle");
        b.cancel_browser_signin();
        assert!(b.browser_login_child.is_none());
    }

    #[test]
    fn session_defaults_logged_out() {
        let b = Backend::default();
        assert!(!b.is_logged_in);
        assert_eq!(b.account_name, "");
        assert_eq!(b.account_label, "Not signed in");
        assert_eq!(b.gate_status, "loggedOut");
    }

    #[test]
    fn gate_status_strings_are_qml_camel() {
        use crate::premium_gate::GateStatus;
        assert_eq!(super::gate_status_str(GateStatus::LoggedOut), "loggedOut");
        assert_eq!(super::gate_status_str(GateStatus::Premium), "premium");
        assert_eq!(
            super::gate_status_str(GateStatus::UpgradeRequired),
            "upgradeRequired"
        );
    }

    #[test]
    fn account_labels_carry_identity_only() {
        use crate::premium_gate::GateStatus;
        assert_eq!(
            super::account_label_for(false, "", GateStatus::LoggedOut),
            "Not signed in"
        );
        assert_eq!(
            super::account_label_for(true, "", GateStatus::Premium),
            "Signed in • Premium"
        );
        assert_eq!(
            super::account_label_for(true, "", GateStatus::UpgradeRequired),
            "Signed in • Upgrade required"
        );
        assert_eq!(
            super::account_label_for(true, "Ada", GateStatus::Premium),
            "Ada • Premium"
        );
        // LoggedOut status always reads logged-out even with a name cached.
        assert_eq!(
            super::account_label_for(true, "Ada", GateStatus::LoggedOut),
            "Not signed in"
        );
    }

    #[test]
    fn session_props_reflect_gate_outcomes_offline() {
        use crate::premium_gate::{GateStatus, Playability, PremiumGate, PremiumSignals};
        use std::time::SystemTime;
        // Premium signals → premium props.
        let premium = PremiumSignals {
            logged_in: true,
            home_ok: true,
            premium_badge: Some(true),
            explore_top_songs: None,
            ads_absent: None,
            topbar_premium: None,
            hires_audio: None,
            playability: Playability::Playable,
        };
        let mut gate = PremiumGate::new_logged_out();
        gate.apply(&premium, SystemTime::now());
        let mut b = Backend::default();
        b.apply_session_state(true, "", gate.status());
        assert!(b.is_logged_in);
        assert_eq!(b.gate_status, "premium");
        assert_eq!(b.account_label, "Signed in • Premium");
        assert_eq!(gate.status(), GateStatus::Premium);

        // Ambiguous signals → UpgradeRequired props (fail-closed, never Premium).
        let ambiguous = PremiumSignals {
            logged_in: true,
            home_ok: false,
            premium_badge: None,
            explore_top_songs: None,
            ads_absent: None,
            topbar_premium: None,
            hires_audio: None,
            playability: Playability::Unknown,
        };
        let mut gate = PremiumGate::new_logged_out();
        gate.apply(&ambiguous, SystemTime::now());
        assert_eq!(gate.status(), GateStatus::UpgradeRequired);
        b.apply_session_state(true, "", gate.status());
        assert!(b.is_logged_in, "authenticated but blocked stays logged-in");
        assert_eq!(b.gate_status, "upgradeRequired");
        assert_eq!(b.account_label, "Signed in • Upgrade required");

        // Logged-out signals → logged-out props.
        b.apply_session_state(false, "", GateStatus::LoggedOut);
        assert!(!b.is_logged_in);
        assert_eq!(b.gate_status, "loggedOut");
        assert_eq!(b.account_label, "Not signed in");
    }

    #[test]
    fn restore_persist_logout_roundtrip_offline() {
        use crate::auth::{MemorySecretStore, SecretStore, Session};
        let mut store = MemorySecretStore::default();
        // Persist via the validated cookie-header lane.
        let (session, _) = crate::auth::login_from_cookie_header(
            &mut store,
            "VISITOR_INFO1_LIVE=abc; __Secure-3PAPISID=fake-test-sapisid",
        )
        .expect("valid cookie header");
        assert!(session.is_logged_in());
        assert!(store.load_cookie().unwrap().is_some());

        // Restore marks logged-in with a fail-closed placeholder gate.
        let mut b = Backend::default();
        assert!(b.apply_restore_offline(&store));
        assert!(b.is_logged_in);
        assert_eq!(b.gate_status, "upgradeRequired", "unprobed restore blocks");

        // Logout clears the store and resets props.
        let session = Session::logout(&mut store).expect("logout");
        assert!(!session.is_logged_in());
        assert_eq!(store.load_cookie().unwrap(), None);
        b.apply_logged_out();
        assert!(!b.is_logged_in);
        assert_eq!(b.gate_status, "loggedOut");
        assert_eq!(b.account_label, "Not signed in");
    }

    #[test]
    fn failed_restore_stays_logged_out_offline() {
        use crate::auth::{MemorySecretStore, SecretStore};
        // Empty store → logged-out.
        let store = MemorySecretStore::default();
        let mut b = Backend::default();
        assert!(!b.apply_restore_offline(&store));
        assert!(!b.is_logged_in);
        assert_eq!(b.gate_status, "loggedOut");

        // Corrupt blob → restore fails → stays logged-out.
        let mut bad = MemorySecretStore::default();
        bad.save_cookie(r#"{"origin": "x"}"#).unwrap();
        let mut b = Backend::default();
        // Seed logged-in to prove failure resets (fail-closed).
        b.apply_session_state(
            true,
            "",
            crate::premium_gate::GateStatus::UpgradeRequired,
        );
        assert!(!b.apply_restore_offline(&bad));
        assert!(!b.is_logged_in, "failed restore must log out");
        assert_eq!(b.gate_status, "loggedOut");
        assert_eq!(b.account_label, "Not signed in");
    }

    #[test]
    fn session_props_carry_no_secrets() {
        use crate::premium_gate::GateStatus;
        const SECRET: &str = "fake-test-sapisid";
        let mut b = Backend::default();
        // Even a hostile account name must not echo raw cookie bytes; the
        // label only echoes the name, and status strings are fixed.
        b.apply_session_state(true, "Ada", GateStatus::Premium);
        for s in [&b.account_name, &b.account_label, &b.gate_status] {
            assert!(!s.contains(SECRET), "secret in session prop: {s}");
            assert!(!s.to_ascii_lowercase().contains("sapisid"));
        }
        b.apply_logged_out();
        for s in [&b.account_name, &b.account_label, &b.gate_status] {
            assert!(!s.contains(SECRET));
        }
        // Reports feeding the slots stay redacted (covered in auth lane).
        let out = b.connect_with_cookie(String::new());
        assert!(!out.contains(SECRET));
    }

    /// Unique scratch dir per test (no new crates; std only). Caller creates
    /// `main.qml` files inside it; removed best-effort at the end.
    fn qml_scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "spinwave_qml_{}_{}_{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn touch_main_qml(dir: &std::path::Path) {
        std::fs::write(dir.join("main.qml"), "import QtQuick\nItem {}\n").expect("main.qml");
    }

    #[test]
    fn qml_resolver_prefers_env_dir() {
        let scratch = qml_scratch("env");
        let qml_dir = scratch.join("custom");
        std::fs::create_dir_all(&qml_dir).expect("custom qml dir");
        touch_main_qml(&qml_dir);
        let found =
            super::find_qml_file(None, Some(&qml_dir), "/nonexistent-manifest").expect("env hit");
        assert_eq!(found, qml_dir.join("main.qml"));
        std::fs::remove_dir_all(&scratch).ok();
    }

    #[test]
    fn qml_resolver_finds_exe_relative_layouts() {
        // Installed layout: <exe-dir>/qml/main.qml.
        let scratch = qml_scratch("exe");
        let exe_dir = scratch.join("bin");
        let qml_dir = exe_dir.join("qml");
        std::fs::create_dir_all(&qml_dir).expect("exe qml dir");
        touch_main_qml(&qml_dir);
        let found = super::find_qml_file(Some(&exe_dir), None, "/nonexistent-manifest")
            .expect("exe-dir hit");
        assert_eq!(found, qml_dir.join("main.qml"));

        // target/debug layout: binary two levels below the root
        // (<exe-dir>/../../qml/main.qml).
        let root = scratch.join("root");
        let deep_exe = root.join("target/debug");
        std::fs::create_dir_all(&deep_exe).expect("deep exe dir");
        let root_qml = root.join("qml");
        std::fs::create_dir_all(&root_qml).expect("root qml dir");
        touch_main_qml(&root_qml);
        let found = super::find_qml_file(Some(&deep_exe), None, "/nonexistent-manifest")
            .expect("walk-up hit");
        assert_eq!(found, root_qml.join("main.qml"));
        std::fs::remove_dir_all(&scratch).ok();
    }

    #[test]
    fn qml_resolver_falls_back_to_manifest_dir() {
        // Source-tree fallback: CARGO_MANIFEST_DIR/qml/main.qml ships in the
        // checked-out tree, so this must resolve in the test harness.
        let found = super::find_qml_file(None, None, env!("CARGO_MANIFEST_DIR"))
            .expect("manifest fallback");
        assert!(found.is_file());
        assert!(found.ends_with("qml/main.qml"));
    }

    #[test]
    fn qml_resolver_missing_names_searched_paths() {
        let err = super::find_qml_file(None, None, "/nonexistent-manifest")
            .expect_err("must fail with no candidates on disk");
        assert!(err.contains("QML main.qml not found"), "fail-fast message");
        assert!(err.contains("/nonexistent-manifest"), "names searched paths");
        assert!(
            err.contains(super::QML_DIR_ENV_VAR),
            "points at the env override"
        );
        assert!(!err.contains("qrc:"), "never a bare qrc ghost");
    }

    /// Offline QML<->Rust contract guard (no Qt): every `Backend.<name>`
    /// reference in qml/ must resolve to a `#[qslot]` (calls with `(`) or a
    /// `qproperty!` (prop reads) exposed camelCase via `ConvertToCamelCase`,
    /// plus QML-side bans for this lane's engine-warning class (bad
    /// `Gradient` orientation, bare `index` delegates). Catches
    /// TypeError/ReferenceError/Unable-to-assign offline; the offscreen
    /// smoke grep-gate covers runtime drift.
    #[test]
    fn qml_backend_contract_resolves_offline() {
        fn snake_to_camel(input: &str) -> String {
            let mut out = String::new();
            for tok in input.split('_') {
                let mut chars = tok.chars();
                if let Some(first) = chars.next() {
                    if out.is_empty() {
                        out.push(first.to_ascii_lowercase());
                    } else {
                        out.push(first.to_ascii_uppercase());
                    }
                    out.push_str(chars.as_str());
                }
            }
            out
        }

        fn is_ident_char(c: char) -> bool {
            c.is_ascii_alphanumeric() || c == '_'
        }

        let manifest = env!("CARGO_MANIFEST_DIR");
        let src =
            std::fs::read_to_string(format!("{manifest}/src/main.rs")).expect("read src/main.rs");

        // The camelCase mapping below only holds with the macro flag.
        let backend_decl = src
            .lines()
            .find(|l| l.trim_start().starts_with("#[qobject"))
            .expect("Backend #[qobject] decl");
        assert!(
            backend_decl.contains("ConvertToCamelCase"),
            "Backend must opt into ConvertToCamelCase so QML camelCase calls resolve"
        );

        // Collect #[qslot] fn names (tolerates doc/attr lines between).
        let mut slots: Vec<String> = Vec::new();
        let mut pending_slot = false;
        for line in src.lines() {
            let t = line.trim();
            if t.starts_with("#[qslot") {
                pending_slot = true;
                continue;
            }
            if pending_slot {
                if let Some(rest) = t.strip_prefix("fn ") {
                    let name: String = rest
                        .chars()
                        .take_while(|c| *c != '(' && !c.is_whitespace())
                        .collect();
                    assert!(!name.is_empty(), "unparseable slot fn line: {t}");
                    slots.push(snake_to_camel(&name));
                    pending_slot = false;
                } else if t.is_empty()
                    || t.starts_with("///")
                    || t.starts_with("//")
                    || t.starts_with("#[")
                {
                    continue;
                } else {
                    pending_slot = false;
                }
            }
        }
        assert!(slots.contains(&"tickPosition".to_string()), "guard sanity");
        assert!(slots.contains(&"upgradeMessage".to_string()), "guard sanity");
        assert!(slots.contains(&"termsUrl".to_string()), "guard sanity");
        // Session lane: restore/logout + the browser sign-in lane must stay
        // exposed to QML.
        assert!(slots.contains(&"restoreSession".to_string()), "guard sanity");
        assert!(slots.contains(&"logout".to_string()), "guard sanity");
        assert!(slots.contains(&"startBrowserSignin".to_string()), "guard sanity");
        assert!(slots.contains(&"pollBrowserSignin".to_string()), "guard sanity");
        assert!(slots.contains(&"finishBrowserSignin".to_string()), "guard sanity");
        assert!(slots.contains(&"cancelBrowserSignin".to_string()), "guard sanity");
        assert!(slots.contains(&"connectWithCookie".to_string()), "guard sanity");
        assert!(slots.contains(&"ytmusicUrl".to_string()), "guard sanity");

        // Collect qproperty! camelCase names (first string literal per line).
        let mut props: Vec<String> = Vec::new();
        for line in src.lines() {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("qproperty!") {
                // Form: qproperty!("camelName", Member = ..., Notify = ...);
                if let Some(start) = rest.find('"') {
                    let after = &rest[start + 1..];
                    if let Some(end) = after.find('"') {
                        props.push(after[..end].to_string());
                    }
                }
            }
        }
        assert!(props.contains(&"isLoggedIn".to_string()), "guard sanity");
        assert!(props.contains(&"gateStatus".to_string()), "guard sanity");
        assert!(props.contains(&"accountName".to_string()), "guard sanity");
        assert!(props.contains(&"accountLabel".to_string()), "guard sanity");

        // Every Backend.<name>( call in qml/ must name a slot.
        let qml_dir = std::path::Path::new(manifest).join("qml");
        let mut qml_files: Vec<std::path::PathBuf> = std::fs::read_dir(&qml_dir)
            .expect("read qml/")
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "qml"))
            .collect();
        qml_files.sort();
        assert!(!qml_files.is_empty(), "qml/ must contain .qml files");
        let mut calls: Vec<(String, String)> = Vec::new();
        for path in &qml_files {
            let text = std::fs::read_to_string(path).expect("read qml file");
            let file = path
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .into_owned();
            for (lineno, line) in text.lines().enumerate() {
                let mut rest = line;
                while let Some(pos) = rest.find("Backend.") {
                    let after = &rest[pos + "Backend.".len()..];
                    let name: String =
                        after.chars().take_while(|c| is_ident_char(*c)).collect();
                    let tail = after[name.len()..].trim_start();
                    if !name.is_empty() && tail.starts_with('(') {
                        calls.push((name, format!("{file}:{}", lineno + 1)));
                    }
                    rest = after;
                }
            }
        }
        assert!(!calls.is_empty(), "guard sanity: expected Backend calls");
        for (name, loc) in &calls {
            assert!(
                slots.contains(name),
                "QML call Backend.{name}( at {loc} resolves to no #[qslot]"
            );
        }

        // Every Backend.<ident> prop read in qml/ must name a slot or prop.
        let mut reads: Vec<(String, String)> = Vec::new();
        for path in &qml_files {
            let text = std::fs::read_to_string(path).expect("read qml file");
            let file = path
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .into_owned();
            for (lineno, line) in text.lines().enumerate() {
                let mut rest = line;
                while let Some(pos) = rest.find("Backend.") {
                    let after = &rest[pos + "Backend.".len()..];
                    let name: String =
                        after.chars().take_while(|c| is_ident_char(*c)).collect();
                    if !name.is_empty() {
                        reads.push((name, format!("{file}:{}", lineno + 1)));
                    }
                    rest = after;
                }
            }
        }
        assert!(!reads.is_empty(), "guard sanity: expected Backend reads");
        for (name, loc) in &reads {
            assert!(
                slots.contains(name) || props.contains(name),
                "QML read Backend.{name} at {loc} resolves to no #[qslot]/qproperty"
            );
        }

        // Model receivers (tracks/albums/artists) resolve against
        // src/models/mod.rs slots; their #[qobject]s must also opt into
        // ConvertToCamelCase (same bug class as Backend.loadHome).
        let models_src = std::fs::read_to_string(format!("{manifest}/src/models/mod.rs"))
            .expect("read src/models/mod.rs");
        let model_decls: Vec<&str> = models_src
            .lines()
            .filter(|l| l.trim_start().starts_with("#[qobject"))
            .collect();
        assert!(
            !model_decls.is_empty(),
            "expected #[qobject] decls in src/models/mod.rs"
        );
        for decl in &model_decls {
            assert!(
                decl.contains("ConvertToCamelCase"),
                "model {decl} must opt into ConvertToCamelCase"
            );
        }
        let mut model_slots: Vec<String> = Vec::new();
        let mut pending_model_slot = false;
        for line in models_src.lines() {
            let t = line.trim();
            if t.starts_with("#[qslot") {
                pending_model_slot = true;
                continue;
            }
            if pending_model_slot {
                if let Some(rest) = t.strip_prefix("fn ") {
                    let name: String = rest
                        .chars()
                        .take_while(|c| *c != '(' && !c.is_whitespace())
                        .collect();
                    assert!(!name.is_empty(), "unparseable model slot fn: {t}");
                    model_slots.push(snake_to_camel(&name));
                    pending_model_slot = false;
                } else if t.is_empty()
                    || t.starts_with("///")
                    || t.starts_with("//")
                    || t.starts_with("#[")
                {
                    continue;
                } else {
                    pending_model_slot = false;
                }
            }
        }
        assert!(
            model_slots.contains(&"loadHome".to_string()),
            "guard sanity: expected loadHome model slot"
        );
        // QAbstractItemModel's own invokable: every list model has it.
        model_slots.push("rowCount".to_string());
        for receiver in ["tracks.", "albums.", "artists."] {
            for path in &qml_files {
                let text = std::fs::read_to_string(path).expect("read qml file");
                let file = path
                    .file_name()
                    .expect("file name")
                    .to_string_lossy()
                    .into_owned();
                for (lineno, line) in text.lines().enumerate() {
                    let mut rest = line;
                    while let Some(pos) = rest.find(receiver) {
                        let after = &rest[pos + receiver.len()..];
                        let name: String =
                            after.chars().take_while(|c| is_ident_char(*c)).collect();
                        let tail = after[name.len()..].trim_start();
                        if !name.is_empty() && tail.starts_with('(') {
                            assert!(
                                model_slots.contains(&name),
                                "QML call {receiver}{name}( at {file}:{} resolves to no model #[qslot]",
                                lineno + 1
                            );
                        }
                        rest = after;
                    }
                }
            }
        }

        // Engine-warning class bans: bad Gradient orientation + bare index.
        for path in &qml_files {
            let text = std::fs::read_to_string(path).expect("read qml file");
            let file = path
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .into_owned();
            assert!(
                !text.contains("Gradient.Diagonal"),
                "{file} uses invalid Gradient.Diagonal (Vertical/Horizontal only)"
            );
        }
        let track_row =
            std::fs::read_to_string(qml_dir.join("TrackRow.qml")).expect("read TrackRow.qml");
        assert!(
            track_row.contains("required property int index"),
            "TrackRow must capture the view index via `required property int index` \
             (Qt6 hides bare `index` once a delegate has required properties)"
        );
    }
}
