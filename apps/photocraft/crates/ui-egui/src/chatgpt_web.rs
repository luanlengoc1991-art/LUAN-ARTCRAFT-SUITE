//! LUAN-ARTCRAFT-SUITE: ChatGPT for the AI / MCP Command Center, through ArtCraft ChatGPT
//! (`tools/chatgpt-bridge` in the suite, a separate app): chatgpt.com in the user's own Chrome,
//! signed in once with their ChatGPT account, so requests use the ChatGPT plan's usage and no API
//! key.
//!
//! PhotoCraft sends the flattened document (full resolution, up to [`MAX_SIDE`]) and the request
//! to it on 127.0.0.1 (port and token from `%APPDATA%\ArtCraft\chatgpt-bridge.json`, written by
//! the running app). Every image ChatGPT gives back becomes its own Smart Object layer, in order
//! (the first at the bottom), at its place on the canvas: full-canvas layers cover the canvas,
//! cropped ones go to the box ChatGPT reports for them. Follow-ups continue the same ChatGPT
//! conversation. PhotoCraft itself stays native: it starts that app as it starts `codex` or
//! `claude`, out of sight, and talks to it over loopback HTTP.

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
/// Longest side of the picture sent to ChatGPT: the canvas at full resolution up to this.
const MAX_SIDE: u32 = 4096;
/// Largest PNG sent; a bigger one is sent at [`SMALLER_SIDE`] (ChatGPT takes about 20 MB).
const MAX_UPLOAD: usize = 18 * 1024 * 1024;
const SMALLER_SIDE: u32 = 2048;
/// Largest answer read back (all the layers as base64 plus text).
const MAX_REPLY: u64 = 512 * 1024 * 1024;

/// The canvas as sent.
pub(crate) struct Snapshot {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// One image ChatGPT gave back.
pub(crate) struct Layer {
    pub bytes: Vec<u8>,
    pub ext: &'static str,
    /// The file's name without its extension (e.g. `02_co_dau`).
    pub name: String,
}

/// What ChatGPT gave back.
pub(crate) struct Reply {
    /// In order: the first goes at the bottom.
    pub layers: Vec<Layer>,
    pub text: String,
    /// The ChatGPT conversation, to continue it with the next request.
    pub conversation: Option<String>,
    /// The size of the picture that was sent, which ChatGPT's boxes refer to.
    pub sent: Option<(u32, u32)>,
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

fn launch(background: bool) -> Result<(), Failure> {
    let exe = exe_path().ok_or(Failure::NotInstalled)?;
    let mut cmd = Command::new(exe);
    if background {
        cmd.arg("--background");
    }
    // Set by VS Code and other Electron hosts; it would start Electron as plain Node.
    cmd.env_remove("ELECTRON_RUN_AS_NODE");
    cmd.spawn().map(drop).map_err(|e| Failure::Other(format!("couldn't start {APP}: {e}")))
}

/// Starts the app with its window (sign-in, state), or brings the window to the front.
pub(crate) fn open_window() -> Result<(), Failure> {
    launch(false)
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

/// The running app (started out of sight when it isn't), as (port, token).
fn connect() -> Result<(u16, String), Failure> {
    let alive = |(port, token): &(u16, String)| http(*port, token, "GET", "/v1/status", &[], Duration::from_secs(10)).is_ok_and(|v| v["ok"] == true);
    if let Some(info) = read_info().filter(alive) {
        return Ok(info);
    }
    launch(true)?;
    let deadline = Instant::now() + Duration::from_secs(60);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500));
        if let Some(info) = read_info().filter(alive) {
            return Ok(info);
        }
    }
    Err(Failure::Other(format!("{APP} didn't start within a minute")))
}

fn encode_png(doc: &photocraft_doc::Document, side: u32) -> Result<Snapshot, String> {
    let img = photocraft_compose::thumbnail(doc, side);
    let (width, height) = (img.width, img.height);
    let image = photocraft_codecs::Image::from_u8(width, height, photocraft_codecs::ChannelLayout::Rgba, img.pixels).map_err(|e| e.to_string())?;
    let png = photocraft_codecs::encode(&image, photocraft_codecs::Format::Png, &Default::default()).map_err(|e| e.to_string())?;
    Ok(Snapshot { png, width, height })
}

