// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! Shared JNI error helpers used by the Android platform modules.
//!
//! Extracted here so both [`super::jni_intents`] and [`super::jni_activity`]
//! can import without circular dependencies.

use crate::error::PickerError;

// ── Error conversions ─────────────────────────────────────────────────────────

pub(super) fn jvm_err(e: jni::errors::Error) -> PickerError {
    PickerError::Platform {
        message: format!("failed to get JavaVM: {e}"),
    }
}

pub(super) fn attach_err(e: jni::errors::Error) -> PickerError {
    PickerError::Platform {
        message: format!("failed to attach JNI thread: {e}"),
    }
}

pub(super) fn platform_err(ctx: &str, e: impl std::fmt::Display) -> PickerError {
    PickerError::Platform {
        message: format!("{ctx}: {e}"),
    }
}
