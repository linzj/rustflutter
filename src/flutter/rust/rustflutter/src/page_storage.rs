// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Remembering where a page was scrolled to (upstream
//! `widgets/page_storage.dart`).
//!
//! A tab that is scrolled half way down, switched away from and come back to,
//! should be half way down again. Nothing in the widget tree can remember
//! that: the tab's state went when the tab did. The bucket is what outlives
//! it -- a map that sits above the pages and that each page writes its scroll
//! offset into on the way out.
//!
//! # What identifies an entry
//!
//! Not the widget, which is gone, and not its position, which moves. Upstream
//! walks up from the reading context collecting every [`PageStorageKey`] it
//! passes, stopping at the [`PageStorage`], and the list is the identity --
//! so two lists inside two tabs are two entries even though both are "the
//! list", and the same list is one entry across a rebuild even though the
//! widget is new.
//!
//! # Recorded divergences
//!
//! * Upstream collects the keys by walking ancestor elements, which this
//!   crate's [`BuildContext`] cannot do. The chain is handed down instead:
//!   [`PageStorage::scope`] provides the chain its own key extends, and a
//!   reader asks for the nearest one. Same identity, arrived at from the
//!   other end -- and it costs an explicit scope where upstream infers one
//!   from a key on any widget.
//! * Upstream's values are `dynamic`. Here a bucket holds `f64`, which is
//!   what everything that uses one actually stores: a scroll offset. A typed
//!   bucket over `Box<dyn Any>` would be the general version and nothing
//!   would use the generality.

use std::cell::RefCell;
use std::rc::Rc;

use crate::framework::{AnyWidget, BuildContext, provide};

/// Upstream `PageStorageKey`: the key a widget is remembered under.
///
/// A distinct type from an ordinary key because it means something different.
/// An ordinary key tells the framework which old element a new widget
/// corresponds to; this one tells the bucket which saved value belongs to
/// this widget, and the two questions have different answers -- a widget can
/// have the first without wanting the second.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PageStorageKey(pub u64);

/// The chain of [`PageStorageKey`]s between a reader and its
/// [`PageStorage`], which is what identifies an entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageStorageIdentifier {
    keys: Vec<PageStorageKey>,
}

impl PageStorageIdentifier {
    pub fn new(keys: Vec<PageStorageKey>) -> PageStorageIdentifier {
        PageStorageIdentifier { keys }
    }

    /// This chain with one more key at the end.
    pub fn extended(&self, key: PageStorageKey) -> PageStorageIdentifier {
        let mut keys = self.keys.clone();
        keys.push(key);
        PageStorageIdentifier { keys }
    }

    pub fn keys(&self) -> &[PageStorageKey] {
        &self.keys
    }

    /// Upstream's `isNotEmpty`, and the reason it is checked: a reader with
    /// no keys above it has no identity, and writing under the empty chain
    /// would make every such reader share one entry.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

/// Upstream `PageStorageBucket`: the map that outlives the pages.
#[derive(Clone, Default)]
pub struct PageStorageBucket {
    storage: Rc<RefCell<Vec<(PageStorageIdentifier, f64)>>>,
}

impl PageStorageBucket {
    pub fn new() -> PageStorageBucket {
        PageStorageBucket::default()
    }

    /// Upstream `writeState` with an explicit identifier.
    ///
    /// Upstream lets a caller pass one instead of deriving it from the
    /// context, for the case where the thing being remembered is not where
    /// the reading happens.
    pub fn write_state_with(&self, identifier: PageStorageIdentifier, data: f64) {
        if identifier.is_empty() {
            return;
        }
        let mut storage = self.storage.borrow_mut();
        match storage.iter_mut().find(|(at, _)| *at == identifier) {
            Some(entry) => entry.1 = data,
            None => storage.push((identifier, data)),
        }
    }

    /// Upstream `readState` with an explicit identifier.
    pub fn read_state_with(&self, identifier: &PageStorageIdentifier) -> Option<f64> {
        if identifier.is_empty() {
            return None;
        }
        self.storage
            .borrow()
            .iter()
            .find(|(at, _)| at == identifier)
            .map(|(_, data)| *data)
    }

    /// Upstream `writeState`: the identifier comes from the context.
    ///
    /// A context with no [`PageStorage::scope`] above it writes nothing,
    /// which is upstream's `isNotEmpty` guard: an entry with no identity
    /// would be shared by everything that has none.
    pub fn write_state(&self, context: &mut BuildContext, data: f64) {
        self.write_state_with(PageStorage::identifier_of(context), data);
    }

    /// Upstream `readState`.
    pub fn read_state(&self, context: &mut BuildContext) -> Option<f64> {
        self.read_state_with(&PageStorage::identifier_of(context))
    }