/// The open document flattened to a PNG at full resolution (longest side at most
/// [`MAX_SIDE`], smaller when the file would be too big to upload), or none.
pub(crate) fn snapshot(app: &PhotocraftApp) -> Result<Option<Snapshot>, String> {
    let Some(d) = app.session.active() else { return Ok(None) };
    let full = encode_png(&d.doc, MAX_SIDE)?;
    if full.png.len() <= MAX_UPLOAD {
        return Ok(Some(full));
    }
    encode_png(&d.doc, SMALLER_SIDE).map(Some)
}

/// Sends the request on a worker thread; the receiver gets the outcome.
pub(crate) fn start(prompt: String, snap: Option<Snapshot>, conversation: Option<String>, ctx: &egui::Context) -> Receiver<Result<Reply, Failure>> {
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let _ = tx.send(request(&prompt, snap.as_ref(), conversation.as_deref()));
        ctx.request_repaint();
    });
    rx
}

fn request(prompt: &str, snap: Option<&Snapshot>, conversation: Option<&str>) -> Result<Reply, Failure> {
    let (port, token) = connect()?;
    let body = json!({
        "prompt": prompt,
        "image": snap.map(|s| base64::engine::general_purpose::STANDARD.encode(&s.png)),
        "width": snap.map(|s| s.width),
        "height": snap.map(|s| s.height),
        "conversation": conversation,
    });
    let body = serde_json::to_vec(&body).map_err(|e| Failure::Other(e.to_string()))?;
    // ChatGPT may think for several minutes and then make many files; the app gives up first.
    let v = http(port, &token, "POST", "/v1/image", &body, Duration::from_secs(20 * 60)).map_err(Failure::Other)?;
    let mut r = reply(&v)?;
    r.sent = snap.map(|s| (s.width, s.height));
    Ok(r)
}

fn ext_of(mime: &str, name: &str) -> &'static str {
    let n = name.to_lowercase();
    if mime.contains("webp") || n.ends_with(".webp") {
        "webp"
    } else if mime.contains("jpeg") || mime.contains("jpg") || n.ends_with(".jpg") || n.ends_with(".jpeg") {
        "jpg"
    } else {
        "png"
    }
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
    let decode = |b: &Value| b.as_str().and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok());
    let mut layers = Vec::new();
    match v["images"].as_array() {
        Some(images) => {
            for (i, im) in images.iter().enumerate() {
                let Some(bytes) = decode(&im["image"]) else { continue };
                let file = im["name"].as_str().unwrap_or("");
                let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
                let name = if stem.is_empty() { format!("ChatGPT {:02}", i + 1) } else { stem.to_string() };
                layers.push(Layer { ext: ext_of(im["mime"].as_str().unwrap_or(""), file), bytes, name });
            }
        }
        None => {
            if let Some(bytes) = decode(&v["image"]) {
                layers.push(Layer { ext: ext_of(v["mime"].as_str().unwrap_or(""), ""), bytes, name: "ChatGPT".into() });
            }
        }
    }
    if layers.is_empty() {
        return Err(Failure::Other("ChatGPT's image couldn't be read".into()));
    }
    let conversation = v["conversation"].as_str().filter(|c| c.starts_with("https://chatgpt.com/c/")).map(str::to_string);
    Ok(Reply { layers, text, conversation, sent: None })
}

/// Where ChatGPT says a layer's object sits on the picture it was sent (pixels).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Placed {
    pub file: String,
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// The JSON list of boxes at the end of ChatGPT's answer (`[{"file", "name", "x", "y", "w", "h"}]`).
pub(crate) fn boxes(text: &str) -> Vec<Placed> {
    // The last `[ {` … `]` that parses: the list asked for at the end of the answer.
    let mut starts: Vec<usize> =
        text.match_indices('[').map(|(i, _)| i).filter(|&i| text.get(i + 1..).is_some_and(|r| r.trim_start().starts_with('{'))).collect();
    starts.reverse();
    for start in starts {
        for (end, _) in text.match_indices(']').filter(|(e, _)| *e > start).collect::<Vec<_>>().into_iter().rev() {
            let Some(slice) = text.get(start..=end) else { continue };
            let Ok(Value::Array(items)) = serde_json::from_str::<Value>(slice) else { continue };
            let num = |v: &Value| v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).filter(|n| n.is_finite());
            return items
                .iter()
                .map(|it| Placed {
                    file: it["file"].as_str().unwrap_or("").to_string(),
                    name: it["name"].as_str().unwrap_or("").to_string(),
                    x: num(&it["x"]).unwrap_or(0.0),
                    y: num(&it["y"]).unwrap_or(0.0),
                    w: num(&it["w"]).unwrap_or(0.0),
                    h: num(&it["h"]).unwrap_or(0.0),
                })
                .collect();
        }
    }
    Vec::new()
}

