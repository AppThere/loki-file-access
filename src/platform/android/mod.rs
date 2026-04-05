// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! Android file-picker implementation using the Storage Access Framework (SAF).
//!
//! This module uses JNI to interact with Android's `Intent.ACTION_OPEN_DOCUMENT`
//! and `Intent.ACTION_CREATE_DOCUMENT` intents.  File access is mediated
//! through content URIs and `ContentResolver`, ensuring that the app never
//! accesses files via filesystem paths (which are unreliable on modern Android).
//!
//! # Persistence
//!
//! After the user selects a file, this module calls
//! `ContentResolver.takePersistableUriPermission()` with both READ and WRITE
//! flags.  This ensures that the URI grant survives app restarts and device
//! reboots, allowing the application to maintain a reliable recent-files list.
//!
//! # Integration
//!
//! The host Android activity must forward `onActivityResult` to
//! [`on_activity_result`] so that the pending future can be resolved.

mod jni_fd;
mod jni_intents;

use std::sync::{Arc, Mutex, OnceLock};

use crate::api::{PickOptions, SaveOptions};
use crate::error::{AccessError, PickerError};
use crate::future::{deliver, new_pick_future};
use crate::token::{
    FileAccessToken, PermissionStatus, ReadSeek, TokenInner, WriteSeek,
};

/// Pending pick state shared between the intent launcher and the JNI callback.
static PENDING_PICK: OnceLock<
    Mutex<Option<Arc<Mutex<crate::future::PickState<Option<String>>>>>>,
> = OnceLock::new();

fn pending_pick(
) -> &'static Mutex<Option<Arc<Mutex<crate::future::PickState<Option<String>>>>>> {
    PENDING_PICK.get_or_init(|| Mutex::new(None))
}

/// Pick a single file for reading via `ACTION_OPEN_DOCUMENT`.
pub(crate) async fn pick_open_single(
    options: PickOptions,
) -> Result<Option<FileAccessToken>, PickerError> {
    let uri = launch_open_intent(&options, false).await?;
    match uri {
        None => Ok(None),
        Some(uri_str) => {
            jni_intents::take_persistable_uri_permission(&uri_str)?;
            let display_name =
                uri_str.rsplit('/').next().unwrap_or("unnamed").to_owned();
            Ok(Some(FileAccessToken {
                inner: TokenInner::Android {
                    uri: uri_str,
                    display_name,
                    mime_type: None,
                },
            }))
        }
    }
}

/// Pick multiple files for reading via `ACTION_OPEN_DOCUMENT`.
pub(crate) async fn pick_open_multi(
    options: PickOptions,
) -> Result<Vec<FileAccessToken>, PickerError> {
    let uri = launch_open_intent(&options, true).await?;
    match uri {
        None => Ok(vec![]),
        Some(uri_str) => {
            jni_intents::take_persistable_uri_permission(&uri_str)?;
            let display_name =
                uri_str.rsplit('/').next().unwrap_or("unnamed").to_owned();
            Ok(vec![FileAccessToken {
                inner: TokenInner::Android {
                    uri: uri_str,
                    display_name,
                    mime_type: None,
                },
            }])
        }
    }
}

/// Pick a save location via `ACTION_CREATE_DOCUMENT`.
pub(crate) async fn pick_save(
    options: SaveOptions,
) -> Result<Option<FileAccessToken>, PickerError> {
    let uri = launch_create_intent(&options).await?;
    match uri {
        None => Ok(None),
        Some(uri_str) => {
            jni_intents::take_persistable_uri_permission(&uri_str)?;
            let display_name = options
                .suggested_name
                .clone()
                .unwrap_or_else(|| "untitled".into());
            Ok(Some(FileAccessToken {
                inner: TokenInner::Android {
                    uri: uri_str,
                    display_name,
                    mime_type: options.mime_type.clone(),
                },
            }))
        }
    }
}

/// Open a content URI for reading.
pub(crate) fn open_read(inner: &TokenInner) -> Result<Box<dyn ReadSeek>, AccessError> {
    match inner {
        TokenInner::Android { uri, .. } => {
            let fd = jni_fd::open_fd(uri, "r")?;
            // SAFETY: `open_fd` returns a valid file descriptor from
            // Android's `ContentResolver.openFileDescriptor` after detaching
            // it.  The caller takes ownership; it must not be double-closed.
            let file = unsafe { std::os::fd::FromRawFd::from_raw_fd(fd) };
            Ok(Box::new(file))
        }
        _ => Err(AccessError::Platform {
            message: "non-Android token on Android platform".into(),
        }),
    }
}

/// Open a content URI for writing.
pub(crate) fn open_write(inner: &TokenInner) -> Result<Box<dyn WriteSeek>, AccessError> {
    match inner {
        TokenInner::Android { uri, .. } => {
            let fd = jni_fd::open_fd(uri, "w")?;
            // SAFETY: Same invariant as `open_read` — see above.
            let file: std::fs::File =
                unsafe { std::os::fd::FromRawFd::from_raw_fd(fd) };
            Ok(Box::new(file))
        }
        _ => Err(AccessError::Platform {
            message: "non-Android token on Android platform".into(),
        }),
    }
}

/// Check whether a persistable URI permission is still held.
pub(crate) fn check_permission(inner: &TokenInner) -> PermissionStatus {
    match inner {
        TokenInner::Android { uri, .. } => jni_fd::check_persisted_permission(uri)
            .unwrap_or(PermissionStatus::Unknown),
        _ => PermissionStatus::Unknown,
    }
}

/// Called from Java's `onActivityResult` via JNI to deliver the selected
/// URI (or `None` if the user cancelled) and wake the pending future.
pub fn on_activity_result(uri: Option<String>) {
    let guard = match pending_pick().lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(ref state) = *guard {
        deliver(state, uri);
    }
}

/// Store the shared state for the in-flight pick operation.
fn store_pending(
    state: Arc<Mutex<crate::future::PickState<Option<String>>>>,
) -> Result<(), PickerError> {
    let mut guard = pending_pick().lock().map_err(|e| PickerError::Internal {
        message: e.to_string(),
    })?;
    *guard = Some(state);
    Ok(())
}

/// Launch `ACTION_OPEN_DOCUMENT` and await the result.
async fn launch_open_intent(
    options: &PickOptions,
    allow_multiple: bool,
) -> Result<Option<String>, PickerError> {
    let (future, state) = new_pick_future::<Option<String>>();
    store_pending(state)?;
    jni_intents::fire_open_document_intent(options, allow_multiple)?;
    Ok(future.await)
}

/// Launch `ACTION_CREATE_DOCUMENT` and await the result.
async fn launch_create_intent(
    options: &SaveOptions,
) -> Result<Option<String>, PickerError> {
    let (future, state) = new_pick_future::<Option<String>>();
    store_pending(state)?;
    jni_intents::fire_create_document_intent(options)?;
    Ok(future.await)
}
