// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! Desktop file-picker implementation (Windows, macOS, Linux, BSD).
//!
//! This module wraps [`rfd::AsyncFileDialog`] and simply `.await`s its
//! futures.  `rfd` carries its own internal dispatch mechanism and does **not**
//! require Tokio or async-std; any executor that correctly yields between polls
//! works (Dioxus, egui, Iced, `pollster::block_on`, etc.).
//!
//! ## Why `.await` instead of `pollster::block_on`
//!
//! On **macOS**, `rfd::AsyncFileDialog` presents `NSOpenPanel` by dispatching
//! to the main thread via Grand Central Dispatch (`dispatch_async` on the main
//! queue).  If the caller blocks its own thread with `pollster::block_on`
//! *and that thread is the main thread* (the typical case in Dioxus Native),
//! GCD can never execute the dispatch — the dialog never appears and the app
//! hangs with a spinning beach ball.
//!
//! Co-operatively awaiting the future with `.await` yields to the executor,
//! keeping the main-thread run loop free to process GCD events.  On
//! **Windows** and **Linux** there is no main-thread constraint, so `.await`
//! is equally correct there.
//!
//! ## Linux fallback
//!
//! On Linux, rfd uses the XDG Desktop Portal via D-Bus.  When the portal is
//! unavailable (e.g. ChromeOS Crostini), rfd returns `None`.  In that case
//! the picker falls back to `zenity` (a GNOME subprocess dialog) if it is
//! installed.  The zenity call runs on a dedicated thread so it does not block
//! the async executor.

mod filters;
#[cfg(target_os = "linux")]
mod zenity;

use filters::{is_valid_extension, mime_types_to_extensions};

use crate::api::{PickOptions, SaveOptions};
use crate::error::{AccessError, PickerError};
use crate::token::{FileAccessToken, PermissionStatus, ReadSeek, TokenInner, WriteSeek};