    /// How many entries the bucket holds. Upstream's map is private; this is
    /// here because the failure this whole file can have is an identity that
    /// changes when it should not, and the way that shows is one entry
    /// becoming several.
    pub fn len(&self) -> usize {
        self.storage.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.storage.borrow().is_empty()
    }
}

impl PartialEq for PageStorageBucket {
    /// Two buckets are the same bucket when they are the same map. A bucket
    /// is identity, not value: two with equal contents are still two places
    /// to save into.
    fn eq(&self, other: &PageStorageBucket) -> bool {
        Rc::ptr_eq(&self.storage, &other.storage)
    }
}

impl std::fmt::Debug for PageStorageBucket {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PageStorageBucket")
            .field("entries", &self.storage.borrow().len())
            .finish()
    }
}

/// Upstream `PageStorage`: installs a bucket for the subtree below it.
pub struct PageStorage;

impl PageStorage {
    /// Upstream's constructor.
    pub fn new(bucket: PageStorageBucket, child: AnyWidget) -> AnyWidget {
        provide(
            bucket,
            // The chain starts empty at the storage itself, so that a reader
            // between the storage and the first scope has no identity --
            // which is upstream's answer for the same position.
            provide(PageStorageIdentifier::default(), child),
        )
    }

    /// Upstream `PageStorage.maybeOf`.
    pub fn maybe_of(context: &mut BuildContext) -> Option<PageStorageBucket> {
        context
            .inherited::<PageStorageBucket>()
            .map(|bucket| (*bucket).clone())
    }

    /// Upstream `PageStorage.of`, which throws when there is none.
    pub fn of(context: &mut BuildContext) -> PageStorageBucket {
        PageStorage::maybe_of(context)
            .expect("PageStorage::of() was called with a context that has no PageStorage above it")
    }

    /// What identifies a reader at this context: the chain of keys between it
    /// and the storage.
    pub fn identifier_of(context: &mut BuildContext) -> PageStorageIdentifier {
        context
            .inherited::<PageStorageIdentifier>()
            .map(|identifier| (*identifier).clone())
            .unwrap_or_default()
    }

    /// Adds one key to the chain for `child`, which is what upstream gets
    /// from putting a [`PageStorageKey`] on a widget.
    ///
    /// The scope has to be explicit here because the chain is handed down
    /// rather than walked up; see the module's divergences.
    pub fn scope(key: PageStorageKey, child: AnyWidget) -> AnyWidget {
        crate::framework::component(Scope {
            key,
            child: RefCell::new(Some(child)),
        })
    }
}

struct Scope {
    key: PageStorageKey,
    child: RefCell<Option<AnyWidget>>,
}

impl crate::framework::Component for Scope {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let extended = PageStorage::identifier_of(context).extended(self.key);
        provide(
            extended,
            self.child
                .borrow_mut()
                .take()
                .expect("a scope builds its child once"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{Component, ElementTree, component, leaf};
    use crate::widgets::SizedBox;

    /// Reads or writes at wherever it is put, and reports what it saw.
    struct Reader {
        write: Option<f64>,
        seen: Rc<RefCell<Option<Option<f64>>>>,
        identifier: Rc<RefCell<Option<PageStorageIdentifier>>>,
    }

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            let bucket = PageStorage::of(context);
            *self.identifier.borrow_mut() = Some(PageStorage::identifier_of(context));
            if let Some(data) = self.write {
                bucket.write_state(context, data);
            }
            *self.seen.borrow_mut() = Some(bucket.read_state(context));
            leaf(|| SizedBox::new(1.0, 1.0))
        }
    }

    fn reader(
        write: Option<f64>,
    ) -> (
        AnyWidget,
        Rc<RefCell<Option<Option<f64>>>>,
        Rc<RefCell<Option<PageStorageIdentifier>>>,
    ) {
        let seen = Rc::new(RefCell::new(None));
        let identifier = Rc::new(RefCell::new(None));
        (
            component(Reader {
                write,
                seen: Rc::clone(&seen),
                identifier: Rc::clone(&identifier),
            }),
            seen,
            identifier,
        )
    }

    #[test]
    fn what_one_page_wrote_the_same_page_reads_back() {
        let bucket = PageStorageBucket::new();
        let (writer, _, _) = reader(Some(120.0));
        let mut tree = ElementTree::new();
        tree.rebuild(PageStorage::new(
            bucket.clone(),
            PageStorage::scope(PageStorageKey(1), writer),
        ));

        // A fresh tree over the *same* bucket -- which is the whole point:
        // the widgets went and the bucket did not.
        let (rereader, seen, _) = reader(None);
        let mut tree = ElementTree::new();
        tree.rebuild(PageStorage::new(
            bucket.clone(),
            PageStorage::scope(PageStorageKey(1), rereader),
        ));
        assert_eq!(*seen.borrow(), Some(Some(120.0)));
    }

