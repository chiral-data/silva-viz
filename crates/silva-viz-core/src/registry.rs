// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Which viewers want a file, and which one wins.

use crate::blob::Blob;
use crate::probe::FileProbe;
use crate::view::View;

/// A factory's bid for a file.
pub struct Claim {
    /// What the menu entry says — "Text", "Hex", "Table (3 columns)".
    pub label: String,
    /// Higher wins a double-click. Everything is visible in the "Open in"
    /// menu regardless, so losing a bid means being second, not being absent.
    pub priority: i32,
}

impl Claim {
    pub fn new(label: impl Into<String>, priority: i32) -> Self {
        Self {
            label: label.into(),
            priority,
        }
    }
}

/// Something that can look at a file and offer to display it.
///
/// The two halves are separate on purpose: [`ViewerFactory::claim`] sees only
/// the head, so bidding on a 3 GB file costs one 4 KiB read, and
/// [`ViewerFactory::open`] is called only for the bid the user actually chose.
pub trait ViewerFactory {
    /// Stable across releases: it is persisted with the open windows, and it is
    /// what a caller passes to [`ViewerRegistry::open`].
    fn id(&self) -> &'static str;

    /// `None` means "not mine" — the honest answer for a text viewer shown a
    /// PNG, and what lets the registry fall through to something that can cope.
    fn claim(&self, probe: &FileProbe<'_>) -> Option<Claim>;

    fn open(&self, blob: Blob) -> Box<dyn View>;
}

#[derive(Default)]
pub struct ViewerRegistry {
    factories: Vec<Box<dyn ViewerFactory>>,
}

impl ViewerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, factory: Box<dyn ViewerFactory>) -> &mut Self {
        self.factories.push(factory);
        self
    }

    /// Every bid for `probe`, best first.
    ///
    /// The sort is stable, so factories that bid the same priority stay in
    /// registration order rather than in whatever order the sort felt like —
    /// which is what stops the "Open in" menu reshuffling between files.
    pub fn claims_for(&self, probe: &FileProbe<'_>) -> Vec<(&'static str, Claim)> {
        let mut bids: Vec<_> = self
            .factories
            .iter()
            .filter_map(|f| f.claim(probe).map(|c| (f.id(), c)))
            .collect();
        bids.sort_by_key(|(_, claim)| std::cmp::Reverse(claim.priority));
        bids
    }

    /// What a double-click opens, or `None` if nothing bid at all.
    pub fn best_for(&self, probe: &FileProbe<'_>) -> Option<&'static str> {
        self.claims_for(probe).first().map(|(id, _)| *id)
    }

    /// Opens `blob` in a named viewer, or `None` if no factory has that id.
    pub fn open(&self, id: &str, blob: Blob) -> Option<Box<dyn View>> {
        self.factories
            .iter()
            .find(|f| f.id() == id)
            .map(|f| f.open(blob))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Stub(&'static str, Option<i32>);

    impl ViewerFactory for Stub {
        fn id(&self) -> &'static str {
            self.0
        }
        fn claim(&self, _probe: &FileProbe<'_>) -> Option<Claim> {
            self.1.map(|p| Claim::new(self.0, p))
        }
        fn open(&self, _blob: Blob) -> Box<dyn View> {
            unimplemented!("the tests here never open a view")
        }
    }

    fn registry(stubs: Vec<Stub>) -> ViewerRegistry {
        let mut registry = ViewerRegistry::new();
        for stub in stubs {
            registry.register(Box::new(stub));
        }
        registry
    }

    #[test]
    fn test_the_highest_bid_wins_and_every_bid_is_still_offered() {
        let registry = registry(vec![
            Stub("hex", Some(-100)),
            Stub("image", Some(20)),
            Stub("text", Some(0)),
        ]);
        let probe = FileProbe::new("a.txt", b"", 0);

        assert_eq!(registry.best_for(&probe), Some("image"));
        let ids: Vec<_> = registry
            .claims_for(&probe)
            .into_iter()
            .map(|c| c.0)
            .collect();
        assert_eq!(ids, ["image", "text", "hex"]);
    }

    #[test]
    fn test_a_factory_that_declines_is_absent_from_the_menu() {
        let registry = registry(vec![Stub("hex", Some(-100)), Stub("text", None)]);
        let probe = FileProbe::new("a.bin", b"", 0);

        let ids: Vec<_> = registry
            .claims_for(&probe)
            .into_iter()
            .map(|c| c.0)
            .collect();
        assert_eq!(ids, ["hex"]);
    }

    #[test]
    fn test_equal_bids_keep_registration_order_so_the_menu_does_not_reshuffle() {
        let registry = registry(vec![Stub("first", Some(5)), Stub("second", Some(5))]);
        let probe = FileProbe::new("a.txt", b"", 0);

        let ids: Vec<_> = registry
            .claims_for(&probe)
            .into_iter()
            .map(|c| c.0)
            .collect();
        assert_eq!(ids, ["first", "second"]);
    }

    #[test]
    fn test_a_file_nobody_wants_has_no_best_viewer_rather_than_a_wrong_one() {
        let registry = registry(vec![Stub("text", None)]);
        let probe = FileProbe::new("a.bin", b"", 0);

        assert_eq!(registry.best_for(&probe), None);
        assert!(registry.claims_for(&probe).is_empty());
    }

    #[test]
    fn test_an_empty_registry_offers_nothing_and_does_not_panic() {
        let registry = ViewerRegistry::new();
        let probe = FileProbe::new("a.txt", b"hello", 5);

        assert_eq!(registry.best_for(&probe), None);
    }
}
