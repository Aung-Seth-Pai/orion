//! Windows implementation of the platform surface.

use std::path::{Path, PathBuf};

use tokio::process::Command;

use super::{percent_encode_path, resolve_browser_exe};

pub const OS: &str = "windows";

/// Hides the console window a spawned interpreter would otherwise flash up.
/// Without it every script run blinks a black rectangle on screen.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub const RUNNABLE_SCRIPT_TYPES: &[&str] = &["powershell", "cmd", "wsl_bash", "python", "node"];

pub const PREFERRED_APPS: &[&str] = &["default", "chrome", "edge", "firefox"];

pub const TERMINAL_LABEL: &str = "Windows Terminal";
pub const FILE_MANAGER_LABEL: &str = "Explorer";
pub const PATH_SEPARATOR: &str = "\\";

/// Windows parks its own notifications in the bottom-right, above the taskbar.
pub const ALERT_AT_TOP: bool = false;

/// Applied to every child process Orion spawns.
///
/// One hook rather than a flag at each call site, because the two platforms
/// need different things here and the call sites should not have to know which:
/// Windows needs the console window suppressed, macOS needs the PATH repaired.
pub fn prepare_child(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

pub fn reveal_path(path: &Path) -> Result<(), String> {
    // explorer.exe deliberately gets no prepare_child: it is a GUI process with
    // no console to hide, and it returns a non-zero exit code even on success.
    Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to launch Explorer: {e}"))
}

pub fn open_terminal_at(path: &str) -> Result<(), String> {
    Command::new("wt")
        .args(["-d", path])
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to open terminal here (is Windows Terminal installed?): {e}"))
}

pub fn open_url_default(url: &str) -> Result<(), String> {
    // The empty "" is the window-title argument `start` expects; without it a
    // quoted URL is taken as the title and nothing opens.
    let mut command = Command::new("cmd");
    command.args(["/C", "start", "", url]);
    prepare_child(&mut command);
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("failed to open URL in default browser: {e}"))
}

fn chrome_exe() -> Result<PathBuf, String> {
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    resolve_browser_exe(
        &[
            PathBuf::from("C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"),
            PathBuf::from("C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe"),
            PathBuf::from(local).join("Google\\Chrome\\Application\\chrome.exe"),
        ],
        "Chrome",
    )
}

fn edge_exe() -> Result<PathBuf, String> {
    resolve_browser_exe(
        &[
            PathBuf::from("C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe"),
            PathBuf::from("C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe"),
        ],
        "Microsoft Edge",
    )
}

fn firefox_exe() -> Result<PathBuf, String> {
    resolve_browser_exe(
        &[
            PathBuf::from("C:\\Program Files\\Mozilla Firefox\\firefox.exe"),
            PathBuf::from("C:\\Program Files (x86)\\Mozilla Firefox\\firefox.exe"),
        ],
        "Firefox",
    )
}

/// Opens `url` in a specific browser, honouring `profile` where the browser
/// supports it. `app` has already been validated against [`PREFERRED_APPS`].
pub fn open_url_in(app: &str, url: &str, profile: Option<&str>) -> Result<(), String> {
    let (exe, display) = match app {
        "chrome" => (chrome_exe()?, "Chrome"),
        "edge" => (edge_exe()?, "Edge"),
        "firefox" => (firefox_exe()?, "Firefox"),
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
/// attempted first (covers absolute .exe paths and PATH executables); if the
/// OS cannot run it directly we fall back to `cmd /C`, which resolves PATHEXT
/// shims like VS Code's `code.cmd`.
pub fn spawn_ide(command_str: &str, target_path: &str) -> Result<(), String> {
    match Command::new(command_str).arg(target_path).spawn() {
        Ok(_) => Ok(()),
        Err(direct_err) => {
            let mut fallback = Command::new("cmd");
            fallback.args(["/C", command_str, target_path]);
            prepare_child(&mut fallback);
            fallback.spawn().map(|_| ()).map_err(|fallback_err| {
                format!(
                    "failed to launch IDE '{command_str}' \
                     (direct: {direct_err}; via cmd: {fallback_err})"
                )
            })
        }
    }
}

/// The extension the temporary script file needs for its interpreter to accept
/// it. `cmd.exe` in particular refuses anything that is not `.bat` or `.cmd`.
pub fn script_extension(script_type: &str) -> &'static str {
    match script_type {
        "powershell" => "ps1",
        "cmd" => "bat",
        "python" => "py",
        "node" => "js",
        _ => "sh",
    }
}

fn windows_to_wsl_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        let drive = (bytes[0] as char).to_ascii_lowercase();
        format!("/mnt/{drive}{}", &text[2..])
    } else {
        text
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
        "powershell" => {
            let mut c = Command::new("powershell.exe");
            c.args(["-ExecutionPolicy", "Bypass", "-File"])
                .arg(temp_path);
            c
        }
        "cmd" => {
            let mut c = Command::new("cmd.exe");
            c.arg("/C").arg(temp_path);
            c
        }
        "python" => {
            let python_exe = custom_python.unwrap_or("python.exe");
            log::info!("Using Python interpreter: {python_exe}");
            let mut c = Command::new(python_exe);
            c.arg(temp_path);
            c
        }
        "node" => {
            let node_exe = custom_node.unwrap_or("node.exe");
            log::info!("Using Node.js interpreter: {node_exe}");
            let mut c = Command::new(node_exe);
            c.arg(temp_path);
            c
        }
        "wsl_bash" => {
            let wsl_path = windows_to_wsl_path(temp_path);
            let mut c = Command::new("wsl.exe");
            c.args(["--exec", "bash", &wsl_path]);
            c
        }
        other => unreachable!("script type '{other}' should have been validated"),
    };
    prepare_child(&mut command);
    command
}

