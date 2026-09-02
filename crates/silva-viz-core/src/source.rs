// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Where files come from.
//!
//! A native build walks a real directory tree; a web build has no filesystem at
//! all and instead holds whatever was dropped onto the canvas. The browser
//! panel is written against the trait and so is identical on both.

use std::cell::RefCell;
use std::rc::Rc;

/// Identifies one entry within one source. Opaque, and only meaningful to the
/// source that issued it.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct EntryId(pub u64);

/// One row in the browser.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FileEntry {
    pub id: EntryId,
    pub name: String,
    pub is_dir: bool,
    /// `None` for directories, and for files whose size could not be read.
    pub size: Option<u64>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SourceError {
    #[error("no entry with that id")]
    NoSuchEntry,
    #[error("`{0}` is a directory")]
    IsADirectory(String),
    #[error("`{0}` is not a directory")]
    NotADirectory(String),
    #[error("{0}")]
    Io(String),
}

/// A tree of directories and files, read lazily.
///
/// `read_range` exists so a viewer can page through a file far larger than
/// memory. A hex dump of a 3 GB file reads 40 rows at a time and never asks for
/// the whole thing; asking [`FileSource::read_all`] for it would be a fine way
/// to lose the process.
pub trait FileSource {
    fn root(&self) -> EntryId;

    /// The entries directly under `dir`, directories first and then files, each
    /// group sorted by name. Cached, so calling this every frame is cheap.
    fn children(&mut self, dir: EntryId) -> Result<&[FileEntry], SourceError>;

    fn entry(&self, id: EntryId) -> Option<&FileEntry>;

    /// A stable, human-readable name for `id` — a filesystem path where there
    /// is one. This is what gets persisted, and what [`FileSource::resolve`]
    /// takes back.
    fn display_path(&self, id: EntryId) -> String;

    /// The inverse of [`FileSource::display_path`], used to reopen a persisted
    /// window. Returns `None` when the entry is gone, which is not an error —
    /// the window is simply not restored.
    fn resolve(&mut self, path: &str) -> Option<EntryId>;

    fn read_all(&mut self, file: EntryId) -> Result<Vec<u8>, SourceError>;

    /// `len` bytes from `offset`, or fewer at the end of the file.
    fn read_range(
        &mut self,
        file: EntryId,
        offset: u64,
        len: usize,
    ) -> Result<Vec<u8>, SourceError>;
}

/// The handle the app and every open [`crate::Blob`] share.
///
/// Reference-counted rather than borrowed, because an open window outlives the
/// browser's idea of a root: choosing a new folder replaces the app's source,
/// and the windows already open keep reading through the one they captured.
pub type SharedSource = Rc<RefCell<dyn FileSource>>;

fn sorted(mut entries: Vec<FileEntry>) -> Vec<FileEntry> {
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    entries
}

// ---------------------------------------------------------------------------
// An in-memory source
// ---------------------------------------------------------------------------

/// A flat set of named byte blobs under a synthetic root.
///
/// This is what the web build browses: files arrive by drag-and-drop or through
/// the file picker, already as bytes, because a browser will not hand out a
/// path. It is not `cfg`-gated — the unit tests in this crate and in the app
/// use it too, which is the cheap way to test a browser without a fixture tree
/// on disk.
#[derive(Default)]
pub struct MemSource {
    /// Index `0` is the synthetic root directory.
    entries: Vec<FileEntry>,
    data: Vec<Vec<u8>>,
    listing: Vec<FileEntry>,
}

impl MemSource {
    pub fn new() -> Self {
        Self {
            entries: vec![FileEntry {
                id: EntryId(0),
                name: "(dropped files)".to_string(),
                is_dir: true,
                size: None,
            }],
            data: vec![Vec::new()],
            listing: Vec::new(),
        }
    }

    /// Adds a file, replacing any earlier one of the same name so that dropping
    /// a file twice does not show it twice.
    pub fn add(&mut self, name: impl Into<String>, bytes: Vec<u8>) -> EntryId {
        let name = name.into();
        if let Some(existing) = self
            .entries
            .iter()
            .position(|e| e.name == name && !e.is_dir)
        {
            self.data[existing] = bytes;
            self.entries[existing].size = Some(self.data[existing].len() as u64);
            self.listing = sorted(self.entries[1..].to_vec());
            return EntryId(existing as u64);
        }
        let id = EntryId(self.entries.len() as u64);
        self.entries.push(FileEntry {
            id,
            name,
            is_dir: false,
            size: Some(bytes.len() as u64),
        });
        self.data.push(bytes);
        self.listing = sorted(self.entries[1..].to_vec());
        id
    }

