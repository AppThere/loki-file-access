// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! Android file-picker implementation using the Storage Access Framework (SAF).
//!
//! File access is mediated through content URIs and `ContentResolver`,
//! ensuring that the app never accesses files via filesystem paths (unreliable
//! on modern Android).
//!
//! # Persistence
//!
//! After the user selects a file, `ContentResolver.takePersistableUriPermission()`
//! is called with READ | WRITE flags.  This ensures the URI grant survives app
//! restarts and device reboots.
//!
//! # NativeActivity integration
//!
//! Call [`init_android`] from `android_main` before launching Dioxus:
//!
//! ```rust,no_run
//! fn android_main(android_app: android_activity::AndroidApp) {
//!     // init_android is a no-op kept for API compatibility; ndk_context
//!     // (initialised by android-activity before android_main) provides the
//!     // Application context used by all JNI calls.
//!     unsafe { loki_file_access::init_android(std::ptr::null_mut()); }
//!     blitz_shell::set_android_app(android_app);
//!     dioxus::launch(App);
//! }
//! ```
//!
//! # Result delivery
//!
//! `ANativeActivityCallbacks` has no `onActivityResult` slot, so results cannot
//! be delivered directly to NativeActivity.  Instead, NativeActivity calls
//! `startActivity(Intent → FilePickerActivity)`, which is a transparent Java
//! trampoline that runs its own `startActivityForResult(ACTION_OPEN_DOCUMENT)`.
//! `FilePickerActivity.onActivityResult` delivers the URI via the pre-compiled
//! JNI hook `Java_io_github_appthere_lokifileaccess_FilePickerActivity_nativeOnResult`.
//!
//! **Prerequisite**: `FilePickerActivity` must be declared in `AndroidManifest.xml`
//! and its compiled `classes.dex` injected into the APK (see `scripts/build-android.ps1`).

mod jni_activity;
mod jni_common;
mod jni_fd;
mod jni_ime;
mod jni_insets;
mod jni_intents;

use std::sync::{Arc, Mutex, OnceLock};

use crate::api::{PickOptions, SaveOptions};
use crate::error::{AccessError, PickerError};
use crate::future::{deliver, new_pick_future};
use crate::token::{FileAccessToken, PermissionStatus, ReadSeek, TokenInner, WriteSeek};

/// Pending pick state shared between the intent launcher and the JNI callback.
///
/// The payload is `Vec<String>`: empty means cancelled, non-empty contains the
/// selected content URIs (one for single-pick, one or more for multi-pick).
static PENDING_PICK: OnceLock<Mutex<Option<Arc<Mutex<crate::future::PickState<Vec<String>>>>>>> =
    OnceLock::new();

fn pending_pick() -> &'static Mutex<Option<Arc<Mutex<crate::future::PickState<Vec<String>>>>>> {
    PENDING_PICK.get_or_init(|| Mutex::new(None))
}

// ── Public Android initialisation ─────────────────────────────────────────────

/// Initialise the file-access layer.
///
/// Must be called from `android_main` before launching Dioxus.  The parameter
/// is accepted for API compatibility but is no longer stored — `startActivity`
/// now uses the Application context from `ndk_context` directly, which is set
/// up by `android-activity` before `android_main` is called.
///
/// # Safety
///
/// The caller is responsible for ensuring `android_main` setup (including
/// `ndk_context` initialisation by `android-activity`) is complete before
/// any file-picker calls are made.
pub unsafe fn init_android(_activity_as_ptr: *mut std::ffi::c_void) {
    // No-op: ndk_context provides the Application object used by all JNI calls.
}

/// Query Android system-bar heights from OS resources.
///
/// Returns `(top_dp, bottom_dp)` — heights in density-independent pixels for
/// the status bar (top) and navigation bar (bottom). Safe to call immediately
/// after [`init_android`] before the window is laid out.
pub fn query_insets_dp() -> (f32, f32) {
    jni_insets::query_insets_dp()
}

