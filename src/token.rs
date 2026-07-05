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

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
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

    /// Delete the underlying file.
    ///
    /// On desktop this removes the file from the filesystem.  On platforms
    /// where deletion is not yet implemented (Android, iOS, WASM) this returns
    /// [`AccessError::Unsupported`] rather than silently succeeding.
    ///
    /// # Errors
    ///
    /// Returns [`AccessError`] if the file cannot be deleted or deletion is
    /// not supported on the current platform.
    #[must_use = "this returns a Result that may contain an error"]
    pub fn delete(&self) -> Result<(), AccessError> {
        crate::platform::delete(&self.inner)
    }

    /// Copy the full contents of this file into `dest`.
    ///
    /// Reads all bytes from `self` via [`open_read`](Self::open_read) and writes
    /// them to `dest` via [`open_write`](Self::open_write).  This works across
    /// every platform because it relies only on the token I/O primitives, not
    /// on filesystem paths.
    ///
    /// # Errors
    ///
    /// Returns [`AccessError`] if either token cannot be opened or an I/O error
    /// occurs while transferring bytes.
    #[must_use = "this returns a Result that may contain an error"]
    pub fn copy_bytes_to(&self, dest: &FileAccessToken) -> Result<(), AccessError> {
        use std::io::{Read as _, Write as _};

        let mut reader = self.open_read()?;
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;

        let mut writer = dest.open_write()?;
        writer.write_all(&bytes)?;
        writer.flush()?;
        Ok(())
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

    // The following tests exercise the platform-routed operations.  They only
    // run on desktop targets, where the desktop backend is compiled in.
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    fn desktop_token_for(path: &std::path::Path) -> FileAccessToken {
        FileAccessToken {
            inner: TokenInner::Desktop {
                path: path.to_path_buf(),
                display_name: path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unnamed")
                    .to_owned(),
            },
        }
    }

    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    #[test]
    fn desktop_delete_removes_file() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("loki_delete_test_{}.txt", std::process::id()));
        std::fs::write(&path, b"hello").expect("write temp file");
        assert!(path.exists());

        let token = desktop_token_for(&path);
        token.delete().expect("delete should succeed");
        assert!(!path.exists(), "file must be gone after delete");
    }

    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    #[test]
    fn desktop_delete_missing_file_is_io_error() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("loki_delete_missing_{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let token = desktop_token_for(&path);
        let err = token
            .delete()
            .expect_err("deleting a missing file must error");
        assert!(matches!(err, AccessError::Io { .. }));
    }

    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    #[test]
    fn desktop_copy_bytes_to_duplicates_contents() {
        let dir = std::env::temp_dir();
        let pid = std::process::id();
        let src_path = dir.join(format!("loki_copy_src_{pid}.txt"));
        let dest_path = dir.join(format!("loki_copy_dest_{pid}.txt"));
        std::fs::write(&src_path, b"copy me").expect("write source");
        let _ = std::fs::remove_file(&dest_path);

        let src = desktop_token_for(&src_path);
        let dest = desktop_token_for(&dest_path);
        src.copy_bytes_to(&dest).expect("copy should succeed");

        let copied = std::fs::read(&dest_path).expect("read dest");
        assert_eq!(copied, b"copy me");

        let _ = std::fs::remove_file(&src_path);
        let _ = std::fs::remove_file(&dest_path);
    }

    // On non-desktop targets the unsupported-platform path is taken: deleting
    // any token must surface AccessError::Unsupported rather than succeed.
    #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
    #[test]
    fn non_desktop_delete_is_unsupported() {
        let token = FileAccessToken {
            inner: TokenInner::Desktop {
                path: PathBuf::from("/tmp/whatever.txt"),
                display_name: "whatever.txt".into(),
            },
        };
        let err = token
            .delete()
            .expect_err("delete must be unsupported off-desktop");
        assert!(matches!(err, AccessError::Unsupported { .. }));
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
