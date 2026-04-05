// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! Capability token for accessing user-selected files.
//!
//! [`FileAccessToken`] is the central type returned by every picker operation.
//! It encapsulates all platform-specific state needed to re-open a file,
//! including Android URIs, iOS security-scoped bookmarks, desktop paths, and
//! in-memory WASM data.
//!
//! Tokens are serializable to a URL-safe base64-encoded JSON string via
//! [`FileAccessToken::serialize`] and [`FileAccessToken::deserialize`], making
//! them suitable for persisting in a recent-files list or application database.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use std::path::PathBuf;

use crate::error::{AccessError, TokenParseError};

/// Status of the permission grant associated with a [`FileAccessToken`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PermissionStatus {
    /// The token's permission is still valid and the file can be opened.
    Valid,
    /// The permission has been revoked by the user or the operating system.
    Revoked,
    /// The permission status cannot be determined on this platform.
    Unknown,
}

/// Internal representation of platform-specific token data.
///
/// This enum is serialized to JSON and then base64-encoded for storage.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) enum TokenInner {
    /// Desktop file identified by filesystem path.
    Desktop {
        /// Absolute path to the file.
        path: PathBuf,
        /// User-visible file name.
        display_name: String,
    },
    /// Android file identified by a content URI.
    Android {
        /// Content URI string (e.g. `content://...`).
        uri: String,
        /// User-visible file name from the document provider.
        display_name: String,
        /// MIME type reported by the document provider.
        mime_type: Option<String>,
    },
    /// iOS file identified by a security-scoped bookmark.
    Ios {
        /// Opaque bookmark data created by `NSURL.bookmarkData(...)`.
        bookmark: Vec<u8>,
        /// User-visible file name.
        display_name: String,
        /// MIME type (often inferred from the file extension).
        mime_type: Option<String>,
    },
    /// WASM file held entirely in memory.
    Wasm {
        /// Complete file contents.
        data: Vec<u8>,
        /// Original file name from the `<input>` element.
        name: String,
        /// MIME type reported by the browser.
        mime_type: Option<String>,
    },
}

/// A serializable capability token representing access to a user-selected file.
///
/// Obtain instances from [`crate::FilePicker`] methods.  Serialize via
/// [`serialize`](Self::serialize) for storage; deserialize to reopen files
/// across app restarts.
#[derive(Debug, Clone)]
pub struct FileAccessToken {
    pub(crate) inner: TokenInner,
}

impl FileAccessToken {
    /// Open the file for reading.  Returns `Read + Seek`.
    ///
    /// # Errors
    ///
    /// Returns [`AccessError`] if permission is revoked or the file cannot be opened.
    #[must_use = "this returns a Result that may contain an error"]
    pub fn open_read(&self) -> Result<Box<dyn ReadSeek>, AccessError> {
        crate::platform::open_read(&self.inner)
    }

    /// Open the file for writing.  Returns `Write + Seek`.
    ///
    /// # Errors
    ///
    /// Returns [`AccessError`] if permission is revoked or the file cannot be opened.
    #[must_use = "this returns a Result that may contain an error"]
    pub fn open_write(&self) -> Result<Box<dyn WriteSeek>, AccessError> {
        crate::platform::open_write(&self.inner)
    }

    /// Returns the user-visible display name of the file (typically the filename).
    #[must_use]
    pub fn display_name(&self) -> &str {
        match &self.inner {
            TokenInner::Desktop { display_name, .. }
            | TokenInner::Android { display_name, .. }
            | TokenInner::Ios { display_name, .. } => display_name,
            TokenInner::Wasm { name, .. } => name,
        }
    }

    /// Returns the MIME type of the file, if known.  Desktop returns `None`.
    #[must_use]
    pub fn mime_type(&self) -> Option<&str> {
        match &self.inner {
            TokenInner::Desktop { .. } => None,
            TokenInner::Android { mime_type, .. }
            | TokenInner::Ios { mime_type, .. }
            | TokenInner::Wasm { mime_type, .. } => mime_type.as_deref(),
        }
    }

    /// Check whether the permission grant for this file is still valid.
    #[must_use]
    pub fn check_permission(&self) -> PermissionStatus {
        crate::platform::check_permission(&self.inner)
    }

    /// Serialize the token to a URL-safe base64-encoded string for storage.
    #[must_use]
    pub fn serialize(&self) -> String {
        // Serialization of the inner enum to JSON should not fail for our
        // data types (no maps with non-string keys, no infinite floats).
        // However, we handle the error path gracefully by returning an
        // empty-object JSON fallback, which will fail on deserialization
        // with a clear error rather than panicking here.
        let json = match serde_json::to_string(&self.inner) {
            Ok(j) => j,
            Err(_) => return URL_SAFE_NO_PAD.encode(b"{}"),
        };
        URL_SAFE_NO_PAD.encode(json.as_bytes())
    }

