// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

package io.github.appthere.lokifileaccess;

import android.app.Activity;
import android.content.ClipData;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;

/**
 * Transparent trampoline Activity for Android file picking via the Storage Access Framework.
 *
 * <p>{@code ANativeActivityCallbacks} has no {@code onActivityResult} callback, so
 * NativeActivity cannot receive file-picker results directly.  This Activity:
 * <ol>
 *   <li>Is started by NativeActivity via {@code startActivity} (no result needed).</li>
 *   <li>Calls {@code startActivityForResult(ACTION_OPEN_DOCUMENT / ACTION_CREATE_DOCUMENT)}.</li>
 *   <li>Receives the result in its own {@code onActivityResult}.</li>
 *   <li>Delivers the selected URI to the Rust future via {@code nativeOnResult}.</li>
 *   <li>Calls {@code finish()} to remove itself from the back stack.</li>
 * </ol>
 *
 * <p>The native library is already loaded by NativeActivity before this Activity is
 * ever created, so {@code System.loadLibrary} is not required here.
 *
 * <p>Register in {@code AndroidManifest.xml}:
 * <pre>{@code
 * <activity
 *     android:name="io.github.appthere.lokifileaccess.FilePickerActivity"
 *     android:theme="@android:style/Theme.Translucent.NoTitleBar"
 *     android:exported="false" />
 * }</pre>
 */
public class FilePickerActivity extends Activity {

    static {
        // Android 7+ scopes native-library JNI lookups to the class loader that
        // called System.loadLibrary.  NativeActivity loads libloki_text.so from
        // the framework bootstrap class loader; FilePickerActivity runs under the
        // application PathClassLoader.  Without this call, nativeOnResult cannot
        // be resolved and throws UnsatisfiedLinkError at runtime.
        // Safe to call if the library is already loaded — ART returns immediately.
        System.loadLibrary("loki_text");
    }

    private static final int REQUEST_OPEN   = 1001;
    private static final int REQUEST_CREATE = 1002;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        Intent src = getIntent();
        String mode = src.getStringExtra("mode");
        if ("CREATE".equals(mode)) {
            launchCreate(src);
        } else {
            launchOpen(src);
        }
    }

    private void launchOpen(Intent src) {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);

        // mime_types is passed as a comma-separated string to avoid JNI array complexity.
        String mimeTypesRaw = src.getStringExtra("mime_types");
        if (mimeTypesRaw != null && !mimeTypesRaw.isEmpty()) {
            String[] mimes = mimeTypesRaw.split(",");
            if (mimes.length == 1) {
                intent.setType(mimes[0]);
            } else {
                intent.setType("*/*");
                intent.putExtra(Intent.EXTRA_MIME_TYPES, mimes);
            }
        } else {
            intent.setType("*/*");
        }

        boolean allowMultiple = src.getBooleanExtra("allow_multiple", false);
        if (allowMultiple) {
            intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, true);
        }

        startActivityForResult(intent, REQUEST_OPEN);
    }

    private void launchCreate(Intent src) {
        Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);

        String mimeType = src.getStringExtra("mime_type");
        intent.setType(mimeType != null ? mimeType : "*/*");

        String suggestedName = src.getStringExtra("suggested_name");
        if (suggestedName != null) {
            intent.putExtra(Intent.EXTRA_TITLE, suggestedName);
        }

        startActivityForResult(intent, REQUEST_CREATE);
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        String result = null;
        if (resultCode == RESULT_OK && data != null) {
            // Multi-select delivers URIs through ClipData; single-select uses getData().
            // Join all URIs with '\n' so a single nativeOnResult call carries the full
            // selection without requiring a JNI array parameter.
            ClipData clip = data.getClipData();
            if (clip != null && clip.getItemCount() > 0) {
                StringBuilder sb = new StringBuilder();
                for (int i = 0; i < clip.getItemCount(); i++) {
                    Uri u = clip.getItemAt(i).getUri();
                    if (u != null) {
                        if (sb.length() > 0) sb.append('\n');
                        sb.append(u.toString());
                    }
                }
                if (sb.length() > 0) result = sb.toString();
            } else {
                Uri u = data.getData();
                if (u != null) result = u.toString();
            }
        }
        nativeOnResult(result);
        finish();
    }

    /**
     * Delivers the selected URI (or {@code null} for cancellation) to the pending Rust future.
     *
     * <p>Resolved via JNI against
     * {@code Java_io_github_appthere_lokifileaccess_FilePickerActivity_nativeOnResult}
     * in the host application's native library, which is already loaded by the time
     * this Activity is created.
     */
    private native void nativeOnResult(String uri);
}
