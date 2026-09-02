# Writing a viewer

A viewer is two traits. Nothing in the shell is special-cased for the five that
ship — `app.rs` registers them through the same call your crate would use, and
if that ever stops being true it will show up here first.

## The whole thing

```rust
use silva_viz_core::{Blob, Claim, FileProbe, View, ViewerFactory};

struct JsonFactory;

impl ViewerFactory for JsonFactory {
    fn id(&self) -> &'static str {
        // Persisted with the open windows, so it must stay stable across
        // releases. Renaming it loses a user's restored layout.
        "json"
    }

    fn claim(&self, probe: &FileProbe<'_>) -> Option<Claim> {
        // You are shown the file's name, size, and first 4 KiB — never the
        // whole thing, so bidding on a 3 GB file costs one read.
        if probe.size() > 16 * 1024 * 1024 || !probe.head_is_utf8() {
            return None;
        }
        let head = String::from_utf8_lossy(probe.head());
        let looks_like_json = head.trim_start().starts_with(['{', '[']);
        if !looks_like_json && probe.ext() != Some("json") {
            // `None` means "not mine". It is the honest answer, and it is what
            // lets the registry fall through to a viewer that can cope.
            return None;
        }
        Some(Claim::new("JSON", 15))
    }

    fn open(&self, blob: Blob) -> Box<dyn View> {
        Box::new(JsonView {
            name: blob.name().to_string(),
            text: blob.read_all().map(|b| String::from_utf8_lossy(&b).into_owned()),
        })
    }
}

struct JsonView {
    name: String,
    text: Result<String, silva_viz_core::SourceError>,
}

impl View for JsonView {
    fn title(&self) -> String {
        format!("{} — JSON", self.name)
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        match &self.text {
            Ok(text) => {
                ui.monospace(text);
            }
            Err(e) => {
                ui.colored_label(ui.visuals().error_fg_color, e.to_string());
            }
        }
    }
}
```

Register it next to the built-ins:

```rust
let mut registry = silva_viz_app::default_registry();
registry.register(Box::new(JsonFactory));
```

## Choosing a priority

Priority only means something relative to the other bids. The scale in use:

| priority | who | why |
| --- | --- | --- |
| `20` | image | recognised from magic bytes, so the evidence is strong |
| `10` | table | a delimiter the first five lines agree on |
| `0` | text | valid UTF-8 under 8 MiB — true of a great many files |
| `-100` | hex | the floor; bids on everything so nothing is un-openable |
| `-200` | metadata | never the default, always available |

Bid above `0` when you recognised the format rather than merely tolerated it.
Ties keep registration order, so two viewers at the same priority stay in a
stable order in the menu rather than swapping between files.

## Reading the file

`Blob` gives you three things:

- `head()` — the bytes `claim` already saw, free.
- `read_all()` — everything, and only ever behind a size guard of your own.
- `read_range(offset, len)` — a window, which is how the hex viewer opens a
  file larger than memory. If your viewer has no ceiling, this is the one to
  use.

There is no way back to the file for writing. That is deliberate.
