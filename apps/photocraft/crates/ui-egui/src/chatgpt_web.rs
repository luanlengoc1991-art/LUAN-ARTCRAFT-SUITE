//! LUAN-ARTCRAFT-SUITE: ChatGPT for the AI / MCP Command Center, through ArtCraft ChatGPT
//! (`tools/chatgpt-bridge` in the suite, a separate app): chatgpt.com in its own window, signed in
//! with the user's own ChatGPT account, so requests use the ChatGPT plan's usage and no API key.
//!
//! PhotoCraft sends the flattened document and the request to it on 127.0.0.1 (port and token
//! from `%APPDATA%\ArtCraft\chatgpt-bridge.json`, written by the running app) and places the image
//! ChatGPT makes as a new layer. PhotoCraft itself stays native: it starts that app as it starts
//! `codex` or `claude`, and talks to it over loopback HTTP.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde_json::{Value, json};

use crate::PhotocraftApp;

/// The app's name: its folder, its exe and its window.
const APP: &str = "ArtCraft ChatGPT";
/// Longest side of the picture sent to ChatGPT.
const MAX_SIDE: u32 = 2048;
/// Largest answer read back (an image as base64 plus text).
const MAX_REPLY: u64 = 96 * 1024 * 1024;

/// What ChatGPT gave back.
pub(crate) struct Reply {
    pub image: Vec<u8>,
    pub ext: &'static str,
    pub text: String,
}

/// Why there is no image.
#[derive(Debug)]
pub(crate) enum Failure {
    /// ArtCraft ChatGPT isn't installed.
    NotInstalled,
    /// The ChatGPT window isn't signed in.
    Login,
    /// The ChatGPT plan has no usage left for now (ChatGPT's own words).
    Limit(String),
    Other(String),
}

/// Where the installer puts the app; `ARTCRAFT_CHATGPT_EXE` overrides it.
pub(crate) fn exe_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("ARTCRAFT_CHATGPT_EXE").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let local = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(local).join("Programs").join(APP).join(format!("{APP}.exe"))).filter(|p| p.is_file())
}

fn info_path() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("APPDATA")?).join("ArtCraft").join("chatgpt-bridge.json"))
}

/// The running app's port and token.
fn read_info() -> Option<(u16, String)> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(info_path()?).ok()?).ok()?;
    let port = u16::try_from(v["port"].as_u64()?).ok()?;
    Some((port, v["token"].as_str()?.to_string()))
}

/// Starts the app, or brings its window to the front when it runs (it allows one instance).
pub(crate) fn open_window() -> Result<(), Failure> {
    let exe = exe_path().ok_or(Failure::NotInstalled)?;
    let mut cmd = Command::new(exe);
    // Set by VS Code and other Electron hosts; it would start Electron as plain Node.
    cmd.env_remove("ELECTRON_RUN_AS_NODE");
    cmd.spawn().map(drop).map_err(|e| Failure::Other(format!("couldn't start {APP}: {e}")))
}

/// One HTTP/1.1 request to the app on loopback; the answer's JSON body.
pub(crate) fn http(port: u16, token: &str, method: &str, path: &str, body: &[u8], timeout: Duration) -> Result<Value, String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(3)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    s.set_write_timeout(Some(Duration::from_secs(60))).map_err(|e| e.to_string())?;
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes()).and_then(|()| s.write_all(body)).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    s.take(MAX_REPLY).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    parse_response(&buf)
}

/// The JSON body of a `Connection: close` response with a Content-Length.
pub(crate) fn parse_response(buf: &[u8]) -> Result<Value, String> {
    let split = buf.windows(4).position(|w| w == b"\r\n\r\n").ok_or("incomplete answer")?;
    let head = String::from_utf8_lossy(buf.get(..split).unwrap_or_default());
    let status: u16 = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or("bad status line")?;
    if status == 401 {
        return Err(format!("{APP} refused the token"));
    }
    serde_json::from_slice(buf.get(split + 4..).unwrap_or_default()).map_err(|e| format!("unreadable answer (HTTP {status}): {e}"))
}

/// The running app (started when it isn't), as (port, token).
fn connect() -> Result<(u16, String), Failure> {
    let alive = |(port, token): &(u16, String)| http(*port, token, "GET", "/v1/status", &[], Duration::from_secs(10)).is_ok_and(|v| v["ok"] == true);
    if let Some(info) = read_info().filter(alive) {
        return Ok(info);
    }
    open_window()?;
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500));
        if let Some(info) = read_info().filter(alive) {
            return Ok(info);
        }
    }
    Err(Failure::Other(format!("{APP} didn't start within a minute")))
}

/// The open document flattened to a PNG (longest side at most [`MAX_SIDE`]), or none.
pub(crate) fn snapshot(app: &PhotocraftApp) -> Result<Option<Vec<u8>>, String> {
    let Some(d) = app.session.active() else { return Ok(None) };
    let img = photocraft_compose::thumbnail(&d.doc, MAX_SIDE);
    let image = photocraft_codecs::Image::from_u8(img.width, img.height, photocraft_codecs::ChannelLayout::Rgba, img.pixels).map_err(|e| e.to_string())?;
    photocraft_codecs::encode(&image, photocraft_codecs::Format::Png, &Default::default()).map(Some).map_err(|e| e.to_string())
}

