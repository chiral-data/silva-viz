// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! MDL molfiles and SDF, recognised by their counts line.

use crate::records::{RecordsView, SDF_LIMIT};
use chem::io::reader::Format;
use silva_viz_core::{Blob, Claim, FileProbe, View, ViewerFactory};

/// The line an MDL molfile keeps its atom and bond counts on, counting from
/// zero. The same constant `chem`'s own parser uses; the three lines before it
/// are a name, a program stamp and a comment, every one of which is
/// legitimately blank and so tells us nothing.
const COUNTS_LINE: usize = 3;

/// Whether the SDF viewer will take this file, and if not, why not.
///
/// Shared shape with `views::text::verdict` in the app crate: a factory that
/// can say *why* it declined is one a metadata viewer can quote, so a short
/// "Open in" menu stops being a mystery.
pub fn verdict(probe: &FileProbe<'_>) -> Result<(), String> {
    if probe.size() > SDF_LIMIT {
        return Err(format!(
            "larger than the {} MiB limit",
            SDF_LIMIT / (1024 * 1024)
        ));
    }
    if probe.has_nul() {
        return Err("contains NUL bytes".to_string());
    }
    if !probe.head_is_utf8() {
        return Err("not valid UTF-8".to_string());
    }
    counts_line_verdict(&String::from_utf8_lossy(probe.head()))
}

/// The evidence, and the whole reason this viewer needs no filename.
///
/// Deliberately the *same* test `chem::io::sdf::parse_counts_line` applies —
/// whitespace-split, first two fields as integers — rather than the
/// specification's fixed-width columns. Claiming by a stricter rule than the
/// parser uses would decline files that read perfectly; claiming by a looser
/// one would open files that cannot be read at all.
///
/// The non-zero atom count is what excludes V3000. A V3000 molfile carries
/// `0` here and puts its real counts in an `M  V30 COUNTS` line that `chem
/// 0.6` does not read — it returns an atomless molecule and reports success —
/// so a viewer that claimed one would show a blank panel and no reason for it.
fn counts_line_verdict(head: &str) -> Result<(), String> {
    let Some(line) = head.lines().nth(COUNTS_LINE) else {
        return Err("fewer than four lines, so no counts line".to_string());
    };
    // No byte slicing: `split_whitespace` cannot land mid-character, and a
    // viewer must not panic on a file someone hands it.
    let mut fields = line.split_whitespace();
    let atoms = fields.next().and_then(|f| f.parse::<usize>().ok());
    let bonds = fields.next().and_then(|f| f.parse::<usize>().ok());
    match (atoms, bonds) {
        (Some(0), Some(_)) if line.contains("V3000") => {
            Err("a V3000 molfile, which chem 0.6 cannot read".to_string())
        }
        (Some(0), Some(_)) => Err("a counts line claiming no atoms".to_string()),
        (Some(_), Some(_)) => Ok(()),
        _ => Err("line four is not a molfile counts line".to_string()),
    }
}

pub struct SdfFactory;

impl ViewerFactory for SdfFactory {
    fn id(&self) -> &'static str {
        // Persisted with the open windows, so renaming it costs a user their
        // restored layout.
        "sdf"
    }

    fn claim(&self, probe: &FileProbe<'_>) -> Option<Claim> {
        verdict(probe).ok()?;
        Some(Claim::new("SDF", 20))
    }

    fn open(&self, blob: Blob) -> Box<dyn View> {
        Box::new(RecordsView::new(blob, Format::Sdf, "SDF"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal but real single-atom molfile.
    const METHANE: &[u8] = b"methane\n  chem\n\n  1  0  0  0  0  0  0  0  0  0999 V2000\n    0.0000    0.0000    0.0000 C   0  0\nM  END\n$$$$\n";

    fn probe<'a>(name: &'a str, head: &'a [u8]) -> FileProbe<'a> {
        FileProbe::new(name, head, head.len() as u64)
    }

    #[test]
    fn test_a_molfile_is_claimed_whatever_it_is_called() {
        for name in ["a.sdf", "a.mol", "pretend.txt", "no-extension"] {
            let claim = SdfFactory
                .claim(&probe(name, METHANE))
                .unwrap_or_else(|| panic!("{name} should be claimed"));
            assert_eq!(claim.label, "SDF");
        }
    }

    #[test]
    fn test_an_sdf_holding_prose_is_refused_so_the_text_viewer_takes_it() {
        let prose = b"just some prose\nabout molecules\nand nothing else\nat all here\n";
        assert!(SdfFactory.claim(&probe("lying.sdf", prose)).is_none());
    }

    #[test]
    fn test_a_molfile_past_the_ceiling_is_left_to_the_hex_viewer() {
        let big = FileProbe::new("enormous.sdf", METHANE, SDF_LIMIT + 1);
        assert!(SdfFactory.claim(&big).is_none());
    }

    #[test]
    fn test_it_outbids_text_so_a_double_click_draws_the_structure() {
        // Priorities only mean anything relative to each other, so the one
        // relationship that matters is pinned rather than assumed.
        assert!(SdfFactory.claim(&probe("a.sdf", METHANE)).unwrap().priority > 0);
    }

    #[test]
    fn test_three_header_lines_with_no_fourth_does_not_panic() {
        let truncated = b"name\n  prog\n\n";
        assert!(SdfFactory.claim(&probe("a.sdf", truncated)).is_none());
        assert!(SdfFactory.claim(&probe("empty.sdf", b"")).is_none());
    }

    #[test]
    fn test_a_counts_line_of_multi_byte_characters_does_not_panic() {
        // Byte-range slicing would panic here rather than decline, and a
        // viewer handed a hostile file must decline.
        let head = "name\n  prog\n\n\u{4f60}\u{597d}\u{4e16}\u{754c}\nM  END\n".as_bytes();
        assert!(SdfFactory.claim(&probe("a.sdf", head)).is_none());
    }

    #[test]
    fn test_a_v3000_molfile_is_declined_and_says_why() {
        // chem 0.6 reads a V3000 record as an atomless molecule and calls it a
        // success, so claiming one would paint an empty panel with no reason.
        let v3000 = b"name\n  prog\n\n  0  0  0  0  0  0  0  0  0  0999 V3000\nM  V30 BEGIN CTAB\nM  V30 COUNTS 21 22\nM  END\n";
        let reason = verdict(&probe("modern.sdf", v3000)).expect_err("V3000 must be declined");
        assert!(reason.contains("V3000"), "{reason}");
    }

    #[test]
    fn test_a_whitespace_separated_counts_line_is_accepted_like_chem_accepts_it() {
        // Not spec-conformant fixed-width, but `chem`'s parser splits on
        // whitespace and reads it, so declining it here would hide a readable
        // file behind the text viewer.
        let loose = b"name\n  prog\n\n1 0 0 0 0 0 0 0 0 999 V2000\n    0.0 0.0 0.0 C 0 0\nM  END\n";
        assert!(SdfFactory.claim(&probe("loose.sdf", loose)).is_some());
    }
}
