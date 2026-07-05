// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! JNI helpers for post-pick SAF operations.
//!
//! Covers `ContentResolver.takePersistableUriPermission`, URI parsing, and the
//! content resolver accessor used by [`super::jni_fd`].  Intent launching is
//! handled by [`super::jni_activity`] (via `FilePickerActivity` trampoline).

use super::jni_common::{attach_err, jvm_err, platform_err};
use crate::error::PickerError;

// ── SAF post-pick operations ──────────────────────────────────────────────────

/// Call `ContentResolver.takePersistableUriPermission` for a content URI.
///
/// Grants READ | WRITE persistable permission so the URI survives app restarts.
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

// ── Display name query ────────────────────────────────────────────────────────

/// Query the human-readable display name for a content URI from ContentResolver.
///
/// Uses `OpenableColumns.DISPLAY_NAME` (`"_display_name"`) via a cursor query.
/// Falls back to the last URI path segment (percent-decoded) on any JNI failure.
pub(super) fn query_display_name(uri_str: &str) -> String {
    query_display_name_inner(uri_str).unwrap_or_else(|| {
        let raw = uri_str.rsplit('/').next().unwrap_or("unnamed");
        percent_decode_last_segment(raw)
    })
}

fn query_display_name_inner(uri_str: &str) -> Option<String> {
    let ctx = ndk_context::android_context();
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;

    let uri_obj = parse_uri(&mut env, uri_str).ok()?;
    let resolver = get_content_resolver(&mut env, &ctx).ok()?;

    // projection = new String[]{"_display_name"}
    let str_cls = env.find_class("java/lang/String").ok()?;
    let col_str = env.new_string("_display_name").ok()?;
    let projection = env.new_object_array(1, &str_cls, &col_str).ok()?;

    // ContentResolver.query(uri, projection, null, null, null) → Cursor
    // A misbehaving SAF provider may throw instead of returning null. Clear any
    // pending JNI exception before returning so subsequent calls are not poisoned.
    let null_obj = jni::objects::JObject::null();
    let query_result = env.call_method(
        &resolver,
        "query",
        "(Landroid/net/Uri;[Ljava/lang/String;Ljava/lang/String;\
         [Ljava/lang/String;Ljava/lang/String;)Landroid/database/Cursor;",
        &[
            jni::objects::JValueGen::Object(&uri_obj),
            jni::objects::JValueGen::Object(&projection),
            jni::objects::JValueGen::Object(&null_obj),
            jni::objects::JValueGen::Object(&null_obj),
            jni::objects::JValueGen::Object(&null_obj),
        ],
    );
    let cursor = match query_result {
        Ok(v) => v.l().ok()?,
        Err(_) => {
            let _ = env.exception_clear();
            return None;
        }
    };

    if cursor.is_null() {
        return None;
    }

    // Extract the display name, then close the cursor unconditionally.
    // read_display_name uses ok()? internally, which can exit without clearing
    // a pending JNI exception from moveToFirst/getColumnIndex/getString.
    // Calling close() while an exception is pending violates the JNI spec and
    // risks an ART abort, so we clear any pending exception first.
    let result = read_display_name(&mut env, &cursor);
    let _ = env.exception_clear();
    let _ = env.call_method(&cursor, "close", "()V", &[]);
    result
}

/// Read `_display_name` from an already-moved-to-first cursor.
/// Separated from the main function so that every return path is guaranteed
/// to reach `cursor.close()` in the caller — no `?` can skip it.
fn read_display_name(
    env: &mut jni::JNIEnv<'_>,
    cursor: &jni::objects::JObject<'_>,
) -> Option<String> {
    let moved = env
        .call_method(cursor, "moveToFirst", "()Z", &[])
        .ok()?
        .z()
        .ok()?;

    if !moved {
        return None;
    }

    // Column index is always 0 when the single-column projection {"_display_name"}
    // is honoured by the provider.  Use getColumnIndex as a defensive check.
    let col_name = env.new_string("_display_name").ok()?;
    let col_idx = env
        .call_method(
            cursor,
            "getColumnIndex",
            "(Ljava/lang/String;)I",
            &[jni::objects::JValueGen::Object(&col_name)],
        )
        .ok()?
        .i()
        .ok()?;

    if col_idx < 0 {
        return None;
    }

    let s_obj = env
        .call_method(
            cursor,
            "getString",
            "(I)Ljava/lang/String;",
            &[jni::objects::JValueGen::Int(col_idx)],
        )
        .ok()?
        .l()
        .ok()?;

    if s_obj.is_null() {
        return None;
    }

    let jstr: jni::objects::JString = s_obj.into();
    env.get_string(&jstr).ok().map(|js| String::from(js))
}

/// Minimal percent-decoder for the last URI path segment used as fallback.
///
/// Decodes `%XX` sequences by accumulating raw bytes and then interpreting the
/// entire buffer as UTF-8.  This correctly handles multi-byte UTF-8 sequences
/// such as `%C3%A9` (é) — decoding each byte individually via `char::from(u8)`
/// would produce mojibake for any non-ASCII character.
///
/// Invalid or incomplete `%XX` sequences (e.g. `%GG`, `%2`, `%`) are emitted
/// literally rather than dropped.  In URI path segments `+` is a literal plus
/// sign, not a space — only `%20` encodes a space.
fn percent_decode_last_segment(s: &str) -> String {
    let mut buf: Vec<u8> = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                buf.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        // Not a valid %XX sequence — emit the byte as-is.
        buf.push(bytes[i]);
        i += 1;
    }
    // from_utf8 avoids a Cow allocation on valid UTF-8 (the common case for
    // URI path segments).  The lossy fallback is only reached for malformed
    // byte sequences, which should not occur in well-formed content URIs.
    let decoded = String::from_utf8(buf)
        .unwrap_or_else(|e| String::from_utf8_lossy(&e.into_bytes()).into_owned());
    if decoded.is_empty() {
        "unnamed".to_string()
    } else {
        decoded
    }
}

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Parse a URI string into a `Uri` JNI object via `Uri.parse(String)`.
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

/// Get the `ContentResolver` from the ndk_context Application/Activity.
pub(super) fn get_content_resolver<'a>(
    env: &mut jni::JNIEnv<'a>,
    ctx: &ndk_context::AndroidContext,
) -> Result<jni::objects::JObject<'a>, PickerError> {
    // SAFETY: ndk_context stores a valid Application jobject.
    let context = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };
    env.call_method(
        &context,
        "getContentResolver",
        "()Landroid/content/ContentResolver;",
        &[],
    )
    .map_err(|e| platform_err("getContentResolver", e))?
    .l()
    .map_err(|e| platform_err("getContentResolver object", e))
}
