// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! Integration tests for desktop file access.
//!
//! These tests are cfg-gated to run only on desktop platforms (not Android,
//! iOS, or WASM) where direct filesystem path access is available.

#![cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]

use loki_file_access::{FileAccessToken, PermissionStatus};
use std::io::{Read, Seek, Write};

#[test]
fn open_read_from_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.txt");
    std::fs::write(&path, "hello world").unwrap();

    let token = FileAccessToken::deserialize(
        &create_desktop_token(&path, "test.txt").serialize(),
    )
    .unwrap();

    let mut reader = token.open_read().unwrap();
    let mut contents = String::new();
    reader.read_to_string(&mut contents).unwrap();
    assert_eq!(contents, "hello world");
}

#[test]
fn open_write_to_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("output.txt");
    std::fs::write(&path, "").unwrap();

    let token = create_desktop_token(&path, "output.txt");

    {
        let mut writer = token.open_write().unwrap();
        writer.write_all(b"written data").unwrap();
    }

    let contents = std::fs::read_to_string(&path).unwrap();
    assert_eq!(contents, "written data");
}

#[test]
fn check_permission_returns_valid_for_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("exists.txt");
    std::fs::write(&path, "data").unwrap();

    let token = create_desktop_token(&path, "exists.txt");
    assert_eq!(token.check_permission(), PermissionStatus::Valid);
}

#[test]
fn check_permission_returns_revoked_for_missing_file() {
    let token = create_desktop_token(
        std::path::Path::new("/tmp/nonexistent_loki_test_file_12345.txt"),
        "nonexistent.txt",
    );
    assert_eq!(token.check_permission(), PermissionStatus::Revoked);
}

#[test]
fn token_round_trip_serialization() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("roundtrip.txt");
    std::fs::write(&path, "round-trip content").unwrap();

    let original = create_desktop_token(&path, "roundtrip.txt");
    let serialized = original.serialize();
    let restored = FileAccessToken::deserialize(&serialized).unwrap();

    assert_eq!(restored.display_name(), "roundtrip.txt");

    let mut reader = restored.open_read().unwrap();
    let mut contents = String::new();
    reader.read_to_string(&mut contents).unwrap();
    assert_eq!(contents, "round-trip content");
}

#[test]
fn display_name_returns_filename() {
    let token = create_desktop_token(
        std::path::Path::new("/tmp/example.csv"),
        "example.csv",
    );
    assert_eq!(token.display_name(), "example.csv");
}

#[test]
fn mime_type_is_none_for_desktop() {
    let token = create_desktop_token(
        std::path::Path::new("/tmp/file.txt"),
        "file.txt",
    );
    assert!(token.mime_type().is_none());
}

#[test]
fn open_read_seek_rewind() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("seek.txt");
    std::fs::write(&path, "abcdef").unwrap();

    let token = create_desktop_token(&path, "seek.txt");
    let mut reader = token.open_read().unwrap();

    let mut buf = [0u8; 3];
    reader.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"abc");

    reader.seek(std::io::SeekFrom::Start(0)).unwrap();
    reader.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, b"abc");
}

/// Helper to construct a desktop `FileAccessToken` directly.
///
/// In production code, tokens are obtained from `FilePicker` methods.
/// For testing we construct them via the token's internal serialization format.
fn create_desktop_token(path: &std::path::Path, name: &str) -> FileAccessToken {
    use base64::Engine as _;

    // Build the JSON payload that TokenInner::Desktop serializes to,
    // then encode it to match FileAccessToken::serialize().
    let json = serde_json::json!({
        "Desktop": {
            "path": path.to_str().unwrap(),
            "display_name": name
        }
    });
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(json.to_string().as_bytes());
    FileAccessToken::deserialize(&encoded).unwrap()
}
