//! OS media controls via souvlaki: MPRIS on Linux (GNOME's media panel,
//! media keys, lock screen), SMTC on Windows, Now Playing on macOS. OS
//! events queue here and `Backend::poll_media_keys` applies them on the UI
//! thread. Metadata only (title, artist, art link); never stream URLs.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition};

pub use souvlaki::{MediaControlEvent as Event, SeekDirection};

/// What the OS currently shows, to send only changes.
#[derive(Default, PartialEq)]
struct Shown {
    video_id: String,
    playing: bool,
}

pub struct Media {
    controls: MediaControls,
    events: Arc<Mutex<Vec<MediaControlEvent>>>,
    shown: Shown,
}

/// Now-playing snapshot pushed to the OS.
pub struct NowPlaying<'a> {
    pub video_id: &'a str,
    pub title: &'a str,
    pub artist: &'a str,
    pub cover_url: &'a str,
    pub duration_secs: i32,
    pub position_secs: i32,
    pub playing: bool,
}

impl Media {
    /// Register with the OS; `None` when unavailable (no session bus, or on
    /// Windows no shown window yet: the caller retries).
    #[cfg(not(test))]
    pub fn start() -> Option<Self> {
        #[cfg(windows)]
        let hwnd = Some(main_window()?);
        #[cfg(not(windows))]
        let hwnd = None;
        let config = souvlaki::PlatformConfig { display_name: "Spinwave", dbus_name: "spinwave", hwnd };
        let mut controls = MediaControls::new(config).ok()?;
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&events);
        controls
            .attach(move |event| {
                if let Ok(mut queue) = sink.lock() {
                    queue.push(event);
                }
            })
            .ok()?;
        Some(Self { controls, events, shown: Shown::default() })
    }

    #[cfg(test)]
    pub fn start() -> Option<Self> {
        None
    }

    pub fn take_events(&self) -> Vec<MediaControlEvent> {
        self.events.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
    }

    /// Push changes to the OS (track change, play/pause, or `force` after a
    /// seek). Best-effort: a failed D-Bus call isn't worth surfacing.
    pub fn sync(&mut self, now: &NowPlaying, force: bool) {
        let shown = Shown { video_id: now.video_id.to_string(), playing: now.playing };
        if !force && shown == self.shown {
            return;
        }
        if shown.video_id != self.shown.video_id {
            let _ = self.controls.set_metadata(MediaMetadata {
                title: Some(now.title),
                artist: Some(now.artist),
                cover_url: (!now.cover_url.is_empty()).then_some(now.cover_url),
                duration: (now.duration_secs > 0).then(|| Duration::from_secs(now.duration_secs as u64)),
                ..Default::default()
            });
        }
        let progress = Some(MediaPosition(Duration::from_secs(now.position_secs.max(0) as u64)));
        let playback = if now.video_id.is_empty() {
            MediaPlayback::Stopped
        } else if now.playing {
            MediaPlayback::Playing { progress }
        } else {
            MediaPlayback::Paused { progress }
        };
        let _ = self.controls.set_playback(playback);
        self.shown = shown;
    }
}

/// This process's visible top-level window (the Qt main window): Windows
/// media controls attach to a window.
#[cfg(all(windows, not(test)))]
fn main_window() -> Option<*mut std::ffi::c_void> {
    type Hwnd = *mut std::ffi::c_void;
    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(visit: extern "system" fn(Hwnd, isize) -> i32, found: isize) -> i32;
        fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
        fn IsWindowVisible(hwnd: Hwnd) -> i32;
    }
    extern "system" fn visit(hwnd: Hwnd, found: isize) -> i32 {
        let mut pid = 0;
        // SAFETY: plain Win32 queries on a handle EnumWindows just gave us.
        let ours = unsafe { GetWindowThreadProcessId(hwnd, &mut pid) } != 0
            && pid == std::process::id()
            && unsafe { IsWindowVisible(hwnd) } != 0;
        if ours {
            // SAFETY: `found` points at the caller's local below, alive for the call.
            unsafe { *(found as *mut Hwnd) = hwnd };
        }
        i32::from(!ours) // 0 stops the walk
    }
    let mut found: Hwnd = std::ptr::null_mut();
    // SAFETY: `visit` only writes through the pointer we pass, during this call.
    unsafe { EnumWindows(visit, &mut found as *mut Hwnd as isize) };
    (!found.is_null()).then_some(found)
}
