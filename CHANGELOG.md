# Changelog

All notable changes to this project are documented here.

## [0.2.0] - 2026-09-04

**Molecular structures open as structures.** An MDL molfile, an SDF or a `.smi`
list of SMILES opens as a record browser: the file's records listed on one
side, the selected molecule drawn on the other, filterable, with depiction
options you can change and that are remembered.

The point of v0.1.0 was a seam — a shell that knows no file formats, hosting
viewers that register themselves and bid for a file by looking at its bytes.
This milestone is the first real test of that claim, and it held: the chemistry
lives in a separate crate, `silva-viz-chem`, registered through the same call a
third-party crate would use. CI pins the other half, that `silva-viz-core`
still builds with no chemistry crate in its tree.

### Features

- **An SDF viewer** (#9). Recognised by the *counts line* — the fourth line of
  every record, and the same line `chem`'s own parser trusts — so the extension
  is never consulted and a `.mol`, an `.sdf` and a `.txt` holding a molfile all
  open alike. Records step, data fields show, and records that failed to parse
  are named with their position rather than dropped.
- **A SMILES viewer** (#11). The one viewer here that reads a filename, because
  SMILES has no magic bytes — `CCO` is a molecule and also three letters. Two
  things keep that honest: the extension is necessary but not sufficient, and a
  file that plainly parses *without* the name is offered at a negative priority,
  so it can be opened on purpose and never by accident.
- **A record browser** (#13): a virtualised list beside the structure with a
  filter and a draggable divider, so a twenty-thousand-record library scrolls
  like a two-record one. The filter matches names and formulas and deliberately
  **not** the SMILES column — a substring match over SMILES is not a
  substructure search, and one that looked like it would quietly miss molecules
  written a different way. That waits on SMARTS.
- **Depiction options** (#15) — which carbons are labelled, whether atoms are
  symbols or coloured dots, whether stored hydrogens appear. Shared by every
  open structure window and remembered between sessions, and reached without
  widening the `View` trait: a view holds a `Ui`, so it can use the same
  persisted store the shell has kept window positions in since v0.1.0.
  `docs/viewers.md` documents the pattern, since any third-party viewer will
  hit the same question.

### Two upstream defects, found by pointing a new consumer at a published crate

Both were invisible to the tests that existed, which is the argument for
building something real on top of a library rather than only testing it.

- **chiral-data/rust-chem#203** — `parse_smiles` read bare two-letter element
  symbols, which the grammar allows only for `Cl` and `Br`. So `Cn` was
  copernicium, and **caffeine written the ordinary way parsed as a 13-atom
  `C7H7CnN3O2` at 450 g/mol**, returned as `Ok` with nothing on stderr. `Cc`
  and `Nc` were unaffected, which is why it had gone unnoticed. The same
  functions also panicked on a non-ASCII string. Fixed upstream and released as
  `chem 0.6.1`, which this milestone requires as its floor — a resolver free to
  pick 0.6.0 would draw the wrong molecule silently.
- **chiral-data/rust-chem#202** — a V2000 molfile with 100 or more atoms cannot
  be read, because a `%3d` field holding three digits fills its width and the
  parser splits on whitespace. It is a *reader* defect: the writer is
  spec-conformant, so hand-written files from other toolkits are equally
  unreadable. Still open; the viewer reports such records rather than losing
  them.

### One defect of our own, and where it was caught

The record list truncated a four-digit line number to two digits.
`Column::auto()` sizes from the *currently visible* rows — its own
documentation says so — and then keeps that width, so a list opened at the top
sized for `40` and never widened.

Worth recording because **the entire automated gate passed it**: 137 tests,
clippy, four CI jobs and a headless render check. Pixel geometry cannot be
asserted headlessly, so a column that misreported the one number a failure
message cites reached a human before it reached a test. Fixed by taking the
floor from the widest value in the file, measured once at open, with the
measured columns drawn monospaced so "longest" and "widest" are the same
question.

### Deliberate limitations

Stated because each is a choice rather than an oversight.

- **A 32 MiB, 20 000-record ceiling.** `chem::io::reader::read` takes the whole
  file as a `&str`, so everything is parsed before anything is shown. A real
  ChEMBL dump will not open. Streaming needs `read_range` and its own story.
- **No substructure search.** Needs SMARTS from `chem`.
- **Aromatic and Kekulé SMILES draw differently** — lowercase input arrives
  aromatic from the parser and gets a dashed inner ring bond, uppercase gets
  plain double bonds. `detect_aromaticity` would unify them, but it mutates
  bond orders and is documented upstream as not an industry-grade model.
- **Options are remembered, not permanent.** They ride in egui's memory blob,
  keyed partly by a `TypeId`, so a toolchain or dependency change can reset
  them — as it can already reset window positions.
- **No keyboard navigation** of the record list, and no thumbnail grid.

### Testing

72 tests to 141. The dependency tree moved 334 to 347 for 9 new packages —
`chem`, `nom`, `bitvec` with its four support crates, `byteorder` and `fxhash`
— the difference being that CI counts `sort -u` lines, so a crate reached by
two paths prints an extra continuation line. The ceiling moved 360 to 375.

## [0.1.0] - 2026-09-02

**The shell, and the seam viewers plug into.** A left-hand file tree and a
workspace of floating viewer windows, native and WebAssembly, knowing no file
formats at all (#6).

- `FileSource`, so the browser never mentions a path — a web build has no
  filesystem, and a tree typed on `std::path::Path` would kill that target and
  every viewer written against it. `read_range` alongside `read_all` is what
  lets the hex viewer open a file larger than memory.
- `ViewerFactory` bids rather than registering an extension. A factory sees the
  first 4 KiB and either claims a file or declines, so a `.txt` holding PNG
  magic opens as an image and a `.png` full of text does not. Every bid appears
  under right-click, which is also why the workspace floats windows rather than
  tabbing them.
- Five format-agnostic viewers: metadata, hex, text, delimited table, image.
- **Fixed:** `Open folder…` deadlocked on macOS (#5). `rfd`'s async dialog
  completes from a callback only the platform's main run loop delivers, so
  awaiting it from the thread that owns that loop hangs with the panel already
  on screen. The blocking API runs its own nested modal loop and needs nothing
  from the frame loop it interrupts.
