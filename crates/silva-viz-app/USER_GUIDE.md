# Using silva-viz

## Starting it

```
cargo run -p silva-viz-app
```

The desktop build opens on your current directory. **File → Open folder…**, the
*Open folder…* button at the top of the left panel, or dropping a folder onto
the window all move it somewhere else.

For the web build:

```
cd crates/silva-viz-app && trunk serve
# http://127.0.0.1:8080
```

There is no filesystem in a browser, so the web build has **Add files…** rather
than *Open folder…*, and drag-and-drop is the usual way in. Files you add live
in the page and disappear when you close it.

## The left panel

- Click a folder to expand or collapse it. Folders are listed before files, and
  each group is sorted by name.
- The box under the root path filters by file name, case-insensitively. Folders
  are never filtered out — hiding one would hide the matching files inside it.
- **Refresh** re-reads the tree after something changed on disk.
- A folder that cannot be read shows the reason in place, in red, rather than
  appearing empty.

## Opening files

- **Double-click**, or select and press **Enter**, opens the file in whichever
  viewer bid highest for it.
- **Right-click → Open in** lists *every* viewer that wanted the file. Pick a
  second one and you get a second window over the same file: a CSV as a table
  and as raw text, scrolling independently.

Windows cascade from the top-left of the workspace and can be dragged,
resized, and collapsed. Twelve can be open at once; opening a thirteenth
retires the oldest, and the status bar says which.

**View** lists every open window with a checkbox, and has *Close all*.

## Why did my file open like that?

Viewers judge a file by its first 4 KiB, not by its name. A `.txt` that begins
with PNG magic bytes really is a PNG and opens as one; a `.png` full of text is
not, and won't.

When that surprises you, open the file in **Metadata**. It shows the size, the
extension, what the bytes look like, and — in as many words — whether the text
viewer would take the file and why not.

## Structure files

An MDL molfile or SDF opens as a drawn structure. One record is on screen at a
time — `◀` and `▶` step through a multi-record file — with the molecule's name,
formula, weight, atom and bond counts below it, followed by any `> <FIELD>`
data values the record carried.

Hydrogens are not drawn, though the counts below the structure still include
them. An SDF usually stores every hydrogen as an atom of its own, and drawing
them buries the skeleton — a benzene ring becomes twelve vertices instead of
six.

Recognition is by the *counts line*, the fourth line of every record, so the
extension is not consulted: a `.mol`, an `.sdf` and a `.txt` holding a molfile
all open the same way, and an `.sdf` that is really prose opens as text.

**Records that could not be read are counted, not hidden.** A warning line
under the details names each one by its position in the file with the parser's
own message. Two cases worth knowing:

- **A V3000 molfile is declined** and opens as text. The underlying library
  reads only V2000, and a V3000 record would otherwise come back as a molecule
  with no atoms — a blank panel with no explanation.
- **A record with 100 or more atoms fails to parse.** The molfile format packs
  each count and atom index into three characters with no separator, so a bond
  between atoms 99 and 100 is written ` 99100` and read back as one number.
  This is a limitation of the library rather than of the file — such files are
  perfectly valid, and other tools read them — so these records appear in the
  warning line rather than vanishing.

## Limits, and what happens at them

| | limit | past it |
| --- | --- | --- |
| text | 8 MiB | declines; the file opens as hex |
| table | 8 MiB | declines |
| image | 64 MiB | declines |
| structure | 32 MiB, 20 000 records | declines past the size; truncates past the count and says so |
| hex | none | pages through the file, 64 KiB at a time |

## What it does not do

It does not write files. There is no save, no rename, and no delete: a `View`
is handed bytes and has no route back to the file.
