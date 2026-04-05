// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! Error types for the `loki-file-access` crate.
//!
//! This module defines all error enums used across the public API surface:
//!
//! - [`PickerError`] — errors originating from the platform file-picker dialog.
//! - [`AccessError`] — errors when reading from or writing to a previously granted file.
//! - [`TokenParseError`] — errors when deserializing a stored [`crate::FileAccessToken`].
//!
//! All enums are `#[non_exhaustive]` so that new variants can be added in
//! future minor versions without breaking downstream matches.

/// Errors that can occur when presenting a file-picker dialog.
///
/// Note that the user cancelling the dialog is **not** an error — it is
/// represented as `Ok(None)` on the single-file methods and `Ok(vec![])` on
/// multi-file methods.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PickerError {
    /// The platform returned an error from the native file-picker API.
    #[error("platform file-picker error: {message}")]
    Platform {
        /// Human-readable description of the platform error.
        message: String,
    },

    /// The current platform does not support the requested operation.
    #[error("operation not supported on this platform: {operation}")]
    Unsupported {
        /// Description of the unsupported operation.
        operation: String,
    },

    /// An internal synchronisation error occurred (e.g. a poisoned mutex).
    #[error("internal synchronisation error: {message}")]
    Internal {
        /// Human-readable description of the internal error.
        message: String,
    },
}

/// Errors that can occur when opening or accessing a previously granted file.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AccessError {
    /// The permission grant for this file has been revoked by the user or OS.
    #[error("file access permission has been revoked")]
    PermissionRevoked,

    /// An I/O error occurred while reading from or writing to the file.
    #[error("I/O error: {source}")]
    Io {
        /// The underlying I/O error.
        #[from]
        source: std::io::Error,
    },

    /// The file descriptor or handle returned by the platform was invalid.
    #[error("invalid file descriptor returned by platform")]
    InvalidDescriptor,

    /// The platform returned an error when attempting to open the file.
    #[error("platform access error: {message}")]
    Platform {
        /// Human-readable description of the platform error.
        message: String,
    },
}

/// Errors that can occur when deserializing a stored [`crate::FileAccessToken`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TokenParseError {
    /// The base64 encoding of the token is invalid.
    #[error("invalid base64 encoding: {message}")]
    InvalidBase64 {
        /// Description of the base64 decode error.
        message: String,
    },

    /// The JSON payload inside the token is malformed.
    #[error("invalid JSON in token: {message}")]
    InvalidJson {
        /// Description of the JSON parse error.
        message: String,
    },

    /// The token contains an unrecognised platform variant.
    #[error("unknown token variant: {variant}")]
    UnknownVariant {
        /// The variant tag that was not recognised.
        variant: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_error_platform_displays_message() {
        let err = PickerError::Platform {
            message: "dialog failed".into(),
        };
        let msg = err.to_string();
        assert!(!msg.is_empty(), "display string must not be empty");
        assert!(msg.contains("dialog failed"));
    }

    #[test]
    fn picker_error_unsupported_displays_message() {
        let err = PickerError::Unsupported {
            operation: "save".into(),
        };
        let msg = err.to_string();
        assert!(!msg.is_empty());
        assert!(msg.contains("save"));
    }

    #[test]
    fn picker_error_internal_displays_message() {
        let err = PickerError::Internal {
            message: "mutex poisoned".into(),
        };
        let msg = err.to_string();
        assert!(!msg.is_empty());
        assert!(msg.contains("mutex poisoned"));
    }

    #[test]
    fn access_error_permission_revoked_displays_message() {
        let err = AccessError::PermissionRevoked;
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn access_error_io_displays_message() {
        let err = AccessError::Io {
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "gone"),
        };
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn access_error_invalid_descriptor_displays_message() {
        let err = AccessError::InvalidDescriptor;
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn access_error_platform_displays_message() {
        let err = AccessError::Platform {
            message: "fd error".into(),
        };
        let msg = err.to_string();
        assert!(!msg.is_empty());
        assert!(msg.contains("fd error"));
    }

    #[test]
    fn token_parse_error_base64_displays_message() {
        let err = TokenParseError::InvalidBase64 {
            message: "bad padding".into(),
        };
        let msg = err.to_string();
        assert!(!msg.is_empty());
        assert!(msg.contains("bad padding"));
    }

    #[test]
    fn token_parse_error_json_displays_message() {
        let err = TokenParseError::InvalidJson {
            message: "unexpected EOF".into(),
        };
        let msg = err.to_string();
        assert!(!msg.is_empty());
        assert!(msg.contains("unexpected EOF"));
    }

    #[test]
    fn token_parse_error_unknown_variant_displays_message() {
        let err = TokenParseError::UnknownVariant {
            variant: "FuturePlatform".into(),
        };
        let msg = err.to_string();
        assert!(!msg.is_empty());
        assert!(msg.contains("FuturePlatform"));
    }
}
