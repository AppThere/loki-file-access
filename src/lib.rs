// SPDX-License-Identifier: MIT
// Copyright (c) 2026 AppThere

//! # loki-file-access
//!
//! Cross-platform, frontend-agnostic file picker and capability-based file
//! access for Rust applications.
//!
//! This crate provides a unified API for presenting native file-picker dialogs
//! and accessing user-selected files across all major platforms:
//!
//! - **Desktop** (Windows, macOS, Linux, BSD) — via the [`rfd`](https://crates.io/crates/rfd) crate
//! - **Android** — via the Storage Access Framework with persistable URI permissions
//! - **iOS** — via `UIDocumentPickerViewController` with security-scoped bookmarks
//! - **WASM** — via `<input type="file">` with in-memory file buffers
//!
//! # Zero UI Framework Dependencies
//!
//! `loki-file-access` has **no UI framework dependencies**.  It returns
//! standard [`Future`] values implemented with only `std` primitives (no Tokio,
//! no async-std required).  It is usable from Dioxus, egui, Iced, Xilem,
//! `pollster::block_on`, or any other async or sync Rust context.
//!
//! # Quick Start
//!
//! ```no_run
//! use loki_file_access::{FilePicker, PickOptions};
//! use std::io::Read;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let picker = FilePicker::new();
//!
//! // Pick a file to open
//! let token = picker
//!     .pick_file_to_open(PickOptions {
//!         mime_types: vec!["text/plain".into()],
//!         ..Default::default()
//!     })
//!     .await?;
//!
//! if let Some(token) = token {
//!     let mut reader = token.open_read()?;
//!     let mut contents = String::new();
//!     reader.read_to_string(&mut contents)?;
//!     println!("File contents: {contents}");
//!
//!     // Serialize the token for later use
//!     let stored = token.serialize();
//!     println!("Token: {stored}");
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Capability Tokens
//!
//! Every picker operation returns a [`FileAccessToken`] — a serializable
//! capability that encapsulates all platform-specific state needed to re-open
//! the file.  Tokens can be serialized to a URL-safe string for storage in a
//! recent-files list and deserialized to re-open files across app restarts.
//!
//! On Android, the token holds a content URI with a persistable permission
//! grant.  On iOS, it holds a security-scoped bookmark.  On desktop, it holds
//! a filesystem path.  On WASM, it holds the file data in memory.

pub mod api;
pub mod error;
pub(crate) mod future;
mod platform;
pub mod token;

// Re-export public types at crate root for convenience.
pub use api::{FilePicker, PickOptions, SaveOptions};
pub use error::{AccessError, PickerError, TokenParseError};
pub use token::{FileAccessToken, PermissionStatus, ReadSeek, WriteSeek};
