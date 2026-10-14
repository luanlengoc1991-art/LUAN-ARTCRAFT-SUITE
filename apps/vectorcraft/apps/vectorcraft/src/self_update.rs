//! Windows only: keeps a portable `vectorcraft.exe` up to date from this fork's GitHub Releases.
//!
//! `.github/workflows/windows-autobuild.yml` builds every push to `main` with
//! `VECTORCRAFT_UPDATE_REPO` (owner/name) and `VECTORCRAFT_UPDATE_BUILD` (the run number) set, and
//! publishes the exe as the latest release, tagged `build-<run number>`. A build made that way
//! checks the latest release in the background at start-up and every [`CHECK_EVERY`] after; when it
//! is newer, it downloads the exe next to the running one, checks its SHA-256 against the one GitHub
//! reports, and swaps the files (Windows lets a running exe be renamed). A banner ([`banner`]) then
//! offers to restart into it; otherwise the next start runs it.
//!
//! Builds without those variables (local `cargo build`, the signed release pipeline) never check.
//! `VECTORCRAFT_NO_UPDATE=1` turns it off, and so do automation runs (`--control`). No new crates:
//! the HTTP work is done by Windows PowerShell, hidden, in a thread that never blocks the window.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const REPO: Option<&str> = option_env!("VECTORCRAFT_UPDATE_REPO");
const BUILD: Option<&str> = option_env!("VECTORCRAFT_UPDATE_BUILD");

/// The release asset that replaces the running exe.
const ASSET: &str = "vectorcraft.exe";

/// How often a running app looks for a newer build.
const CHECK_EVERY: Duration = Duration::from_secs(10 * 60);

/// A newer build swapped in on disk: its build number and the exe path to start it from.
static READY: Mutex<Option<(String, PathBuf)>> = Mutex::new(None);
/// The window, so a finished update shows its banner without waiting for input.
static CTX: OnceLock<egui::Context> = OnceLock::new();
/// "Later" was chosen: the banner stays hidden; the next start runs the new build anyway.
static DISMISSED: AtomicBool = AtomicBool::new(false);
/// "Restart now" was chosen: [`relaunch_if_requested`] starts the new build as this one exits.
static RELAUNCH: AtomicBool = AtomicBool::new(false);

/// Queries the latest release and downloads the exe when its build number is higher. Prints the new
/// build number on success. Single quotes only: `powershell.exe -Command` drops double quotes.
const SCRIPT: &str = r"$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$ProgressPreference = 'SilentlyContinue'
$h = @{ 'User-Agent' = 'vectorcraft-updater'; 'Accept' = 'application/vnd.github+json' }
$r = Invoke-RestMethod -UseBasicParsing -Headers $h -Uri ('https://api.github.com/repos/' + $env:VC_REPO + '/releases/latest')
if ($r.tag_name -notmatch '^build-(\d+)$') { exit 0 }
$n = [int64]$Matches[1]
if ($n -le [int64]$env:VC_BUILD) { exit 0 }
$a = $r.assets | Where-Object { $_.name -eq $env:VC_ASSET } | Select-Object -First 1
if (-not $a) { exit 0 }
Invoke-WebRequest -UseBasicParsing -Headers @{ 'User-Agent' = 'vectorcraft-updater' } -Uri $a.browser_download_url -OutFile $env:VC_OUT
if ($a.digest -match '^sha256:([0-9a-fA-F]{64})$') {
  if ((Get-FileHash -Algorithm SHA256 -LiteralPath $env:VC_OUT).Hash -ne $Matches[1]) {
    Remove-Item -Force -LiteralPath $env:VC_OUT
    throw 'checksum mismatch'
  }
}
Write-Output $n";

/// Starts the background checks, unless this build or run doesn't update itself.
pub fn spawn(automation: bool) {
    let (Some(repo), Some(build)) = (REPO, BUILD) else { return };
    if automation || std::env::var_os("VECTORCRAFT_NO_UPDATE").is_some_and(|v| !v.is_empty() && v != "0") {
        return;
    }
    let spawned = std::thread::Builder::new().name("self-update".into()).spawn(move || {
        loop {
            match run(repo, build) {
                Ok(Some(ready)) => {
                    log::info!("updated to build {} (from {build}); it runs from the next start", ready.0);
                    if let Ok(mut slot) = READY.lock() {
                        *slot = Some(ready);
                    }
                    if let Some(ctx) = CTX.get() {
                        ctx.request_repaint();
                    }
                    // The exe on disk is now newer than any check this process could make.
                    return;
                }
                Ok(None) => log::info!("build {build} is up to date"),
                Err(e) => log::warn!("update check failed: {e}"),
            }
            std::thread::sleep(CHECK_EVERY);
        }
    });
    if let Err(e) = spawned {
        log::warn!("update check not started: {e}");
    }
}

