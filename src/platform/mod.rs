// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! Platform dispatch layer.
//!
//! This module selects the appropriate platform implementation at compile time
//! using `cfg` attributes and re-exports the four operations that the public
//! API delegates to:
//!
//! - [`pick_open_single`] — pick one file for reading
//! - [`pick_open_multi`] — pick multiple files for reading
//! - [`pick_save`] — pick a save location
//! - [`open_read`] / [`open_write`] — open a token for I/O
//! - [`check_permission`] — query whether a token is still valid

#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
mod desktop;
#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
pub(crate) use desktop::{
    check_permission, open_read, open_write, pick_open_multi, pick_open_single, pick_save,
};

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub(crate) use android::{
    check_permission, open_read, open_write, pick_open_multi, pick_open_single, pick_save,
};

#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
pub(crate) use ios::{
    check_permission, open_read, open_write, pick_open_multi, pick_open_single, pick_save,
};

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub(crate) use wasm::{
    check_permission, open_read, open_write, pick_open_multi, pick_open_single, pick_save,
};

// Ensure a compile error on unsupported platforms rather than silent failure.
#[cfg(not(any(
    target_os = "android",
    target_os = "ios",
    target_arch = "wasm32",
    target_os = "windows",
    target_os = "macos",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly",
)))]
compile_error!(
    "loki-file-access: unsupported target platform. \
     Supported: Windows, macOS, Linux, BSD, Android, iOS, WASM."
);
