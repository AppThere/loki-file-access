// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! JNI helpers for launching `FilePickerActivity`.
//!
//! `FilePickerActivity` is a thin Java trampoline that works around the
//! `ANativeActivityCallbacks` limitation: NativeActivity has no
//! `onActivityResult` slot, but a plain `Activity` subclass does.
//!
//! Flow:
//! 1. NativeActivity calls `startActivity(Intent → FilePickerActivity)`.
//! 2. `FilePickerActivity.onCreate` calls `startActivityForResult(ACTION_OPEN_*)`.
//! 3. `FilePickerActivity.onActivityResult` receives the URI and calls
//!    `nativeOnResult(uri)` — the pre-compiled JNI hook in the Rust binary.
//! 4. The Rust future resolves.

use super::jni_common::{attach_err, jvm_err, platform_err};
use crate::api::{PickOptions, SaveOptions};
use crate::error::PickerError;

// Fully-qualified Java class name for FilePickerActivity.
// NOTE: do NOT use this as a ComponentName package — that field identifies the
// APK, not the Java package.  Use Intent.setClassName(Context, String) instead
// so the runtime APK package name is derived from the Application context.
const FPA_CLASS: &str = "io.github.appthere.lokifileaccess.FilePickerActivity";

// ── Public entry points ───────────────────────────────────────────────────────

/// Start `FilePickerActivity` to open one or more files.
pub(super) fn fire_open_file_picker(
    options: &PickOptions,
    allow_multiple: bool,
) -> Result<(), PickerError> {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.map_err(jvm_err)?;
    let mut env = vm.attach_current_thread().map_err(attach_err)?;

    let intent = build_fpa_intent(&mut env, "OPEN")?;

    let mimes = options.mime_types.join(",");
    put_string_extra(&mut env, &intent, "mime_types", &mimes)?;

    // putExtra(String, boolean) — JNI signature: (Ljava/lang/String;Z)
    let key = env
        .new_string("allow_multiple")
        .map_err(|e| platform_err("allow_multiple key", e))?;
    env.call_method(
        &intent,
        "putExtra",
        "(Ljava/lang/String;Z)Landroid/content/Intent;",
        &[
            jni::objects::JValueGen::Object(&key),
            jni::objects::JValueGen::Bool(u8::from(allow_multiple)),
        ],
    )
    .map_err(|e| platform_err("putExtra allow_multiple", e))?;

    start_activity(&mut env, &intent)
}

/// Start `FilePickerActivity` to create/save a file.
pub(super) fn fire_create_file_picker(options: &SaveOptions) -> Result<(), PickerError> {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.map_err(jvm_err)?;
    let mut env = vm.attach_current_thread().map_err(attach_err)?;

    let intent = build_fpa_intent(&mut env, "CREATE")?;

    if let Some(ref mime) = options.mime_type {
        put_string_extra(&mut env, &intent, "mime_type", mime)?;
    }
    if let Some(ref name) = options.suggested_name {
        put_string_extra(&mut env, &intent, "suggested_name", name)?;
    }

    start_activity(&mut env, &intent)
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Build an explicit `Intent` targeting `FilePickerActivity` with a `mode` extra.
///
/// Uses `Intent.setClassName(Context, String)` rather than constructing a
/// `ComponentName` directly.  `ComponentName(pkg, cls)` uses `pkg` as the
/// *application* identifier (the APK package name), not the Java package.
/// Hardcoding the Java package `io.github.appthere.lokifileaccess` would cause
/// Android to return `START_CLASS_NOT_FOUND` (-92) because no installed APK has
/// that package name.  `setClassName(Context, cls)` derives the package from the
/// Application context at runtime, correctly targeting this APK regardless of
/// what package name the host app uses.
fn build_fpa_intent<'a>(
    env: &mut jni::JNIEnv<'a>,
    mode: &str,
) -> Result<jni::objects::JObject<'a>, PickerError> {
    // new Intent()
    let intent_cls = env
        .find_class("android/content/Intent")
        .map_err(|e| platform_err("Intent class", e))?;
    let intent = env
        .new_object(&intent_cls, "()V", &[])
        .map_err(|e| platform_err("Intent()", e))?;

    // intent.setClassName(context, FPA_CLASS) — resolves the APK package from
    // the Application context so the component is found in this app's process.
    let ctx = ndk_context::android_context();
    // SAFETY: ndk_context stores the Application jobject initialised before android_main.
    let context = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };
    let cls_str = env
        .new_string(FPA_CLASS)
        .map_err(|e| platform_err("fpa class string", e))?;
    env.call_method(
        &intent,
        "setClassName",
        "(Landroid/content/Context;Ljava/lang/String;)Landroid/content/Intent;",
        &[
            jni::objects::JValueGen::Object(&context),
            jni::objects::JValueGen::Object(&cls_str),
        ],
    )
    .map_err(|e| platform_err("setClassName", e))?;

    // intent.putExtra("mode", mode)
    put_string_extra(env, &intent, "mode", mode)?;

    Ok(intent)
}

