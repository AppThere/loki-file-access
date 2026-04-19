// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! MIME-type-to-extension mapping and extension validation for `rfd` filters.
//!
//! Kept in its own file to stay within the 300-line ceiling for each source
//! file while allowing `super` to use both helpers without qualification.

/// Returns `true` if `ext` is safe to pass as an `rfd` filter extension.
///
/// Windows `IFileOpenDialog::SetFileTypes` requires extension strings that
/// contain no dots, slashes, or whitespace.  MIME subtype fallbacks for
/// `vnd.*` types (e.g. `vnd.openxmlformats-officedocument.wordprocessingml.document`)
/// contain dots and would produce a filter pattern that matches no real files,
/// causing the dialog to display only folder entries.
pub(super) fn is_valid_extension(ext: &str) -> bool {
    !ext.is_empty()
        && ext
            .bytes()
            .all(|b| b != b'.' && b != b'/' && b != b'\\' && !b.is_ascii_whitespace())
}

/// Convert MIME types to file extensions for the `rfd` filter.
///
/// Returns one extension string per input MIME type.  Results that are not
/// valid Windows `SetFileTypes` extension strings (containing dots, slashes,
/// or whitespace) should be discarded by the caller via [`is_valid_extension`]
/// before passing to `rfd::AsyncFileDialog::add_filter`.
///
/// For MIME types not in the static table the subtype component is returned
/// as-is; [`is_valid_extension`] will reject any subtype that contains dots
/// (e.g. all `vnd.*` types not listed here).
pub(super) fn mime_types_to_extensions(mime_types: &[String]) -> Vec<String> {
    mime_types
        .iter()
        .map(|mime| match mime.as_str() {
            // Plain text / markup
            "text/plain" => "txt".into(),
            "text/html" => "html".into(),
            "text/css" => "css".into(),
            "text/csv" => "csv".into(),
            "text/markdown" | "text/x-markdown" => "md".into(),
            "text/rtf" | "application/rtf" => "rtf".into(),
            // Data formats
            "application/json" => "json".into(),
            "application/xml" | "text/xml" => "xml".into(),
            "application/pdf" => "pdf".into(),
            // Archives
            "application/zip" => "zip".into(),
            "application/x-tar" => "tar".into(),
            "application/gzip" | "application/x-gzip" => "gz".into(),
            "application/x-bzip2" => "bz2".into(),
            "application/x-7z-compressed" => "7z".into(),
            // Images
            "image/png" => "png".into(),
            "image/jpeg" => "jpg".into(),
            "image/gif" => "gif".into(),
            "image/svg+xml" => "svg".into(),
            "image/webp" => "webp".into(),
            "image/tiff" => "tiff".into(),
            "image/bmp" => "bmp".into(),
            // Audio / video
            "audio/mpeg" => "mp3".into(),
            "audio/wav" | "audio/x-wav" => "wav".into(),
            "audio/ogg" => "ogg".into(),
            "audio/flac" => "flac".into(),
            "video/mp4" => "mp4".into(),
            "video/webm" => "webm".into(),
            "video/x-matroska" => "mkv".into(),
            // Microsoft Office (legacy binary formats)
            "application/msword" => "doc".into(),
            "application/vnd.ms-excel" => "xls".into(),
            "application/vnd.ms-powerpoint" => "ppt".into(),
            // Microsoft Office Open XML
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
                "docx".into()
            }
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx".into(),
            "application/vnd.openxmlformats-officedocument.presentationml.presentation" => {
                "pptx".into()
            }
            // OpenDocument formats
            "application/vnd.oasis.opendocument.text" => "odt".into(),
            "application/vnd.oasis.opendocument.spreadsheet" => "ods".into(),
            "application/vnd.oasis.opendocument.presentation" => "odp".into(),
            // E-book / other documents
            "application/epub+zip" => "epub".into(),
            other => {
                // Fall back to the subtype component; callers must discard
                // results that fail `is_valid_extension` (e.g. vnd.* subtypes
                // not listed above that contain dots).
                other.split('/').nth(1).unwrap_or(other).to_owned()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_mime_types_produce_valid_extensions() {
        let cases = [
            ("text/plain", "txt"),
            ("application/pdf", "pdf"),
            ("application/msword", "doc"),
            ("application/vnd.openxmlformats-officedocument.wordprocessingml.document", "docx"),
            ("application/vnd.ms-excel", "xls"),
            ("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", "xlsx"),
            ("application/vnd.oasis.opendocument.text", "odt"),
            ("image/svg+xml", "svg"),
            ("application/epub+zip", "epub"),
        ];
        for (mime, expected_ext) in cases {
            let result = mime_types_to_extensions(&[mime.to_owned()]);
            assert_eq!(result[0], expected_ext, "wrong ext for {mime}");
            assert!(
                is_valid_extension(&result[0]),
                "ext '{}' for '{mime}' failed is_valid_extension",
                result[0]
            );
        }
    }

    #[test]
    fn unlisted_vnd_mime_type_produces_invalid_extension() {
        // An unlisted vnd.* type hits the fallback and produces a dotted
        // subtype that must be rejected by is_valid_extension.
        let result =
            mime_types_to_extensions(&["application/vnd.example.format".to_owned()]);
        assert!(!is_valid_extension(&result[0]), "dotted fallback must be invalid");
    }

    #[test]
    fn is_valid_extension_rejects_dots_and_slashes() {
        assert!(!is_valid_extension(""));
        assert!(!is_valid_extension("vnd.foo.bar"));
        assert!(!is_valid_extension("foo/bar"));
        assert!(!is_valid_extension("foo bar"));
        assert!(is_valid_extension("docx"));
        assert!(is_valid_extension("txt"));
        assert!(is_valid_extension("7z"));
    }
}
