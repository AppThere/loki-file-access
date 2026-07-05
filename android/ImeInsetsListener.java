// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

package io.github.appthere.lokifileaccess;

import android.app.Activity;
import android.os.Build;
import android.view.View;
import android.view.WindowInsets;

/**
 * Bridges Android soft-keyboard (IME) visibility changes to native code.
 *
 * <p>On a {@code NativeActivity} the OS never reports when the user dismisses or
 * re-summons the soft keyboard (back button, swipe-down gesture, keyboard hide
 * key), and the surface is not resized.  This listener observes the decor view's
 * window insets — which include the IME inset on API 30+ — and calls
 * {@code nativeOnImeInsetsChanged} on every visibility transition so the Rust
 * side can re-reserve (or release) the bottom safe area.
 *
 * <p>The native method is bound from Rust via {@code RegisterNatives}, so no
 * {@code System.loadLibrary} is required here and the binding is independent of
 * the host application's native-library name.
 */
public final class ImeInsetsListener implements View.OnApplyWindowInsetsListener {

    private boolean lastImeVisible;

    /**
     * Install the listener on the activity's decor view.
     *
     * <p>Runs on the UI thread (View listeners must be set there) and is a no-op
     * below API 30, where {@code WindowInsets.Type.ime()} / {@code isVisible}
     * are unavailable — matching the query-side API-30 fallback in Rust.
     */
    public static void install(final Activity activity) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) {
            return;
        }
        activity.runOnUiThread(new Runnable() {
            @Override
            public void run() {
                View decor = activity.getWindow().getDecorView();
                decor.setOnApplyWindowInsetsListener(new ImeInsetsListener());
                // Kick an immediate inset dispatch so the initial state is known.
                decor.requestApplyInsets();
            }
        });
    }

    @Override
    public WindowInsets onApplyWindowInsets(View v, WindowInsets insets) {
        boolean imeVisible = insets.isVisible(WindowInsets.Type.ime());
        if (imeVisible != lastImeVisible) {
            lastImeVisible = imeVisible;
            nativeOnImeInsetsChanged(imeVisible);
        }
        // Pass through so we neither consume nor alter the system's inset handling.
        return v.onApplyWindowInsets(insets);
    }

    private static native void nativeOnImeInsetsChanged(boolean imeVisible);
}
