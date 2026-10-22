//! macOS implementation of the platform surface.

use std::path::{Path, PathBuf};

use tokio::process::Command;

use super::{percent_encode_path, resolve_browser_exe};

pub const OS: &str = "macos";

/// PowerShell, `cmd` and WSL are absent here, and no attempt is made to find
/// `pwsh`: a PowerShell script written against Windows paths and COM objects
/// would not work under it anyway, so offering it would promise more than it
/// could deliver.
pub const RUNNABLE_SCRIPT_TYPES: &[&str] = &["zsh", "bash", "python", "node"];

pub const PREFERRED_APPS: &[&str] = &["default", "safari", "chrome", "edge", "firefox"];

pub const TERMINAL_LABEL: &str = "Terminal";
pub const FILE_MANAGER_LABEL: &str = "Finder";
pub const PATH_SEPARATOR: &str = "/";

/// macOS stacks its notifications down from the top-right.
pub const ALERT_AT_TOP: bool = true;

/// Directories a developer's tools actually live in, which a GUI app does not
/// inherit.
///
/// An app launched from the Dock or by `launchd` gets the bare system PATH
/// (`/usr/bin:/bin:/usr/sbin:/sbin`) rather than the one the user's shell
/// builds. Homebrew installs nothing there, so `node` and `python3` resolve
/// fine in `tauri dev` from a terminal and then fail with "No such file or
/// directory" in the shipped app — a difference that only shows up after
/// release. Appending rather than prepending keeps a system interpreter
/// authoritative, and the existing `python_path` / `node_path` settings remain
/// the override for anything unusual.
const EXTRA_PATH: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin", "/opt/local/bin"];

/// Applied to every child process Orion spawns.
///
/// One hook rather than a flag at each call site, because the two platforms
/// need different things here and the call sites should not have to know which:
/// Windows needs the console window suppressed, macOS needs the PATH repaired.
pub fn prepare_child(command: &mut Command) {
    let current = std::env::var("PATH").unwrap_or_default();
    let mut parts: Vec<&str> = current.split(':').filter(|p| !p.is_empty()).collect();
    for extra in EXTRA_PATH {
        if !parts.contains(extra) {
            parts.push(extra);
        }
    }
    command.env("PATH", parts.join(":"));
}

pub fn reveal_path(path: &Path) -> Result<(), String> {
    Command::new("/usr/bin/open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to open Finder: {e}"))
}

pub fn open_terminal_at(path: &str) -> Result<(), String> {
    Command::new("/usr/bin/open")
        .args(["-a", "Terminal", path])
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to open Terminal here: {e}"))
}

pub fn open_url_default(url: &str) -> Result<(), String> {
    Command::new("/usr/bin/open")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to open URL in default browser: {e}"))
}

/// Resolves the executable *inside* an application bundle.
///
/// `open -a "Google Chrome" --args --profile-directory=X` is the obvious
/// approach and it silently drops the arguments whenever Chrome is already
/// running, which is most of the time — the link opens in whichever profile
/// happened to be frontmost. Invoking the bundled binary directly behaves the
/// same way as the Windows path does, so a per-profile link means the same
/// thing on both platforms.
fn bundled_exe(app_name: &str, binary: &str, display: &str) -> Result<PathBuf, String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let relative = format!("{app_name}.app/Contents/MacOS/{binary}");
    resolve_browser_exe(
        &[
            PathBuf::from("/Applications").join(&relative),
            PathBuf::from(home).join("Applications").join(&relative),
        ],
        display,
    )
}

/// Opens `url` in a specific browser, honouring `profile` where the browser
/// supports it. `app` has already been validated against [`PREFERRED_APPS`].
pub fn open_url_in(app: &str, url: &str, profile: Option<&str>) -> Result<(), String> {
    // Safari has no profile switch and no supported way to be driven by its
    // bundled binary, so it goes through `open` like the system default.
    if app == "safari" {
        return Command::new("/usr/bin/open")
            .args(["-a", "Safari", url])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("failed to launch Safari: {e}"));
    }

    let (exe, display) = match app {
        "chrome" => (bundled_exe("Google Chrome", "Google Chrome", "Chrome")?, "Chrome"),
        "edge" => (
            bundled_exe("Microsoft Edge", "Microsoft Edge", "Microsoft Edge")?,
            "Edge",
        ),
        "firefox" => (bundled_exe("Firefox", "firefox", "Firefox")?, "Firefox"),
        other => return Err(format!("Unknown preferred app '{other}'")),
    };

    let mut command = Command::new(exe);
    if let Some(profile) = profile {
        if app == "firefox" {
            command.args(["-P", profile]);
        } else {
            command.arg(format!("--profile-directory={profile}"));
        }
    }
    command
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to launch {display}: {e}"))
}

/// Spawns the user-configured IDE command against a path. A direct spawn is
/// attempted first (covers absolute paths and anything on PATH); the fallback
/// goes through a login shell, which is what finds a `code` shim installed in
/// `/usr/local/bin` or by a version manager.
pub fn spawn_ide(command_str: &str, target_path: &str) -> Result<(), String> {
    let mut direct = Command::new(command_str);
    direct.arg(target_path);
    prepare_child(&mut direct);

    match direct.spawn() {
        Ok(_) => Ok(()),
        Err(direct_err) => {
            // The command and the path are passed as positional parameters
            // rather than interpolated into the script, so neither can be
            // read as shell syntax however it is spelled.
            Command::new("/bin/sh")
                .args(["-lc", "exec \"$0\" \"$1\"", command_str, target_path])
                .spawn()
                .map(|_| ())
                .map_err(|fallback_err| {
                    format!(
                        "failed to launch IDE '{command_str}' \
                         (direct: {direct_err}; via shell: {fallback_err})"
                    )
                })
        }
    }
}