/// Query orientation-aware safe-area insets from the activity window, in dp.
///
/// Returns `(top, bottom, left, right)` from the real window insets (system bars
/// + display cutout), which — unlike [`query_insets_dp`] — change with
/// orientation. `activity_ptr` is `AndroidApp::activity_as_ptr()`. Returns
/// `None` before the window is laid out / on API < 30; callers fall back to
/// [`query_insets_dp`].
pub fn query_window_insets_dp(activity_ptr: *mut std::ffi::c_void) -> Option<(f32, f32, f32, f32)> {
    jni_insets::query_window_insets_dp(activity_ptr)
}

// ── Soft-keyboard (IME) visibility signal ─────────────────────────────────────

pub use jni_ime::{install_ime_listener, set_ime_visibility_listener};

// ── JNI result callback (called from Java FilePickerActivity) ─────────────────

/// Delivers the selected URI (or `null` for cancellation) to the pending Rust future.
///
/// Called by `FilePickerActivity.onActivityResult` via JNI.  The method is
/// declared `private native void nativeOnResult(String)` in the Java class
/// `io.github.appthere.lokifileaccess.FilePickerActivity`, so the JVM resolves
/// it to this exported symbol automatically once the native library is loaded.
///
/// **The native library is loaded by NativeActivity before `FilePickerActivity`
/// is ever created**, so `System.loadLibrary` is not needed in the Java class.
///
/// # Safety
///
/// Must be called from a JNI-attached Java thread.
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_appthere_lokifileaccess_FilePickerActivity_nativeOnResult(
    mut env: jni::JNIEnv<'_>,
    _this: jni::objects::JObject<'_>,
    uri: jni::objects::JString<'_>,
) {
    // The Java side sends a '\n'-delimited string of content URIs.
    // Null or an empty string means the user cancelled.
    let uris: Vec<String> = if uri.is_null() {
        Vec::new()
    } else {
        env.get_string(&uri)
            .ok()
            .map(|s| {
                let joined: String = s.into();
                joined
                    .split('\n')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    on_activity_result(uris);
}

// ── Pick / save entry points ──────────────────────────────────────────────────

/// Pick a single file for reading via `ACTION_OPEN_DOCUMENT`.
pub(crate) async fn pick_open_single(
    options: PickOptions,
) -> Result<Option<FileAccessToken>, PickerError> {
    let uris = launch_open_intent(&options, false).await?;
    match uris.into_iter().next() {
        None => Ok(None),
        Some(uri_str) => {
            jni_intents::take_persistable_uri_permission(&uri_str)?;
            let display_name = jni_intents::query_display_name(&uri_str);
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
    let uris = launch_open_intent(&options, true).await?;
    let mut tokens = Vec::with_capacity(uris.len());
    for uri_str in uris {
        // Skip URIs whose persistable grant fails (e.g. a cloud provider that
        // revoked the grant between SAF delivery and this call, or a URI that
        // exceeds Android's per-app persisted-permission quota).  Aborting the
        // entire batch with `?` would orphan grants already taken for earlier
        // URIs — those cannot be un-granted, silently consuming quota.
        if jni_intents::take_persistable_uri_permission(&uri_str).is_err() {
            tracing::warn!("loki-file-access: skipping URI with failed permission grant");
            continue;
        }
        let display_name = jni_intents::query_display_name(&uri_str);
        tokens.push(FileAccessToken {
            inner: TokenInner::Android {
                uri: uri_str,
                display_name,
                mime_type: None,
            },
        });
    }
    Ok(tokens)
}

/// Pick a save location via `ACTION_CREATE_DOCUMENT`.
pub(crate) async fn pick_save(
    options: SaveOptions,
) -> Result<Option<FileAccessToken>, PickerError> {
    let uris = launch_create_intent(&options).await?;
    match uris.into_iter().next() {
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
            // Android's `ContentResolver.openFileDescriptor` after detaching it.
            // The caller takes ownership; it must not be double-closed.
            let file: std::fs::File = unsafe { std::os::fd::FromRawFd::from_raw_fd(fd) };
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
            let file: std::fs::File = unsafe { std::os::fd::FromRawFd::from_raw_fd(fd) };
            Ok(Box::new(file))
        }
        _ => Err(AccessError::Platform {
            message: "non-Android token on Android platform".into(),
        }),
    }
}

/// Delete the file referenced by a token.
///
/// Deleting a SAF document URI requires `DocumentsContract.deleteDocument`,
/// which is not yet wired through JNI.  Return an explicit unsupported error
/// rather than silently succeeding.
pub(crate) fn delete(_inner: &TokenInner) -> Result<(), AccessError> {
    Err(AccessError::Unsupported {
        operation: "delete (Android SAF content-URI deletion not implemented)".into(),
    })
}

/// Check whether a persistable URI permission is still held.
pub(crate) fn check_permission(inner: &TokenInner) -> PermissionStatus {
    match inner {
        TokenInner::Android { uri, .. } => {
            jni_fd::check_persisted_permission(uri).unwrap_or(PermissionStatus::Unknown)
        }
        _ => PermissionStatus::Unknown,
    }
}

/// Deliver the selected URIs (or an empty Vec for cancellation) to the pending future.
pub fn on_activity_result(uris: Vec<String>) {
    let guard = match pending_pick().lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(ref state) = *guard {
        deliver(state, uris);
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Seconds to wait before treating a non-returning file picker as abandoned.
const PICKER_TIMEOUT_SECS: u64 = 600;

/// Spawn a background thread that cancels the pick after [`PICKER_TIMEOUT_SECS`].
///
/// If `nativeOnResult` fires before the deadline, the PickState already has a
/// result (`result.is_some()`).  The thread then exits without overwriting it,
/// so a real URI is never silently replaced by a spurious cancellation.
///
/// This guards against Android 12+ silently blocking `startActivity` when the
/// app has no foreground window: without this thread, `future.await` hangs
/// forever.  The spawned thread exits immediately after the normal pick
/// completes, so the OS thread count stays bounded in typical usage.
fn spawn_timeout_guard(state: Arc<Mutex<crate::future::PickState<Vec<String>>>>) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(PICKER_TIMEOUT_SECS));
        // Only deliver if the pick has not already resolved.  Overwriting a
        // real result here would silently cancel a successful file open.
        let waker = {
            let mut guard = match state.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            if guard.result.is_some() {
                return; // Real result already delivered; nothing to do.
            }
            guard.result = Some(Vec::new());
            guard.waker.take()
        };
        if let Some(w) = waker {
            w.wake();
        }
    });
}

fn store_pending(
    state: Arc<Mutex<crate::future::PickState<Vec<String>>>>,
) -> Result<(), PickerError> {
    let mut guard = pending_pick().lock().map_err(|e| PickerError::Internal {
        message: e.to_string(),
    })?;
    *guard = Some(state);
    Ok(())
}

/// Launch `FilePickerActivity` to open a file and await the result.
///
/// Returns an empty `Vec` on cancellation, or one URI per selected file.
async fn launch_open_intent(
    options: &PickOptions,
    allow_multiple: bool,
) -> Result<Vec<String>, PickerError> {
    let (future, state) = new_pick_future::<Vec<String>>();
    // Clone the Arc before moving `state` into store_pending so the timeout
    // thread can deliver an empty result if nativeOnResult never fires.
    let timeout_state = Arc::clone(&state);
    store_pending(state)?;
    jni_activity::fire_open_file_picker(options, allow_multiple)?;
    spawn_timeout_guard(timeout_state);
    Ok(future.await)
}

/// Launch `FilePickerActivity` to save a file and await the result.
///
/// Returns an empty `Vec` on cancellation, or a single-element `Vec` on success.
async fn launch_create_intent(options: &SaveOptions) -> Result<Vec<String>, PickerError> {
    let (future, state) = new_pick_future::<Vec<String>>();
    let timeout_state = Arc::clone(&state);
    store_pending(state)?;
    jni_activity::fire_create_file_picker(options)?;
    spawn_timeout_guard(timeout_state);
    Ok(future.await)
}
