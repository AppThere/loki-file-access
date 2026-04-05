// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! WASM file-picker implementation using `<input type="file">`.
//!
//! This module creates a hidden `<input type="file">` element in the DOM,
//! configures it with MIME-type filters and multi-select options, and listens
//! for the `change` event to capture the user's selection.  Selected files are
//! read entirely into memory as `Vec<u8>`.
//!
//! # Save behaviour
//!
//! WASM has no traditional save dialog.  [`pick_save`] triggers a browser
//! download via a Blob URL.  The returned [`crate::FileAccessToken`] wraps an
//! in-memory buffer that the caller can write to before the download is
//! initiated.  This is a fundamental platform limitation documented in the
//! public [`crate::FilePicker::pick_file_to_save`] doc comment.
//!
//! # Persistence
//!
//! WASM tokens hold file data in memory and do not survive page reloads.
//! Serialising a WASM token preserves the data, but restoring it on a new
//! page load gives back the original bytes — there is no way to re-acquire
//! filesystem access.

use crate::api::{PickOptions, SaveOptions};
use crate::error::{AccessError, PickerError};
use crate::token::{
    FileAccessToken, PermissionStatus, ReadSeek, TokenInner, WriteSeek,
};

/// Pick a single file for reading.
pub(crate) async fn pick_open_single(
    options: PickOptions,
) -> Result<Option<FileAccessToken>, PickerError> {
    let tokens = pick_files(options, false).await?;
    Ok(tokens.into_iter().next())
}

/// Pick multiple files for reading.
pub(crate) async fn pick_open_multi(
    options: PickOptions,
) -> Result<Vec<FileAccessToken>, PickerError> {
    pick_files(options, true).await
}

/// Trigger a browser download.
///
/// On WASM there is no save dialog — this creates an empty in-memory buffer
/// token.  The caller should write data into it, then call a platform-specific
/// download trigger (e.g. creating a Blob URL and clicking an anchor element).
pub(crate) async fn pick_save(
    options: SaveOptions,
) -> Result<Option<FileAccessToken>, PickerError> {
    let name = options
        .suggested_name
        .unwrap_or_else(|| "download".into());
    Ok(Some(FileAccessToken {
        inner: TokenInner::Wasm {
            data: Vec::new(),
            name,
            mime_type: options.mime_type,
        },
    }))
}

/// Open a WASM token for reading (reads from the in-memory buffer).
pub(crate) fn open_read(inner: &TokenInner) -> Result<Box<dyn ReadSeek>, AccessError> {
    match inner {
        TokenInner::Wasm { data, .. } => {
            Ok(Box::new(std::io::Cursor::new(data.clone())))
        }
        _ => Err(AccessError::Platform {
            message: "non-WASM token on WASM platform".into(),
        }),
    }
}

/// Open a WASM token for writing (writes to an in-memory buffer).
pub(crate) fn open_write(inner: &TokenInner) -> Result<Box<dyn WriteSeek>, AccessError> {
    match inner {
        TokenInner::Wasm { data, .. } => {
            Ok(Box::new(std::io::Cursor::new(data.clone())))
        }
        _ => Err(AccessError::Platform {
            message: "non-WASM token on WASM platform".into(),
        }),
    }
}

/// WASM tokens are always valid while the page is loaded.
pub(crate) fn check_permission(inner: &TokenInner) -> PermissionStatus {
    match inner {
        TokenInner::Wasm { .. } => PermissionStatus::Valid,
        _ => PermissionStatus::Unknown,
    }
}

/// Create and trigger a hidden `<input type="file">` element.
async fn pick_files(
    options: PickOptions,
    multiple: bool,
) -> Result<Vec<FileAccessToken>, PickerError> {
    use wasm_bindgen::JsCast as _;
    use wasm_bindgen_futures::JsFuture;

    let window = web_sys::window().ok_or_else(|| PickerError::Platform {
        message: "no global window object".into(),
    })?;
    let document = window.document().ok_or_else(|| PickerError::Platform {
        message: "no document object".into(),
    })?;

    let input: web_sys::HtmlInputElement = document
        .create_element("input")
        .map_err(|e| PickerError::Platform {
            message: format!("create_element failed: {e:?}"),
        })?
        .dyn_into()
        .map_err(|_| PickerError::Platform {
            message: "element is not HtmlInputElement".into(),
        })?;

    input.set_type("file");

    if !options.mime_types.is_empty() {
        input.set_accept(&options.mime_types.join(","));
    }

    if multiple {
        input.set_multiple(true);
    }

    // Create a promise that resolves when the change event fires.
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        let resolve_clone = resolve.clone();
        let cb = wasm_bindgen::closure::Closure::once_into_js(move || {
            let _ = resolve_clone.call0(&wasm_bindgen::JsValue::NULL);
        });
        let _ = input.add_event_listener_with_callback(
            "change",
            cb.as_ref().unchecked_ref(),
        );
    });

    // Trigger the file dialog.
    input.click();

    // Wait for the user to select files.
    JsFuture::from(promise).await.map_err(|e| {
        PickerError::Platform {
            message: format!("file input promise rejected: {e:?}"),
        }
    })?;

    // Read selected files.
    let file_list = match input.files() {
        Some(fl) => fl,
        None => return Ok(vec![]),
    };

    let mut tokens = Vec::new();
    for i in 0..file_list.length() {
        let file = match file_list.get(i) {
            Some(f) => f,
            None => continue,
        };
        let name = file.name();
        let mime = file.type_();
        let mime_type = if mime.is_empty() { None } else { Some(mime) };

        let array_buf_promise = file.array_buffer();
        let array_buf = JsFuture::from(array_buf_promise).await.map_err(|e| {
            PickerError::Platform {
                message: format!("arrayBuffer() failed: {e:?}"),
            }
        })?;
        let uint8 = js_sys::Uint8Array::new(&array_buf);
        let data = uint8.to_vec();

        tokens.push(FileAccessToken {
            inner: TokenInner::Wasm {
                data,
                name,
                mime_type,
            },
        });
    }

    Ok(tokens)
}