    #[test]
    fn two_pages_with_different_keys_are_two_entries() {
        // The failure this file exists to prevent: two lists in two tabs are
        // both "the list", and they must not share a scroll offset.
        let bucket = PageStorageBucket::new();
        let (first, _, _) = reader(Some(10.0));
        let (second, _, _) = reader(Some(20.0));
        let mut tree = ElementTree::new();
        tree.rebuild(PageStorage::new(
            bucket.clone(),
            crate::framework::many(vec![first, second], |children| {
                let mut flex = crate::render::RenderFlex::column();
                for child in children {
                    flex = flex.push(child);
                }
                flex
            }),
        ));
        // Both wrote at the empty chain, so nothing was stored at all.
        assert_eq!(bucket.len(), 0);

        // With a scope each, they are two entries.
        let bucket = PageStorageBucket::new();
        let (first, _, _) = reader(Some(10.0));
        let (second, _, _) = reader(Some(20.0));
        let mut tree = ElementTree::new();
        tree.rebuild(PageStorage::new(
            bucket.clone(),
            crate::framework::many(
                vec![
                    PageStorage::scope(PageStorageKey(1), first),
                    PageStorage::scope(PageStorageKey(2), second),
                ],
                |children| {
                    let mut flex = crate::render::RenderFlex::column();
                    for child in children {
                        flex = flex.push(child);
                    }
                    flex
                },
            ),
        ));
        assert_eq!(bucket.len(), 2);
    }

    #[test]
    fn a_reader_with_no_scope_above_it_has_no_identity_and_stores_nothing() {
        // Upstream's `isNotEmpty` guard. Without it every keyless reader
        // shares one entry, so two lists on one page would fight over one
        // scroll offset.
        let bucket = PageStorageBucket::new();
        let (writer, seen, identifier) = reader(Some(99.0));
        let mut tree = ElementTree::new();
        tree.rebuild(PageStorage::new(bucket.clone(), writer));
        assert!(identifier.borrow().as_ref().expect("built").is_empty());
        assert_eq!(bucket.len(), 0);
        assert_eq!(*seen.borrow(), Some(None));
    }

    #[test]
    fn nesting_scopes_makes_a_longer_chain() {
        // The identity is the whole path, not the innermost key: the same
        // list inside two different tabs is two entries because the tab's key
        // is above it.
        let bucket = PageStorageBucket::new();
        let (inner, _, identifier) = reader(Some(7.0));
        let mut tree = ElementTree::new();
        tree.rebuild(PageStorage::new(
            bucket.clone(),
            PageStorage::scope(
                PageStorageKey(1),
                PageStorage::scope(PageStorageKey(2), inner),
            ),
        ));
        assert_eq!(
            identifier.borrow().as_ref().expect("built").keys(),
            &[PageStorageKey(1), PageStorageKey(2)]
        );
    }

    #[test]
    fn the_same_key_under_a_different_parent_is_a_different_entry() {
        let bucket = PageStorageBucket::new();
        for tab in [1u64, 2] {
            let (list, _, _) = reader(Some(tab as f64 * 100.0));
            let mut tree = ElementTree::new();
            tree.rebuild(PageStorage::new(
                bucket.clone(),
                PageStorage::scope(
                    PageStorageKey(tab),
                    // The same inner key in both tabs, which is exactly what
                    // "the list" would be.
                    PageStorage::scope(PageStorageKey(9), list),
                ),
            ));
        }
        assert_eq!(bucket.len(), 2, "one entry per tab, not one shared");
        assert_eq!(
            bucket.read_state_with(&PageStorageIdentifier::new(vec![
                PageStorageKey(1),
                PageStorageKey(9)
            ])),
            Some(100.0)
        );
        assert_eq!(
            bucket.read_state_with(&PageStorageIdentifier::new(vec![
                PageStorageKey(2),
                PageStorageKey(9)
            ])),
            Some(200.0)
        );
    }

    #[test]
    fn writing_the_same_identity_twice_replaces_rather_than_appends() {
        // A page writes its offset on every scroll, so an appending bucket
        // would grow without bound and read back the first value forever.
        let bucket = PageStorageBucket::new();
        let identifier = PageStorageIdentifier::new(vec![PageStorageKey(1)]);
        bucket.write_state_with(identifier.clone(), 1.0);
        bucket.write_state_with(identifier.clone(), 2.0);
        assert_eq!(bucket.len(), 1);
        assert_eq!(bucket.read_state_with(&identifier), Some(2.0));
    }

    #[test]
    fn two_buckets_are_the_same_bucket_only_when_they_are_the_same_map() {
        // A bucket is identity and not value: two with equal contents are
        // still two places to save into, and treating them as equal would
        // make an unrelated page's storage answer for this one.
        let bucket = PageStorageBucket::new();
        assert_eq!(bucket, bucket.clone());
        assert_ne!(bucket, PageStorageBucket::new());
    }
}