    pub fn is_empty(&self) -> bool {
        self.listing.is_empty()
    }

    fn slot(&self, id: EntryId) -> Result<usize, SourceError> {
        let ix = id.0 as usize;
        let entry = self.entries.get(ix).ok_or(SourceError::NoSuchEntry)?;
        if entry.is_dir {
            return Err(SourceError::IsADirectory(entry.name.clone()));
        }
        Ok(ix)
    }
}

impl FileSource for MemSource {
    fn root(&self) -> EntryId {
        EntryId(0)
    }

    fn children(&mut self, dir: EntryId) -> Result<&[FileEntry], SourceError> {
        if dir != EntryId(0) {
            let name = self.display_path(dir);
            return Err(SourceError::NotADirectory(name));
        }
        Ok(&self.listing)
    }

    fn entry(&self, id: EntryId) -> Option<&FileEntry> {
        self.entries.get(id.0 as usize)
    }

    fn display_path(&self, id: EntryId) -> String {
        self.entry(id)
            .map(|e| e.name.clone())
            .unwrap_or_else(|| "<gone>".to_string())
    }

    fn resolve(&mut self, path: &str) -> Option<EntryId> {
        self.entries
            .iter()
            .find(|e| !e.is_dir && e.name == path)
            .map(|e| e.id)
    }

    fn read_all(&mut self, file: EntryId) -> Result<Vec<u8>, SourceError> {
        let ix = self.slot(file)?;
        Ok(self.data[ix].clone())
    }

    fn read_range(
        &mut self,
        file: EntryId,
        offset: u64,
        len: usize,
    ) -> Result<Vec<u8>, SourceError> {
        let ix = self.slot(file)?;
        let bytes = &self.data[ix];
        let start = (offset as usize).min(bytes.len());
        let end = start.saturating_add(len).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }
}

// ---------------------------------------------------------------------------
// A filesystem source
// ---------------------------------------------------------------------------

#[cfg(not(target_arch = "wasm32"))]
mod fs_source {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use std::io::{Read, Seek, SeekFrom};
    use std::path::{Path, PathBuf};

    /// A real directory tree, listed one directory at a time on expansion.
    pub struct FsSource {
        paths: Vec<PathBuf>,
        entries: Vec<FileEntry>,
        index: HashMap<PathBuf, EntryId>,
        listed: HashMap<EntryId, Vec<FileEntry>>,
        root: EntryId,
    }

    impl FsSource {
        pub fn new(root: impl AsRef<Path>) -> Self {
            let mut source = Self {
                paths: Vec::new(),
                entries: Vec::new(),
                index: HashMap::new(),
                listed: HashMap::new(),
                root: EntryId(0),
            };
            source.root = source.intern(root.as_ref().to_path_buf(), true, None);
            source
        }

        pub fn root_path(&self) -> &Path {
            &self.paths[self.root.0 as usize]
        }

        fn intern(&mut self, path: PathBuf, is_dir: bool, size: Option<u64>) -> EntryId {
            if let Some(id) = self.index.get(&path) {
                return *id;
            }
            let id = EntryId(self.paths.len() as u64);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                // A root like `/` has no file name, and showing nothing for it
                // would leave the tree with an unlabelled top row.
                .unwrap_or_else(|| path.to_string_lossy().into_owned());
            self.entries.push(FileEntry {
                id,
                name,
                is_dir,
                size,
            });
            self.index.insert(path.clone(), id);
            self.paths.push(path);
            id
        }

        fn path(&self, id: EntryId) -> Result<&Path, SourceError> {
            self.paths
                .get(id.0 as usize)
                .map(PathBuf::as_path)
                .ok_or(SourceError::NoSuchEntry)
        }