/// Sends the request on a worker thread; the receiver gets the outcome.
pub(crate) fn start(prompt: String, png: Option<Vec<u8>>, ctx: &egui::Context) -> Receiver<Result<Reply, Failure>> {
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let _ = tx.send(request(&prompt, png.as_deref()));
        ctx.request_repaint();
    });
    rx
}

fn request(prompt: &str, png: Option<&[u8]>) -> Result<Reply, Failure> {
    let (port, token) = connect()?;
    let image = png.map(|p| base64::engine::general_purpose::STANDARD.encode(p));
    let body = serde_json::to_vec(&json!({"prompt": prompt, "image": image})).map_err(|e| Failure::Other(e.to_string()))?;
    // Image requests take one to three minutes; the app gives up after eight.
    let v = http(port, &token, "POST", "/v1/image", &body, Duration::from_secs(10 * 60)).map_err(Failure::Other)?;
    reply(&v)
}

/// The app's answer as a [`Reply`] or a [`Failure`].
pub(crate) fn reply(v: &Value) -> Result<Reply, Failure> {
    let text = v["text"].as_str().unwrap_or("").trim().to_string();
    if v["ok"] != true {
        let error = v["error"].as_str().unwrap_or("ChatGPT failed").to_string();
        return Err(match v["code"].as_str() {
            Some("login") => Failure::Login,
            Some("limit") => Failure::Limit(if text.is_empty() { error } else { text }),
            _ => Failure::Other(error),
        });
    }
    let image = v["image"]
        .as_str()
        .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
        .ok_or_else(|| Failure::Other("ChatGPT's image couldn't be read".into()))?;
    let ext = match v["mime"].as_str().unwrap_or("") {
        m if m.contains("webp") => "webp",
        m if m.contains("jpeg") || m.contains("jpg") => "jpg",
        _ => "png",
    };
    Ok(Reply { image, ext, text })
}

/// Puts ChatGPT's image in the app: a new pixel layer "ChatGPT" fitted to the canvas, or a new
/// document when none is open.
pub(crate) fn place(app: &mut PhotocraftApp, reply: &Reply) -> Result<(), String> {
    let name = format!("ChatGPT.{}", reply.ext);
    let Some(d) = app.session.active() else {
        return app.open_bytes(&name, &reply.image).map(drop);
    };
    let (cw, ch) = (f64::from(d.doc.size.width), f64::from(d.doc.size.height));
    let img = photocraft_codecs::decode(&reply.image).map_err(|e| format!("ChatGPT's image: {e}"))?;
    let (w, h) = (f64::from(img.width().max(1)), f64::from(img.height().max(1)));
    let scale = (cw / w).min(ch / h) * 100.0;
    let dir = std::env::temp_dir().join("PhotoCraft-ChatGPT");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(name);
    std::fs::write(&path, &reply.image).map_err(|e| e.to_string())?;
    app.run("file.placeEmbedded", json!({"path": path.to_string_lossy(), "scale": scale, "fit": false, "smartObject": false})).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_read_from_the_http_response() {
        let ok = parse_response(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\n{\"ok\":true}").unwrap();
        assert_eq!(ok, json!({"ok": true}));
        assert!(parse_response(b"HTTP/1.1 401 Unauthorized\r\n\r\n{}").unwrap_err().contains("token"));
        assert!(parse_response(b"HTTP/1.1 200 OK\r\n").is_err());
    }

    #[test]
    fn replies_become_images_or_reasons() {
        let r = reply(&json!({"ok": true, "image": "iVBORw0KGgo=", "mime": "image/webp", "text": " Xong "})).unwrap();
        assert_eq!((r.ext, r.text.as_str(), r.image.len()), ("webp", "Xong", 8));
        assert!(matches!(reply(&json!({"ok": false, "code": "login", "error": "x"})), Err(Failure::Login)));
        assert!(
            matches!(reply(&json!({"ok": false, "code": "limit", "error": "x", "text": "You've hit the limit"})), Err(Failure::Limit(t)) if t == "You've hit the limit")
        );
        assert!(matches!(reply(&json!({"ok": false, "code": "timeout", "error": "slow"})), Err(Failure::Other(e)) if e == "slow"));
        assert!(matches!(reply(&json!({"ok": true})), Err(Failure::Other(_))));
    }

    #[test]
    fn the_open_document_is_sent_as_a_png() {
        let mut app = crate::PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        assert!(snapshot(&app).unwrap().is_none());
        app.run("file.new", json!({"width": 64, "height": 32})).unwrap();
        let png = snapshot(&app).unwrap().unwrap();
        let img = photocraft_codecs::decode(&png).unwrap();
        assert_eq!((img.width(), img.height()), (64, 32));
    }

    #[test]
    fn the_image_lands_as_a_layer_fitted_to_the_canvas() {
        let mut app = crate::PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width": 200, "height": 100})).unwrap();
        let before = app.session.active().unwrap().doc.layers.len();
        let img = photocraft_codecs::Image::from_u8(40, 20, photocraft_codecs::ChannelLayout::Rgba, vec![200; 40 * 20 * 4]).unwrap();
        let png = photocraft_codecs::encode(&img, photocraft_codecs::Format::Png, &Default::default()).unwrap();
        place(&mut app, &Reply { image: png, ext: "png", text: String::new() }).unwrap();
        let doc = &app.session.active().unwrap().doc;
        assert_eq!(doc.layers.len(), before + 1);
        let layer = doc.layers.last().unwrap();
        assert!(layer.name.starts_with("ChatGPT"), "{}", layer.name);
    }
}
