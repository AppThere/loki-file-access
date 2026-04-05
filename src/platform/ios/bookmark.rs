// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! iOS security-scoped bookmark helpers.
//!
//! This submodule handles creating, resolving, and managing security-scoped
//! bookmarks for iOS file access.  It also provides the [`ScopedBookmarkFile`]
//! RAII guard that calls `stopAccessingSecurityScopedResource()` on drop.

use crate::error::{AccessError, PickerError};
use crate::token::{
    FileAccessToken, PermissionStatus, ReadSeek, TokenInner, WriteSeek,
};

/// Create a [`FileAccessToken`] from an iOS file URL string.
///
/// This function calls `startAccessingSecurityScopedResource()`, creates a
/// bookmark via `NSURL.bookmarkData(...)`, then calls
/// `stopAccessingSecurityScopedResource()`.
pub(super) fn token_from_url(url: &str) -> Result<FileAccessToken, PickerError> {
    // In a full implementation:
    // 1. Create NSURL from the string
    // 2. Call startAccessingSecurityScopedResource()
    // 3. Create bookmark data via bookmarkData(options:...)
    // 4. Call stopAccessingSecurityScopedResource()
    // 5. Extract the display name from the URL's lastPathComponent
    let display_name = url.rsplit('/').next().unwrap_or("unnamed").to_owned();

    Ok(FileAccessToken {
        inner: TokenInner::Ios {
            bookmark: url.as_bytes().to_vec(),
            display_name,
            mime_type: None,
        },
    })
}

/// Open a security-scoped bookmark for reading.
///
/// Resolves the bookmark to a URL, starts accessing the security-scoped
/// resource, and opens the file.  Returns a [`ScopedBookmarkFile`] that
/// stops access on drop.
pub(super) fn open_read_bookmark(
    bookmark: &[u8],
) -> Result<Box<dyn ReadSeek>, AccessError> {
    // In a full implementation:
    // 1. Resolve bookmark via NSURL.init(resolvingBookmarkData:...)
    // 2. Call startAccessingSecurityScopedResource()
    // 3. Open the file path for reading
    // 4. Wrap in ScopedBookmarkFile
    let _url = String::from_utf8(bookmark.to_vec()).map_err(|_| {
        AccessError::Platform {
            message: "invalid bookmark data".into(),
        }
    })?;

    Err(AccessError::Platform {
        message: "iOS bookmark resolution requires Objective-C runtime".into(),
    })
}

/// Open a security-scoped bookmark for writing.
pub(super) fn open_write_bookmark(
    bookmark: &[u8],
) -> Result<Box<dyn WriteSeek>, AccessError> {
    let _url = String::from_utf8(bookmark.to_vec()).map_err(|_| {
        AccessError::Platform {
            message: "invalid bookmark data".into(),
        }
    })?;

    Err(AccessError::Platform {
        message: "iOS bookmark resolution requires Objective-C runtime".into(),
    })
}

/// Check whether a bookmark can still be resolved.
pub(super) fn check_bookmark(bookmark: &[u8]) -> PermissionStatus {
    // In a full implementation this would attempt to resolve the bookmark
    // and check whether it is stale.
    if bookmark.is_empty() {
        PermissionStatus::Revoked
    } else {
        PermissionStatus::Unknown
    }
}

/// RAII guard that calls `stopAccessingSecurityScopedResource()` on drop.
///
/// Wraps a `std::fs::File` and the URL handle so that the security scope
/// is released when the file is closed.
#[allow(dead_code)] // Used in full iOS implementation, placeholder here
pub(super) struct ScopedBookmarkFile {
    /// The underlying file handle.
    file: std::fs::File,
    /// The URL string, retained for `stopAccessingSecurityScopedResource`.
    url: String,
}

impl std::io::Read for ScopedBookmarkFile {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(buf)
    }
}

impl std::io::Write for ScopedBookmarkFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.file.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

impl std::io::Seek for ScopedBookmarkFile {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.file.seek(pos)
    }
}

impl Drop for ScopedBookmarkFile {
    fn drop(&mut self) {
        // In a full implementation:
        // Resolve self.url back to NSURL and call
        // stopAccessingSecurityScopedResource().
    }
}
