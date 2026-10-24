//! Playback queue model (P3).
//!
//! Handover notes:
//! - Pure queue logic: current / next-up / upcoming, [`LoopMode`]
//!   off-all-one, shuffle, history-backed prev, and manual [`Queue::skip`]
//!   (dequeues the next-up item once, revealing the following item).
//! - Radio/autoplay refill lands here via [`Queue::refill`]: callers feed
//!   `get_watch_playlist` rows and only unseen `video_id`s are appended.
//! - No network, no media bytes, no URLs here — items carry metadata
//!   (`video_id` for InnerTube lookups) only. All covered by offline tests.
//!
//! P4 gaps: persist queue across restarts (library/polish); surface the
//! queue into the QML popup model from the controller snapshot.

use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

/// One queue entry: metadata only (never a media URL or media bytes).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QueueItem {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: u32,
    /// Cover art URL for the now-playing surfaces (metadata only).
    pub thumbnail_url: String,
}

impl QueueItem {
    /// Minimal constructor for tests / controller plumbing.
    pub fn new(
        video_id: impl Into<String>,
        title: impl Into<String>,
        artist: impl Into<String>,
    ) -> Self {
        Self {
            video_id: video_id.into(),
            title: title.into(),
            artist: artist.into(),
            album: String::new(),
            duration_secs: 0,
            thumbnail_url: String::new(),
        }
    }

    /// Build from a `ytmusic-rs`-shaped JSON dict. Returns `None` when there
    /// is no `videoId` (fail-safe: unplayable rows never enter the queue).
    pub fn from_json(v: &serde_json::Value) -> Option<Self> {
        let video_id = v
            .get("videoId")
            .or_else(|| v.get("video_id"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if video_id.is_empty() {
            return None;
        }
        let artist = v
            .get("artist")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                v.get("artists")
                    .and_then(|x| x.as_array())
                    .and_then(|xs| xs.first())
                    .and_then(|a| {
                        a.get("name").and_then(serde_json::Value::as_str)
                    })
                    .map(str::to_string)
            })
            .unwrap_or_default();
        let duration_secs = v
            .get("durationSeconds")
            .and_then(serde_json::Value::as_u64)
            .map(|s| s.min(u64::from(u32::MAX)) as u32)
            .or_else(|| {
                v.get("duration")
                    .or_else(|| v.get("length"))
                    .and_then(serde_json::Value::as_str)
                    .map(parse_duration_label)
            })
            .unwrap_or(0);
        Some(Self {
            video_id: video_id.to_string(),
            title: v
                .get("title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            artist,
            // Our own rows carry a plain string (ytmusic dicts nest it).
            album: v
                .get("album")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            duration_secs,
            thumbnail_url: crate::models::thumbnail_url_from_json(v),
        })
    }
}

impl From<&crate::models::Track> for QueueItem {
    fn from(t: &crate::models::Track) -> Self {
        Self {
            video_id: t.video_id.clone(),
            title: t.title.clone(),
            artist: t.artist.clone(),
            album: t.album.clone(),
            duration_secs: parse_duration_label(&t.duration),
            thumbnail_url: t.thumbnail_url.clone(),
        }
    }
}

/// Parse `m:ss` / `h:mm:ss` labels into seconds. Garbage becomes 0
/// (fail-safe: unknown length never blocks the queue). Pure, unit-tested.
pub fn parse_duration_label(s: &str) -> u32 {
    let s = s.trim();
    if s.is_empty() {
        return 0;
    }
    let mut total: u32 = 0;
    for part in s.split(':') {
        let n: u32 = match part.trim().parse() {
            Ok(n) => n,
            Err(_) => return 0,
        };
        total = total.saturating_mul(60).saturating_add(n);
    }
    total
}

/// Loop mode. QML strings are `"off" | "all" | "one"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopMode {
    #[default]
    Off,
    All,
    One,
}

impl LoopMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::All => "all",
            Self::One => "one",
        }
    }

    /// Inverse of [`LoopMode::as_str`].
    pub fn from_label(label: &str) -> Option<Self> {
        [Self::Off, Self::All, Self::One].into_iter().find(|m| m.as_str() == label)
    }

    /// Next mode: off -> all -> one -> off.
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }
}

/// Playback queue: current track, upcoming lane, play history, shuffle and
/// loop state. Pure and fully unit-testable offline.
pub struct Queue {
    current: Option<QueueItem>,
    upcoming: VecDeque<QueueItem>,
    history: Vec<QueueItem>,
    shuffle: bool,
    loop_mode: LoopMode,
    rng: u64,
}

impl Default for Queue {
    fn default() -> Self {
        Self::new()
    }
}

