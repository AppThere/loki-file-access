// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! JNI query for Android system-bar heights.
//!
//! Uses `Resources.getDimensionPixelSize` with the well-known system resource
//! identifiers `status_bar_height` and `navigation_bar_height`. These resource
//! identifiers are available immediately at process start — before the window
//! is laid out — making them safe to query from `android_main`.
//!
//! Returns heights in density-independent pixels (dp / CSS px) so callers can
//! apply them directly as CSS `padding` values.

use jni::JNIEnv;
use jni::objects::{JObject, JValueGen};

/// Query Android system-bar heights from OS resources.
///
/// Returns `(top_dp, bottom_dp)` where:
/// - `top_dp` is the status-bar height (top of screen)
/// - `bottom_dp` is the navigation-bar height (bottom of screen; 0 when using
///   full gesture navigation with no visible bar)
///
/// Falls back to `(24.0, 0.0)` on any JNI failure so the status bar area is
/// always reserved even if the exact height cannot be determined.
pub(super) fn query_insets_dp() -> (f32, f32) {
    // Clear any stale JNI exception from the fallback path so the caller's
    // thread remains usable. Any pending exception is cleared at the end of
    // do_query regardless of the outcome.
    do_query().unwrap_or((24.0, 0.0))
}

// ── Implementation ────────────────────────────────────────────────────────────

fn do_query() -> Option<(f32, f32)> {
    let ctx = ndk_context::android_context();
    // SAFETY: ndk_context stores the JVM pointer initialised by android-activity
    // before android_main is called. It is valid for the process lifetime.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;

    let result = query_with_env(&mut env);

    // Clear any pending JNI exception so the calling thread stays usable.
    let _ = env.exception_clear();

    result
}

fn query_with_env(env: &mut JNIEnv<'_>) -> Option<(f32, f32)> {
    let ctx = ndk_context::android_context();
    // SAFETY: ndk_context stores the Application jobject; Application IS a
    // Context and provides Resources, so this is safe for resource queries.
    let context = unsafe { JObject::from_raw(ctx.context().cast()) };

    // context.getResources() → android.content.res.Resources
    let resources = env
        .call_method(
            &context,
            "getResources",
            "()Landroid/content/res/Resources;",
            &[],
        )
        .ok()?
        .l()
        .ok()?;

    // resources.getDisplayMetrics() → android.util.DisplayMetrics
    let metrics = env
        .call_method(
            &resources,
            "getDisplayMetrics",
            "()Landroid/util/DisplayMetrics;",
            &[],
        )
        .ok()?
        .l()
        .ok()?;

    // DisplayMetrics.density: float — e.g. 1.0 (mdpi), 2.0 (xhdpi), 3.0 (xxhdpi)
    let density = env.get_field(&metrics, "density", "F").ok()?.f().ok()?;
    if density <= 0.0 {
        return None;
    }

    let top_dp = dimen_dp(env, &resources, "status_bar_height", density).unwrap_or(24.0);
    let bottom_dp = dimen_dp(env, &resources, "navigation_bar_height", density).unwrap_or(0.0);

    Some((top_dp, bottom_dp))
}

// ── Orientation-aware window insets (edge-to-edge) ──────────────────────────────

/// Query the actual per-side safe-area insets from the activity's window, in dp.
///
/// Returns `(top, bottom, left, right)` from
/// `decorView.getRootWindowInsets().getInsets(systemBars | displayCutout | ime)`
/// — the real, orientation-aware insets for an edge-to-edge window (e.g. in
/// landscape the navigation bar / cutout move to a side, so `left`/`right`
/// become non-zero and `top` shrinks).  Unlike [`query_insets_dp`], this is not
/// orientation-independent.
///
/// The mask also includes the soft-keyboard (IME) inset, so when the keyboard
/// is visible the returned `bottom` grows to the keyboard height (the
/// `getInsets` union takes the per-side max, and `ime()` only contributes a
/// bottom inset).  Re-querying this after the keyboard is shown/hidden — see the
/// IME-settle re-sync in `blitz-shell` — lets the app reserve a bottom safe
/// area for the keyboard on a `NativeActivity` whose surface does not resize.
///
/// `activity_ptr` is the activity `jobject` from
/// `android_activity::AndroidApp::activity_as_ptr()` — the `ndk_context` context
/// is the *Application*, which has no window, so the activity must be passed in.
///
/// Returns `None` (caller should fall back to [`query_insets_dp`]) when the view
/// is not yet attached (`getRootWindowInsets` is null), on API < 30
/// (`getInsets(int)` unavailable), or on any JNI failure.
pub(super) fn query_window_insets_dp(
    activity_ptr: *mut std::ffi::c_void,
) -> Option<(f32, f32, f32, f32)> {
    if activity_ptr.is_null() {
        return None;
    }
    let ctx = ndk_context::android_context();
    // SAFETY: ndk_context stores the JVM pointer initialised by android-activity
    // before android_main; valid for the process lifetime.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    let result = window_insets_with_env(&mut env, activity_ptr);
    // Clear any pending exception (e.g. NoSuchMethodError on API < 30) so the
    // calling thread stays usable.
    let _ = env.exception_clear();
    result
}

