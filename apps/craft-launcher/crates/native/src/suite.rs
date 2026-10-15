//! LUAN-ARTCRAFT-SUITE: keeping every one of the owner's computers the same.
//!
//! - [`fetch_sync`]: the suite repo's `config/suite-sync.json`, the apps every computer keeps
//!   installed (the UI installs the missing ones).
//! - ArtCraft MCP: one MCP server for every installed app (`tools/artcraft-mcp`). Its newest
//!   release is found without the GitHub API (the `releases/latest` redirect names the tag, and
//!   `SHA256SUMS.txt` vouches for the file), installed per user into
//!   `%LOCALAPPDATA%\Programs\ArtCraft MCP\artcraft-mcp.exe` and registered as the MCP server
//!   `artcraft` with Claude Code (user scope, via its own CLI) and Cursor (`~/.cursor/mcp.json`,
//!   with `--compact`). Windows only: that is the one platform ArtCraft MCP is built for.

use std::path::{Path, PathBuf};
use std::time::Duration;

use artcraft_launcher_core::protocol::JobPhase;
use artcraft_launcher_core::release::{self, Asset, Release};
use artcraft_launcher_core::suite::{self, MCP_ASSET, MCP_NAME, MCP_SERVER, McpState, SUITE_REPO, SuiteSync};
use artcraft_launcher_core::version::{self, Version};
use serde_json::{Value, json};

use crate::http::{Http, Stop};
use crate::install::{self, Job};
use crate::paths::{Paths, write_atomic};

fn sync_cache(paths: &Paths) -> PathBuf {
    paths.cache.join("suite-sync.json")
}

/// The last list fetched, for the first paint and when offline.
pub fn cached_sync(paths: &Paths) -> Option<SuiteSync> {
    std::fs::read_to_string(sync_cache(paths)).ok().and_then(|t| SuiteSync::parse(&t).ok())
}

/// The suite list from GitHub (saved for next time), or the saved one when it can't be read.
pub fn fetch_sync(http: &Http, paths: &Paths) -> Option<SuiteSync> {
    match http.get_text(suite::SYNC_URL) {
        Ok(Some(text)) => match SuiteSync::parse(&text) {
            Ok(list) => {
                if let Err(e) = write_atomic(&sync_cache(paths), text.as_bytes()) {
                    log::warn!("couldn't save the suite list: {e}");
                }
                Some(list)
            }
            Err(e) => {
                log::warn!("suite list: {e}");
                cached_sync(paths)
            }
        },
        Ok(None) => None,
        Err(e) => {
            log::info!("suite list: {e}");
            cached_sync(paths)
        }
    }
}

/// Where ArtCraft MCP is installed (whether or not it is there yet). `None` where it isn't built.
pub fn mcp_exe() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    dirs::data_local_dir().map(|d| d.join("Programs").join(MCP_NAME).join("artcraft-mcp.exe"))
}

/// The version of the copy at `exe` (`artcraft-mcp 0.1.0`).
fn installed_version(exe: &Path) -> Option<Version> {
    if !exe.is_file() {
        return None;
    }
    let out = crate::process::capture(&exe.to_string_lossy(), &["--version"], Duration::from_secs(10))?;
    version::parse(out.split_whitespace().nth(1)?)
}