impl Queue {
    pub fn new() -> Self {
        // Non-zero xorshift seed (time-derived; tests assert set membership,
        // never order, so this stays deterministic-safe).
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64)
            .unwrap_or(0x9E37_79B9)
            | 1;
        Self {
            current: None,
            upcoming: VecDeque::new(),
            history: Vec::new(),
            shuffle: false,
            loop_mode: LoopMode::Off,
            rng: seed,
        }
    }

    /// Currently playing item, if any.
    pub fn current(&self) -> Option<&QueueItem> {
        self.current.as_ref()
    }

    /// Next-up preview (the item Next would play, shuffle aside).
    pub fn next_up(&self) -> Option<&QueueItem> {
        self.upcoming.front()
    }

    /// Upcoming lane (cloned preview for snapshots).
    pub fn upcoming_list(&self) -> Vec<QueueItem> {
        self.upcoming.iter().cloned().collect()
    }

    /// Replace the queue with `items`, making `index` current. Duplicate
    /// video ids are dropped, history is the rows before the pick, and the
    /// upcoming lane is the rows after it — the list the user clicked is the
    /// queue (no radio inference).
    pub fn set_from_list(&mut self, items: Vec<QueueItem>, index: usize) {
        let Some(chosen) = items.get(index).cloned() else {
            self.clear();
            return;
        };
        let mut seen = std::collections::HashSet::new();
        let mut unique: Vec<QueueItem> = Vec::new();
        for item in items {
            if item.video_id.is_empty() || !seen.insert(item.video_id.clone()) {
                continue;
            }
            unique.push(item);
        }
        let pos = unique
            .iter()
            .position(|item| item.video_id == chosen.video_id)
            .unwrap_or(0);
        self.history = unique[..pos].to_vec();
        self.current = Some(unique[pos].clone());
        self.upcoming = unique[pos + 1..].iter().cloned().collect();
    }

    /// Remove `video_id` from the upcoming lane and return it (queue popup
    /// row tap). No current/history change.
    pub fn take_upcoming(&mut self, video_id: &str) -> Option<QueueItem> {
        let index = self
            .upcoming
            .iter()
            .position(|item| item.video_id == video_id)?;
        self.upcoming.remove(index)
    }

    /// Metadata-only JSON for one queue row (no URLs/secrets). Used to fill
    /// the queue popup's model.
    pub fn to_json(item: &QueueItem) -> serde_json::Value {
        serde_json::json!({
            "video_id": item.video_id,
            "title": item.title,
            "artist": item.artist,
            "album": item.album,
            "duration": crate::models::format_duration_secs(item.duration_secs),
            "thumbnail_url": item.thumbnail_url,
        })
    }

    /// Play order as one list (history, current, upcoming) plus the current
    /// index, for saving; [`Queue::set_from_list`] restores it.
    pub fn snapshot(&self) -> Option<(Vec<QueueItem>, usize)> {
        let current = self.current.clone()?;
        let mut items = self.history.clone();
        let index = items.len();
        items.push(current);
        items.extend(self.upcoming.iter().cloned());
        Some((items, index))
    }

    /// Current + upcoming count (for the `queueCount` QML prop).
    pub fn len(&self) -> usize {
        usize::from(self.current.is_some()) + self.upcoming.len()
    }

    /// 1-based position of the current item in play order (0 = nothing
    /// current). Total tracks = `position() - 1 + len()`.
    pub fn position(&self) -> usize {
        if self.current.is_some() { self.history.len() + 1 } else { 0 }
    }

    pub fn set_shuffle(&mut self, shuffle: bool) {
        self.shuffle = shuffle;
    }

    pub fn set_loop_mode(&mut self, mode: LoopMode) {
        self.loop_mode = mode;
    }

    pub fn cycle_loop(&mut self) -> LoopMode {
        self.loop_mode = self.loop_mode.cycle();
        self.loop_mode
    }

    #[cfg(test)]
    /// Append one item to the upcoming lane.
    pub fn enqueue(&mut self, item: QueueItem) {
        self.upcoming.push_back(item);
    }

    /// Make `item` current, pushing the previous current into history.
    pub fn set_current(&mut self, item: QueueItem) {
        if let Some(cur) = self.current.take() {
            self.history.push(cur);
        }
        self.current = Some(item);
    }

    /// Advance to the next track:
    /// - `One` replays the current item (or starts the lane when empty).
    /// - otherwise the next upcoming item plays (random pick when shuffle
    ///   is on); the old current moves into history.
    /// - exhausted lane + `All` restarts from the beginning (history order);
    ///   exhausted lane + `Off` clears current and returns `None`.
    pub fn advance(&mut self) -> Option<QueueItem> {
        if self.loop_mode == LoopMode::One && self.current.is_some() {
            return self.current.clone();
        }
        self.skip()
    }

    /// Manual Next: like [`Queue::advance`] but never replays under `One`
    /// (the user asked for a different track).
    pub fn skip(&mut self) -> Option<QueueItem> {
        // A shuffle pick rotates the lane so the rows after it come next
        // (and the skipped-over rows wrap to the end): turning shuffle off
        // continues from the picked song, not from the original front.
        if self.shuffle && !self.upcoming.is_empty() {
            let i = self.rand_below(self.upcoming.len());
            self.upcoming.rotate_left(i);
        }
        let next = self.upcoming.pop_front();
        match next {
            Some(item) => {
                if let Some(cur) = self.current.take() {
                    self.history.push(cur);
                }
                self.current = Some(item.clone());
                Some(item)
            }
            None => {
                if self.loop_mode == LoopMode::All
                    && (!self.history.is_empty() || self.current.is_some())
                {
                    self.restart_all();
                    if let Some(item) = self.upcoming.pop_front() {
                        self.current = Some(item.clone());
                        return Some(item);
                    }
                }
                if let Some(cur) = self.current.take() {
                    self.history.push(cur);
                }
                None
            }
        }
    }

    /// Previous track: restore the most recent history item, parking the
    /// current item at the front of the upcoming lane. `None` when history
    /// is empty (caller restarts the current track instead).
    pub fn prev(&mut self) -> Option<QueueItem> {
        let prev = self.history.pop()?;
        if let Some(cur) = self.current.take() {
            self.upcoming.push_front(cur);
        }
        self.current = Some(prev.clone());
        Some(prev)
    }

    /// Radio/autoplay refill: append `tracks`, skipping `video_id`s already
    /// in current / history / upcoming. Returns the number added.
    pub fn refill(&mut self, tracks: Vec<QueueItem>) -> usize {
        let mut added = 0;
        for t in tracks {
            if t.video_id.is_empty() || self.contains(&t.video_id) {
                continue;
            }
            self.upcoming.push_back(t);
            added += 1;
        }
        added
    }

    /// Drop everything (stop state).
    pub fn clear(&mut self) {
        self.current = None;
        self.upcoming.clear();
        self.history.clear();
    }

    fn contains(&self, video_id: &str) -> bool {
        self.current
            .as_ref()
            .is_some_and(|c| c.video_id == video_id)
            || self.history.iter().any(|h| h.video_id == video_id)
            || self.upcoming.iter().any(|u| u.video_id == video_id)
    }

    /// Loop-all restart: history (+ current) back into the upcoming lane in
    /// original play order.
    fn restart_all(&mut self) {
        let mut items = std::mem::take(&mut self.history);
        if let Some(cur) = self.current.take() {
            items.push(cur);
        }
        self.upcoming = items.into();
    }

    fn rand_below(&mut self, n: usize) -> usize {
        debug_assert!(n > 0);
        // xorshift64* (no extra deps for one shuffle pick).
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        let scaled = (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as usize;
        scaled % n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> QueueItem {
        QueueItem::new(id, format!("title-{id}"), format!("artist-{id}"))
    }

    fn queued(ids: &[&str]) -> Queue {
        let mut q = Queue::new();
        for id in ids {
            q.enqueue(item(id));
        }
        q
    }

    #[test]
    fn advance_plays_in_order() {
        let mut q = queued(&["a", "b", "c"]);
        assert_eq!(q.advance().as_ref().map(|t| t.video_id.as_str()), Some("a"));
        assert_eq!(q.advance().as_ref().map(|t| t.video_id.as_str()), Some("b"));
        assert_eq!(q.current().map(|t| t.video_id.as_str()), Some("b"));
        assert_eq!(q.next_up().map(|t| t.video_id.as_str()), Some("c"));
        assert_eq!(q.len(), 2, "current + one upcoming");
    }

    #[test]
    fn loop_off_ends_queue() {
        let mut q = queued(&["a"]);
        assert!(q.advance().is_some());
        assert!(q.advance().is_none(), "exhausted lane ends");
        assert!(q.current().is_none());
        assert_eq!(q.position(), 0);
    }

    #[test]
    fn loop_all_restarts_from_beginning() {
        let mut q = queued(&["a", "b"]);
        q.set_loop_mode(LoopMode::All);
        assert_eq!(q.advance().unwrap().video_id, "a");
        assert_eq!(q.advance().unwrap().video_id, "b");
        // Lane exhausted -> restart from the beginning.
        assert_eq!(q.advance().unwrap().video_id, "a");
        assert_eq!(q.current().unwrap().video_id, "a");
    }

    #[test]
    fn loop_one_replays_current() {
        let mut q = queued(&["a", "b"]);
        q.set_loop_mode(LoopMode::One);
        assert_eq!(q.advance().unwrap().video_id, "a");
        assert_eq!(q.advance().unwrap().video_id, "a", "one replays");
        assert_eq!(q.advance().unwrap().video_id, "a");
        assert_eq!(q.next_up().unwrap().video_id, "b", "lane untouched");
        assert_eq!(q.skip().unwrap().video_id, "b", "manual next leaves one-loop");
    }

    #[test]
    fn loop_mode_cycles_off_all_one() {
        assert_eq!(LoopMode::Off.cycle(), LoopMode::All);
        assert_eq!(LoopMode::All.cycle(), LoopMode::One);
        assert_eq!(LoopMode::One.cycle(), LoopMode::Off);
    }

    #[test]
    fn shuffle_preserves_set() {
        let mut q = queued(&["a", "b", "c", "d", "e"]);
        q.set_shuffle(true);
        let mut played = Vec::new();
        while let Some(t) = q.advance() {
            played.push(t.video_id.clone());
        }
        played.sort_unstable();
        assert_eq!(played, vec!["a", "b", "c", "d", "e"]);
    }

    #[test]
    fn shuffle_off_continues_after_the_shuffled_pick() {
        let ids = ["a", "b", "c", "d", "e", "f"];
        let mut q = queued(&ids);
        q.skip(); // "a" current, b..f upcoming
        q.set_shuffle(true);
        let picked = q.skip().expect("shuffled pick").video_id;
        q.set_shuffle(false);
        let next = q.skip().expect("next in order").video_id;
        let at = ids.iter().position(|id| *id == picked).unwrap();
        // Wraps past the end to the earliest unplayed row.
        let expected = ids[at + 1..]
            .iter()
            .chain(ids[1..at].iter())
            .next()
            .unwrap();
        assert_eq!(next, *expected, "picked {picked}");
    }

    #[test]
    fn prev_restores_history() {
        let mut q = queued(&["b"]);
        q.set_current(item("a"));
        q.advance();
        assert_eq!(q.current().unwrap().video_id, "b");
        assert_eq!(q.prev().unwrap().video_id, "a");
        assert_eq!(q.current().unwrap().video_id, "a");

        let mut fresh = queued(&["a"]);
        assert!(fresh.prev().is_none(), "no history restarts instead");
    }

    #[test]
    fn refill_appends_unseen_only() {
        let mut q = queued(&["b"]);
        q.set_current(item("a"));
        let added = q.refill(vec![item("a"), item("b"), item("c"), item("d")]);
        assert_eq!(added, 2, "a (current) + b (upcoming) deduped");
        assert_eq!(q.next_up().unwrap().video_id, "b");
    }

    #[test]
    fn from_json_filters_missing_video_id() {
        let good = serde_json::json!({
            "videoId": "vid-1",
            "title": "T",
            "artists": [{"name": "Ar"}],
            "duration": "3:24",
        });
        let t = QueueItem::from_json(&good).expect("parses");
        assert_eq!(t.artist, "Ar");
        assert_eq!(t.duration_secs, 204);
        assert!(QueueItem::from_json(&serde_json::json!({"title": "x"})).is_none());
    }

    #[test]
    fn duration_labels_parse() {
        assert_eq!(parse_duration_label("3:24"), 204);
        assert_eq!(parse_duration_label("1:02:03"), 3723);
        assert_eq!(parse_duration_label("0:47"), 47);
        assert_eq!(parse_duration_label("bogus"), 0);
        assert_eq!(parse_duration_label(""), 0);
    }

    #[test]
    fn track_converts_to_queue_item() {
        let t = crate::models::Track {
            title: "Midnight Static".to_string(),
            artist: "Glass Coast".to_string(),
            album: "Neon Tide".to_string(),
            video_id: "mock-vid-001".to_string(),
            duration: "3:24".to_string(),
            thumbnail_url: String::new(),
        };
        let q = QueueItem::from(&t);
        assert_eq!(q.video_id, "mock-vid-001");
        assert_eq!(q.duration_secs, 204);
    }

    #[test]
    fn list_seeding_sets_current_history_and_upcoming() {
        let mut q = Queue::new();
        let items = vec![
            QueueItem::new("a", "A", "1"),
            QueueItem::new("b", "B", "2"),
            QueueItem::new("c", "C", "3"),
            QueueItem::new("c", "C dup", "3"),
        ];
        q.set_from_list(items, 1);
        assert_eq!(q.current().map(|i| i.video_id.as_str()), Some("b"));
        assert_eq!(q.next_up().map(|i| i.video_id.as_str()), Some("c"));
        assert_eq!(q.len(), 2, "current + upcoming; history is separate");
        assert_eq!(q.take_upcoming("c").map(|i| i.video_id), Some("c".to_string()));
        assert!(q.next_up().is_none());
    }
}