        fn list(&mut self, dir: EntryId) -> Result<Vec<FileEntry>, SourceError> {
            let path = self.path(dir)?.to_path_buf();
            let read = fs::read_dir(&path).map_err(|e| SourceError::Io(e.to_string()))?;
            let mut out = Vec::new();
            for item in read {
                // One unreadable entry must not lose the rest of the
                // directory, so a failed item is skipped rather than returned.
                let Ok(item) = item else { continue };
                let child = item.path();
                // `file_type` does not follow symlinks, which is what stops a
                // link pointing at its own ancestor from being expandable
                // forever.
                let Ok(kind) = item.file_type() else { continue };
                let is_dir = kind.is_dir();
                let size = if is_dir {
                    None
                } else {
                    item.metadata().ok().map(|m| m.len())
                };
                let id = self.intern(child, is_dir, size);
                out.push(self.entries[id.0 as usize].clone());
            }
            Ok(sorted(out))
        }

        fn file_path(&self, file: EntryId) -> Result<PathBuf, SourceError> {
            let entry = self.entry(file).ok_or(SourceError::NoSuchEntry)?;
            if entry.is_dir {
                return Err(SourceError::IsADirectory(entry.name.clone()));
            }
            Ok(self.path(file)?.to_path_buf())
        }
    }

    impl FileSource for FsSource {
        fn root(&self) -> EntryId {
            self.root
        }

        fn children(&mut self, dir: EntryId) -> Result<&[FileEntry], SourceError> {
            if !self.listed.contains_key(&dir) {
                let kids = self.list(dir)?;
                self.listed.insert(dir, kids);
            }
            Ok(&self.listed[&dir])
        }

        fn entry(&self, id: EntryId) -> Option<&FileEntry> {
            self.entries.get(id.0 as usize)
        }

        fn display_path(&self, id: EntryId) -> String {
            self.path(id)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| "<gone>".to_string())
        }

        fn resolve(&mut self, path: &str) -> Option<EntryId> {
            let path = PathBuf::from(path);
            if let Some(id) = self.index.get(&path) {
                return Some(*id);
            }
            let meta = fs::metadata(&path).ok()?;
            let is_dir = meta.is_dir();
            let size = (!is_dir).then_some(meta.len());
            Some(self.intern(path, is_dir, size))
        }

        fn read_all(&mut self, file: EntryId) -> Result<Vec<u8>, SourceError> {
            let path = self.file_path(file)?;
            fs::read(&path).map_err(|e| SourceError::Io(e.to_string()))
        }

        fn read_range(
            &mut self,
            file: EntryId,
            offset: u64,
            len: usize,
        ) -> Result<Vec<u8>, SourceError> {
            let path = self.file_path(file)?;
            let mut handle = fs::File::open(&path).map_err(|e| SourceError::Io(e.to_string()))?;
            handle
                .seek(SeekFrom::Start(offset))
                .map_err(|e| SourceError::Io(e.to_string()))?;
            let mut buf = Vec::with_capacity(len);
            handle
                .take(len as u64)
                .read_to_end(&mut buf)
                .map_err(|e| SourceError::Io(e.to_string()))?;
            Ok(buf)
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use fs_source::FsSource;

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> MemSource {
        let mut src = MemSource::new();
        src.add("zebra.txt", b"z".to_vec());
        src.add("Apple.csv", b"a,b\n1,2\n".to_vec());
        src
    }

    #[test]
    fn test_children_are_sorted_case_insensitively() {
        let mut src = source();
        let names: Vec<_> = src
            .children(src.root())
            .unwrap()
            .iter()
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(names, ["Apple.csv", "zebra.txt"]);
    }

    #[test]
    fn test_adding_the_same_name_twice_replaces_rather_than_duplicates() {
        let mut src = source();
        src.add("zebra.txt", b"zz".to_vec());
        assert_eq!(src.children(src.root()).unwrap().len(), 2);
        let id = src.resolve("zebra.txt").unwrap();
        assert_eq!(src.read_all(id).unwrap(), b"zz");
    }

    #[test]
    fn test_a_range_past_the_end_returns_what_is_there_rather_than_failing() {
        let mut src = source();
        let id = src.resolve("zebra.txt").unwrap();
        assert_eq!(src.read_range(id, 0, 4096).unwrap(), b"z");
        assert!(src.read_range(id, 900, 10).unwrap().is_empty());
    }

    #[test]
    fn test_reading_a_directory_names_it_rather_than_returning_empty_bytes() {
        let mut src = source();
        let err = src.read_all(src.root()).unwrap_err();
        assert!(matches!(err, SourceError::IsADirectory(_)));
    }

    #[test]
    fn test_a_path_that_is_gone_resolves_to_nothing_rather_than_panicking() {
        let mut src = source();
        assert!(src.resolve("no-such-file").is_none());
    }
}
