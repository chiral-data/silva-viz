// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The seam a scientific file browser is built on.
//!
//! Three ideas, and deliberately nothing else:
//!
//! - [`FileSource`] — where files come from. The browser talks to this and
//!   never to [`std::path::Path`], which is the only reason the same browser
//!   compiles for the web, where there is no filesystem.
//! - [`ViewerFactory`] — something that can *bid* for a file. A factory
//!   inspects the first few kilobytes and either claims the file or declines,
//!   so a `.txt` holding PNG magic bytes opens as an image.
//! - [`View`] — one open window's contents.
//!
//! Nothing here knows what any particular file format is. Viewers for a given
//! domain live downstream and register themselves, which is the property the
//! whole crate exists to provide.

mod blob;
mod probe;
mod registry;
mod source;
mod task;
mod view;

pub use blob::Blob;
pub use probe::{FileProbe, HEAD_LEN, ImageKind};
pub use registry::{Claim, ViewerFactory, ViewerRegistry};
pub use source::{EntryId, FileEntry, FileSource, MemSource, SharedSource, SourceError};
pub use task::Task;
pub use view::View;

#[cfg(not(target_arch = "wasm32"))]
pub use source::FsSource;
