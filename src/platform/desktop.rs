// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! Desktop file-picker implementation (Windows, macOS, Linux, BSD).
//!
//! This module wraps the [`rfd`] crate's async file dialog API.  Since `rfd`
//! requires either a Tokio or async-std runtime for its async API, and this
//! crate intentionally avoids runtime dependencies, we use
//! `pollster::block_on` to drive the `rfd` future synchronously.
//!
//! **Trade-off**: On desktop, the picker call blocks the current thread until
//! the user closes the dialog.  This is acceptable because native file dialogs
//! are modal and block the UI thread on all desktop platforms anyway.  The
//! outer `async fn` signature allows callers to `.await` the result in an
//! async context without special-casing desktop.

use crate::api::{PickOptions, SaveOptions};
use crate::error::{AccessError, PickerError};
use crate::token::{FileAccessToken, PermissionStatus, ReadSeek, TokenInner, WriteSeek};

/// Pick a single file for reading.
pub(crate) async fn pick_open_single(
    options: PickOptions,
) -> Result<Option<FileAccessToken>, PickerError> {
    let mut dialog = rfd::AsyncFileDialog::new();

    if !options.mime_types.is_empty() {
        let extensions = mime_types_to_extensions(&options.mime_types);
        let ext_refs: Vec<&str> = extensions.iter().map(String::as_str).collect();
        let label = options.filter_label.as_deref().unwrap_or("Files");
        dialog = dialog.add_filter(label, &ext_refs);
    }

    let handle = pollster::block_on(dialog.pick_file());

    match handle {
        None => Ok(None),
        Some(h) => {
            let path = h.path().to_path_buf();
            let display_name = file_name_from_path(&path);
            Ok(Some(FileAccessToken {
                inner: TokenInner::Desktop { path, display_name },
            }))
        }
    }
}

/// Pick multiple files for reading.
pub(crate) async fn pick_open_multi(
    options: PickOptions,
) -> Result<Vec<FileAccessToken>, PickerError> {
    let mut dialog = rfd::AsyncFileDialog::new();

    if !options.mime_types.is_empty() {
        let extensions = mime_types_to_extensions(&options.mime_types);
        let ext_refs: Vec<&str> = extensions.iter().map(String::as_str).collect();
        let label = options.filter_label.as_deref().unwrap_or("Files");
        dialog = dialog.add_filter(label, &ext_refs);
    }

    let handles = pollster::block_on(dialog.pick_files());

    match handles {
        None => Ok(vec![]),
        Some(list) => {
            let tokens = list
                .into_iter()
                .map(|h| {
                    let path = h.path().to_path_buf();
                    let display_name = file_name_from_path(&path);
                    FileAccessToken {
                        inner: TokenInner::Desktop { path, display_name },
                    }
                })
                .collect();
            Ok(tokens)
        }
    }
}

/// Pick a save location.
pub(crate) async fn pick_save(
    options: SaveOptions,
) -> Result<Option<FileAccessToken>, PickerError> {
    let mut dialog = rfd::AsyncFileDialog::new();

    if let Some(ref name) = options.suggested_name {
        dialog = dialog.set_file_name(name);
    }

    if let Some(ref mime) = options.mime_type {
        let extensions = mime_types_to_extensions(std::slice::from_ref(mime));
        let ext_refs: Vec<&str> = extensions.iter().map(String::as_str).collect();
        if !ext_refs.is_empty() {
            dialog = dialog.add_filter("File", &ext_refs);
        }
    }

    let handle = pollster::block_on(dialog.save_file());

    match handle {
        None => Ok(None),
        Some(h) => {
            let path = h.path().to_path_buf();
            let display_name = file_name_from_path(&path);
            Ok(Some(FileAccessToken {
                inner: TokenInner::Desktop { path, display_name },
            }))
        }
    }
}

/// Open a token for reading.
pub(crate) fn open_read(inner: &TokenInner) -> Result<Box<dyn ReadSeek>, AccessError> {
    match inner {
        TokenInner::Desktop { path, .. } => {
            let file = std::fs::File::open(path)?;
            Ok(Box::new(file))
        }
        _ => Err(AccessError::Platform {
            message: "non-desktop token on desktop platform".into(),
        }),
    }
}

/// Open a token for writing.
pub(crate) fn open_write(inner: &TokenInner) -> Result<Box<dyn WriteSeek>, AccessError> {
    match inner {
        TokenInner::Desktop { path, .. } => {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(path)?;
            Ok(Box::new(file))
        }
        _ => Err(AccessError::Platform {
            message: "non-desktop token on desktop platform".into(),
        }),
    }
}

/// Check permission status for a token.
pub(crate) fn check_permission(inner: &TokenInner) -> PermissionStatus {
    match inner {
        TokenInner::Desktop { path, .. } => {
            if path.exists() {
                PermissionStatus::Valid
            } else {
                PermissionStatus::Revoked
            }
        }
        _ => PermissionStatus::Unknown,
    }
}

/// Extract a display name from a filesystem path.
fn file_name_from_path(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unnamed")
        .to_owned()
}

/// Convert MIME types to file extensions for the `rfd` filter.
///
/// This is a best-effort mapping for common MIME types.  Unknown MIME types
/// are passed through as-is, stripping the `type/` prefix (e.g.
/// `application/pdf` → `pdf`).
fn mime_types_to_extensions(mime_types: &[String]) -> Vec<String> {
    mime_types
        .iter()
        .map(|mime| {
            match mime.as_str() {
                "text/plain" => "txt".into(),
                "text/html" => "html".into(),
                "text/css" => "css".into(),
                "text/csv" => "csv".into(),
                "application/json" => "json".into(),
                "application/pdf" => "pdf".into(),
                "application/xml" => "xml".into(),
                "application/zip" => "zip".into(),
                "image/png" => "png".into(),
                "image/jpeg" => "jpg".into(),
                "image/gif" => "gif".into(),
                "image/svg+xml" => "svg".into(),
                "image/webp" => "webp".into(),
                "audio/mpeg" => "mp3".into(),
                "video/mp4" => "mp4".into(),
                other => {
                    // Fall back to the subtype portion of the MIME type.
                    other
                        .split('/')
                        .nth(1)
                        .unwrap_or(other)
                        .to_owned()
                }
            }
        })
        .collect()
}
