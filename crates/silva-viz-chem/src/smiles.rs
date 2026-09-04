// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! SMILES lists, recognised by their name and then by whether they parse.

use crate::records::{RecordsView, SIZE_LIMIT};
use chem::io::reader::Format;
use silva_viz_core::{Blob, Claim, FileProbe, View, ViewerFactory};

/// The extensions that mean "one SMILES per line": the usual two, plus the
/// isomeric and canonical variants some toolkits write.
const EXTENSIONS: &[&str] = &["smi", "smiles", "ism", "can"];

/// How many of the head's data lines are examined. Enough to see past a header
/// and to want several agreeing lines before claiming an unnamed file, small
/// enough that the cost is fixed however large the file is.
const SNIFF_LINES: usize = 5;

/// Bid for a file whose *name* says SMILES. Above the table viewer's 10, so a
/// tab-separated `.smi` opens as structures rather than as two columns.
const NAMED: i32 = 15;

/// Bid for a file that parses but is not named like one: present in the
/// "Open in" menu, never what a double-click opens. The same use of the scale
/// the metadata viewer makes at -200.
const UNNAMED: i32 = -50;

/// The first whitespace token of each of the head's first [`SNIFF_LINES`] data
/// lines — the same lines `chem::io::reader::read_smiles` will look at, and
/// the same field of each.
fn candidates(head: &str) -> Vec<&str> {
    head.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_whitespace().next())
        .take(SNIFF_LINES)
        .collect()
}

/// Whether a token is a molecule, and a big enough one to be evidence.
///
/// `min_atoms` is the dial between the two bids. A named file needs only one
/// atom, because `.smi` plus anything parseable is already strong. An unnamed
/// one needs two, because single letters — `B C N O P S F I` are all valid
/// SMILES — turn far too much English prose into a molecule.
fn parses(token: &str, min_atoms: usize) -> bool {
    chem::io::smiles::parse_smiles(token)
        .map(|m| m.num_atoms() >= min_atoms)
        .unwrap_or(false)
}

/// Whether the SMILES viewer will take this file, and if not, why not.
///
/// Shared with the metadata viewer's shape, as `sdf::verdict` and
/// `views::text::verdict` are: a factory that can say why it declined is one
/// something else can quote.
pub fn verdict(probe: &FileProbe<'_>) -> Result<(), String> {
    bid(probe).map(|_| ())
}

/// The claim this file earns, or why it earns none.
///
/// SMILES has no magic bytes — `CCO` is a molecule and also three letters —
/// so this is the one viewer in the repository that reads the filename, and it
/// says so out loud rather than letting a reader discover the exception. Two
/// things keep it from being a licence to guess: the extension is necessary but
/// not sufficient, and a file that parses without the name gets a negative
/// priority instead of the default.
fn bid(probe: &FileProbe<'_>) -> Result<i32, String> {
    if probe.size() > SIZE_LIMIT {
        return Err(format!(
            "larger than the {} MiB limit",
            SIZE_LIMIT / (1024 * 1024)
        ));
    }
    if probe.has_nul() {
        return Err("contains NUL bytes".to_string());
    }
    if !probe.head_is_utf8() {
        return Err("not valid UTF-8".to_string());
    }

    let head = String::from_utf8_lossy(probe.head());
    let tokens = candidates(&head);
    if tokens.is_empty() {
        return Err("no lines to read".to_string());
    }
    let named = probe.ext().is_some_and(|e| EXTENSIONS.contains(&e));

    if named {
        // *Any* of the first few lines, not the first. A vendor `.smi` often
        // opens with a `smiles name activity` header, which cannot parse — and
        // that header is meant to be reported as a failed record, which needs
        // the file claimed first. Requiring line one would decline exactly the
        // files this viewer exists for.
        if tokens.iter().any(|t| parses(t, 1)) {
            return Ok(NAMED);
        }
        return Err("named like SMILES, but nothing in the first lines parses".to_string());
    }

    // No such extension, so the bar is agreement: several lines running, all
    // of them molecules of more than one atom.
    if tokens.len() >= 3 && tokens.iter().all(|t| parses(t, 2)) {
        return Ok(UNNAMED);
    }
    Err("not named like SMILES, and the first lines do not all parse".to_string())
}

pub struct SmilesFactory;

