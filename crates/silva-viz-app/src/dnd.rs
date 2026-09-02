// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Files dragged onto the window.
//!
//! egui hands native and web builds different halves of the same struct — a
//! path on the desktop, bytes in a browser, because a browser will not disclose
//! a path. Normalising that here is what keeps the `cfg` out of the app.

/// One dropped item, in whichever form the platform could provide.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Dropped {
    /// A path, which may be a directory. Native only.
    Path(String),
    /// Contents, because there is no path to be had. Web only.
    Bytes { name: String, bytes: Vec<u8> },
}

/// What was dropped on this frame.
pub fn take(ctx: &egui::Context) -> Vec<Dropped> {
    ctx.input(|i| i.raw.dropped_files.iter().map(convert).collect::<Vec<_>>())
        .into_iter()
        .flatten()
        .collect()
}

fn convert(file: &egui::DroppedFile) -> Option<Dropped> {
    if let Some(path) = &file.path {
        return Some(Dropped::Path(path.to_string_lossy().into_owned()));
    }
    let bytes = file.bytes.as_ref()?;
    Some(Dropped::Bytes {
        // egui leaves `name` empty for some sources; an unnamed file would
        // otherwise become an unlabelled row in the browser.
        name: if file.name.is_empty() {
            "(dropped)".to_string()
        } else {
            file.name.clone()
        },
        bytes: bytes.to_vec(),
    })
}

/// Whether anything is currently hovering over the window, so the app can say
/// that dropping is possible before the drop happens.
pub fn hovering(ctx: &egui::Context) -> bool {
    ctx.input(|i| !i.raw.hovered_files.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_a_native_drop_is_read_as_a_path() {
        let file = egui::DroppedFile {
            path: Some("/tmp/a.txt".into()),
            ..Default::default()
        };
        assert_eq!(convert(&file), Some(Dropped::Path("/tmp/a.txt".into())));
    }

    #[test]
    fn test_a_web_drop_is_read_as_bytes_under_its_name() {
        let file = egui::DroppedFile {
            name: "a.txt".into(),
            bytes: Some(Arc::from(&b"hi"[..])),
            ..Default::default()
        };
        assert_eq!(
            convert(&file),
            Some(Dropped::Bytes {
                name: "a.txt".into(),
                bytes: b"hi".to_vec()
            })
        );
    }

    #[test]
    fn test_a_drop_with_neither_a_path_nor_bytes_is_ignored_rather_than_shown() {
        assert_eq!(convert(&egui::DroppedFile::default()), None);
    }

    #[test]
    fn test_an_unnamed_drop_still_gets_a_label() {
        let file = egui::DroppedFile {
            bytes: Some(Arc::from(&b"hi"[..])),
            ..Default::default()
        };
        let Some(Dropped::Bytes { name, .. }) = convert(&file) else {
            panic!("bytes should have been read");
        };
        assert!(!name.is_empty());
    }
}