/// On Linux, check the `LOKI_FILE_ACCESS_BACKEND` environment variable.
///
/// Setting it to `"none"` disables the picker and surfaces a clear error
/// instead of a silent no-op.  This is a developer escape hatch for
/// environments where neither the XDG Desktop Portal nor zenity is available.
/// Valid values: `"auto"` (default), `"none"`.
#[cfg(target_os = "linux")]
fn check_backend_env() -> Result<(), PickerError> {
    if std::env::var("LOKI_FILE_ACCESS_BACKEND").as_deref() == Ok("none") {
        return Err(PickerError::Platform {
            message:
                "file picker disabled via LOKI_FILE_ACCESS_BACKEND=none. \
                 On Linux, the XDG Desktop Portal or zenity must be available. \
                 Unset LOKI_FILE_ACCESS_BACKEND or set it to \"auto\" to re-enable."
                    .into(),
        });
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
#[inline(always)]
fn check_backend_env() -> Result<(), PickerError> {
    Ok(())
}

/// Pick a single file for reading.
///
/// Converts `options.mime_types` to file-extension filters understood by the
/// native dialog.  On Windows, extensions that contain dots or other
/// characters invalid for `IFileOpenDialog::SetFileTypes` are silently
/// dropped; if all extensions are invalid the filter is omitted entirely so
/// that all files remain visible.
///
/// On Linux, if the XDG Desktop Portal is unavailable, falls back to zenity.
///
/// # Errors
///
/// Returns [`PickerError`] if the platform dialog could not be presented.
///
/// # Examples
///
/// ```no_run
/// # async fn example() -> Result<(), loki_file_access::PickerError> {
/// use loki_file_access::{FilePicker, PickOptions};
/// let token = FilePicker::new()
///     .pick_file_to_open(PickOptions {
///         mime_types: vec!["application/pdf".into()],
///         ..Default::default()
///     })
///     .await?;
/// # Ok(()) }
/// ```
pub(crate) async fn pick_open_single(
    options: PickOptions,
) -> Result<Option<FileAccessToken>, PickerError> {
    check_backend_env()?;

    // Capture filter data before building the rfd dialog so the zenity fallback
    // thread closure can take ownership of the same values.
    let filter_label = options.filter_label.as_deref().unwrap_or("Files").to_owned();
    let filter_exts: Vec<String> = if !options.mime_types.is_empty() {
        mime_types_to_extensions(&options.mime_types)
            .into_iter()
            .filter(|e| is_valid_extension(e))
            .collect()
    } else {
        vec![]
    };

    let mut dialog = rfd::AsyncFileDialog::new();
    if !filter_exts.is_empty() {
        let ext_refs: Vec<&str> = filter_exts.iter().map(String::as_str).collect();
        dialog = dialog.add_filter(&filter_label, &ext_refs);
    }

    match dialog.pick_file().await {
        Some(h) => {
            let path = h.path().to_path_buf();
            let display_name = file_name_from_path(&path);
            Ok(Some(FileAccessToken {
                inner: TokenInner::Desktop { path, display_name },
            }))
        }
        None => {
            #[cfg(target_os = "linux")]
            {
                if zenity::is_zenity_available() {
                    tracing::debug!("XDG Desktop Portal unavailable; falling back to zenity");
                    let (fut, state) = crate::future::new_pick_future();
                    std::thread::spawn(move || {
                        let result =
                            zenity::pick_file("Open File".into(), filter_label, filter_exts)
                                .map(|opt| {
                                    opt.map(|path| {
                                        let display_name = file_name_from_path(&path);
                                        FileAccessToken {
                                            inner: TokenInner::Desktop { path, display_name },
                                        }
                                    })
                                });
                        crate::future::deliver(&state, result);
                    });
                    return fut.await;
                }
                tracing::warn!(
                    "File picker returned no result. \
                     On ChromeOS Crostini or minimal Linux environments, \
                     install zenity for file dialog support: sudo apt install zenity"
                );
            }
            Ok(None)
        }
    }
}

/// Pick multiple files for reading.
///
/// Applies the same extension-validation logic as [`pick_open_single`]:
/// invalid extensions (containing dots etc.) are dropped, and if none remain
/// the filter is omitted so all files stay visible.
///
/// On Linux, if the XDG Desktop Portal is unavailable, falls back to zenity.
///
/// # Errors
///
/// Returns [`PickerError`] if the platform dialog could not be presented.
///
/// # Examples
///
/// ```no_run
/// # async fn example() -> Result<(), loki_file_access::PickerError> {
/// use loki_file_access::{FilePicker, PickOptions};
/// let tokens = FilePicker::new()
///     .pick_files_to_open(PickOptions {
///         mime_types: vec!["image/png".into(), "image/jpeg".into()],
///         ..Default::default()
///     })
///     .await?;
/// # Ok(()) }
/// ```
pub(crate) async fn pick_open_multi(
    options: PickOptions,
) -> Result<Vec<FileAccessToken>, PickerError> {
    check_backend_env()?;

    let filter_label = options.filter_label.as_deref().unwrap_or("Files").to_owned();
    let filter_exts: Vec<String> = if !options.mime_types.is_empty() {
        mime_types_to_extensions(&options.mime_types)
            .into_iter()
            .filter(|e| is_valid_extension(e))
            .collect()
    } else {
        vec![]
    };

    let mut dialog = rfd::AsyncFileDialog::new();
    if !filter_exts.is_empty() {
        let ext_refs: Vec<&str> = filter_exts.iter().map(String::as_str).collect();
        dialog = dialog.add_filter(&filter_label, &ext_refs);
    }

    match dialog.pick_files().await {
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
        None => {
            #[cfg(target_os = "linux")]
            {
                if zenity::is_zenity_available() {
                    tracing::debug!("XDG Desktop Portal unavailable; falling back to zenity");
                    let (fut, state) = crate::future::new_pick_future();
                    std::thread::spawn(move || {
                        let result =
                            zenity::pick_files("Open Files".into(), filter_label, filter_exts)
                                .map(|paths| {
                                    paths
                                        .into_iter()
                                        .map(|path| {
                                            let display_name = file_name_from_path(&path);
                                            FileAccessToken {
                                                inner: TokenInner::Desktop { path, display_name },
                                            }
                                        })
                                        .collect()
                                });
                        crate::future::deliver(&state, result);
                    });
                    return fut.await;
                }
                tracing::warn!(
                    "File picker returned no result. \
                     On ChromeOS Crostini or minimal Linux environments, \
                     install zenity for file dialog support: sudo apt install zenity"
                );
            }
            Ok(vec![])
        }
    }
}

/// Pick a save location.
///
/// Applies the same extension-validation logic as the open-picker functions:
/// the MIME type is converted to an extension, and `add_filter` is only called
/// if the resulting extension is valid for the native dialog.
///
/// On Linux, if the XDG Desktop Portal is unavailable, falls back to zenity.
///
/// # Errors
///
/// Returns [`PickerError`] if the platform dialog could not be presented.
///
/// # Examples
///
/// ```no_run
/// # async fn example() -> Result<(), loki_file_access::PickerError> {
/// use loki_file_access::{FilePicker, SaveOptions};
/// let token = FilePicker::new()
///     .pick_file_to_save(SaveOptions {
///         mime_type: Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document".into()),
///         suggested_name: Some("report.docx".into()),
///     })
///     .await?;
/// # Ok(()) }
/// ```
pub(crate) async fn pick_save(
    options: SaveOptions,
) -> Result<Option<FileAccessToken>, PickerError> {
    check_backend_env()?;

    let suggested_name = options.suggested_name;
    let filter_label = "File".to_owned();
    let filter_exts: Vec<String> = if let Some(ref mime) = options.mime_type {
        mime_types_to_extensions(std::slice::from_ref(mime))
            .into_iter()
            .filter(|e| is_valid_extension(e))
            .collect()
    } else {
        vec![]
    };

    let mut dialog = rfd::AsyncFileDialog::new();
    if let Some(ref name) = suggested_name {
        dialog = dialog.set_file_name(name);
    }
    if !filter_exts.is_empty() {
        let ext_refs: Vec<&str> = filter_exts.iter().map(String::as_str).collect();
        dialog = dialog.add_filter(&filter_label, &ext_refs);
    }

    match dialog.save_file().await {
        Some(h) => {
            let path = h.path().to_path_buf();
            let display_name = file_name_from_path(&path);
            Ok(Some(FileAccessToken {
                inner: TokenInner::Desktop { path, display_name },
            }))
        }
        None => {
            #[cfg(target_os = "linux")]
            {
                if zenity::is_zenity_available() {
                    tracing::debug!("XDG Desktop Portal unavailable; falling back to zenity");
                    let (fut, state) = crate::future::new_pick_future();
                    std::thread::spawn(move || {
                        let result = zenity::pick_save(
                            "Save File".into(),
                            suggested_name,
                            filter_label,
                            filter_exts,
                        )
                        .map(|opt| {
                            opt.map(|path| {
                                let display_name = file_name_from_path(&path);
                                FileAccessToken {
                                    inner: TokenInner::Desktop { path, display_name },
                                }
                            })
                        });
                        crate::future::deliver(&state, result);
                    });
                    return fut.await;
                }
                tracing::warn!(
                    "Save-file picker returned no result. \
                     On ChromeOS Crostini or minimal Linux environments, \
                     install zenity for file dialog support: sudo apt install zenity"
                );
            }
            Ok(None)
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