/// ArtCraft MCP's newest release, from github.com's pages (no API quota).
pub fn latest_mcp(http: &Http) -> Result<Option<Release>, String> {
    let latest = format!("https://github.com/{SUITE_REPO}/releases/latest");
    let Some(target) = http.redirect_target(&latest)? else { return Ok(None) };
    let Some(tag) = target.rsplit_once("/releases/tag/").map(|(_, t)| t.trim_end_matches('/').to_string()) else { return Ok(None) };
    let Some(version) = suite::mcp_version_from_tag(&tag) else { return Ok(None) };
    let base = format!("{}{tag}/", release::download_prefix(SUITE_REPO));
    let sums_url = format!("{base}SHA256SUMS.txt");
    let Some(sums) = http.get_text(&sums_url)? else { return Ok(None) };
    let Some((sha, _)) = release::parse_sha256sums(&sums).into_iter().find(|(_, n)| n == MCP_ASSET) else { return Ok(None) };
    Ok(Some(Release {
        name: format!("{MCP_NAME} {version}"),
        html_url: format!("https://github.com/{SUITE_REPO}/releases/tag/{tag}"),
        version: Some(version),
        tag,
        assets: vec![
            Asset { name: MCP_ASSET.into(), size: 0, url: format!("{base}{MCP_ASSET}"), sha256: Some(sha) },
            Asset { name: "SHA256SUMS.txt".into(), size: 0, url: sums_url, sha256: None },
        ],
        ..Release::default()
    }))
}

/// What is on this computer now, with `latest` as the newest release.
pub fn state(latest: Option<Release>) -> McpState {
    let Some(exe) = mcp_exe() else { return McpState::default() };
    let installed = installed_version(&exe);
    let (claude, cursor) = if installed.is_some() { (claude_registered(&exe), cursor_registered(&exe)) } else { (None, None) };
    McpState { supported: true, installed, latest, claude, cursor }
}