/// The extension the temporary script file needs for its interpreter to accept
/// it. Every interpreter here is invoked explicitly against the file, so the
/// extension is for legibility in a crash log rather than a requirement.
pub fn script_extension(script_type: &str) -> &'static str {
    match script_type {
        "python" => "py",
        "node" => "js",
        _ => "sh",
    }
}

/// Builds the interpreter invocation for a script Orion has already confirmed
/// it can run on this platform.
pub fn interpreter(
    script_type: &str,
    temp_path: &Path,
    custom_python: Option<&str>,
    custom_node: Option<&str>,
) -> Command {
    let mut command = match script_type {
        // Absolute paths: both ship with the OS, and this is the one place
        // where falling back to PATH could run something unexpected.
        "zsh" => {
            let mut c = Command::new("/bin/zsh");
            c.arg(temp_path);
            c
        }
        "bash" => {
            let mut c = Command::new("/bin/bash");
            c.arg(temp_path);
            c
        }
        "python" => {
            // There is no `python` on a stock macOS — only `python3`.
            let python_exe = custom_python.unwrap_or("python3");
            log::info!("Using Python interpreter: {python_exe}");
            let mut c = Command::new(python_exe);
            c.arg(temp_path);
            c
        }
        "node" => {
            let node_exe = custom_node.unwrap_or("node");
            log::info!("Using Node.js interpreter: {node_exe}");
            let mut c = Command::new(node_exe);
            c.arg(temp_path);
            c
        }
        other => unreachable!("script type '{other}' should have been validated"),
    };
    prepare_child(&mut command);
    command
}

/// Extra environment a script's interpreter needs on this platform. Nothing
/// here needs a bridge like WSLENV — the variables are inherited directly.
pub fn extra_script_env(_script_type: &str, _names: &[&str]) -> Vec<(String, String)> {
    Vec::new()
}

/// True for an absolute native path. Relative paths are deliberately excluded —
/// they have no meaning without a working directory, and accepting them would
/// turn a typo into a silently broken resource.
pub fn is_absolute_native_path(target: &str) -> bool {
    // A Windows path is not absolute here. It is also not something to try to
    // launch: storing it as a file:// URL would produce a resource that fails
    // at launch with an obscure error instead of refusing up front.
    target.starts_with('/')
}

/// Turns an absolute native path into the `file://` URL the launcher expects.
/// Only called for a path [`is_absolute_native_path`] has already accepted, so
/// it already begins with the slash that completes the empty authority.
pub fn file_url_from_native_path(path: &str) -> String {
    format!("file://{}", percent_encode_path(path))
}

/// Where a pre-Milestone-7 install kept its database. Orion never shipped for
/// macOS before this port, so there is nothing to carry forward.
pub fn legacy_data_dir() -> Option<PathBuf> {
    None
}

/// Marks the tray icon as a template image so the system tints it — black on a
/// light menu bar, white on a dark one. Without this the icon keeps its own
/// colours and goes nearly invisible in one of the two appearances.
pub fn finish_tray(tray: &tauri::tray::TrayIcon) {
    if let Err(e) = tray.set_icon_as_template(true) {
        log::warn!("Could not mark the tray icon as a template: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_paths_are_absolute() {
        for path in ["/Users/me/cv.pdf", "/", "/Volumes/Backup/notes.md"] {
            assert!(is_absolute_native_path(path), "{path}");
        }
    }

    #[test]
    fn relative_and_windows_paths_are_not() {
        for path in [
            "cv.pdf",
            "./cv.pdf",
            "Users/me/cv.pdf",
            "",
            "C:\\Users\\me\\cv.pdf",
            "\\\\server\\share",
            "https://example.com",
        ] {
            assert!(!is_absolute_native_path(path), "{path}");
        }
    }

    #[test]
    fn every_runnable_type_has_an_extension() {
        for t in RUNNABLE_SCRIPT_TYPES {
            assert!(!script_extension(t).is_empty(), "{t}");
        }
    }

    #[test]
    fn shell_scripts_are_written_as_sh() {
        assert_eq!(script_extension("zsh"), "sh");
        assert_eq!(script_extension("bash"), "sh");
    }

    #[test]
    fn a_posix_path_becomes_a_file_url_with_three_slashes() {
        // Four slashes (file:////Users/...) is the easy mistake here, and it
        // resolves to a host of "" with the path /Users — which no longer
        // points at the file.
        assert_eq!(
            file_url_from_native_path("/Users/me/Resume.pdf"),
            "file:///Users/me/Resume.pdf"
        );
    }

    #[test]
    fn spaces_and_hashes_are_encoded() {
        assert_eq!(
            file_url_from_native_path("/Users/me/My Docs/draft #2.pdf"),
            "file:///Users/me/My%20Docs/draft%20%232.pdf"
        );
    }

    #[test]
    fn no_environment_bridge_is_needed() {
        assert!(extra_script_env("bash", &["API_KEY"]).is_empty());
    }
}
