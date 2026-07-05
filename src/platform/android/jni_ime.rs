// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! JNI install of the Android soft-keyboard (IME) visibility listener.
//!
//! `ImeInsetsListener` (a Java shim) observes the decor view's window insets and
//! calls back into `nativeOnImeInsetsChanged` on every IME visibility change —
//! including the user-initiated dismiss / re-summon that the OS otherwise never
//! reports to a `NativeActivity`.
//!
//! Install flow:
//! 1. Load the shim through the *application* class loader — a JNI-attached
//!    native thread's default `FindClass` loader resolves only framework
//!    classes, so an app class must be reached via the activity's own loader.
//! 2. Bind the native callback with `RegisterNatives`, so the binding does not
//!    depend on the host application's `.so` name (unlike symbol-name
//!    resolution, which is class-loader-scoped on Android 7+).
//! 3. Call the shim's static `install`, which registers the decor-view listener
//!    on the UI thread.

use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

use jni::objects::{JClass, JObject, JValueGen};
use jni::sys::jboolean;
use jni::{JNIEnv, NativeMethod};

/// Fully-qualified name of the Java shim (for `ClassLoader.loadClass`).
const IME_CLASS_DOT: &str = "io.github.appthere.lokifileaccess.ImeInsetsListener";

/// Closure invoked (on the Android UI thread) whenever the soft keyboard's
/// visibility changes. `true` = keyboard now visible, `false` = collapsed.
type ImeCallback = Box<dyn Fn(bool) + Send + Sync>;
static IME_CALLBACK: OnceLock<Mutex<Option<ImeCallback>>> = OnceLock::new();

fn ime_callback() -> &'static Mutex<Option<ImeCallback>> {
    IME_CALLBACK.get_or_init(|| Mutex::new(None))
}

/// Register the closure invoked on every soft-keyboard visibility change.
///
/// Call once before [`install_ime_listener`]. A later call replaces the
/// previous closure.
pub fn set_ime_visibility_listener(callback: ImeCallback) {
    if let Ok(mut guard) = ime_callback().lock() {
        *guard = Some(callback);
    }
}

/// Install the decor-view IME inset listener for `activity_ptr`
/// (`AndroidApp::activity_as_ptr()`).
///
/// Returns `true` when the Java `install` was invoked. Returns `false` on a null
/// pointer or any JNI failure; the Java side additionally no-ops below API 30,
/// matching the query-side fallback. Installing twice simply replaces the decor
/// view's listener.
pub fn install_ime_listener(activity_ptr: *mut c_void) -> bool {
    if activity_ptr.is_null() {
        return false;
    }
    let ctx = ndk_context::android_context();
    // SAFETY: ndk_context stores the JVM pointer initialised by android-activity
    // before android_main; valid for the process lifetime.
    let vm = match unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) } {
        Ok(vm) => vm,
        Err(_) => return false,
    };
    let mut env = match vm.attach_current_thread() {
        Ok(env) => env,
        Err(_) => return false,
    };
    let ok = install_with_env(&mut env, activity_ptr).is_some();
    // Clear any pending exception (e.g. from a failed class load) so the calling
    // thread stays usable.
    let _ = env.exception_clear();
    ok
}

fn install_with_env(env: &mut JNIEnv<'_>, activity_ptr: *mut c_void) -> Option<()> {
    // SAFETY: `activity_ptr` is the global-ref activity jobject owned by
    // AndroidApp, valid for the activity's lifetime.
    let activity = unsafe { JObject::from_raw(activity_ptr.cast()) };

    let class = load_app_class(env, &activity)?;
    register_native(env, &class)?;

    // ImeInsetsListener.install(activity)
    env.call_static_method(
        &class,
        "install",
        "(Landroid/app/Activity;)V",
        &[JValueGen::Object(&activity)],
    )
    .ok()?;
    Some(())
}

/// Load `ImeInsetsListener` through the application class loader.
///
/// A native thread's default class loader resolves only framework classes, so
/// we reach the app class via the activity's own `getClassLoader().loadClass`.
fn load_app_class<'a>(env: &mut JNIEnv<'a>, activity: &JObject<'_>) -> Option<JClass<'a>> {
    let activity_class = env.get_object_class(activity).ok()?;
    let loader = env
        .call_method(
            &activity_class,
            "getClassLoader",
            "()Ljava/lang/ClassLoader;",
            &[],
        )
        .ok()?
        .l()
        .ok()?;
    let name = env.new_string(IME_CLASS_DOT).ok()?;
    let class_obj = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValueGen::Object(&name)],
        )
        .ok()?
        .l()
        .ok()?;
    Some(JClass::from(class_obj))
}

/// Bind `nativeOnImeInsetsChanged` to [`ime_insets_changed`] via
/// `RegisterNatives`, so the callback resolves regardless of the host
/// application's native-library name.
fn register_native(env: &mut JNIEnv<'_>, class: &JClass<'_>) -> Option<()> {
    let methods = [NativeMethod {
        name: "nativeOnImeInsetsChanged".into(),
        sig: "(Z)V".into(),
        fn_ptr: ime_insets_changed as *mut c_void,
    }];
    env.register_native_methods(class, &methods).ok()
}

/// JNI callback invoked by `ImeInsetsListener.onApplyWindowInsets` on the
/// Android UI thread whenever the soft keyboard's visibility changes.
extern "system" fn ime_insets_changed(_env: JNIEnv<'_>, _class: JClass<'_>, ime_visible: jboolean) {
    let visible = ime_visible != 0;
    if let Ok(guard) = ime_callback().lock()
        && let Some(callback) = guard.as_ref()
    {
        callback(visible);
    }
}
