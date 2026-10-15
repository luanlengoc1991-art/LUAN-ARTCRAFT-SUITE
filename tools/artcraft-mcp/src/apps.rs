//! The 13 ArtCraft apps, where they are installed, and how to start their MCP servers.

use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// How an app's `mcp --<bridge>` flag wants the address of the running window.
#[derive(Clone, Copy)]
pub enum Addr {
    /// `127.0.0.1:PORT`
    HostPort,
    /// `PORT`
    Port,
}

/// Flags that confine a headless server's file access to a folder.
#[derive(Clone, Copy)]
pub enum Roots {
    None,
    /// `--automation-read-root DIR --automation-write-root DIR`
    Automation,
    /// `--root DIR` (PdfCraft)
    Root,
}

pub struct AppSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub what: &'static str,
    /// The `mcp` flag that bridges to a running window, and its address form. None: headless only.
    pub bridge: Option<(&'static str, Addr)>,
    /// Control port the gateway opens the window on. Distinct per app, so several can be open.
    pub port: u16,
    pub roots: Roots,
    /// PhotoCraft's control channel is authenticated with a shared token file.
    pub token: bool,
}

const fn app(
    id: &'static str,
    name: &'static str,
    what: &'static str,
    bridge: Option<(&'static str, Addr)>,
    port: u16,
    roots: Roots,
) -> AppSpec {
    AppSpec { id, name, what, bridge, port, roots, token: false }
}

pub const APPS: &[AppSpec] = &[
    AppSpec {
        token: true,
        ..app("photocraft", "PhotoCraft", "image editor", Some(("--bridge", Addr::HostPort)), 7971, Roots::Automation)
    },
    app("vectorcraft", "VectorCraft", "vector illustration", Some(("--connect", Addr::HostPort)), 7979, Roots::Automation),
    app("filmcraft", "FilmCraft", "video editor", Some(("--bridge", Addr::HostPort)), 9876, Roots::None),
    app("lightcraft", "LightCraft", "photo library, raw", Some(("--connect", Addr::HostPort)), 7980, Roots::None),
    app("pdfcraft", "PdfCraft", "PDF workbench", None, 0, Roots::Root),
    app("effectcraft", "EffectCraft", "motion graphics, VFX", Some(("--bridge", Addr::HostPort)), 9877, Roots::None),
    app("designcraft", "DesignCraft", "page layout", Some(("--connect", Addr::Port)), 7984, Roots::None),
    app("figcraft", "FigCraft", "UI design, prototyping", Some(("--attach", Addr::Port)), 7985, Roots::None),
    app("wordcraft", "WordCraft", "word processor", Some(("--connect", Addr::HostPort)), 7981, Roots::Automation),
    app("soundcraft", "SoundCraft", "audio recording, mixing", Some(("--connect", Addr::Port)), 7801, Roots::Automation),
    app("deckcraft", "DeckCraft", "presentations", Some(("--connect", Addr::Port)), 7990, Roots::None),
    app("cadcraft", "CADCraft", "CAD, drafting", Some(("--connect", Addr::HostPort)), 7982, Roots::None),
    app("gridcraft", "GridCraft", "spreadsheets", Some(("--connect", Addr::HostPort)), 7983, Roots::None),
];

pub fn spec(id: &str) -> Option<&'static AppSpec> {
    APPS.iter().find(|a| a.id.eq_ignore_ascii_case(id))
}

/// `%LOCALAPPDATA%\ArtCraft\artcraft-mcp`: config, tool cache, logs, token files.
pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("ArtCraft").join("artcraft-mcp")
}

#[derive(Default, PartialEq, Eq, Clone, Copy)]
pub enum Gui {
    /// Drive the app's window when it is open, otherwise run headless.
    #[default]
    Auto,
    /// Always headless.
    Never,
}

#[derive(Default)]
pub struct AppOverride {
    pub cli: Option<PathBuf>,
    pub port: Option<u16>,
    pub args: Vec<String>,
    pub disabled: bool,
}

