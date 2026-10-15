//! artcraft-mcp: one MCP server (stdio) for every ArtCraft app installed on this machine.
//!
//! It does not change any app. Each app keeps its own MCP server (`<id>-cli mcp`); this program
//! starts them on demand and forwards calls, so one registration in Claude Code or Cursor reaches
//! all of them. See README.md.

mod apps;
mod child;
mod gateway;

use std::path::PathBuf;

const HELP: &str = "\
artcraft-mcp: one MCP server for all installed ArtCraft apps

USAGE
  artcraft-mcp [--compact] [--headless] [--config FILE]   MCP server on stdio
  artcraft-mcp --list [--config FILE]                     show installed apps and test each MCP server

  --compact    expose only the artcraft_* tools (for clients that cap the tool count)
  --headless   never drive app windows; every app runs headless
  --config     config file (default %LOCALAPPDATA%\\ArtCraft\\artcraft-mcp\\config.json)
";

fn main() {
    let mut opts = gateway::Options::default();
    let mut list = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--compact" => opts.compact = true,
            "--headless" => opts.headless = true,
            "--list" => list = true,
            "--config" => opts.config = args.next().map(PathBuf::from),
            "-V" | "--version" => {
                println!("artcraft-mcp {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "-h" | "--help" => {
                print!("{HELP}");
                return;
            }
            other => {
                eprintln!("artcraft-mcp: unknown argument `{other}`\n\n{HELP}");
                std::process::exit(2);
            }
        }
    }
    keep_std_handles_private();
    let r = if list { gateway::print_apps(opts) } else { gateway::serve(opts) };
    if let Err(e) = r {
        eprintln!("artcraft-mcp: {e}");
        std::process::exit(1);
    }
}

/// Windows hands every inheritable handle to a child process, not just the three it is given. An
/// app window opened by `artcraft_open` would then hold the client's pipes open after this server
/// exits, so the client never sees end-of-file. Our own stdio is ours alone.
#[cfg(windows)]
fn keep_std_handles_private() {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(which: u32) -> *mut c_void;
        fn SetHandleInformation(handle: *mut c_void, mask: u32, flags: u32) -> i32;
    }
    const HANDLE_FLAG_INHERIT: u32 = 1;
    // STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE are -10, -11, -12.
    for which in [-10i32, -11, -12] {
        // SAFETY: plain Win32 calls on this process's own standard handles.
        unsafe {
            let h = GetStdHandle(which as u32);
            if !h.is_null() && h as isize != -1 {
                SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
}

#[cfg(not(windows))]
fn keep_std_handles_private() {}
