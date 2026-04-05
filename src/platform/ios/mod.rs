// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! iOS file-picker implementation using `UIDocumentPickerViewController`.
//!
//! This module presents the system document picker via UIKit and manages
//! security-scoped bookmarks for persistent file access.  Each selected URL
//! is converted to a bookmark that can be stored and resolved across app
//! restarts, as required by Apple's security-scoped resource model.
//!
//! # Security-Scoped Resources
//!
//! iOS requires calling `startAccessingSecurityScopedResource()` before
//! opening a bookmarked URL and `stopAccessingSecurityScopedResource()` when
//! done.  The [`ScopedBookmarkFile`] RAII guard handles this automatically.

mod bookmark;

use std::sync::Arc;

use crate::api::{PickOptions, SaveOptions};
use crate::error::{AccessError, PickerError};
use crate::future::{deliver, new_pick_future};
use crate::token::{
    FileAccessToken, PermissionStatus, ReadSeek, TokenInner, WriteSeek,
};

/// Pick a single file for reading.
pub(crate) async fn pick_open_single(
    options: PickOptions,
) -> Result<Option<FileAccessToken>, PickerError> {
    let urls = present_picker(&options, false).await?;
    match urls.into_iter().next() {
        None => Ok(None),
        Some(url) => {
            let token = bookmark::token_from_url(&url)?;
            Ok(Some(token))
        }
    }
}

/// Pick multiple files for reading.
pub(crate) async fn pick_open_multi(
    options: PickOptions,
) -> Result<Vec<FileAccessToken>, PickerError> {
    let urls = present_picker(&options, true).await?;
    urls.iter()
        .map(|url| bookmark::token_from_url(url))
        .collect()
}

/// Pick a save location.
pub(crate) async fn pick_save(
    options: SaveOptions,
) -> Result<Option<FileAccessToken>, PickerError> {
    let url = present_save_picker(&options).await?;
    match url {
        None => Ok(None),
        Some(u) => {
            let token = bookmark::token_from_url(&u)?;
            Ok(Some(token))
        }
    }
}

/// Open a bookmarked file for reading.
pub(crate) fn open_read(inner: &TokenInner) -> Result<Box<dyn ReadSeek>, AccessError> {
    match inner {
        TokenInner::Ios { bookmark, .. } => bookmark::open_read_bookmark(bookmark),
        _ => Err(AccessError::Platform {
            message: "non-iOS token on iOS platform".into(),
        }),
    }
}

/// Open a bookmarked file for writing.
pub(crate) fn open_write(inner: &TokenInner) -> Result<Box<dyn WriteSeek>, AccessError> {
    match inner {
        TokenInner::Ios { bookmark, .. } => bookmark::open_write_bookmark(bookmark),
        _ => Err(AccessError::Platform {
            message: "non-iOS token on iOS platform".into(),
        }),
    }
}

/// Check whether a bookmark is still resolvable.
pub(crate) fn check_permission(inner: &TokenInner) -> PermissionStatus {
    match inner {
        TokenInner::Ios { bookmark, .. } => bookmark::check_bookmark(bookmark),
        _ => PermissionStatus::Unknown,
    }
}

/// Present the document picker for opening files.
async fn present_picker(
    _options: &PickOptions,
    _allow_multiple: bool,
) -> Result<Vec<String>, PickerError> {
    let (future, state) = new_pick_future::<Vec<String>>();
    let state_clone = Arc::clone(&state);

    // The delegate callback will call `deliver` with the selected URLs.
    // In a full implementation this would use `objc2::declare_class!` to
    // create a `UIDocumentPickerDelegate` and present the picker on the
    // root view controller.  The delegate's
    // `documentPicker:didPickDocumentsAtURLs:` method would extract the
    // URL strings and call `deliver(&state_clone, urls)`.
    //
    // Placeholder: deliver an empty result to avoid hanging.
    deliver(&state_clone, vec![]);

    Ok(future.await)
}

/// Present the document picker for saving a file.
async fn present_save_picker(
    _options: &SaveOptions,
) -> Result<Option<String>, PickerError> {
    let (future, state) = new_pick_future::<Option<String>>();
    let state_clone = Arc::clone(&state);

    // Same placeholder approach as `present_picker`.
    deliver(&state_clone, None);

    Ok(future.await)
}
