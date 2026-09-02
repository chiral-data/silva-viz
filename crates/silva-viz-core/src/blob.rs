// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A handle to one file's bytes, without saying where they live.

use crate::probe::{FileProbe, HEAD_LEN};
use crate::source::{EntryId, SharedSource, SourceError};
use std::rc::Rc;

struct Inner {
    name: String,
    path: String,
    size: u64,
    head: Vec<u8>,
    source: SharedSource,
    id: EntryId,
}

/// One file, as a viewer sees it: a name, a size, a probed head, and two ways
/// to ask for the rest.
///
/// Cheap to clone, because opening the same file in three viewers should cost
/// one read of the head and nothing else. It keeps its source alive, so a
/// window stays readable after the browser has moved to a different root.
#[derive(Clone)]
pub struct Blob {
    inner: Rc<Inner>,
}

impl Blob {
    /// Reads the head and captures everything a viewer needs to bid.
    pub fn open(source: SharedSource, id: EntryId) -> Result<Self, SourceError> {
        let (name, path, size) = {
            let borrowed = source.borrow();
            let entry = borrowed.entry(id).ok_or(SourceError::NoSuchEntry)?;
            if entry.is_dir {
                return Err(SourceError::IsADirectory(entry.name.clone()));
            }
            (
                entry.name.clone(),
                borrowed.display_path(id),
                entry.size.unwrap_or(0),
            )
        };
        let head = source.borrow_mut().read_range(id, 0, HEAD_LEN)?;
        Ok(Self {
            inner: Rc::new(Inner {
                name,
                path,
                size,
                head,
                source,
                id,
            }),
        })
    }

    pub fn name(&self) -> &str {
        &self.inner.name
    }

    pub fn path(&self) -> &str {
        &self.inner.path
    }

    pub fn size(&self) -> u64 {
        self.inner.size
    }

    pub fn head(&self) -> &[u8] {
        &self.inner.head
    }

    pub fn probe(&self) -> FileProbe<'_> {
        FileProbe::new(&self.inner.name, &self.inner.head, self.inner.size)
    }

    /// The whole file. Only call this behind a size guard — see
    /// [`Blob::read_range`], which is what a viewer of unbounded files uses.
    pub fn read_all(&self) -> Result<Vec<u8>, SourceError> {
        self.inner.source.borrow_mut().read_all(self.inner.id)
    }

    pub fn read_range(&self, offset: u64, len: usize) -> Result<Vec<u8>, SourceError> {
        self.inner
            .source
            .borrow_mut()
            .read_range(self.inner.id, offset, len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{FileSource, MemSource};
    use std::cell::RefCell;

    fn blob(name: &str, bytes: &[u8]) -> Blob {
        let mut mem = MemSource::new();
        let id = mem.add(name, bytes.to_vec());
        let source: SharedSource = Rc::new(RefCell::new(mem));
        Blob::open(source, id).unwrap()
    }

    #[test]
    fn test_the_head_is_capped_even_when_the_file_is_not() {
        let big = vec![b'x'; HEAD_LEN * 3];
        let blob = blob("big.txt", &big);
        assert_eq!(blob.head().len(), HEAD_LEN);
        assert_eq!(blob.size(), (HEAD_LEN * 3) as u64);
    }

    #[test]
    fn test_a_clone_shares_the_read_head_rather_than_reading_again() {
        let blob = blob("a.txt", b"hello");
        let twin = blob.clone();
        assert_eq!(twin.head(), b"hello");
        assert_eq!(twin.read_all().unwrap(), b"hello");
    }

    #[test]
    fn test_opening_a_directory_is_refused_before_any_read_happens() {
        let mem = MemSource::new();
        let root = mem.root();
        let source: SharedSource = Rc::new(RefCell::new(mem));
        assert!(matches!(
            Blob::open(source, root),
            Err(SourceError::IsADirectory(_))
        ));
    }
}
