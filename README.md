# silva-viz

A file browser for scientific applications: browse on the left, open each file
in its own floating viewer.

![the window: a file tree on the left, three cascaded viewer windows over the rest]

A small desktop and web application built on [egui](https://github.com/emilk/egui).
It is a *viewer*, not an editor: it opens files and never writes them.

Scientific work leaves a directory full of unlike things — a structure file next
to a run log next to a results table next to a rendered figure. A viewer that
knows one format sends the rest to a text editor. This one lets a viewer for any
format register itself, and decides what to open by looking at the bytes.

```
cargo run -p silva-viz-app          # desktop
cd crates/silva-viz-app && trunk serve   # http://127.0.0.1:8080
```

## What it does

- **A real tree on the left.** Pick a folder and expand it lazily; a directory
  with fifty thousand entries costs the same as one with twenty.
- **A window per open file**, cascaded, draggable, and closable — up to twelve,
  after which the oldest is retired.
- **Viewers bid for a file by looking at it.** A `.txt` that begins with PNG
  magic bytes opens as an image; a `.png` that is really text does not. Every
  viewer that wanted a file is offered under right-click → *Open in*, so the
  same file can be open as a table *and* as raw text, side by side.
- **Nothing is ever un-openable.** The hex viewer bids on everything at the
  lowest priority and pages through the file, so a 3 GB blob opens as fast as a
  3 KB one.

Seven viewers ship: metadata, hex, text, delimited table, image (PNG, JPEG,
GIF, BMP), and two for **molecular structures** — an MDL molfile or SDF, and a
`.smi` list of SMILES. Either opens as a **record browser**: a scrollable,
filterable list of the file's records on one side, the selected structure and
its details on the other, with a divider you can drag. The list is virtualised,
so a twenty-thousand-record library costs the same to scroll as a two-record
one.

The filter matches names and formulas — not the SMILES column, deliberately,
because a text match over SMILES is not a substructure search and should not
look like one.

How structures are drawn — which carbons are labelled, whether atoms are
symbols or coloured dots, whether stored hydrogens appear — is adjustable per
window, shared by every window, and remembered between sessions.

The structure viewers are where "by the bytes, never by the name" meets its
limit, and they meet it differently. A molfile is recognised by its *counts
line*, the fourth line of every record, so a `.mol`, an `.sdf` and a `.txt`
holding a molfile all open alike. SMILES has no magic bytes at all — `CCO` is a
molecule and also three letters — so that viewer is the only one here that
reads a filename. Two things keep it honest: the extension is necessary but not
sufficient, and a file that plainly parses without the name is offered under
right-click → *Open in* at a negative priority, so it can be opened on purpose
and never by accident.

## Adding a viewer

A viewer is a `ViewerFactory` and a `View`, and the built-in ones have no
privileged access to the shell — they are registered exactly the way yours
would be. [`docs/viewers.md`](docs/viewers.md) has the whole of a working
viewer in one listing, then the two things that are not obvious: how to choose
a bid priority, and where to keep a setting a user can change.

## Layout

| crate | what it is |
| --- | --- |
| `silva-viz-core` | the seam: `FileSource`, `ViewerFactory`, `View`, `Blob` |
| `silva-viz-app` | the eframe shell and the five format-agnostic viewers |
| `silva-viz-chem` | the SDF and SMILES viewers, built on [`chem`](https://crates.io/crates/chem) |

`silva-viz-chem` is a downstream crate registered through the same call a
third-party one would use, and CI pins the other half of that claim:
`silva-viz-core` still builds with no chemistry crate in its tree.

The browser talks to `FileSource` and never to `std::path::Path`, which is the
only reason the same panel compiles for the web — where there is no filesystem
and files arrive by drag-and-drop.

## Licence

[Mozilla Public License 2.0](LICENSE). File-level copyleft: a change to a file
in this repository stays under the MPL, and a larger work that merely links the
crates does not have to.
