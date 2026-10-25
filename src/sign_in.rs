//! `spinwave --sign-in`: the browser sign-in window, run as a child process
//! of the app. It opens Google's YouTube Music sign-in in an off-the-record
//! QtWebEngine view (cookies in memory only), reads the site's cookies over
//! the engine's local DevTools port, prints the `Cookie` header on stdout
//! exactly once and exits 0. Closing the window first exits 2. Nothing is
//! written to disk; stderr carries short static diagnostics only.
//!
//! shortcut: the DevTools port is a random 127.0.0.1 port open while the
//! window is up; any local process could read the sign-in cookies in that
//! window. Browser pages are kept out by the origin allow-list. Upgrade to
//! a C++ cookie-store hook if qtbridge ever exposes one.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

const LANDING: &str = "https://music.youtube.com/";
/// Set once the Google session reaches YouTube Music.
const PROOF: &str = "__Secure-3PAPISID";

/// Run the sign-in window; returns the process exit code.
pub fn run(qml_dir: &str) -> i32 {
    let Some(port) = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .ok()
        .map(|a| a.port())
    else {
        eprintln!("sign-in: no free local port");
        return 3;
    };
    std::env::set_var("QTWEBENGINE_REMOTE_DEBUGGING", format!("127.0.0.1:{port}"));
    // Only clients without an Origin (us) may connect, not web pages; an
    // explicit list also stops Qt from adding `*`.
    let flags = std::env::var("QTWEBENGINE_CHROMIUM_FLAGS").unwrap_or_default();
    std::env::set_var("QTWEBENGINE_CHROMIUM_FLAGS", format!("{flags} --remote-allow-origins=http://127.0.0.1:{port}"));

    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(600));
        if let Some(header) = session_header(port) {
            let mut out = std::io::stdout();
            let _ = writeln!(out, "{header}");
            let _ = out.flush();
            std::process::exit(0);
        }
    });

    let qml = std::path::Path::new(qml_dir).join("SignInWindow.qml");
    qtbridge::QApp::new().load_qml_from_file(&crate::file_url(&qml)).run();
    2
}

/// The site's `Cookie` header once the proof cookie exists.
fn session_header(port: u16) -> Option<String> {
    let targets: serde_json::Value = serde_json::from_str(&http_get(port, "/json")?).ok()?;
    let ws_url = targets
        .as_array()?
        .iter()
        .find(|t| t["type"] == "page")?["webSocketDebuggerUrl"]
        .as_str()?
        .to_string();
    let path = ws_url.split_once(&format!("127.0.0.1:{port}"))?.1;
    let request = serde_json::json!({ "id": 1, "method": "Network.getCookies", "params": { "urls": [LANDING] } });
    let reply: serde_json::Value = serde_json::from_str(&ws_request(port, path, &request.to_string())?).ok()?;
    cookie_header(&reply["result"]["cookies"])
}

/// `name=value; ...` from a CDP cookie list, if it holds the proof cookie.
fn cookie_header(cookies: &serde_json::Value) -> Option<String> {
    let pairs: Vec<(&str, &str)> = cookies
        .as_array()?
        .iter()
        .filter_map(|c| Some((c["name"].as_str()?, c["value"].as_str()?)))
        .filter(|(name, value)| !name.is_empty() && !value.is_empty())
        .collect();
    pairs.iter().any(|(name, _)| *name == PROOF).then(|| {
        pairs.iter().map(|(name, value)| format!("{name}={value}")).collect::<Vec<_>>().join("; ")
    })
}

fn connect(port: u16) -> Option<TcpStream> {
    let stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    Some(stream)
}

/// GET with a `Content-Length` body (the DevTools server keeps the
/// connection open, so reading to EOF would stall).
fn http_get(port: u16, path: &str) -> Option<String> {
    let mut stream = connect(port)?;
    write!(stream, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").ok()?;
    let mut reader = BufReader::new(stream);
    let mut len = None;
    let mut line = String::new();
    while line != "\r\n" {
        line.clear();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                len = value.trim().parse().ok();
            }
        }
    }
    let mut body = vec![0u8; len?];
    reader.read_exact(&mut body).ok()?;
    String::from_utf8(body).ok()
}

/// One request/response over a fresh WebSocket (RFC 6455, text frames).
fn ws_request(port: u16, path: &str, message: &str) -> Option<String> {
    let stream = connect(port)?;
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut writer = stream;
    write!(
        writer,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
    )
    .ok()?;
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    if !line.contains(" 101 ") {
        return None;
    }
    while line != "\r\n" {
        line.clear();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
    }
    writer.write_all(&client_frame(message.as_bytes())).ok()?;
    let mut payload = Vec::new();
    loop {
        let mut head = [0u8; 2];
        reader.read_exact(&mut head).ok()?;
        let len = match head[1] & 0x7f {
            126 => {
                let mut n = [0u8; 2];
                reader.read_exact(&mut n).ok()?;
                u64::from(u16::from_be_bytes(n))
            }
            127 => {
                let mut n = [0u8; 8];
                reader.read_exact(&mut n).ok()?;
                u64::from_be_bytes(n)
            }
            n => u64::from(n),
        };
        let mut chunk = vec![0u8; usize::try_from(len).ok()?];
        reader.read_exact(&mut chunk).ok()?;
        payload.extend_from_slice(&chunk);
        if head[0] & 0x80 != 0 {
            return String::from_utf8(payload).ok();
        }
    }
}

/// A masked client text frame (servers reject unmasked client frames).
fn client_frame(payload: &[u8]) -> Vec<u8> {
    const MASK: [u8; 4] = [0x37, 0xfa, 0x21, 0x3d];
    let mut frame = vec![0x81];
    match payload.len() {
        n if n < 126 => frame.push(0x80 | n as u8),
        n if n <= 0xffff => {
            frame.push(0x80 | 126);
            frame.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            frame.push(0x80 | 127);
            frame.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    frame.extend_from_slice(&MASK);
    frame.extend(payload.iter().enumerate().map(|(i, b)| b ^ MASK[i % 4]));
    frame
}

#[cfg(test)]
mod tests {
    #[test]
    fn header_needs_the_proof_cookie() {
        let partial = serde_json::json!([{ "name": "NID", "value": "1" }]);
        assert_eq!(super::cookie_header(&partial), None);
        let full = serde_json::json!([
            { "name": "SID", "value": "a" },
            { "name": "empty", "value": "" },
            { "name": "__Secure-3PAPISID", "value": "b" },
        ]);
        assert_eq!(super::cookie_header(&full).as_deref(), Some("SID=a; __Secure-3PAPISID=b"));
    }

    #[test]
    fn client_frames_are_masked() {
        let frame = super::client_frame(b"hi");
        assert_eq!(&frame[..2], &[0x81, 0x82]);
        assert_eq!(frame[6] ^ frame[2], b'h');
        assert_eq!(super::client_frame(&[0; 300])[1], 0x80 | 126);
    }
}
