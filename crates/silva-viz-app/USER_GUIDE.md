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

## Limits, and what happens at them

| | limit | past it |
| --- | --- | --- |
| text | 8 MiB | declines; the file opens as hex |
| table | 8 MiB | declines |
| image | 64 MiB | declines |
| hex | none | pages through the file, 64 KiB at a time |

## What it does not do

It does not write files. There is no save, no rename, and no delete: a `View`
is handed bytes and has no route back to the file.
