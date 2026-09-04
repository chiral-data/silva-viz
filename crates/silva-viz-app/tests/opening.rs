// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A filesystem test, and the web build has no filesystem: `FsSource` does not
// exist on wasm32, so the whole file compiles to nothing there.
#![cfg(not(target_arch = "wasm32"))]

//! What a real directory of real files actually opens as.
//!
//! The unit tests bid against a hand-built [`FileProbe`]. This one walks a
//! filesystem with the same `FsSource` the app uses, reads the same heads
//! through the same `Blob`, and asks the same registry — so a mistake in the
//! wiring between them has somewhere to show up.

use silva_viz_app::default_registry;
use silva_viz_core::{Blob, FsSource, SharedSource};
use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01";

/// A minimal but real single-atom molfile, coordinates and all.
const MOLFILE: &[u8] = b"methane\n  chem\n\n  1  0  0  0  0  0  0  0  0  0999 V2000\n    0.0000    0.0000    0.0000 C   0  0\nM  END\n$$$$\n";

/// Three drug-like molecules, one per line, the way a `.smi` file comes.
const SMILES: &[u8] =
    b"CC(=O)Oc1ccccc1C(=O)O aspirin\nCn1cnc2c1c(=O)n(C)c(=O)n2C caffeine\nc1ccccc1 benzene\n";

/// A directory of awkward files, removed when the test ends.
struct Fixture(PathBuf);

impl Fixture {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("silva-viz-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("a temp directory");
        Self(dir)
    }

    fn write(&self, name: &str, bytes: &[u8]) {
        fs::write(self.0.join(name), bytes).expect("writing the fixture");
    }