/// Downloads and verifies `release`, swaps it in and registers it. Returns the toast text.
pub fn install_mcp(release: &Release, http: &Http, paths: &Paths, stop: &Stop, phase: &mut dyn FnMut(JobPhase)) -> Result<String, String> {
    let exe = mcp_exe().ok_or("ArtCraft MCP is built for Windows only")?;
    let asset = release.assets.iter().find(|a| a.name == MCP_ASSET).ok_or("the release has no ArtCraft-MCP.exe")?;
    let mut job = Job { http, paths, stop, phase };
    let file = install::download(release, asset, &mut job)?;
    phase(JobPhase::Installing);
    let dir = exe.parent().ok_or("no install folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("couldn't create {} ({e})", dir.display()))?;
    let updating = exe.exists();
    if updating {
        // Claude Code or Cursor may be running it: a running exe can't be overwritten, but it can
        // be renamed out of the way.
        let old = exe.with_extension("exe.old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(&exe, &old).map_err(|e| format!("couldn't replace {} ({e})", exe.display()))?;
    }
    std::fs::copy(&file, &exe).map_err(|e| format!("couldn't install {} ({e})", exe.display()))?;
    let _ = std::fs::remove_file(&file);
    let got = installed_version(&exe);
    if got.as_ref() != release.version.as_ref() {
        return Err(format!("{MCP_NAME} was copied but doesn't start ({})", exe.display()));
    }
    // A client it couldn't be registered with shows as such in the launcher (and in the log).
    let _ = register(&exe);
    // The launcher's own message templates, so the toast is translated like an app's.
    let v = release.version_label();
    Ok(if updating { format!("{MCP_NAME} updated to {v}") } else { format!("{MCP_NAME} installed: {v}") })
}

/// Makes sure ArtCraft MCP is registered wherever Claude Code and Cursor are installed.
pub fn register(exe: &Path) -> (Option<bool>, Option<bool>) {
    let claude = match claude_registered(exe) {
        Some(false) => Some(register_claude(exe)),
        other => other,
    };
    if claude == Some(true) {
        allow_in_claude();
    }
    let cursor = match cursor_registered(exe) {
        Some(false) => Some(register_cursor(exe)),
        other => other,
    };
    (claude, cursor)
}

fn home() -> Option<PathBuf> {
    dirs::home_dir()
}

/// The owner drives the apps by asking Claude: ArtCraft MCP's tools run without a prompt each time
/// (`mcp__artcraft` in `~/.claude/settings.json`'s `permissions.allow`, added once, never removed).
fn allow_in_claude() {
    let Some(path) = home().map(|h| h.join(".claude").join("settings.json")) else { return };
    let existing = std::fs::read_to_string(&path).ok();
    match claude_settings(existing.as_deref()) {
        Ok(Some(text)) => {
            if let Err(e) = write_atomic(&path, text.as_bytes()) {
                log::warn!("{e}");
            }
        }
        Ok(None) => {}
        // Never overwrite settings we can't read.
        Err(e) => log::warn!("{}: {e}", path.display()),
    }
}

/// `existing` Claude Code user settings with `mcp__artcraft` allowed; `None` when it already is.
fn claude_settings(existing: Option<&str>) -> Result<Option<String>, String> {
    let rule = format!("mcp__{MCP_SERVER}");
    let mut v = match existing.map(|t| t.trim_start_matches('\u{feff}').trim()) {
        None | Some("") => json!({}),
        Some(t) => serde_json::from_str::<Value>(t).map_err(|e| format!("unreadable ({e})"))?,
    };
    let root = v.as_object_mut().ok_or("not a JSON object")?;
    let perms = root.entry("permissions").or_insert_with(|| json!({}));
    let perms = perms.as_object_mut().ok_or("permissions is not an object")?;
    let allow = perms.entry("allow").or_insert_with(|| json!([]));
    let allow = allow.as_array_mut().ok_or("permissions.allow is not a list")?;
    if allow.iter().any(|r| r.as_str() == Some(rule.as_str())) {
        return Ok(None);
    }
    allow.push(json!(rule));
    serde_json::to_string_pretty(&v).map(|s| Some(s + "\n")).map_err(|e| e.to_string())
}

fn same_path(a: &str, b: &Path) -> bool {
    let b = b.to_string_lossy();
    if cfg!(windows) { a.eq_ignore_ascii_case(&b) } else { a == b }
}

fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()
}

/// `Some(registered)` when Claude Code is on this computer.
fn claude_registered(exe: &Path) -> Option<bool> {
    let config = home()?.join(".claude.json");
    let present = config.is_file() || find_claude().is_some();
    if !present {
        return None;
    }
    let v = read_json(&config).unwrap_or(Value::Null);
    Some(v["mcpServers"][MCP_SERVER]["command"].as_str().is_some_and(|c| same_path(c, exe)))
}

/// Claude Code's own CLI: on PATH, the native installer's, or bundled with the VS Code / Cursor
/// extension (the newest).
fn find_claude() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "claude.exe" } else { "claude" };
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let p = dir.join(exe);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let home = home()?;
    let local = home.join(".local").join("bin").join(exe);
    if local.is_file() {
        return Some(local);
    }
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for editor in [".vscode", ".cursor", ".vscode-insiders", ".windsurf"] {
        let Ok(dirs) = std::fs::read_dir(home.join(editor).join("extensions")) else { continue };
        for d in dirs.flatten() {
            if !d.file_name().to_string_lossy().starts_with("anthropic.claude-code-") {
                continue;
            }
            let p = d.path().join("resources").join("native-binary").join(exe);
            let modified = d.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            if p.is_file() && best.as_ref().is_none_or(|(t, _)| modified > *t) {
                best = Some((modified, p));
            }
        }
    }
    best.map(|(_, p)| p)
}

fn register_claude(exe: &Path) -> bool {
    let Some(claude) = find_claude() else {
        log::warn!(
            "Claude Code is set up here but its CLI wasn't found; register ArtCraft MCP with: claude mcp add {MCP_SERVER} -s user -- \"{}\"",
            exe.display()
        );
        return false;
    };
    let claude = claude.to_string_lossy().to_string();
    let t = Duration::from_secs(60);
    let _ = crate::process::run(&claude, &["mcp", "remove", MCP_SERVER, "-s", "user"], t);
    match crate::process::run(&claude, &["mcp", "add", MCP_SERVER, "-s", "user", "--", &exe.to_string_lossy()], t) {
        Ok(_) => claude_registered(exe) == Some(true),
        Err(e) => {
            log::warn!("claude mcp add: {e}");
            false
        }
    }
}

