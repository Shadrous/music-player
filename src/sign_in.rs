//! Windows browser sign-in (`spinwave --sign-in`): Google's YouTube Music
//! sign-in in an InPrivate Edge WebView2 window, so cookies stay in memory.
//! The cookie manager is polled in-process; the `Cookie` header goes to
//! stdout exactly once and the process exits 0. Closing the window first
//! exits 2. Same contract as the Linux helper (`browser_login_helper.py`).

/// Set once the Google session reaches YouTube Music.
const PROOF: &str = "__Secure-3PAPISID";

/// `name=value; ...` for the site's cookies, once they hold the proof cookie.
#[cfg_attr(not(windows), allow(dead_code))]
fn cookie_header<'a>(cookies: impl IntoIterator<Item = (&'a str, &'a str)>) -> Option<String> {
    let pairs: Vec<_> = cookies.into_iter().filter(|(name, value)| !name.is_empty() && !value.is_empty()).collect();
    pairs.iter().any(|(name, _)| *name == PROOF).then(|| {
        pairs.iter().map(|(name, value)| format!("{name}={value}")).collect::<Vec<_>>().join("; ")
    })
}

#[cfg(windows)]
pub fn run() -> i32 {
    // WebView2 needs a writable profile folder (by default it sits beside
    // the exe). InPrivate keeps cookies out of it; it goes when we exit.
    // shortcut: removal is best-effort if WebView2 still holds a file; it then stays in %TEMP% (no cookies in it).
    let data_dir = std::env::temp_dir().join(format!("spinwave-sign-in-{}", std::process::id()));
    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &data_dir);
    let code = window();
    let _ = std::fs::remove_dir_all(&data_dir);
    code
}

#[cfg(windows)]
fn window() -> i32 {
    use std::io::Write;
    use std::time::{Duration, Instant};
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoop};
    use tao::platform::run_return::EventLoopExtRunReturn;

    const SIGN_IN_URL: &str = "https://accounts.google.com/ServiceLogin?ltmpl=music&service=youtube\
        &passive=true&continue=https%3A%2F%2Fwww.youtube.com%2Fsignin\
        %3Faction_handle_signin%3Dtrue%26next%3Dhttps%253A%252F%252Fmusic.youtube.com%252F";
    const LANDING: &str = "https://music.youtube.com/";

    let mut event_loop = EventLoop::new();
    let Ok(window) = tao::window::WindowBuilder::new()
        .with_title("Sign in to YouTube Music")
        .with_inner_size(tao::dpi::LogicalSize::new(1000.0, 760.0))
        .build(&event_loop)
    else {
        eprintln!("sign-in window could not open");
        return 3;
    };
    let webview = match wry::WebViewBuilder::new().with_incognito(true).with_url(SIGN_IN_URL).build(&window) {
        Ok(webview) => webview,
        Err(_) => {
            eprintln!("Edge WebView2 is unavailable");
            return 3;
        }
    };

    let mut code = 2;
    let mut next_poll = Instant::now();
    event_loop.run_return(|event, _, control_flow| {
        if let Event::WindowEvent { event: WindowEvent::CloseRequested, .. } = event {
            *control_flow = ControlFlow::Exit;
            return;
        }
        if Instant::now() >= next_poll {
            next_poll = Instant::now() + Duration::from_millis(600);
            let cookies = webview.cookies_for_url(LANDING).unwrap_or_default();
            if let Some(header) = cookie_header(cookies.iter().map(|c| (c.name(), c.value()))) {
                let mut out = std::io::stdout();
                let _ = writeln!(out, "{header}");
                let _ = out.flush();
                code = 0;
                *control_flow = ControlFlow::Exit;
                return;
            }
        }
        *control_flow = ControlFlow::WaitUntil(next_poll);
    });
    code
}

#[cfg(test)]
mod tests {
    #[test]
    fn header_needs_the_proof_cookie() {
        assert_eq!(super::cookie_header([("NID", "1")]), None);
        let full = [("SID", "a"), ("empty", ""), ("__Secure-3PAPISID", "b")];
        assert_eq!(super::cookie_header(full).as_deref(), Some("SID=a; __Secure-3PAPISID=b"));
    }
}