/// Scale (percent) and centre on the canvas for an image of `img` pixels: a full-canvas layer
/// (the canvas's proportions) covers the canvas; a cropped one goes to its box (on the picture of
/// `sent` pixels that was sent); without a box it is fitted in the middle.
pub(crate) fn placement(img: (f64, f64), canvas: (f64, f64), sent: Option<(f64, f64)>, bbox: Option<&Placed>) -> (f64, [f64; 2]) {
    let (w, h) = (img.0.max(1.0), img.1.max(1.0));
    let (cw, ch) = canvas;
    let middle = [cw / 2.0, ch / 2.0];
    let same_shape = ((w / h) - (cw / ch)).abs() / (cw / ch) < 0.02;
    let sent = sent.unwrap_or(canvas);
    let k = cw / sent.0.max(1.0);
    match bbox.filter(|b| b.w > 1.0 && b.h > 1.0) {
        // A file as big as the picture sent is a full-canvas layer, whatever its box says.
        Some(b) if !(same_shape && (w - sent.0).abs() <= 2.0) => {
            let scale = (b.w * k / w).min(b.h * k / h) * 100.0;
            (scale, [(b.x + b.w / 2.0) * k, (b.y + b.h / 2.0) * k])
        }
        _ if same_shape => (cw / w * 100.0, middle),
        _ => ((cw / w).min(ch / h) * 100.0, middle),
    }
}

/// Whether an 8-bit RGBA image has nothing visible (ChatGPT sometimes returns an empty layer).
fn fully_transparent(img: &photocraft_codecs::Image) -> bool {
    img.layout() == photocraft_codecs::ChannelLayout::Rgba
        && img.sample_type() == photocraft_codecs::SampleType::U8
        && img.data().as_chunks::<4>().0.iter().all(|[_, _, _, a]| *a == 0)
}

/// A layer name as a file name (Windows-safe), so the placed layer carries it.
fn file_stem(name: &str) -> String {
    let s: String = name.chars().map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_end_matches('.').to_string();
    if s.is_empty() { "ChatGPT".into() } else { s.chars().take(80).collect() }
}

