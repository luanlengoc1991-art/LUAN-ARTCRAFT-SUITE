//! LUAN-ARTCRAFT-SUITE: the owner's suite, the same on every computer.
//!
//! - `config/suite-sync.json` in the suite repo lists the apps every one of the owner's computers
//!   keeps installed ([`SuiteSync`]); each launcher installs the ones it is missing.
//! - ArtCraft MCP (`tools/artcraft-mcp`, one MCP server for every installed app) is installed,
//!   updated and registered with Claude Code and Cursor by the launcher ([`McpState`]).

use serde::{Deserialize, Serialize};

use crate::release::Release;
use crate::version::{self, Version};

/// The monorepo: the sync list and ArtCraft MCP's releases live here.
pub const SUITE_REPO: &str = "luanlengoc1991-art/LUAN-ARTCRAFT-SUITE";
/// The list of apps every computer keeps installed.
pub const SYNC_URL: &str = "https://raw.githubusercontent.com/luanlengoc1991-art/LUAN-ARTCRAFT-SUITE/main/config/suite-sync.json";
/// Job id of ArtCraft MCP's install (in `Event::Progress` / `Event::JobFinished`).
pub const MCP_ID: &str = "artcraft-mcp";
pub const MCP_NAME: &str = "ArtCraft MCP";
/// The release file, under a fixed name (`.github/workflows/artcraft-mcp.yml`).
pub const MCP_ASSET: &str = "ArtCraft-MCP.exe";
/// Release tags are `artcraft-mcp-v<version>`.
pub const MCP_TAG_PREFIX: &str = "artcraft-mcp-v";
/// The MCP server's name in Claude Code and Cursor.
pub const MCP_SERVER: &str = "artcraft";

/// `config/suite-sync.json`: what every one of the owner's computers has.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuiteSync {
    /// App ids (`photocraft`, …) to keep installed. Unknown ids are dropped.
    #[serde(default)]
    pub apps: Vec<String>,
    /// Keep ArtCraft MCP installed and registered.
    #[serde(default)]
    pub mcp: bool,
}

impl SuiteSync {
    /// Reads the list; it comes from the network, so only known app ids survive.
    pub fn parse(json: &str) -> Result<Self, String> {
        let mut s: SuiteSync = serde_json::from_str(json).map_err(|e| format!("unreadable suite list ({e})"))?;
        s.apps.retain(|id| crate::CraftApp::by_id(id).is_some());
        s.apps.dedup();
        Ok(s)
    }
}

/// ArtCraft MCP on this computer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct McpState {
    /// This platform has an ArtCraft MCP build (Windows).
    pub supported: bool,
    /// The installed version, if any.
    pub installed: Option<Version>,
    /// The newest release, once known.
    pub latest: Option<Release>,
    /// Registered with Claude Code; `None` when Claude Code isn't on this computer.
    pub claude: Option<bool>,
    /// Registered with Cursor; `None` when Cursor isn't on this computer.
    pub cursor: Option<bool>,
    /// Registered with the Claude desktop app (its chat); `None` when it isn't on this computer.
    pub desktop: Option<bool>,
    /// When the installed copy was put in place (Unix seconds).
    pub installed_at: Option<i64>,
}

impl McpState {
    pub fn latest_version(&self) -> Option<&Version> {
        self.latest.as_ref().and_then(|r| r.version.as_ref())
    }

    /// Installed, and a newer release is out.
    pub fn update_available(&self) -> bool {
        matches!((self.latest_version(), &self.installed), (Some(l), Some(i)) if version::is_newer(l, i))
    }
}

/// `artcraft-mcp-v0.1.0` → `0.1.0`.
pub fn mcp_version_from_tag(tag: &str) -> Option<Version> {
    version::parse(tag.strip_prefix(MCP_TAG_PREFIX)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_list_keeps_known_apps_only() {
        let s = SuiteSync::parse(r#"{"apps":["photocraft","nope","gridcraft"],"mcp":true}"#).unwrap();
        assert_eq!(s.apps, ["photocraft", "gridcraft"]);
        assert!(s.mcp);
        assert_eq!(SuiteSync::parse("{}").unwrap(), SuiteSync::default());
        assert!(SuiteSync::parse("not json").is_err());
    }

    #[test]
    fn mcp_tags_and_updates() {
        assert_eq!(mcp_version_from_tag("artcraft-mcp-v0.2.0"), version::parse("0.2.0"));
        assert_eq!(mcp_version_from_tag("v0.2.0"), None);
        let latest = Release { tag: "artcraft-mcp-v0.2.0".into(), version: version::parse("0.2.0"), ..Release::default() };
        let mut s = McpState { latest: Some(latest), installed: version::parse("0.1.0"), ..McpState::default() };
        assert!(s.update_available());
        s.installed = version::parse("0.2.0");
        assert!(!s.update_available());
        s.installed = None;
        assert!(!s.update_available(), "not installed is not an update");
    }
}
