//! LUAN-ARTCRAFT-SUITE: the same apps and ArtCraft MCP on every one of the owner's computers.
//!
//! - The suite list (`config/suite-sync.json` in the suite repo) names the apps every computer
//!   keeps: each one missing here is installed once it is known not to be installed.
//! - ArtCraft MCP gets a bar above the apps: its version, whether Claude Code and Cursor have it,
//!   and Install / Update. It installs by itself when the list asks for it and updates with
//!   automatic updates.
//! - Whatever the launcher installs or updates by itself ends with a system notification.

use artcraft_launcher_core::protocol::{Command, JobPhase};
use artcraft_launcher_core::status::AppStatus;
use artcraft_launcher_core::suite::{MCP_ID, MCP_NAME};
use egui::{RichText, Ui};

use crate::app::{Host, LauncherUi};
use crate::i18n::fmt;
use crate::theme::{self, Tokens};
use crate::widgets::{self, Kind};

impl LauncherUi {
    /// Installs the listed apps this computer lacks, and keeps ArtCraft MCP installed and current.
    pub(crate) fn sync_suite(&mut self) {
        if !self.caps.manage_apps {
            return;
        }
        if self.settings.sync_suite
            && let Some(list) = self.suite_sync.clone()
        {
            let (platform, manage) = (self.platform, self.caps.manage_apps);
            let missing: Vec<String> = list
                .apps
                .iter()
                .filter(|id| !self.sync_attempted.contains(*id))
                .filter(|id| {
                    self.entry(id).is_some_and(|e| {
                        e.detected && e.job.is_none() && !e.ready && e.paused.is_none() && e.status(platform, manage) == AppStatus::Available
                    })
                })
                .cloned()
                .collect();
            for id in missing {
                log::info!("suite sync: installing {id}, which the suite list has and this computer doesn't");
                self.sync_attempted.insert(id.clone());
                self.auto_jobs.insert(id.clone());
                self.send(Command::Install { app: id });
            }
        }
        let Some(mcp) = &self.mcp else { return };
        if !mcp.supported || self.mcp_job.is_some() {
            return;
        }
        let listed = self.settings.sync_suite && self.suite_sync.as_ref().is_some_and(|s| s.mcp);
        let wanted = (mcp.installed.is_none() && listed) || (mcp.update_available() && self.settings.auto_update);
        if let Some(v) = mcp.latest.as_ref().map(|r| r.version_label())
            && wanted
            && self.mcp_attempted.as_ref() != Some(&v)
        {
            log::info!("suite sync: installing {MCP_NAME} {v}");
            self.mcp_attempted = Some(v);
            self.auto_jobs.insert(MCP_ID.to_string());
            self.send(Command::InstallMcp);
        }
    }

    /// A job the launcher started by itself ended: tell the user what it did.
    pub(crate) fn auto_job_finished(&mut self, app: &str, result: &Result<String, String>) {
        // The launcher's own update is always automatic: its first start says so.
        let auto = self.auto_jobs.remove(app) || app == artcraft_launcher_core::catalog::LAUNCHER_ID;
        if auto && let Ok(text) = result {
            self.done_notices.push(text.clone());
        }
    }

    /// Local date and time, as in "Oct 15, 2026 11:42" (the month in the UI's language).
    pub(crate) fn local_time_text(&self, unix: i64) -> String {
        let local = unix + self.utc_offset;
        let secs = local.rem_euclid(86_400);
        format!("{} {:02}:{:02}", crate::views::date_text(local), secs / 3600, secs % 3600 / 60)
    }

    /// Sends the notifications [`Self::auto_job_finished`] collected.
    pub(crate) fn flush_done_notices(&mut self, host: &mut dyn Host) {
        let done = std::mem::take(&mut self.done_notices);
        if !self.settings.notify || done.is_empty() {
            return;
        }
        // When it happened, so the user knows which update this was.
        let when = format!("ArtCraft Launcher · {}", self.local_time_text(self.backend.now()));
        for text in done {
            // The backend's messages ("PhotoCraft updated to 0.9.3"), translated like toasts.
            host.notify(&crate::i18n::msg(&text), &when);
        }
    }

    /// ArtCraft MCP's bar above the apps (Windows, where it is built).
    pub(crate) fn mcp_bar(&mut self, ui: &mut Ui) {
        let Some(mcp) = self.mcp.clone() else { return };
        if !mcp.supported || !self.caps.manage_apps {
            return;
        }
        let t = Tokens::get(ui.ctx());
        let update = mcp.update_available();
        egui::Frame::new().fill(t.sunken).stroke(t.hairline()).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::tag(ui, "MCP", t.invert_bg, t.invert_fg);
                ui.add_space(4.0);
                ui.label(RichText::new(MCP_NAME).family(theme::semibold()).color(t.ink_strong));
                let version = match (&mcp.installed, mcp.installed_at) {
                    // The version, and the day it was put on this computer.
                    (Some(v), Some(at)) => format!("{v} · {}", crate::views::date_text(at + self.utc_offset)),
                    (Some(v), None) => v.to_string(),
                    (None, _) => tl!("Not installed").to_string(),
                };
                widgets::badge(ui, &version, if update { t.warn } else { t.muted });
                for (client, registered) in [("Claude Code", mcp.claude), ("Cursor", mcp.cursor), ("Claude Desktop", mcp.desktop)] {
                    if let Some(on) = registered {
                        widgets::badge(ui, client, if on { t.good } else { t.faint });
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(phase) = self.mcp_job {
                        let (fraction, caption) = match phase {
                            JobPhase::Downloading { done, total } if total > 0 => (
                                Some(done as f32 / total as f32),
                                fmt(
                                    tl!("Downloading {done} of {total}"),
                                    &[("done", &widgets::human_bytes(done)), ("total", &widgets::human_bytes(total))],
                                ),
                            ),
                            JobPhase::Downloading { .. } | JobPhase::Reconnecting { .. } => (None, tl!("Downloading…").to_string()),
                            JobPhase::Verifying => (None, tl!("Verifying…").to_string()),
                            JobPhase::Waiting => (None, tl!("Waiting for other installs…").to_string()),
                            JobPhase::Installing | JobPhase::Removing => (None, tl!("Installing…").to_string()),
                        };
                        widgets::progress(ui, fraction, &caption, t.accent, 200.0);
                        return;
                    }
                    let action = match (&mcp.installed, &mcp.latest) {
                        (None, Some(_)) => Some(tl!("Install")),
                        (Some(_), Some(_)) if update => Some(tl!("Update")),
                        _ => None,
                    };
                    if let Some(label) = action
                        && widgets::button(ui, label, Kind::Primary, 0.0).clicked()
                    {
                        self.mcp_attempted = mcp.latest.as_ref().map(|r| r.version_label());
                        self.send(Command::InstallMcp);
                    }
                    ui.add_space(8.0);
                    let about = tl!("One MCP server for Claude Code and Cursor, with the tools of every installed app");
                    ui.add(egui::Label::new(RichText::new(about).size(12.0).color(t.muted)).truncate());
                });
            });
        });
        ui.add_space(8.0);
    }
}
