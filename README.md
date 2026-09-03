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

Six viewers ship: metadata, hex, text, delimited table, image (PNG, JPEG, GIF,
BMP), and **molecular structures** — an MDL molfile or SDF opens as a drawn
structure, one record at a time, with its data fields beside it.

The structure viewer is the one exception to "by the bytes, never by the name",
and only half an exception: a molfile is recognised by its *counts line*, the
fourth line of every record, so a `.mol`, an `.sdf` and a `.txt` holding a
molfile all open the same way. SMILES is the format with no magic bytes at all
— `CCO` is a molecule and also three letters — and when its viewer arrives it
will have to read the filename, which is documented where it happens.

## Adding a viewer

A viewer is a `ViewerFactory` and a `View`, and the built-in ones have no
privileged access to the shell — they are registered exactly the way yours
would be. See [`docs/viewers.md`](docs/viewers.md); it is about thirty lines.

## Layout

| crate | what it is |
| --- | --- |
| `silva-viz-core` | the seam: `FileSource`, `ViewerFactory`, `View`, `Blob` |
| `silva-viz-app` | the eframe shell and the five format-agnostic viewers |
| `silva-viz-chem` | the structure viewers, built on [`chem`](https://crates.io/crates/chem) |

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