fn window_insets_with_env(
    env: &mut JNIEnv<'_>,
    activity_ptr: *mut std::ffi::c_void,
) -> Option<(f32, f32, f32, f32)> {
    // SAFETY: `activity_ptr` is a global reference jobject owned by AndroidApp,
    // valid for the activity's lifetime.
    let activity = unsafe { JObject::from_raw(activity_ptr.cast()) };

    // activity.getWindow().getDecorView().getRootWindowInsets()
    let window = env
        .call_method(&activity, "getWindow", "()Landroid/view/Window;", &[])
        .ok()?
        .l()
        .ok()?;
    let decor = env
        .call_method(&window, "getDecorView", "()Landroid/view/View;", &[])
        .ok()?
        .l()
        .ok()?;
    let insets_obj = env
        .call_method(
            &decor,
            "getRootWindowInsets",
            "()Landroid/view/WindowInsets;",
            &[],
        )
        .ok()?
        .l()
        .ok()?;
    if insets_obj.as_raw().is_null() {
        return None; // view not attached yet
    }

    // mask = WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout()
    //        | WindowInsets.Type.ime()      (all static int methods, API 30+).
    let type_cls = env.find_class("android/view/WindowInsets$Type").ok()?;
    let system_bars = env
        .call_static_method(&type_cls, "systemBars", "()I", &[])
        .ok()?
        .i()
        .ok()?;
    let cutout = env
        .call_static_method(&type_cls, "displayCutout", "()I", &[])
        .ok()?
        .i()
        .ok()?;
    // Fold in the soft-keyboard (IME) inset so `bottom` reserves space for the
    // keyboard when it is visible. `getInsets` returns the per-side union and
    // `ime()` contributes only a bottom inset, so top/left/right are unaffected
    // while the keyboard is hidden (its bottom inset is then 0). `ime()` is API
    // 30+, the same level as `getInsets(int)`, so the existing `.ok()?`
    // fallback to `query_insets_dp` already covers API < 30.
    let ime = env
        .call_static_method(&type_cls, "ime", "()I", &[])
        .ok()?
        .i()
        .ok()?;
    let mask = system_bars | cutout | ime;

    // insets = windowInsets.getInsets(mask) → android.graphics.Insets (API 30+)
    let insets = env
        .call_method(
            &insets_obj,
            "getInsets",
            "(I)Landroid/graphics/Insets;",
            &[JValueGen::Int(mask)],
        )
        .ok()?
        .l()
        .ok()?;
    let left = env.get_field(&insets, "left", "I").ok()?.i().ok()?;
    let top = env.get_field(&insets, "top", "I").ok()?.i().ok()?;
    let right = env.get_field(&insets, "right", "I").ok()?.i().ok()?;
    let bottom = env.get_field(&insets, "bottom", "I").ok()?.i().ok()?;

    let density = display_density(env)?;
    if density <= 0.0 {
        return None;
    }
    Some((
        top as f32 / density,
        bottom as f32 / density,
        left as f32 / density,
        right as f32 / density,
    ))
}

/// Display density (e.g. 2.625) from the Application's `DisplayMetrics`.
fn display_density(env: &mut JNIEnv<'_>) -> Option<f32> {
    let ctx = ndk_context::android_context();
    // SAFETY: ndk_context stores the Application jobject; it provides Resources.
    let context = unsafe { JObject::from_raw(ctx.context().cast()) };
    let resources = env
        .call_method(
            &context,
            "getResources",
            "()Landroid/content/res/Resources;",
            &[],
        )
        .ok()?
        .l()
        .ok()?;
    let metrics = env
        .call_method(
            &resources,
            "getDisplayMetrics",
            "()Landroid/util/DisplayMetrics;",
            &[],
        )
        .ok()?
        .l()
        .ok()?;
    env.get_field(&metrics, "density", "F").ok()?.f().ok()
}

/// Look up one Android system dimension resource and convert physical pixels → dp.
fn dimen_dp(
    env: &mut JNIEnv<'_>,
    resources: &JObject<'_>,
    name: &str,
    density: f32,
) -> Option<f32> {
    let name_jstr = env.new_string(name).ok()?;
    let type_jstr = env.new_string("dimen").ok()?;
    let pkg_jstr = env.new_string("android").ok()?;

    // resources.getIdentifier(name, "dimen", "android") → int resource ID
    let res_id: i32 = env
        .call_method(
            resources,
            "getIdentifier",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)I",
            &[
                JValueGen::Object(&name_jstr),
                JValueGen::Object(&type_jstr),
                JValueGen::Object(&pkg_jstr),
            ],
        )
        .ok()?
        .i()
        .ok()?;

    if res_id == 0 {
        return Some(0.0);
    }

    // resources.getDimensionPixelSize(resId) → int physical pixels
    let px: i32 = env
        .call_method(
            resources,
            "getDimensionPixelSize",
            "(I)I",
            &[JValueGen::Int(res_id)],
        )
        .ok()?
        .i()
        .ok()?;

    Some(px as f32 / density)
}
