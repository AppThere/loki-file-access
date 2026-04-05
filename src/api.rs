// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! Public API surface for presenting file-picker dialogs.
//!
//! This module defines [`FilePicker`], [`PickOptions`], and [`SaveOptions`] —
//! the primary entry points for all file-picker operations.  Platform-specific
//! behaviour is fully abstracted behind these types.

use crate::error::PickerError;
use crate::token::FileAccessToken;

/// Options for opening an existing file via a platform file-picker dialog.
///
/// # Examples
///
/// ```
/// use loki_file_access::PickOptions;
///
/// let opts = PickOptions {
///     mime_types: vec!["image/png".into(), "image/jpeg".into()],
///     filter_label: Some("Images".into()),
///     multi: false,
/// };
/// ```
#[derive(Debug, Clone, Default)]
pub struct PickOptions {
    /// MIME types to filter in the picker dialog.
    ///
    /// An empty vector means all file types are shown.
    pub mime_types: Vec<String>,

    /// Display label for the file-type filter in the picker UI.
    ///
    /// Not all platforms support this (e.g. Android ignores it).
    pub filter_label: Option<String>,

    /// Whether the user may select multiple files.
    ///
    /// When `false`, at most one file is returned.
    pub multi: bool,
}

/// Options for saving a new file or overwriting an existing one.
///
/// # Examples
///
/// ```
/// use loki_file_access::SaveOptions;
///
/// let opts = SaveOptions {
///     mime_type: Some("text/plain".into()),
///     suggested_name: Some("notes.txt".into()),
/// };
/// ```
#[derive(Debug, Clone, Default)]
pub struct SaveOptions {
    /// The MIME type of the file being saved.
    ///
    /// Used by some platforms (Android SAF) to pre-filter the save location.
    pub mime_type: Option<String>,

    /// Suggested filename including extension.
    ///
    /// The user may change this in the save dialog.
    pub suggested_name: Option<String>,
}

/// Frontend-agnostic file picker that delegates to the native platform dialog.
///
/// `FilePicker` has no state and is cheap to construct.  All methods return
/// standard [`Future`] values that can be awaited from any async runtime,
/// including `pollster::block_on` for synchronous contexts.
///
/// # Examples
///
/// ```no_run
/// use loki_file_access::{FilePicker, PickOptions};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let picker = FilePicker::new();
/// let token = picker
///     .pick_file_to_open(PickOptions::default())
///     .await?;
///
/// if let Some(token) = token {
///     println!("Selected: {}", token.display_name());
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Default)]
pub struct FilePicker;

impl FilePicker {
    /// Create a new `FilePicker` instance.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Present a platform dialog for the user to select a single file.
    ///
    /// Returns `Ok(Some(token))` if the user selected a file, or `Ok(None)`
    /// if the user cancelled the dialog.  The `multi` field of `options` is
    /// ignored — use [`pick_files_to_open`](Self::pick_files_to_open) for
    /// multi-selection.
    ///
    /// # Errors
    ///
    /// Returns [`PickerError`] if the platform dialog could not be presented.
    #[must_use = "this returns a Result that may contain an error"]
    pub async fn pick_file_to_open(
        &self,
        options: PickOptions,
    ) -> Result<Option<FileAccessToken>, PickerError> {
        let opts = PickOptions {
            multi: false,
            ..options
        };
        crate::platform::pick_open_single(opts).await
    }

    /// Present a platform dialog for the user to select multiple files.
    ///
    /// Returns a (possibly empty) vector of tokens.  An empty vector means
    /// the user cancelled the dialog.  The `multi` field of `options` is
    /// forced to `true`.
    ///
    /// # Errors
    ///
    /// Returns [`PickerError`] if the platform dialog could not be presented.
    #[must_use = "this returns a Result that may contain an error"]
    pub async fn pick_files_to_open(
        &self,
        options: PickOptions,
    ) -> Result<Vec<FileAccessToken>, PickerError> {
        let opts = PickOptions {
            multi: true,
            ..options
        };
        crate::platform::pick_open_multi(opts).await
    }

    /// Present a platform dialog for the user to choose a save location.
    ///
    /// Returns `Ok(Some(token))` if the user confirmed a save location, or
    /// `Ok(None)` if the user cancelled.
    ///
    /// # Platform notes
    ///
    /// On WASM, this triggers a browser download via a Blob URL rather than
    /// presenting a traditional save dialog.  The returned token wraps an
    /// in-memory buffer.
    ///
    /// # Errors
    ///
    /// Returns [`PickerError`] if the platform dialog could not be presented.
    #[must_use = "this returns a Result that may contain an error"]
    pub async fn pick_file_to_save(
        &self,
        options: SaveOptions,
    ) -> Result<Option<FileAccessToken>, PickerError> {
        crate::platform::pick_save(options).await
    }
}