    /// Deserialize a token from a string previously returned by [`serialize`](Self::serialize).
    ///
    /// # Errors
    ///
    /// Returns [`TokenParseError`] if the string is malformed.
    pub fn deserialize(s: &str) -> Result<Self, TokenParseError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(s)
            .map_err(|e| TokenParseError::InvalidBase64 {
                message: e.to_string(),
            })?;

        let json = String::from_utf8(bytes).map_err(|e| TokenParseError::InvalidBase64 {
            message: e.to_string(),
        })?;

        let inner: TokenInner =
            serde_json::from_str(&json).map_err(|e| TokenParseError::InvalidJson {
                message: e.to_string(),
            })?;

        Ok(Self { inner })
    }
}

/// Trait object combining [`std::io::Read`] and [`std::io::Seek`].
pub trait ReadSeek: std::io::Read + std::io::Seek + Send {}
impl<T: std::io::Read + std::io::Seek + Send> ReadSeek for T {}

/// Trait object combining [`std::io::Write`] and [`std::io::Seek`].
pub trait WriteSeek: std::io::Write + std::io::Seek + Send {}
impl<T: std::io::Write + std::io::Seek + Send> WriteSeek for T {}

impl std::fmt::Display for FileAccessToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.serialize())
    }
}

impl std::str::FromStr for FileAccessToken {
    type Err = TokenParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::deserialize(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_desktop_token() {
        let token = FileAccessToken {
            inner: TokenInner::Desktop {
                path: PathBuf::from("/tmp/test.txt"),
                display_name: "test.txt".into(),
            },
        };
        let serialized = token.serialize();
        let restored = FileAccessToken::deserialize(&serialized).unwrap();
        assert_eq!(restored.display_name(), "test.txt");
        assert!(restored.mime_type().is_none());
    }

    #[test]
    fn round_trip_android_token() {
        let token = FileAccessToken {
            inner: TokenInner::Android {
                uri: "content://com.example/doc/1".into(),
                display_name: "photo.jpg".into(),
                mime_type: Some("image/jpeg".into()),
            },
        };
        let serialized = token.serialize();
        let restored = FileAccessToken::deserialize(&serialized).unwrap();
        assert_eq!(restored.display_name(), "photo.jpg");
        assert_eq!(restored.mime_type(), Some("image/jpeg"));
    }

    #[test]
    fn round_trip_ios_token() {
        let token = FileAccessToken {
            inner: TokenInner::Ios {
                bookmark: vec![0xDE, 0xAD, 0xBE, 0xEF],
                display_name: "notes.pdf".into(),
                mime_type: Some("application/pdf".into()),
            },
        };
        let serialized = token.serialize();
        let restored = FileAccessToken::deserialize(&serialized).unwrap();
        assert_eq!(restored.display_name(), "notes.pdf");
        assert_eq!(restored.mime_type(), Some("application/pdf"));
    }

    #[test]
    fn round_trip_wasm_token() {
        let token = FileAccessToken {
            inner: TokenInner::Wasm {
                data: vec![1, 2, 3, 4, 5],
                name: "data.bin".into(),
                mime_type: Some("application/octet-stream".into()),
            },
        };
        let serialized = token.serialize();
        let restored = FileAccessToken::deserialize(&serialized).unwrap();
        assert_eq!(restored.display_name(), "data.bin");
        assert_eq!(restored.mime_type(), Some("application/octet-stream"));
    }

    #[test]
    fn deserialize_invalid_base64_returns_error() {
        let result = FileAccessToken::deserialize("not!valid!base64!!!");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TokenParseError::InvalidBase64 { .. }
        ));
    }

    #[test]
    fn deserialize_invalid_json_returns_error() {
        let bad = URL_SAFE_NO_PAD.encode(b"not json");
        let result = FileAccessToken::deserialize(&bad);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            TokenParseError::InvalidJson { .. }
        ));
    }

    #[test]
    fn display_and_from_str_round_trip() {
        let token = FileAccessToken {
            inner: TokenInner::Desktop {
                path: PathBuf::from("/tmp/x.txt"),
                display_name: "x.txt".into(),
            },
        };
        let s = token.to_string();
        let restored: FileAccessToken = s.parse().unwrap();
        assert_eq!(restored.display_name(), "x.txt");
    }
}
