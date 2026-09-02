// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The viewers that ship in the box.
//!
//! All five are ordinary [`silva_viz_core::ViewerFactory`] implementations with
//! no privileged access to the shell — they are registered in `app.rs` exactly
//! the way a downstream crate would register its own. `docs/viewers.md` is the
//! same code written out for someone doing that, and if these five ever need
//! something the trait cannot express, that document is where it shows up.

pub mod hex;
pub mod image;
pub mod meta;
pub mod table;
pub mod text;

/// A byte count as a person would say it.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

/// Byte offsets of each line start, plus a sentinel at the end.
///
/// Holding offsets rather than a `Vec<String>` is what lets an 8 MiB file open
/// without a second copy of itself: a virtualised list slices the original for
/// the forty lines actually on screen.
pub fn line_offsets(text: &str) -> Vec<usize> {
    let mut offsets = vec![0usize];
    offsets.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    // A trailing newline ends the last line rather than starting an empty one.
    if text.ends_with('\n') {
        offsets.pop();
    }
    offsets.push(text.len());
    offsets
}

/// Line `n` of `text`, without its line ending.
pub fn line<'a>(text: &'a str, offsets: &[usize], n: usize) -> &'a str {
    let Some(&start) = offsets.get(n) else {
        return "";
    };
    let end = offsets.get(n + 1).copied().unwrap_or(text.len());
    text[start..end].trim_end_matches(['\n', '\r'])
}

/// How many lines [`line_offsets`] describes.
pub fn line_count(offsets: &[usize]) -> usize {
    offsets.len().saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sizes_read_the_way_a_person_would_say_them() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(999), "999 B");
        assert_eq!(human_size(1024), "1.0 KiB");
        assert_eq!(human_size(3 * 1024 * 1024 * 1024), "3.0 GiB");
    }

    #[test]
    fn test_a_trailing_newline_does_not_invent_an_empty_last_line() {
        let text = "a\nb\n";
        let offsets = line_offsets(text);
        assert_eq!(line_count(&offsets), 2);
        assert_eq!(line(text, &offsets, 1), "b");
    }

    #[test]
    fn test_a_file_without_a_trailing_newline_keeps_its_last_line() {
        let text = "a\nb";
        let offsets = line_offsets(text);
        assert_eq!(line_count(&offsets), 2);
        assert_eq!(line(text, &offsets, 1), "b");
    }

    #[test]
    fn test_windows_line_endings_are_not_shown_as_stray_characters() {
        let text = "a\r\nb\r\n";
        let offsets = line_offsets(text);
        assert_eq!(line(text, &offsets, 0), "a");
        assert_eq!(line_count(&offsets), 2);
    }

    #[test]
    fn test_an_empty_file_has_one_empty_line_rather_than_a_panic() {
        let offsets = line_offsets("");
        assert_eq!(line_count(&offsets), 1);
        assert_eq!(line("", &offsets, 0), "");
        assert_eq!(line("", &offsets, 99), "");
    }
}