impl ViewerFactory for SmilesFactory {
    fn id(&self) -> &'static str {
        // Persisted with the open windows, so renaming it costs a user their
        // restored layout.
        "smiles"
    }

    fn claim(&self, probe: &FileProbe<'_>) -> Option<Claim> {
        Some(Claim::new("SMILES", bid(probe).ok()?))
    }

    fn open(&self, blob: Blob) -> Box<dyn View> {
        Box::new(RecordsView::new(blob, Format::Smiles, "SMILES"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIBRARY: &[u8] =
        b"CC(=O)Oc1ccccc1C(=O)O aspirin\nCn1cnc2c1c(=O)n(C)c(=O)n2C caffeine\nc1ccccc1 benzene\n";

    fn probe<'a>(name: &'a str, head: &'a [u8]) -> FileProbe<'a> {
        FileProbe::new(name, head, head.len() as u64)
    }

    #[test]
    fn test_every_smiles_extension_is_claimed() {
        for name in ["a.smi", "a.smiles", "a.ism", "a.can", "SHOUTING.SMI"] {
            let claim = SmilesFactory
                .claim(&probe(name, LIBRARY))
                .unwrap_or_else(|| panic!("{name} should be claimed"));
            assert_eq!(claim.label, "SMILES");
            assert_eq!(claim.priority, NAMED, "{name}");
        }
    }

    #[test]
    fn test_a_vendor_header_line_does_not_stop_the_file_being_claimed() {
        // The case that decides the whole probe. A header cannot parse, and it
        // is meant to show up in the failure list — which needs the file
        // opened, so the probe must look past line one.
        let with_header = b"smiles name activity\nCCO ethanol 1.5\nc1ccccc1 benzene 2.5\n";
        let claim = SmilesFactory
            .claim(&probe("vendor.smi", with_header))
            .expect("a header must not stop the claim");
        assert_eq!(claim.priority, NAMED);
    }

    #[test]
    fn test_a_smi_full_of_prose_is_refused_so_the_text_viewer_takes_it() {
        let prose = b"the quick brown fox\njumped over the lazy dog\nand thought little of it\n";
        assert!(SmilesFactory.claim(&probe("notes.smi", prose)).is_none());
    }

    #[test]
    fn test_a_txt_of_smiles_is_offered_but_never_the_default() {
        // The negative bid: openable on purpose, never by accident.
        let claim = SmilesFactory
            .claim(&probe("results.txt", LIBRARY))
            .expect("a file that plainly parses should be offered");
        assert!(claim.priority < 0, "priority was {}", claim.priority);
    }

    #[test]
    fn test_a_txt_of_prose_is_not_offered_at_all() {
        // Single letters are all valid SMILES, so prose has to be refused on
        // agreement rather than on any one line.
        let prose = b"CONS are a thing\nIN the beginning\nsomething else entirely\n";
        assert!(SmilesFactory.claim(&probe("notes.txt", prose)).is_none());
    }

    #[test]
    fn test_one_parsing_line_is_not_enough_without_the_name() {
        let mostly_prose = b"CCO\nthis line is prose\nand so is this one\n";
        assert!(
            SmilesFactory
                .claim(&probe("notes.txt", mostly_prose))
                .is_none()
        );
    }

    #[test]
    fn test_it_outbids_the_table_viewer_on_a_tab_separated_smi() {
        // Otherwise a `smiles<TAB>name` file opens as two columns of text.
        let tabbed = b"CCO\tethanol\nCCC\tpropane\nc1ccccc1\tbenzene\n";
        assert!(
            SmilesFactory
                .claim(&probe("a.smi", tabbed))
                .unwrap()
                .priority
                > 10
        );
    }

    #[test]
    fn test_a_file_past_the_ceiling_is_left_to_the_hex_viewer() {
        let big = FileProbe::new("enormous.smi", LIBRARY, SIZE_LIMIT + 1);
        assert!(SmilesFactory.claim(&big).is_none());
    }

    #[test]
    fn test_an_empty_or_comment_only_file_does_not_panic() {
        for head in [&b""[..], b"# just a comment\n", b"\n\n\n"] {
            assert!(SmilesFactory.claim(&probe("a.smi", head)).is_none());
        }
    }

    #[test]
    fn test_a_binary_file_named_smi_is_refused() {
        let binary = &[0u8, 1, 2, 255, 0];
        assert!(SmilesFactory.claim(&probe("a.smi", binary)).is_none());
    }

    #[test]
    fn test_the_reason_for_declining_is_reported_rather_than_just_none() {
        let prose = b"the quick brown fox\njumped over the lazy dog\nand kept going\n";
        let reason = verdict(&probe("notes.smi", prose)).expect_err("must decline");
        assert!(reason.contains("named like SMILES"), "{reason}");
    }
}
