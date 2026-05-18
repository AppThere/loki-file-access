// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! Zenity subprocess fallback for Linux environments where the XDG Desktop
//! Portal is unavailable (e.g. ChromeOS Crostini).
//!
//! Requires `zenity` to be installed: `sudo apt install zenity`

use std::path::PathBuf;
use std::process::Command;

use crate::error::PickerError;

/// Returns `true` if zenity is installed and executable.
pub(super) fn is_zenity_available() -> bool {
    Command::new("zenity")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Open a single-file picker via zenity.
///
/// Returns `Ok(Some(path))` on selection, `Ok(None)` on cancel.
pub(super) fn pick_file(
    title: String,
    filter_label: String,
    filter_exts: Vec<String>,
) -> Result<Option<PathBuf>, PickerError> {
    let mut cmd = build_open_cmd(&filter_label, &filter_exts);
    cmd.arg("--title").arg(title);
    run_single(cmd)
}

/// Open a multi-file picker via zenity.
///
/// Returns `Ok(vec)` of selected paths, empty on cancel.
pub(super) fn pick_files(
    title: String,
    filter_label: String,
    filter_exts: Vec<String>,
) -> Result<Vec<PathBuf>, PickerError> {
    let mut cmd = build_open_cmd(&filter_label, &filter_exts);
    cmd.arg("--multiple");
    cmd.arg("--separator").arg("\n");
    cmd.arg("--title").arg(title);
    run_multi(cmd)
}

/// Open a save-file dialog via zenity.
///
/// Returns `Ok(Some(path))` on selection, `Ok(None)` on cancel.
pub(super) fn pick_save(
    title: String,
    default_name: Option<String>,
    filter_label: String,
    filter_exts: Vec<String>,
) -> Result<Option<PathBuf>, PickerError> {
    let mut cmd = build_open_cmd(&filter_label, &filter_exts);
    cmd.arg("--save");
    cmd.arg("--confirm-overwrite");
    cmd.arg("--title").arg(title);
    if let Some(name) = default_name {
        cmd.arg("--filename").arg(name);
    }
    run_single(cmd)
}

/// Build a base `zenity --file-selection` command with an optional file filter.
fn build_open_cmd(filter_label: &str, filter_exts: &[String]) -> Command {
    let mut cmd = Command::new("zenity");
    cmd.arg("--file-selection");
    if !filter_exts.is_empty() {
        // zenity --file-filter format: "Label | *.ext1 *.ext2"
        let pattern = filter_exts
            .iter()
            .map(|e| format!("*.{}", e.trim_start_matches('.')))
            .collect::<Vec<_>>()
            .join(" ");
        cmd.arg("--file-filter")
            .arg(format!("{filter_label} | {pattern}"));
    }
    cmd
}

/// Run zenity and parse a single path from stdout.
///
/// Exit code 1 means the user cancelled — returned as `Ok(None)`, not an error.
fn run_single(mut cmd: Command) -> Result<Option<PathBuf>, PickerError> {
    let output = cmd.output().map_err(|e| PickerError::Platform {
        message: format!("failed to launch zenity: {e}"),
    })?;
    if !output.status.success() {
        return Ok(None);
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if path.is_empty() {
        Ok(None)
    } else {
        Ok(Some(PathBuf::from(path)))
    }
}

/// Run zenity and parse newline-separated paths from stdout.
///
/// Exit code 1 means the user cancelled — returned as `Ok(vec![])`, not an error.
fn run_multi(mut cmd: Command) -> Result<Vec<PathBuf>, PickerError> {
    let output = cmd.output().map_err(|e| PickerError::Platform {
        message: format!("failed to launch zenity: {e}"),
    })?;
    if !output.status.success() {
        return Ok(vec![]);
    }
    let paths = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect();
    Ok(paths)
}