/// Add a `String` extra to an `Intent`.
fn put_string_extra(
    env: &mut jni::JNIEnv<'_>,
    intent: &jni::objects::JObject<'_>,
    key: &str,
    value: &str,
) -> Result<(), PickerError> {
    let k = env
        .new_string(key)
        .map_err(|e| platform_err("extra key", e))?;
    let v = env
        .new_string(value)
        .map_err(|e| platform_err("extra value", e))?;
    env.call_method(
        intent,
        "putExtra",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;",
        &[
            jni::objects::JValueGen::Object(&k),
            jni::objects::JValueGen::Object(&v),
        ],
    )
    .map_err(|e| platform_err("putExtra string", e))?;
    Ok(())
}

/// Call `Context.startActivity(intent)` using the Application context.
///
/// `ndk_context` provides the Application object set by android-activity before
/// `android_main` is called.  Starting an Activity from a non-Activity context
/// requires `FLAG_ACTIVITY_NEW_TASK`, which is added to the intent here.
///
/// `FilePickerActivity` is a transparent trampoline: it receives its own
/// `onActivityResult` from the SAF picker (within its own task) and delivers
/// the URI to Rust via `nativeOnResult`.  No result needs to flow back to
/// NativeActivity, so the new-task restriction is not a problem.
fn start_activity(
    env: &mut jni::JNIEnv<'_>,
    intent: &jni::objects::JObject<'_>,
) -> Result<(), PickerError> {
    // FLAG_ACTIVITY_NEW_TASK — required when calling startActivity from a
    // non-Activity Context such as Application.
    const FLAG_ACTIVITY_NEW_TASK: i32 = 0x1000_0000;
    let flags_result = env.call_method(
        intent,
        "addFlags",
        "(I)Landroid/content/Intent;",
        &[jni::objects::JValueGen::Int(FLAG_ACTIVITY_NEW_TASK)],
    );
    if let Err(e) = flags_result {
        // Clear any pending JNI exception before returning so subsequent
        // JNI calls on this env do not trigger an ART abort.
        let _ = env.exception_clear();
        return Err(platform_err("addFlags", e));
    }

    let ctx = ndk_context::android_context();
    // SAFETY: ndk_context stores the Application jobject initialised by
    // android-activity before android_main runs.  Valid for the process lifetime.
    let context = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };

    let result = env.call_method(
        &context,
        "startActivity",
        "(Landroid/content/Intent;)V",
        &[jni::objects::JValueGen::Object(intent)],
    );

    if result.is_err() {
        // Clear any pending JNI exception (e.g. ActivityNotFoundException) so
        // subsequent JNI calls in this env do not trigger an ART abort.
        let _ = env.exception_clear();
    }

    result
        .map(|_| ())
        .map_err(|e| platform_err("startActivity", e))
}
