//! The whole operating-system surface, in one place.
//!
//! Everything Orion does that cannot be written once for every platform lives
//! behind this module: launching a folder, opening a URL in a named browser,
//! picking an interpreter for a script, parsing a native path, and the handful
//! of window-placement conventions that differ between desktops.
//!
//! The rule the rest of the crate follows is that **no other module carries a
//! `#[cfg(target_os)]` attribute**. `commands.rs` calls `platform::reveal_path`
//! and does not know or care what that runs. The payoff is that the entire
//! delta between platforms can be read side by side in two files of a few
//! hundred lines each, which is both what makes the behaviour auditable and
//! what would make a third platform tractable rather than a rewrite.

use std::path::PathBuf;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
compile_error!(
    "Orion targets Windows and macOS. Adding a platform means adding a module \
     under src/platform/ that provides the same surface as windows.rs — the \
     rest of the crate should not need to change."
);

/// Every script type Orion can **store**, across all platforms.
///
/// Deliberately wider than what the current machine can run. A setup exported
/// on Windows and imported on a Mac keeps its PowerShell scripts verbatim: they
/// are shown, they are editable, they simply refuse to run with an explanation.
/// Translating them into shell would be worse than useless — it would be
/// plausible and wrong.
pub const ALL_SCRIPT_TYPES: &[&str] = &[
    "powershell",
    "cmd",
    "wsl_bash",
    "python",
    "node",
    "zsh",
    "bash",
];

/// Every `preferred_app` value Orion can store, across all platforms. As with
/// script types, a resource set that names Safari survives a trip through a
/// Windows install unchanged.
pub const ALL_PREFERRED_APPS: &[&str] = &["default", "safari", "chrome", "edge", "firefox"];

/// True when this build can actually execute `script_type`.
pub fn can_run_script(script_type: &str) -> bool {
    RUNNABLE_SCRIPT_TYPES.contains(&script_type)
}

/// Why a stored script cannot run here, phrased for the user, or `None` when it
/// can. The wording matters: the script has not been broken and is not going to
/// be deleted, so the message says where it *does* work rather than only that
/// it does not work here.
pub fn unrunnable_reason(script_type: &str) -> Option<String> {
    if can_run_script(script_type) {
        return None;
    }
    let home = match script_type {
        "powershell" | "cmd" | "wsl_bash" => "Windows",
        "zsh" | "bash" => "macOS",
        _ => {
            return Some(format!(
                "'{script_type}' is not a script type this version of Orion knows how to run."
            ))
        }
    };
    Some(format!(
        "{} scripts only run on {home}. This one is kept exactly as written, so it still works \
         when you open this setup on {home}.",
        script_label(script_type)
    ))
}

/// The name to show for a script type, on any platform — a stored type must be
/// labellable even where it cannot run.
pub fn script_label(script_type: &str) -> &str {
    match script_type {
        "powershell" => "PowerShell",
        "cmd" => "Batch",
        "wsl_bash" => "WSL bash",
        "python" => "Python",
        "node" => "Node.js",
        "zsh" => "zsh",
        "bash" => "bash",
        other => other,
    }
}

/// Percent-encodes the characters that would otherwise change how a URL parses.
///
/// `/` and `:` are left alone because they are structural (the separator, and
/// the drive colon on Windows). Everything outside the unreserved set is
/// escaped, which covers the two that actually bite in practice: a space, and a
/// `#` in a filename silently truncating the path into a fragment.
///
/// Shared, because the encoding rules are a property of URLs rather than of the
/// platform — only what gets fed in differs.
pub(crate) fn percent_encode_path(path: &str) -> String {
    const KEEP: &[u8] = b"-._~/:";
    let mut out = String::with_capacity(path.len());
    for byte in path.as_bytes() {
        if byte.is_ascii_alphanumeric() || KEEP.contains(byte) {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// First existing file among `candidates`, or a message naming what to do.
///
/// Shared because the shape of the problem is identical on both platforms —
/// only the candidate paths differ.
pub(crate) fn resolve_browser_exe(
    candidates: &[PathBuf],
    display_name: &str,
) -> Result<PathBuf, String> {
    for candidate in candidates {
        if candidate.is_file() {
            return Ok(candidate.clone());
        }
    }
    Err(format!(
        "{display_name} was not found on this system. Install it or use the system default browser."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_runnable_type_is_a_storable_type() {
        // A platform that can run something Orion cannot store would fail at
        // the INSERT rather than at the point of choice.
        for t in RUNNABLE_SCRIPT_TYPES {
            assert!(
                ALL_SCRIPT_TYPES.contains(t),
                "{t} is runnable here but missing from ALL_SCRIPT_TYPES"
            );
        }
    }

    #[test]
    fn every_storable_type_has_a_label() {
        for t in ALL_SCRIPT_TYPES {
            assert!(!script_label(t).is_empty(), "{t}");
        }
        // The ids that are not already how you would write the name. `zsh` and
        // `bash` are deliberately absent: their label IS their id, so an
        // "a label must differ from its id" rule would be wrong rather than
        // strict.
        assert_eq!(script_label("powershell"), "PowerShell");
        assert_eq!(script_label("cmd"), "Batch");
        assert_eq!(script_label("wsl_bash"), "WSL bash");
        assert_eq!(script_label("node"), "Node.js");
    }

    #[test]
    fn runnable_types_have_no_reason_and_others_do() {
        for t in ALL_SCRIPT_TYPES {
            if can_run_script(t) {
                assert_eq!(unrunnable_reason(t), None, "{t}");
            } else {
                let reason = unrunnable_reason(t).unwrap_or_else(|| panic!("{t}"));
                // The message must name the platform it does work on, or it
                // reads as "your script is broken".
                assert!(
                    reason.contains("Windows") || reason.contains("macOS"),
                    "{t}: {reason}"
                );
            }
        }
    }

    #[test]
    fn an_unknown_type_from_a_newer_backup_is_explained_not_panicked() {
        let reason = unrunnable_reason("fish").expect("unknown types cannot run");
        assert!(reason.contains("fish"));
    }

    #[test]
    fn preferred_apps_are_a_subset_of_the_storable_set() {
        for app in PREFERRED_APPS {
            assert!(
                ALL_PREFERRED_APPS.contains(app),
                "{app} is offered here but cannot be stored"
            );
        }
    }
}