/// `Some(registered)` when Cursor is on this computer.
fn cursor_registered(exe: &Path) -> Option<bool> {
    let dir = home()?.join(".cursor");
    if !dir.is_dir() {
        return None;
    }
    let v = read_json(&dir.join("mcp.json")).unwrap_or(Value::Null);
    Some(v["mcpServers"][MCP_SERVER]["command"].as_str().is_some_and(|c| same_path(c, exe)))
}

/// Adds (or points at `exe`) the `artcraft` entry of `~/.cursor/mcp.json`, keeping every other one.
fn register_cursor(exe: &Path) -> bool {
    let Some(path) = home().map(|h| h.join(".cursor").join("mcp.json")) else { return false };
    let existing = std::fs::read_to_string(&path).ok();
    let text = match cursor_config(existing.as_deref(), exe) {
        Ok(t) => t,
        Err(e) => {
            // Never overwrite a file we can't read: it holds the user's other servers.
            log::warn!("{}: {e}", path.display());
            return false;
        }
    };
    match write_atomic(&path, text.as_bytes()) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("{e}");
            false
        }
    }
}

/// `existing` (Cursor's `mcp.json`, if any) with the `artcraft` server set to `exe`.
fn cursor_config(existing: Option<&str>, exe: &Path) -> Result<String, String> {
    let mut v = match existing.map(|t| t.trim_start_matches('\u{feff}').trim()) {
        None | Some("") => json!({}),
        Some(t) => serde_json::from_str::<Value>(t).map_err(|e| format!("unreadable ({e})"))?,
    };
    let root = v.as_object_mut().ok_or("not a JSON object")?;
    let servers = root.entry("mcpServers").or_insert_with(|| json!({}));
    let servers = servers.as_object_mut().ok_or("mcpServers is not an object")?;
    servers.insert(MCP_SERVER.into(), json!({ "command": exe.to_string_lossy(), "args": ["--compact"] }));
    serde_json::to_string_pretty(&v).map(|s| s + "\n").map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_settings_add_the_rule_once() {
        let before = r#"{"model":"opus","permissions":{"allow":["Bash(git *)"],"deny":["X"]}}"#;
        let after: Value = serde_json::from_str(&claude_settings(Some(before)).unwrap().unwrap()).unwrap();
        assert_eq!(after["model"], "opus");
        assert_eq!(after["permissions"]["deny"][0], "X");
        assert_eq!(after["permissions"]["allow"], json!(["Bash(git *)", "mcp__artcraft"]));
        assert_eq!(claude_settings(Some(&after.to_string())).unwrap(), None, "already allowed");
        assert!(claude_settings(None).unwrap().is_some());
        assert!(claude_settings(Some("{broken")).is_err(), "never overwrite unreadable settings");
    }

    #[test]
    fn cursor_config_keeps_other_servers() {
        let exe = Path::new(r"C:\Users\x\AppData\Local\Programs\ArtCraft MCP\artcraft-mcp.exe");
        let before = r#"{"mcpServers":{"other":{"command":"a.exe"}},"keep":1}"#;
        let after: Value = serde_json::from_str(&cursor_config(Some(before), exe).unwrap()).unwrap();
        assert_eq!(after["mcpServers"]["other"]["command"], "a.exe");
        assert_eq!(after["keep"], 1);
        assert_eq!(after["mcpServers"]["artcraft"]["args"][0], "--compact");
        assert!(same_path(after["mcpServers"]["artcraft"]["command"].as_str().unwrap(), exe));
        assert!(cursor_config(Some(""), exe).is_ok());
        assert!(cursor_config(None, exe).is_ok());
        assert!(cursor_config(Some("{broken"), exe).is_err(), "never overwrite an unreadable file");
        assert!(cursor_config(Some("[]"), exe).is_err());
    }
}