/// Lets a finished update repaint the window to show its banner.
pub fn set_context(ctx: &egui::Context) {
    let _ = CTX.set(ctx.clone());
}

/// Bottom-right banner once a newer build is ready: restart into it now, or later.
pub fn banner(ctx: &egui::Context) {
    if DISMISSED.load(Ordering::Relaxed) || RELAUNCH.load(Ordering::Relaxed) {
        return;
    }
    let Some(build) = READY.lock().ok().and_then(|slot| slot.as_ref().map(|(build, _)| build.clone())) else { return };
    egui::Area::new(egui::Id::new("self-update-banner"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -40.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(egui::RichText::new(format!("Đã có bản mới (build {build})")).strong());
                ui.label("Khởi động lại để dùng ngay, hoặc lần mở sau sẽ tự dùng bản mới.");
                ui.horizontal(|ui| {
                    if ui.button("Khởi động lại để cập nhật").clicked() {
                        RELAUNCH.store(true, Ordering::Relaxed);
                        // The ordinary close: documents with unsaved changes are asked about first.
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("Để sau").clicked() {
                        DISMISSED.store(true, Ordering::Relaxed);
                    }
                });
            });
        });
}

/// As the app exits: starts the new build if "Restart now" was chosen.
pub fn relaunch_if_requested() {
    if !RELAUNCH.load(Ordering::Relaxed) {
        return;
    }
    let Some(exe) = READY.lock().ok().and_then(|slot| slot.as_ref().map(|(_, exe)| exe.clone())) else { return };
    if let Err(e) = std::process::Command::new(&exe).spawn() {
        log::warn!("couldn't start the new build {}: {e}", exe.display());
    }
}

fn run(repo: &str, build: &str) -> Result<Option<(String, PathBuf)>, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let exe = std::env::current_exe().map_err(|e| format!("current exe: {e}"))?;
    let dir = exe.parent().ok_or("the exe has no folder")?;
    let name = exe.file_name().ok_or("the exe has no name")?.to_string_lossy().into_owned();
    remove_leftovers(dir, &name);

    let pid = std::process::id();
    let download = dir.join(format!("{name}.{pid}.new"));
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", SCRIPT])
        .env("VC_REPO", repo)
        .env("VC_BUILD", build)
        .env("VC_ASSET", ASSET)
        .env("VC_OUT", &download)
        .stdin(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("powershell: {e}"))?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&download);
        return Err(String::from_utf8_lossy(&out.stderr).trim().chars().take(500).collect());
    }
    let new = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if new.is_empty() {
        return Ok(None);
    }
    if let Err(e) = check_exe(&download) {
        let _ = std::fs::remove_file(&download);
        return Err(e);
    }
    // Move the running exe aside, then put the new one in its place; undo the first step if the
    // second fails, so the folder always has a working vectorcraft.exe.
    let old = dir.join(format!("{name}.{pid}.old"));
    std::fs::rename(&exe, &old).map_err(|e| {
        let _ = std::fs::remove_file(&download);
        format!("can't replace {} (put it in a folder you can write to): {e}", exe.display())
    })?;
    if let Err(e) = std::fs::rename(&download, &exe) {
        let _ = std::fs::rename(&old, &exe);
        let _ = std::fs::remove_file(&download);
        return Err(format!("can't move the new exe into place: {e}"));
    }
    Ok(Some((new, exe)))
}

/// A downloaded file is only swapped in when it is a plausible Windows executable.
fn check_exe(path: &std::path::Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("reading the download: {e}"))?;
    if bytes.len() < 1024 * 1024 || !bytes.starts_with(b"MZ") {
        return Err(format!("the download is not a Windows exe ({} bytes)", bytes.len()));
    }
    Ok(())
}

/// Deletes the files earlier updates left: replaced exes (once their process has exited) and
/// interrupted downloads. Files still in use fail to delete and are tried again next start.
fn remove_leftovers(dir: &std::path::Path, name: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let prefix = format!("{name}.");
    for entry in entries.flatten() {
        let file = entry.file_name().to_string_lossy().into_owned();
        if file.starts_with(&prefix) && (file.ends_with(".old") || file.ends_with(".new")) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}