/// Puts ChatGPT's images in the app, each as its own Smart Object layer (sharp at any size), in
/// order, at its place on the canvas; with no document open the first opens as a new one.
/// (layers added, images skipped because they were fully transparent).
pub(crate) fn place(app: &mut PhotocraftApp, reply: &Reply) -> Result<(usize, usize), String> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let dir = std::env::temp_dir().join("PhotoCraft-ChatGPT").join(stamp.to_string());
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let found = boxes(&reply.text);
    let mut layers = reply.layers.iter().enumerate();
    let mut sent = reply.sent.map(|(w, h)| (f64::from(w), f64::from(h)));
    let (mut added, mut skipped) = (0, 0);
    if app.session.active().is_none() {
        let Some((_, first)) = layers.next() else { return Ok((0, 0)) };
        app.open_bytes(&format!("{}.{}", file_stem(&first.name), first.ext), &first.bytes)?;
        sent = None;
        added += 1;
    }
    for (i, layer) in layers {
        let Some(d) = app.session.active() else { break };
        let canvas = (f64::from(d.doc.size.width), f64::from(d.doc.size.height));
        let img = photocraft_codecs::decode(&layer.bytes).map_err(|e| format!("{}: {e}", layer.name))?;
        if fully_transparent(&img) {
            skipped += 1;
            continue;
        }
        // The box for this file: by its name, else by its place in the list.
        let bbox =
            found.iter().find(|b| !b.file.is_empty() && b.file.rsplit_once('.').map_or(b.file.as_str(), |(s, _)| s) == layer.name).or_else(|| found.get(i));
        let (scale, center) = placement((f64::from(img.width()), f64::from(img.height())), canvas, sent, bbox);
        let label = bbox
            .map(|b| b.name.trim())
            .filter(|n| !n.is_empty())
            .map_or_else(|| layer.name.clone(), |n| format!("{} {n}", layer.name.split('_').next().unwrap_or("")).trim().to_string());
        let path = dir.join(format!("{}.{}", file_stem(&label), layer.ext));
        std::fs::write(&path, &layer.bytes).map_err(|e| e.to_string())?;
        app.run("file.placeEmbedded", json!({"path": path.to_string_lossy(), "scale": scale, "center": center, "fit": false, "smartObject": true}))?;
        added += 1;
    }
    Ok((added, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = photocraft_codecs::Image::from_u8(w, h, photocraft_codecs::ChannelLayout::Rgba, vec![200; (w * h * 4) as usize]).unwrap();
        photocraft_codecs::encode(&img, photocraft_codecs::Format::Png, &Default::default()).unwrap()
    }

    #[test]
    fn answers_are_read_from_the_http_response() {
        let ok = parse_response(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\n{\"ok\":true}").unwrap();
        assert_eq!(ok, json!({"ok": true}));
        assert!(parse_response(b"HTTP/1.1 401 Unauthorized\r\n\r\n{}").unwrap_err().contains("token"));
        assert!(parse_response(b"HTTP/1.1 200 OK\r\n").is_err());
    }

    #[test]
    fn replies_become_layers_or_reasons() {
        let r = reply(&json!({"ok": true, "images": [
            {"name": "01_nen.png", "mime": "image/png", "image": "iVBORw0KGgo="},
            {"name": "02_co_dau.webp", "mime": "image/webp", "image": "iVBORw0KGgo="}
        ], "text": " Xong ", "conversation": "https://chatgpt.com/c/abc"}))
        .unwrap();
        assert_eq!(r.layers.iter().map(|l| (l.name.as_str(), l.ext)).collect::<Vec<_>>(), [("01_nen", "png"), ("02_co_dau", "webp")]);
        assert_eq!((r.text.as_str(), r.conversation.as_deref()), ("Xong", Some("https://chatgpt.com/c/abc")));
        let one = reply(&json!({"ok": true, "image": "iVBORw0KGgo=", "mime": "image/jpeg"})).unwrap();
        assert_eq!((one.layers.len(), one.layers[0].ext, one.conversation), (1, "jpg", None));
        assert!(matches!(reply(&json!({"ok": false, "code": "login", "error": "x"})), Err(Failure::Login)));
        assert!(
            matches!(reply(&json!({"ok": false, "code": "limit", "error": "x", "text": "You've hit the limit"})), Err(Failure::Limit(t)) if t == "You've hit the limit")
        );
        assert!(matches!(reply(&json!({"ok": false, "code": "timeout", "error": "slow"})), Err(Failure::Other(e)) if e == "slow"));
        assert!(matches!(reply(&json!({"ok": true})), Err(Failure::Other(_))));
    }

    #[test]
    fn the_boxes_list_is_read_from_the_end_of_the_answer() {
        let text = "Đã tách [xong].\njson\nCopy code\n[\n {\"file\": \"01_nen.png\", \"name\": \"Nền\", \"x\": 0, \"y\": 0, \"w\": 941, \"h\": 1672},\n {\"file\": \"02_co_dau.png\", \"name\": \"Cô dâu\", \"x\": \"100\", \"y\": 300, \"w\": 400, \"h\": 900}\n]\nHết.";
        let b = boxes(text);
        assert_eq!(b.len(), 2);
        assert_eq!((b[1].name.as_str(), b[1].x, b[1].w), ("Cô dâu", 100.0, 400.0));
        assert!(boxes("không có danh sách [1, 2]").is_empty());
    }

    #[test]
    fn layers_go_where_they_were() {
        let canvas = (1882.0, 3344.0);
        let sent = Some((941.0, 1672.0));
        // A full-canvas file covers the canvas, scaled up from the picture sent.
        assert_eq!(placement((941.0, 1672.0), canvas, sent, None), (200.0, [941.0, 1672.0]));
        // A cropped file goes to its box, scaled with the canvas.
        let b = Placed { file: "02.png".into(), name: "Cô dâu".into(), x: 100.0, y: 300.0, w: 400.0, h: 900.0 };
        let (scale, center) = placement((400.0, 900.0), canvas, sent, Some(&b));
        assert_eq!((scale.round(), center), (200.0, [600.0, 1500.0]));
        // A full-canvas file stays full-canvas even with a box around its object.
        assert_eq!(placement((941.0, 1672.0), canvas, sent, Some(&b)).0, 200.0);
        // Without a box, a cropped file is fitted in the middle.
        assert_eq!(placement((1000.0, 1000.0), (2000.0, 1000.0), None, None), (100.0, [1000.0, 500.0]));
    }

    #[test]
    fn the_open_document_is_sent_as_a_png_at_full_size() {
        let mut app = crate::PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        assert!(snapshot(&app).unwrap().is_none());
        app.run("file.new", json!({"width": 3000, "height": 1500})).unwrap();
        let snap = snapshot(&app).unwrap().unwrap();
        let img = photocraft_codecs::decode(&snap.png).unwrap();
        assert_eq!((img.width(), img.height(), snap.width, snap.height), (3000, 1500, 3000, 1500));
    }

    #[test]
    fn every_image_lands_as_a_smart_object_layer_in_order() {
        let mut app = crate::PhotocraftApp::new(photocraft_engine::Session::new(), Default::default());
        app.run("file.new", json!({"width": 200, "height": 100})).unwrap();
        let before = app.session.active().unwrap().doc.layers.len();
        let empty = {
            let img = photocraft_codecs::Image::from_u8(100, 50, photocraft_codecs::ChannelLayout::Rgba, vec![0; 100 * 50 * 4]).unwrap();
            photocraft_codecs::encode(&img, photocraft_codecs::Format::Png, &Default::default()).unwrap()
        };
        let layers = vec![
            Layer { bytes: png(100, 50), ext: "png", name: "01_nen".into() },
            Layer { bytes: empty, ext: "png", name: "02_trong".into() },
            Layer { bytes: png(40, 20), ext: "png", name: "03_co_dau".into() },
        ];
        let text = r#"[{"file": "01_nen.png", "name": "", "x": 0, "y": 0, "w": 100, "h": 50}, {"file": "03_co_dau.png", "name": "Cô dâu", "x": 10, "y": 5, "w": 40, "h": 20}]"#;
        let r = Reply { layers, text: text.into(), conversation: None, sent: Some((100, 50)) };
        assert_eq!(place(&mut app, &r).unwrap(), (2, 1), "the empty image is skipped");
        let doc = &app.session.active().unwrap().doc;
        assert_eq!(doc.layers.len(), before + 2);
        let names: Vec<&str> = doc.layers.iter().rev().take(2).map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["03 Cô dâu", "01_nen"], "the last file on top");
        assert!(doc.layers.iter().rev().take(2).all(|l| matches!(l.content, photocraft_doc::LayerContent::Smart(_))));
    }

    #[test]
    fn without_a_document_the_first_image_opens_as_one() {
        // The desktop app's importer, as main.rs sets it up.
        let services = crate::Services {
            import: Some(Box::new(|name, bytes, depth| {
                photocraft_io::import_with_svg_group_depth(name, bytes, depth).map(|r| (r.document, r.warnings)).map_err(|e| e.to_string())
            })),
            ..Default::default()
        };
        let mut app = crate::PhotocraftApp::new(photocraft_engine::Session::new(), services);
        let r = Reply {
            layers: vec![
                Layer { bytes: png(64, 32), ext: "png", name: "ChatGPT 01".into() },
                Layer { bytes: png(64, 32), ext: "png", name: "ChatGPT 02".into() },
            ],
            text: String::new(),
            conversation: None,
            sent: None,
        };
        assert_eq!(place(&mut app, &r).unwrap(), (2, 0));
        let d = app.session.active().unwrap();
        assert_eq!((d.doc.size.width, d.doc.size.height), (64, 32));
    }
}