/// Extra environment a script's interpreter needs on this platform.
///
/// `wsl.exe` does not forward custom environment variables into the Linux guest
/// unless they are listed in WSLENV. The `/u` flag marks each variable as "pass
/// from Windows into WSL only".
pub fn extra_script_env(script_type: &str, names: &[&str]) -> Vec<(String, String)> {
    if script_type != "wsl_bash" || names.is_empty() {
        return Vec::new();
    }
    let ours = names
        .iter()
        .map(|key| format!("{key}/u"))
        .collect::<Vec<_>>()
        .join(":");
    let value = match std::env::var("WSLENV") {
        // Never clobber a pre-existing WSLENV the user relies on.
        Ok(existing) if !existing.trim().is_empty() => format!("{ours}:{existing}"),
        _ => ours,
    };
    vec![("WSLENV".to_string(), value)]
}

/// True for an absolute native path: a drive-letter path (`C:\...` or `C:/...`)
/// or a UNC share (`\\server\share`). Relative paths are deliberately excluded —
/// they have no meaning without a working directory, and accepting them would
/// turn a typo into a silently broken resource.
pub fn is_absolute_native_path(target: &str) -> bool {
    let bytes = target.as_bytes();
    let drive_letter = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/');
    let unc = target.starts_with(r"\\") || target.starts_with("//");
    drive_letter || unc
}

/// Turns an absolute native path into the `file://` URL the launcher expects.
/// Only called for a path [`is_absolute_native_path`] has already accepted.
pub fn file_url_from_native_path(path: &str) -> String {
    // Backslashes are not legal in a URL path; the drive colon and the
    // separators both survive percent_encode_path untouched.
    let encoded = percent_encode_path(&path.replace('\\', "/"));

    // A UNC path already starts with two slashes, giving file://<host>/<share>.
    // A drive path needs the empty authority, giving file:///C:/...
    if encoded.starts_with("//") {
        format!("file:{encoded}")
    } else {
        format!("file:///{encoded}")
    }
}

/// Where the pre-Milestone-7 identifier kept its database, so an existing
/// install can be carried forward. `None` on a platform that never shipped
/// under the old identifier.
pub fn legacy_data_dir() -> Option<PathBuf> {
    std::env::var("APPDATA")
        .ok()
        .map(|appdata| PathBuf::from(appdata).join("com.orion.cockpit"))
}

/// Last chance to adjust the tray icon once it exists. Nothing to do here; the
/// macOS build uses it to mark the icon as a template.
pub fn finish_tray(_tray: &tauri::tray::TrayIcon) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_and_unc_paths_are_absolute() {
        for path in [
            "C:\\Users\\me\\cv.pdf",
            "c:/Users/me/cv.pdf",
            "\\\\server\\share\\file.txt",
            "//server/share/file.txt",
        ] {
            assert!(is_absolute_native_path(path), "{path}");
        }
    }

    #[test]
    fn relative_and_posix_paths_are_not() {
        // A bare POSIX path is not absolute HERE even though it is on macOS:
        // treating it as one would mint a file:// URL pointing at nothing.
        for path in [
            "cv.pdf",
            ".\\cv.pdf",
            "Users/me/cv.pdf",
            "/Users/me/cv.pdf",
            "C:",
            "",
            "https://example.com",
        ] {
            assert!(!is_absolute_native_path(path), "{path}");
        }
    }

    #[test]
    fn wsl_paths_are_translated_to_mnt() {
        assert_eq!(
            windows_to_wsl_path(Path::new("C:\\Temp\\orion-cmd-1.sh")),
            "/mnt/c/Temp/orion-cmd-1.sh"
        );
    }

    #[test]
    fn a_path_without_a_drive_letter_passes_through_unchanged() {
        assert_eq!(
            windows_to_wsl_path(Path::new("/already/posix")),
            "/already/posix"
        );
    }

    #[test]
    fn cmd_scripts_get_an_extension_cmd_exe_will_accept() {
        // cmd.exe refuses to run a file that is not .bat or .cmd, so this is
        // load-bearing rather than cosmetic.
        assert_eq!(script_extension("cmd"), "bat");
    }

    #[test]
    fn every_runnable_type_has_an_extension() {
        for t in RUNNABLE_SCRIPT_TYPES {
            assert!(!script_extension(t).is_empty(), "{t}");
        }
    }

    #[test]
    fn a_drive_path_becomes_a_file_url_with_an_empty_authority() {
        assert_eq!(
            file_url_from_native_path(r"C:\Users\me\Resume.pdf"),
            "file:///C:/Users/me/Resume.pdf"
        );
    }

    #[test]
    fn a_unc_path_keeps_its_host_as_the_authority() {
        assert_eq!(
            file_url_from_native_path(r"\\nas\share\spec.pdf"),
            "file://nas/share/spec.pdf"
        );
    }

    #[test]
    fn spaces_and_hashes_are_encoded() {
        // An unencoded # truncates the path into a fragment, which fails as a
        // silently wrong target rather than as an error.
        assert_eq!(
            file_url_from_native_path(r"C:\My Docs\draft #2.pdf"),
            "file:///C:/My%20Docs/draft%20%232.pdf"
        );
    }

    #[test]
    fn wslenv_is_only_built_for_wsl_scripts_with_vars() {
        assert!(extra_script_env("python", &["API_KEY"]).is_empty());
        assert!(extra_script_env("wsl_bash", &[]).is_empty());

        let env = extra_script_env("wsl_bash", &["API_KEY", "TOKEN"]);
        assert_eq!(env.len(), 1);
        assert_eq!(env[0].0, "WSLENV");
        assert!(env[0].1.starts_with("API_KEY/u:TOKEN/u"));
    }
}