/// `config.json` in [`data_dir`]. Every field is optional.
#[derive(Default)]
pub struct Config {
    pub search_dirs: Vec<PathBuf>,
    pub folder: Option<PathBuf>,
    pub gui: Gui,
    pub apps: HashMap<String, AppOverride>,
}

impl Config {
    pub fn path() -> PathBuf {
        data_dir().join("config.json")
    }

    pub fn load(path: &Path) -> Result<Config, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let v: Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut c = Config::default();
        if let Some(dirs) = v["searchDirs"].as_array() {
            c.search_dirs = dirs.iter().filter_map(Value::as_str).map(PathBuf::from).collect();
        }
        c.folder = v["folder"].as_str().filter(|s| !s.is_empty()).map(PathBuf::from);
        if v["gui"].as_str() == Some("never") {
            c.gui = Gui::Never;
        }
        if let Some(apps) = v["apps"].as_object() {
            for (id, a) in apps {
                c.apps.insert(
                    id.to_ascii_lowercase(),
                    AppOverride {
                        cli: a["cli"].as_str().map(PathBuf::from),
                        port: a["port"].as_u64().and_then(|p| u16::try_from(p).ok()),
                        args: a["args"].as_array().map(|x| x.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect()).unwrap_or_default(),
                        disabled: a["disabled"].as_bool().unwrap_or(false),
                    },
                );
            }
        }
        Ok(c)
    }
}

/// An app found on this machine.
pub struct Installed {
    pub spec: &'static AppSpec,
    pub cli: PathBuf,
    pub gui: Option<PathBuf>,
    pub port: u16,
    pub extra_args: Vec<String>,
    /// Size and mtime of the CLI: the tool cache is valid while it is unchanged.
    pub stamp: String,
}

impl Installed {
    pub fn headless_args(&self, cfg: &Config) -> Vec<String> {
        let mut a = vec!["mcp".to_owned()];
        if let Some(dir) = &cfg.folder {
            let dir = dir.display().to_string();
            match self.spec.roots {
                Roots::Automation => a.extend(["--automation-read-root".into(), dir.clone(), "--automation-write-root".into(), dir]),
                Roots::Root => a.extend(["--root".into(), dir]),
                Roots::None => {}
            }
        }
        a.extend(self.extra_args.iter().cloned());
        a
    }

    /// `mcp` arguments that drive the open window, if the app can.
    pub fn bridge_args(&self) -> Option<Vec<String>> {
        let (flag, form) = self.spec.bridge?;
        let addr = match form {
            Addr::HostPort => format!("127.0.0.1:{}", self.port),
            Addr::Port => self.port.to_string(),
        };
        let mut a = vec!["mcp".to_owned(), flag.to_owned(), addr];
        a.extend(self.token_args());
        Some(a)
    }

    /// Arguments that open the window with its control channel on [`Self::port`].
    pub fn gui_args(&self) -> Vec<String> {
        let mut a = vec!["--control".to_owned(), self.port.to_string()];
        a.extend(self.token_args());
        a
    }

    fn token_args(&self) -> Vec<String> {
        if !self.spec.token {
            return Vec::new();
        }
        let file = data_dir().join(format!("{}.token", self.spec.id));
        vec!["--control-token-file".into(), file.display().to_string()]
    }

    pub fn can_open(&self) -> bool {
        self.gui.is_some() && self.spec.bridge.is_some()
    }
}

fn default_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"] {
        if let Some(p) = std::env::var_os(var) {
            dirs.push(PathBuf::from(p));
        }
    }
    if let Some(p) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(p).join("Programs"));
    }
    // The folder this exe sits in, and its parent: a portable layout like D:\Apps\ArtCraft-MCP\.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dirs.push(dir.to_path_buf());
            if let Some(up) = dir.parent() {
                dirs.push(up.to_path_buf());
            }
        }
    }
    dirs
}

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() }
}

