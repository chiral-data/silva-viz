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

An MDL molfile or SDF, or a `.smi` list of SMILES, opens as a **record
browser**: the file's records listed on the left, the selected one drawn on the
right. Click a row to draw it, and drag the divider to give either side more
room. The list is virtualised, so a twenty-thousand-record library scrolls as
cheaply as a two-record one.

Below the structure: the molecule's name, formula, weight, atom and bond
counts, and any `> <FIELD>` data values the record carried.

The list's first column is the record's **position** — the same number the
warning line names, which is a record number for an SDF and a physical line
number for a `.smi`.

**The filter box matches names and formulas, and nothing else.** Type
`caffeine` or `C8H10N4O2`; case does not matter. It deliberately does **not**
search the SMILES column, even though that column is on screen: a text match
over SMILES is not a substructure search. SMILES is not a canonical form, so
searching `c1ccccc1` would find some benzene-containing molecules and quietly
miss others written a different way — an answer that looks chemical and is not.
Real substructure search needs SMARTS, and will arrive as its own feature.

### Options

An **Options** section under the details controls how structures are drawn.
It is collapsed until you open it.

- **Carbons** — which carbons get a visible label. Chemical convention leaves
  them as implicit vertices, so a benzene ring is six lines rather than six
  `C` glyphs; this chooses the exceptions. `Default` labels only what a bare
  vertex would not show, `Terminal` adds chain ends, `Acyclic` adds everything
  outside a ring, `All` labels every one, `None` labels nothing.
- **Atoms** — `Labels` draws element symbols, `Balls` draws coloured dots
  (legible where text would not be), `None` draws bonds only.
- **Hydrogens** — whether hydrogens stored as atoms are drawn. Off by default:
  an SDF usually carries every hydrogen explicitly, and drawing them buries
  the skeleton — a benzene ring becomes twelve vertices instead of six. The
  counts below the structure always include them either way.

  It has **no effect on a `.smi` file**, because SMILES leaves hydrogens
  implicit — there are no hydrogen atoms in the graph to hide.

**The setting is shared by every open structure window, and remembered between
sessions.** Change it in one window and every other structure follows in the
same frame; that is deliberate, not a glitch. It is kept with the same
mechanism as your window positions, which means the same caveat applies: an
upgrade that changes the stored format can reset it, and settings are written
periodically rather than instantly, so a hard kill immediately after a change
may lose it.

For a molfile, recognition is by the *counts line*, the fourth line of every
record, so the extension is not consulted: a `.mol`, an `.sdf` and a `.txt`
holding a molfile all open the same way, and an `.sdf` that is really prose
opens as text.

**SMILES is the exception, because it has no magic bytes.** `CCO` is a molecule
and also three letters, so `.smi`, `.smiles`, `.ism` and `.can` are recognised
by name — and then only if something in the first few lines actually parses. A
file of prose named `.smi` opens as text.

The reverse also works, on purpose. A `.txt` whose first lines are all SMILES
is offered under right-click → *Open in*, but never opens that way by default,
so you can look at a results file as structures without every text file
becoming a molecule.

Positions in the warning line count the way the file does: an SDF failure names
a **record**, a `.smi` failure names a **line** — the physical line, counting
blanks and `#` comments — and shows the text that would not parse. A `.smi`
starting with a `smiles name activity` header will report that header as a
failed line 1, which is the honest answer rather than a silent skip.

Hydrogens are hidden for molfiles, which carry them as atoms. SMILES leaves
them implicit, so there is nothing to hide.

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
| structures (SDF, SMILES) | 32 MiB, 20 000 records | declines past the size; truncates past the count and says so |
| hex | none | pages through the file, 64 KiB at a time |

## What it does not do

It does not write files. There is no save, no rename, and no delete: a `View`
is handed bytes and has no route back to the file.
