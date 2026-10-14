//! Windows only: keeps a portable `vectorcraft.exe` up to date from this fork's GitHub Releases.
//!
//! `.github/workflows/windows-autobuild.yml` builds every push to `main` with
//! `VECTORCRAFT_UPDATE_REPO` (owner/name) and `VECTORCRAFT_UPDATE_BUILD` (the run number) set, and
//! publishes `vectorcraft.exe` and `vectorcraft-cli.exe` (the MCP server) as the latest release,
//! tagged `build-<run number>`. A build made that way checks the latest release in the background at
//! start-up and every [`CHECK_EVERY`] after; when it is newer, it downloads both exes next to the
//! running one, checks their SHA-256 against the ones GitHub reports, and swaps the files (Windows
//! lets a running exe be renamed, the CLI serving an MCP client included). A banner ([`banner`])
//! then offers to restart into it; otherwise the next start runs it.
//!
//! Builds without those variables (local `cargo build`, the signed release pipeline) never check.
//! `VECTORCRAFT_NO_UPDATE=1` turns it off, and so do runs without preferences (agents' test runs).
//! No new crates: the HTTP work is done by Windows PowerShell, hidden, in a thread that never blocks
//! the window.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

const REPO: Option<&str> = option_env!("VECTORCRAFT_UPDATE_REPO");
const BUILD: Option<&str> = option_env!("VECTORCRAFT_UPDATE_BUILD");

/// The release asset that replaces the running exe.
const ASSET: &str = "vectorcraft.exe";
/// Updated alongside it when the release has it, so the MCP server speaks the app's protocol.
const CLI_ASSET: &str = "vectorcraft-cli.exe";

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

/// Queries the latest release and, when its build number is higher, downloads each asset named in
/// `VC_ASSETS` it has to `<VC_DIR>\<name>.<VC_TAG>.new`. Prints the new build number on success.
/// Single quotes only: `powershell.exe -Command` drops double quotes.
const SCRIPT: &str = r"$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$ProgressPreference = 'SilentlyContinue'
$h = @{ 'User-Agent' = 'vectorcraft-updater'; 'Accept' = 'application/vnd.github+json' }
$r = Invoke-RestMethod -UseBasicParsing -Headers $h -Uri ('https://api.github.com/repos/' + $env:VC_REPO + '/releases/latest')
if ($r.tag_name -notmatch '^build-(\d+)$') { exit 0 }
$n = [int64]$Matches[1]
if ($n -le [int64]$env:VC_BUILD) { exit 0 }
$names = $env:VC_ASSETS -split ';'
if (-not ($r.assets | Where-Object { $_.name -eq $names[0] })) { exit 0 }
foreach ($a in ($r.assets | Where-Object { $names -contains $_.name })) {
  $out = Join-Path $env:VC_DIR ($a.name + '.' + $env:VC_TAG + '.new')
  Invoke-WebRequest -UseBasicParsing -Headers @{ 'User-Agent' = 'vectorcraft-updater' } -Uri $a.browser_download_url -OutFile $out
  if ($a.digest -match '^sha256:([0-9a-fA-F]{64})$') {
    if ((Get-FileHash -Algorithm SHA256 -LiteralPath $out).Hash -ne $Matches[1]) {
      Remove-Item -Force -LiteralPath $out
      throw ('checksum mismatch: ' + $a.name)
    }
  }
}
Write-Output $n";

/// Starts the background checks, unless this build or run doesn't update itself.
pub fn spawn(test_run: bool) {
    let (Some(repo), Some(build)) = (REPO, BUILD) else { return };
    if test_run || std::env::var_os("VECTORCRAFT_NO_UPDATE").is_some_and(|v| !v.is_empty() && v != "0") {
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

/// As the app exits: starts the new build if "Restart now" was chosen, with this run's arguments
/// (`--control <port>` among them, so an MCP client reaches the new build too).
pub fn relaunch_if_requested() {
    if !RELAUNCH.load(Ordering::Relaxed) {
        return;
    }
    let Some(exe) = READY.lock().ok().and_then(|slot| slot.as_ref().map(|(_, exe)| exe.clone())) else { return };
    if let Err(e) = std::process::Command::new(&exe).args(std::env::args_os().skip(1)).spawn() {
        log::warn!("couldn't start the new build {}: {e}", exe.display());
    }
}

fn run(repo: &str, build: &str) -> Result<Option<(String, PathBuf)>, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let exe = std::env::current_exe().map_err(|e| format!("current exe: {e}"))?;
    let dir = exe.parent().ok_or("the exe has no folder")?.to_path_buf();
    let name = exe.file_name().ok_or("the exe has no name")?.to_string_lossy().into_owned();
    for file in [name.as_str(), ASSET, CLI_ASSET] {
        remove_leftovers(&dir, file);
    }

    let pid = std::process::id().to_string();
    let downloads = [ASSET, CLI_ASSET].map(|asset| dir.join(format!("{asset}.{pid}.new")));
    let discard = || {
        for d in &downloads {
            let _ = std::fs::remove_file(d);
        }
    };
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", SCRIPT])
        .env("VC_REPO", repo)
        .env("VC_BUILD", build)
        .env("VC_ASSETS", format!("{ASSET};{CLI_ASSET}"))
        .env("VC_DIR", &dir)
        .env("VC_TAG", &pid)
        .stdin(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("powershell: {e}"))?;
    if !out.status.success() {
        discard();
        return Err(String::from_utf8_lossy(&out.stderr).trim().chars().take(500).collect());
    }
    let new = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if new.is_empty() {
        discard();
        return Ok(None);
    }
    let [app_download, cli_download] = &downloads;
    if let Err(e) = swap(app_download, &exe, &pid) {
        discard();
        return Err(e);
    }
    // The MCP server, when the release has it: a failure leaves the old one, not the app's update.
    if cli_download.exists()
        && let Err(e) = swap(cli_download, &dir.join(CLI_ASSET), &pid)
    {
        log::warn!("{CLI_ASSET} not updated: {e}");
        discard();
    }
    Ok(Some((new, exe)))
}

/// Puts `download` in place of `target`, moving a `target` that is running aside first; undoes that
/// if the second step fails, so the folder always keeps a working exe.
fn swap(download: &std::path::Path, target: &std::path::Path, pid: &str) -> Result<(), String> {
    check_exe(download)?;
    let old = target.with_file_name(format!("{}.{pid}.old", target.file_name().unwrap_or_default().to_string_lossy()));
    let had_target = target.exists();
    if had_target {
        std::fs::rename(target, &old)
            .map_err(|e| format!("can't replace {} (put it in a folder you can write to): {e}", target.display()))?;
    }
    if let Err(e) = std::fs::rename(download, target) {
        if had_target {
            let _ = std::fs::rename(&old, target);
        }
        return Err(format!("can't move the new {} into place: {e}", target.display()));
    }
    Ok(())
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
