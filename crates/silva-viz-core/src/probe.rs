// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What a factory is shown before it decides whether a file is its business.

/// How much of a file a factory gets to look at.
///
/// Enough for every magic number in use and for a representative sample of a
/// text file's lines; small enough that probing a 3 GB file costs one read.
pub const HEAD_LEN: usize = 4096;

/// An image format recognised from its first bytes.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ImageKind {
    Png,
    Jpeg,
    Gif,
    Bmp,
}

impl ImageKind {
    pub fn label(self) -> &'static str {
        match self {
            ImageKind::Png => "PNG",
            ImageKind::Jpeg => "JPEG",
            ImageKind::Gif => "GIF",
            ImageKind::Bmp => "BMP",
        }
    }
}

/// A file's name, size, and first [`HEAD_LEN`] bytes.
pub struct FileProbe<'a> {
    name: &'a str,
    ext: Option<String>,
    head: &'a [u8],
    size: u64,
}

impl<'a> FileProbe<'a> {
    pub fn new(name: &'a str, head: &'a [u8], size: u64) -> Self {
        // Lowercased once here rather than at every comparison; a factory that
        // matched on `ext == "csv"` would otherwise silently miss `DATA.CSV`.
        let ext = name
            .rsplit_once('.')
            .map(|(_, e)| e.to_lowercase())
            .filter(|e| !e.is_empty());
        Self {
            name,
            ext,
            head,
            size,
        }
    }

    pub fn name(&self) -> &str {
        self.name
    }

    pub fn ext(&self) -> Option<&str> {
        self.ext.as_deref()
    }

    pub fn head(&self) -> &[u8] {
        self.head
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn starts_with(&self, magic: &[u8]) -> bool {
        self.head.starts_with(magic)
    }

    /// Whether the head is text.
    ///
    /// A truncated multi-byte character at the very end of the head is *not* a
    /// failure — the head is a prefix of the file, and cutting a three-byte
    /// character in half is the cut's fault, not the file's. Rejecting it would
    /// send a large UTF-8 file to the hex viewer roughly one time in five.
    pub fn head_is_utf8(&self) -> bool {
        match std::str::from_utf8(self.head) {
            Ok(_) => true,
            Err(e) => e.error_len().is_none() && self.head.len() >= HEAD_LEN,
        }
    }

    /// Whether the head contains a NUL, the oldest and still the best single
    /// signal that a file is not text.
    pub fn has_nul(&self) -> bool {
        self.head.contains(&0)
    }

    pub fn image_kind(&self) -> Option<ImageKind> {
        if self.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(ImageKind::Png)
        } else if self.starts_with(&[0xFF, 0xD8, 0xFF]) {
            Some(ImageKind::Jpeg)
        } else if self.starts_with(b"GIF87a") || self.starts_with(b"GIF89a") {
            Some(ImageKind::Gif)
        } else if self.starts_with(b"BM") {
            Some(ImageKind::Bmp)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_the_extension_is_lowercased_so_a_shouting_filename_still_matches() {
        let probe = FileProbe::new("DATA.CSV", b"a,b", 3);
        assert_eq!(probe.ext(), Some("csv"));
    }

    #[test]
    fn test_a_file_with_no_extension_has_none_rather_than_an_empty_string() {
        assert_eq!(FileProbe::new("README", b"", 0).ext(), None);
        assert_eq!(FileProbe::new("trailing.", b"", 0).ext(), None);
    }

    #[test]
    fn test_a_dotfile_is_not_mistaken_for_an_extension() {
        // `.gitignore` is a name, not an extension — but `rsplit_once` reads it
        // as one, so this pins the behaviour that is actually shipped rather
        // than the one that would be nice.
        assert_eq!(
            FileProbe::new(".gitignore", b"", 0).ext(),
            Some("gitignore")
        );
    }

    #[test]
    fn test_a_character_cut_in_half_by_the_head_boundary_is_still_text() {
        // The head is a prefix, so the last character may be truncated. A full
        // head that ends mid-character is text; a short one that does is not,
        // because nothing was cut off it.
        let mut head = vec![b'a'; HEAD_LEN - 1];
        head.push(0xE2); // the first byte of a three-byte character
        assert!(FileProbe::new("x.txt", &head, 9_000).head_is_utf8());
        assert!(!FileProbe::new("x.txt", &[b'a', 0xE2], 2).head_is_utf8());
    }

    #[test]
    fn test_invalid_bytes_in_the_middle_are_not_text_whatever_the_length() {
        let head = [b'a', 0xFF, b'b'];
        assert!(!FileProbe::new("x.txt", &head, 3).head_is_utf8());
    }

    #[test]
    fn test_an_image_is_recognised_by_its_bytes_and_not_by_its_name() {
        let png = FileProbe::new("pretend.txt", b"\x89PNG\r\n\x1a\n....", 12);
        assert_eq!(png.image_kind(), Some(ImageKind::Png));
        let liar = FileProbe::new("real.png", b"not an image at all", 19);
        assert_eq!(liar.image_kind(), None);
    }
}