/// Looks for `file` in `dir` and up to `depth` levels of subfolders.
fn find_file(dir: &Path, file: &str, depth: u32) -> Option<PathBuf> {
    let direct = dir.join(file);
    if direct.is_file() {
        return Some(direct);
    }
    if depth == 0 {
        return None;
    }
    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    subdirs.sort();
    subdirs.iter().find_map(|d| find_file(d, file, depth - 1))
}

fn find_cli(spec: &AppSpec, dirs: &[PathBuf]) -> Option<PathBuf> {
    let cli = exe(&format!("{}-cli", spec.id));
    for dir in dirs {
        if let Some(p) = find_file(dir, &cli, 0) {
            return Some(p);
        }
        for sub in [spec.name, spec.id] {
            if let Some(p) = find_file(&dir.join(sub), &cli, 2) {
                return Some(p);
            }
        }
    }
    None
}

fn stamp(path: &Path) -> String {
    match std::fs::metadata(path) {
        Ok(m) => {
            let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
            format!("{}-{t}", m.len())
        }
        Err(_) => String::new(),
    }
}

pub fn discover(cfg: &Config) -> Vec<Installed> {
    let mut dirs = cfg.search_dirs.clone();
    dirs.extend(default_search_dirs());
    let mut found = Vec::new();
    for spec in APPS {
        let o = cfg.apps.get(spec.id);
        if o.is_some_and(|o| o.disabled) {
            continue;
        }
        let cli = match o.and_then(|o| o.cli.clone()) {
            Some(p) if p.is_file() => Some(p),
            Some(_) => None,
            None => find_cli(spec, &dirs),
        };
        let Some(cli) = cli else { continue };
        let gui = cli.parent().map(|d| d.join(exe(spec.id))).filter(|p| p.is_file());
        found.push(Installed {
            spec,
            stamp: stamp(&cli),
            gui,
            port: o.and_then(|o| o.port).unwrap_or(spec.port),
            extra_args: o.map(|o| o.args.clone()).unwrap_or_default(),
            cli,
        });
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_ports_are_unique() {
        for (i, a) in APPS.iter().enumerate() {
            for b in &APPS[i + 1..] {
                assert_ne!(a.id, b.id);
                if a.bridge.is_some() && b.bridge.is_some() {
                    assert_ne!(a.port, b.port, "{} and {}", a.id, b.id);
                }
            }
        }
        assert_eq!(APPS.len(), 13);
    }

    #[test]
    fn bridge_args_follow_each_apps_flag() {
        let inst = |id: &str| Installed {
            spec: spec(id).unwrap(),
            cli: PathBuf::from("x"),
            gui: None,
            port: spec(id).unwrap().port,
            extra_args: vec![],
            stamp: String::new(),
        };
        assert_eq!(inst("vectorcraft").bridge_args().unwrap(), ["mcp", "--connect", "127.0.0.1:7979"]);
        assert_eq!(inst("figcraft").bridge_args().unwrap(), ["mcp", "--attach", "7985"]);
        assert!(inst("pdfcraft").bridge_args().is_none());
        let photo = inst("photocraft").bridge_args().unwrap();
        assert_eq!(photo[..3], ["mcp", "--bridge", "127.0.0.1:7971"]);
        assert_eq!(photo[3], "--control-token-file");
    }

    #[test]
    fn headless_args_confine_to_the_folder() {
        let cfg = Config { folder: Some(PathBuf::from("W")), ..Config::default() };
        let mk = |id: &str| Installed {
            spec: spec(id).unwrap(),
            cli: PathBuf::from("x"),
            gui: None,
            port: 0,
            extra_args: vec![],
            stamp: String::new(),
        };
        assert_eq!(mk("pdfcraft").headless_args(&cfg), ["mcp", "--root", "W"]);
        assert_eq!(mk("gridcraft").headless_args(&cfg), ["mcp"]);
        assert_eq!(mk("vectorcraft").headless_args(&cfg).len(), 5);
    }
}
