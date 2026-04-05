// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! Low-level JNI helpers for Android SAF integration.
//!
//! This submodule contains all direct JNI calls used by the Android platform
//! implementation, including intent launching, permission management, and
//! file-descriptor acquisition.

use crate::api::{PickOptions, SaveOptions};
use crate::error::PickerError;

/// Fire `ACTION_OPEN_DOCUMENT` via JNI.
pub(super) fn fire_open_document_intent(
    options: &PickOptions,
    _allow_multiple: bool,
) -> Result<(), PickerError> {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.map_err(jvm_err)?;
    let mut env = vm.attach_current_thread().map_err(attach_err)?;

    let intent = create_intent(&mut env, "android.intent.action.OPEN_DOCUMENT")?;

    if let Some(mime) = options.mime_types.first() {
        set_intent_type(&mut env, &intent, mime)?;
    }

    start_activity_for_result(&mut env, &ctx, &intent, 1001)
}

/// Fire `ACTION_CREATE_DOCUMENT` via JNI.
pub(super) fn fire_create_document_intent(
    options: &SaveOptions,
) -> Result<(), PickerError> {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.map_err(jvm_err)?;
    let mut env = vm.attach_current_thread().map_err(attach_err)?;

    let intent = create_intent(&mut env, "android.intent.action.CREATE_DOCUMENT")?;

    if let Some(ref mime) = options.mime_type {
        set_intent_type(&mut env, &intent, mime)?;
    }

    if let Some(ref name) = options.suggested_name {
        let key = env
            .new_string("android.intent.extra.TITLE")
            .map_err(|e| platform_err("TITLE key", e))?;
        let val = env
            .new_string(name)
            .map_err(|e| platform_err("title value", e))?;
        env.call_method(
            &intent,
            "putExtra",
            "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;",
            &[
                jni::objects::JValueGen::Object(&key),
                jni::objects::JValueGen::Object(&val),
            ],
        )
        .map_err(|e| platform_err("putExtra", e))?;
    }

    start_activity_for_result(&mut env, &ctx, &intent, 1002)
}

/// Call `ContentResolver.takePersistableUriPermission` for a URI.
pub(super) fn take_persistable_uri_permission(uri: &str) -> Result<(), PickerError> {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.map_err(jvm_err)?;
    let mut env = vm.attach_current_thread().map_err(attach_err)?;

    let uri_obj = parse_uri(&mut env, uri)?;
    let resolver = get_content_resolver(&mut env, &ctx)?;

    // FLAG_GRANT_READ_URI_PERMISSION (1) | FLAG_GRANT_WRITE_URI_PERMISSION (2)
    env.call_method(
        &resolver,
        "takePersistableUriPermission",
        "(Landroid/net/Uri;I)V",
        &[
            jni::objects::JValueGen::Object(&uri_obj),
            jni::objects::JValueGen::Int(3),
        ],
    )
    .map_err(|e| platform_err("takePersistableUriPermission", e))?;

    Ok(())
}

// ── Shared helpers ───��──────────────────────────────────────────────────

/// Create an `Intent` with the given action string.
fn create_intent<'a>(
    env: &mut jni::JNIEnv<'a>,
    action: &str,
) -> Result<jni::objects::JObject<'a>, PickerError> {
    let cls = env
        .find_class("android/content/Intent")
        .map_err(|e| platform_err("Intent class", e))?;
    let action_str = env
        .new_string(action)
        .map_err(|e| platform_err("action string", e))?;
    env.new_object(
        &cls,
        "(Ljava/lang/String;)V",
        &[jni::objects::JValueGen::Object(&action_str)],
    )
    .map_err(|e| platform_err("create Intent", e))
}

/// Set the MIME type on an intent.
fn set_intent_type(
    env: &mut jni::JNIEnv<'_>,
    intent: &jni::objects::JObject<'_>,
    mime: &str,
) -> Result<(), PickerError> {
    let mime_str = env
        .new_string(mime)
        .map_err(|e| platform_err("MIME string", e))?;
    env.call_method(
        intent,
        "setType",
        "(Ljava/lang/String;)Landroid/content/Intent;",
        &[jni::objects::JValueGen::Object(&mime_str)],
    )
    .map_err(|e| platform_err("setType", e))?;
    Ok(())
}

/// Call `startActivityForResult` on the current activity.
fn start_activity_for_result(
    env: &mut jni::JNIEnv<'_>,
    ctx: &ndk_context::AndroidContext,
    intent: &jni::objects::JObject<'_>,
    request_code: i32,
) -> Result<(), PickerError> {
    let activity = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };
    env.call_method(
        &activity,
        "startActivityForResult",
        "(Landroid/content/Intent;I)V",
        &[
            jni::objects::JValueGen::Object(intent),
            jni::objects::JValueGen::Int(request_code),
        ],
    )
    .map_err(|e| platform_err("startActivityForResult", e))?;
    Ok(())
}

/// Parse a URI string into a `Uri` JNI object.
pub(super) fn parse_uri<'a>(
    env: &mut jni::JNIEnv<'a>,
    uri: &str,
) -> Result<jni::objects::JObject<'a>, PickerError> {
    let cls = env
        .find_class("android/net/Uri")
        .map_err(|e| platform_err("Uri class", e))?;
    let s = env
        .new_string(uri)
        .map_err(|e| platform_err("URI string", e))?;
    env.call_static_method(
        &cls,
        "parse",
        "(Ljava/lang/String;)Landroid/net/Uri;",
        &[jni::objects::JValueGen::Object(&s)],
    )
    .map_err(|e| platform_err("Uri.parse", e))?
    .l()
    .map_err(|e| platform_err("Uri.parse object", e))
}

/// Get the `ContentResolver` from the activity context.
pub(super) fn get_content_resolver<'a>(
    env: &mut jni::JNIEnv<'a>,
    ctx: &ndk_context::AndroidContext,
) -> Result<jni::objects::JObject<'a>, PickerError> {
    let activity = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };
    env.call_method(
        &activity,
        "getContentResolver",
        "()Landroid/content/ContentResolver;",
        &[],
    )
    .map_err(|e| platform_err("getContentResolver", e))?
    .l()
    .map_err(|e| platform_err("getContentResolver object", e))
}

pub(super) fn jvm_err(e: jni::errors::JniError) -> PickerError {
    PickerError::Platform {
        message: format!("failed to get JavaVM: {e}"),
    }
}

pub(super) fn attach_err(e: jni::errors::JniError) -> PickerError {
    PickerError::Platform {
        message: format!("failed to attach JNI thread: {e}"),
    }
}

pub(super) fn platform_err(ctx: &str, e: impl std::fmt::Display) -> PickerError {
    PickerError::Platform {
        message: format!("{ctx}: {e}"),
    }
}