    fn source(&self) -> SharedSource {
        Rc::new(RefCell::new(FsSource::new(&self.0)))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Every viewer that bids for `name`, best first.
fn viewers(source: &SharedSource, name: &str) -> Vec<String> {
    let path = source.borrow().display_path(source.borrow().root());
    let id = source
        .borrow_mut()
        .resolve(&format!("{path}/{name}"))
        .unwrap_or_else(|| panic!("{name} should resolve"));
    let blob = Blob::open(source.clone(), id).expect("opening the blob");
    default_registry()
        .claims_for(&blob.probe())
        .into_iter()
        .map(|(id, _)| id.to_string())
        .collect()
}

#[test]
fn test_each_kind_of_file_opens_in_the_viewer_that_recognised_it() {
    let fixture = Fixture::new("kinds");
    fixture.write("notes.txt", b"hello\nworld\n");
    fixture.write("data.csv", b"a,b,c\n1,2,3\n4,5,6\n7,8,9\n");
    fixture.write("picture.png", PNG);
    fixture.write("aspirin.sdf", MOLFILE);
    fixture.write("library.smi", SMILES);
    fixture.write("opaque.bin", &[0u8, 1, 2, 3, 255]);
    let source = fixture.source();

    assert_eq!(viewers(&source, "notes.txt")[0], "text");
    assert_eq!(viewers(&source, "data.csv")[0], "table");
    assert_eq!(viewers(&source, "picture.png")[0], "image");
    assert_eq!(viewers(&source, "aspirin.sdf")[0], "sdf");
    assert_eq!(viewers(&source, "library.smi")[0], "smiles");
    // Nothing recognised it, so it falls to the floor rather than nowhere.
    assert_eq!(viewers(&source, "opaque.bin"), ["hex", "meta"]);
}

#[test]
fn test_a_png_named_txt_opens_as_a_picture_and_text_is_not_even_offered() {
    // The case the sniffing design exists for. A viewer chosen by extension
    // would open this and paint a screenful of replacement characters.
    let fixture = Fixture::new("liar");
    fixture.write("pretend.txt", PNG);
    let source = fixture.source();

    let offered = viewers(&source, "pretend.txt");
    assert_eq!(offered[0], "image");
    assert!(!offered.contains(&"text".to_string()), "{offered:?}");
}

#[test]
fn test_a_molfile_named_txt_opens_as_a_structure_through_a_real_filesystem() {
    // The unit tests bid against a hand-built probe. This one proves the head
    // that `Blob` actually reads off a disk is enough to recognise a molfile —
    // the counts line is on the fourth line, well inside the 4 KiB head.
    let fixture = Fixture::new("molfile-liar");
    fixture.write("mystery.txt", MOLFILE);
    let source = fixture.source();

    let offered = viewers(&source, "mystery.txt");
    assert_eq!(offered[0], "sdf");
    // Still text underneath, because a molfile is text — unlike a PNG, which
    // the text viewer refuses outright.
    assert!(offered.contains(&"text".to_string()), "{offered:?}");
}

#[test]
fn test_a_txt_of_smiles_is_offered_the_structure_viewer_but_opens_as_text() {
    // SMILES has no magic bytes, so its viewer is the one that reads a
    // filename. This is the half that keeps that honest: the same bytes under
    // a `.txt` name are still openable as structures, deliberately, and still
    // open as text by default.
    let fixture = Fixture::new("smiles-txt");
    fixture.write("results.txt", SMILES);
    let source = fixture.source();

    let offered = viewers(&source, "results.txt");
    assert_eq!(offered[0], "text");
    assert!(offered.contains(&"smiles".to_string()), "{offered:?}");
}

#[test]
fn test_a_text_file_named_png_is_refused_by_the_image_viewer() {
    // The same rule in the other direction, which is the half that a naive
    // "sniff first, fall back to the extension" implementation gets wrong.
    let fixture = Fixture::new("lying-png");
    fixture.write("lying.png", b"this is plainly not a png at all\n");
    let source = fixture.source();

    let offered = viewers(&source, "lying.png");
    assert_eq!(offered[0], "text");
    assert!(!offered.contains(&"image".to_string()), "{offered:?}");
}

#[test]
fn test_a_file_far_larger_than_memory_is_opened_by_reading_a_window_of_it() {
    // A sparse 3 GiB file. If anything on this path called `read_all` the test
    // would not fail, it would take the machine down with it — which is the
    // point: this is the only check that the hex viewer really does page.
    const HUGE: u64 = 3 * 1024 * 1024 * 1024;
    let fixture = Fixture::new("huge");
    let path = fixture.0.join("huge.bin");
    let file = fs::File::create(&path).expect("creating the sparse file");
    file.set_len(HUGE).expect("a sparse 3 GiB file");
    drop(file);

    let source = fixture.source();
    let id = source
        .borrow_mut()
        .resolve(&path.to_string_lossy())
        .expect("the huge file should resolve");
    let blob = Blob::open(source, id).expect("opening a 3 GiB file must not read it");

    assert_eq!(blob.size(), HUGE);
    // The head is capped whatever the file's size is.
    assert_eq!(blob.head().len(), silva_viz_core::HEAD_LEN);
    // And a row near the end costs one read, not three gigabytes of them.
    assert_eq!(blob.read_range(HUGE - 16, 16).unwrap().len(), 16);

    let offered = viewers(&fixture.source(), "huge.bin");
    assert_eq!(offered, ["hex", "meta"], "text must decline at this size");
}

#[test]
fn test_a_directory_that_cannot_be_read_is_reported_rather_than_shown_empty() {
    let fixture = Fixture::new("locked");
    fs::create_dir(fixture.0.join("secret")).expect("a subdirectory");
    fixture.write("secret/inside.txt", b"hidden\n");

    let source = fixture.source();
    let root = source.borrow().root();
    let locked = source.borrow_mut().children(root).unwrap()[0].id;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let dir = fixture.0.join("secret");
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o000)).unwrap();
        let listed = source.borrow_mut().children(locked).map(<[_]>::to_vec);
        // Restored before the assert so a failure still leaves a removable
        // directory behind.
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();

        // Running as root defeats the permission bits entirely, and a test that
        // silently passes for the wrong reason is worse than one that is
        // skipped.
        if unsafe { libc_geteuid() } != 0 {
            assert!(listed.is_err(), "an unreadable directory must say so");
        }
    }

    let _ = locked;
}

#[cfg(unix)]
unsafe fn libc_geteuid() -> u32 {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() }
}
