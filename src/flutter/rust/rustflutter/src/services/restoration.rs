// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Putting an application back where the reader left it.
//!
//! Upstream's `services/restoration.dart`. When the operating system kills a
//! backgrounded application and the reader returns to it, the platform hands
//! back a blob it was given earlier and the application rebuilds itself around
//! it -- the same tab selected, the same text half-typed, the same list
//! scrolled to the same row.
//!
//! # A tree of buckets, keyed by where a widget is
//!
//! The blob is a tree. Each widget that wants to remember something claims a
//! child bucket from its parent by a **restoration id**, and writes into it.
//! Come back, and the same widget claims the same id and finds what it wrote.
//! Nothing is keyed on object identity, because none of those objects exist any
//! more.
//!
//! Two well-known keys shape every bucket, and they are one character each
//! because the whole tree crosses a channel on every change: `"c"` holds the
//! children and `"v"` holds this bucket's own values.
//!
//! # The platform is the data source, not a dependency of the machinery
//!
//! `coverage_ledger.json` had all twenty-five of upstream's restoration classes
//! down as blocked-engine, on the grounds that the engine and the framework
//! sides were both empty. Only [`RestorationManager`]'s two platform calls are
//! engine work -- the rest, this file included, is a tree over a map. Ported on
//! the same footing as `overlay.rs` was before there was a host: the logic is
//! the part that can be got right in advance.
//!
//! What is not here is the host. Nothing serves `flutter/restoration` in this
//! repository, so a bucket tree built here is never handed over and never comes
//! back. `SystemMouseCursor` sits in exactly that position and is counted the
//! same way.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};

use crate::services::codec::Value;

/// Upstream's `_childrenMapKey`.
pub const CHILDREN_KEY: &str = "c";
/// Upstream's `_valuesMapKey`.
pub const VALUES_KEY: &str = "v";

/// What one bucket holds, before it is wrapped in the bookkeeping.
///
/// A `BTreeMap` rather than a `Vec` of pairs: upstream's is a Dart `Map` whose
/// iteration order does not matter here because the whole thing is serialised
/// by key, and ordering it makes two runs that wrote the same things produce
/// the same blob -- which is what lets a test compare one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BucketData {
    pub values: BTreeMap<String, Value>,
    pub children: BTreeMap<String, BucketData>,
}

impl BucketData {
    /// The `{"c": ..., "v": ...}` form that crosses the channel.
    ///
    /// **An empty map is left out rather than written empty**, which is
    /// upstream's behaviour in `remove` -- it drops the values key once the
    /// last value goes. The blob is sent on every change, so an empty map per
    /// bucket per frame is not free.
    pub fn to_value(&self) -> Value {
        let mut pairs = Vec::new();
        if !self.children.is_empty() {
            pairs.push((
                Value::String(CHILDREN_KEY.into()),
                Value::Map(
                    self.children
                        .iter()
                        .map(|(id, child)| (Value::String(id.clone()), child.to_value()))
                        .collect(),
                ),
            ));
        }
        if !self.values.is_empty() {
            pairs.push((
                Value::String(VALUES_KEY.into()),
                Value::Map(
                    self.values
                        .iter()
                        .map(|(id, value)| (Value::String(id.clone()), value.clone()))
                        .collect(),
                ),
            ));
        }
        Value::Map(pairs)
    }

    /// The inverse: what the platform handed back.
    ///
    /// Anything that is not the shape this writes is read as an empty bucket
    /// rather than refused. Upstream is equally forgiving, and for a good
    /// reason -- the blob may have been written by an older version of the
    /// application, and losing the reader's place is better than refusing to
    /// start.
    pub fn from_value(value: &Value) -> BucketData {
        let Value::Map(pairs) = value else {
            return BucketData::default();
        };
        let mut data = BucketData::default();
        for (key, entry) in pairs {
            let Value::String(key) = key else { continue };
            let Value::Map(entries) = entry else { continue };
            if key == VALUES_KEY {
                for (id, value) in entries {
                    if let Value::String(id) = id {
                        data.values.insert(id.clone(), value.clone());
                    }
                }
            } else if key == CHILDREN_KEY {
                for (id, child) in entries {
                    if let Value::String(id) = id {
                        data.children
                            .insert(id.clone(), BucketData::from_value(child));
                    }
                }
            }
        }
        data
    }
}

/// Upstream `RestorationBucket`: one node of the tree, and the handle a widget
/// holds onto it.
///
/// Shared by handle because a bucket is reached from two directions -- its
/// owner writes to it, and its parent serialises it -- and both must see the
/// same data. Upstream gets that from Dart's object identity.
#[derive(Clone)]
pub struct RestorationBucket {
    inner: Rc<RefCell<BucketInner>>,
}

struct BucketInner {
    restoration_id: String,
    data: BucketData,
    /// Which ids have been handed out this frame. Upstream's
    /// `_claimedChildren`, and the reason claiming an id twice is answered with
    /// an empty bucket rather than the existing one -- see
    /// [`RestorationBucket::claim_child`].
    claimed: Vec<String>,
    /// The live children, so a write below reaches the blob above.
    children: Vec<(String, RestorationBucket)>,
    parent: Option<Weak<RefCell<BucketInner>>>,
    needs_serialization: bool,
}

impl RestorationBucket {
    /// Upstream's `RestorationBucket.empty`: a bucket with nothing in it, for a
    /// widget whose id has never been seen before.
    pub fn empty(restoration_id: impl Into<String>) -> RestorationBucket {
        RestorationBucket {
            inner: Rc::new(RefCell::new(BucketInner {
                restoration_id: restoration_id.into(),
                data: BucketData::default(),
                claimed: Vec::new(),
                children: Vec::new(),
                parent: None,
                needs_serialization: false,
            })),
        }
    }

    /// A root bucket over data the platform handed back.
    pub fn from_data(restoration_id: impl Into<String>, data: BucketData) -> RestorationBucket {
        let bucket = RestorationBucket::empty(restoration_id);
        bucket.inner.borrow_mut().data = data;
        bucket
    }

    pub fn restoration_id(&self) -> String {
        self.inner.borrow().restoration_id.clone()
    }

    /// Upstream's `read<P>`.
    pub fn read(&self, restoration_id: &str) -> Option<Value> {
        self.inner.borrow().data.values.get(restoration_id).cloned()
    }

    pub fn contains(&self, restoration_id: &str) -> bool {
        self.inner.borrow().data.values.contains_key(restoration_id)
    }

    /// Upstream's `write<P>`.
    ///
    /// **Writing the value that is already there marks nothing dirty.** The
    /// whole tree is serialised and sent whenever anything in it changed, so a
    /// widget that rewrites its state every frame -- which is the ordinary case
    /// for one driven by a controller -- would otherwise send the blob sixty
    /// times a second for no change.
    pub fn write(&self, restoration_id: impl Into<String>, value: Value) {
        let restoration_id = restoration_id.into();
        let changed = {
            let inner = &mut *self.inner.borrow_mut();
            match inner.data.values.get(&restoration_id) {
                Some(existing) if *existing == value => false,
                _ => {
                    inner.data.values.insert(restoration_id, value);
                    true
                }
            }
        };
        if changed {
            self.mark_needs_serialization();
        }
    }

    /// Upstream's `remove<P>`.
    ///
    /// Answers what was there. Removing something that was not is not a change
    /// and marks nothing -- upstream tests `containsKey` before removing for
    /// exactly that.
    pub fn remove(&self, restoration_id: &str) -> Option<Value> {
        let removed = self.inner.borrow_mut().data.values.remove(restoration_id);
        if removed.is_some() {
            self.mark_needs_serialization();
        }
        removed
    }

    /// Upstream's `claimChild`, whose three cases are its whole content.
    ///
    /// 1. **The id is already claimed this frame.** Answered with an *empty*
    ///    bucket, not with the existing one. Upstream's reasoning is worth
    ///    keeping: the current owner may give the id up later in the same
    ///    frame -- a list rebuilding its rows claims and releases in whatever
    ///    order the build happens to take -- so the claim is granted in
    ///    anticipation and an assertion at the end of the frame checks that the
    ///    old owner really did surrender it.
    /// 2. **The id has no data.** Also an empty bucket: a widget appearing for
    ///    the first time.
    /// 3. **The id has data and nobody holds it.** A bucket wrapping that data,
    ///    which is the case restoration exists for.
    ///
    /// The first two are the same line upstream, and they are the same line
    /// here, because "somebody else has it" and "there is nothing there" both
    /// mean this claimant starts empty.
    pub fn claim_child(&self, restoration_id: impl Into<String>) -> RestorationBucket {
        let restoration_id = restoration_id.into();
        let already_claimed = self.inner.borrow().claimed.contains(&restoration_id);
        let has_data = self
            .inner
            .borrow()
            .data
            .children
            .contains_key(&restoration_id);

        let child = if already_claimed || !has_data {
            RestorationBucket::empty(restoration_id.clone())
        } else {
            let data = self.inner.borrow().data.children[&restoration_id].clone();
            RestorationBucket::from_data(restoration_id.clone(), data)
        };
        self.adopt_child(&child);
        child
    }

    /// Whether `restoration_id` has been claimed since the last
    /// [`RestorationBucket::finalize`]. Upstream keeps the same set to assert
    /// against at the end of a frame.
    pub fn is_claimed(&self, restoration_id: &str) -> bool {
        self.inner
            .borrow()
            .claimed
            .contains(&restoration_id.to_string())
    }

    /// Upstream's `adoptChild`: `child` becomes one of this bucket's, and its
    /// data becomes part of this bucket's blob.
    pub fn adopt_child(&self, child: &RestorationBucket) {
        let id = child.restoration_id();
        {
            let inner = &mut *self.inner.borrow_mut();
            if !inner.claimed.contains(&id) {
                inner.claimed.push(id.clone());
            }
            inner.children.retain(|(existing, _)| *existing != id);
            inner.children.push((id, child.clone()));
        }
        child.inner.borrow_mut().parent = Some(Rc::downgrade(&self.inner));
        self.mark_needs_serialization();
    }

    /// Upstream's `_dropChild`: the child is no longer part of this blob.
    pub fn drop_child(&self, child: &RestorationBucket) {
        let id = child.restoration_id();
        {
            let inner = &mut *self.inner.borrow_mut();
            inner.children.retain(|(existing, _)| *existing != id);
            inner.claimed.retain(|claimed| *claimed != id);
            inner.data.children.remove(&id);
        }
        child.inner.borrow_mut().parent = None;
        self.mark_needs_serialization();
    }

    /// Upstream's `_markNeedsSerialization`, which walks up.
    ///
    /// A write at a leaf has to reach the root, because the root is what gets
    /// sent. Upstream reaches the manager directly since every bucket holds
    /// one; here the walk is up the parents, which arrives at the same place
    /// without a manager having to exist yet.
    pub fn mark_needs_serialization(&self) {
        let mut at = Some(self.inner.clone());
        while let Some(inner) = at {
            let parent = {
                let mut inner = inner.borrow_mut();
                inner.needs_serialization = true;
                inner.parent.as_ref().and_then(Weak::upgrade)
            };
            at = parent;
        }
    }

    /// Whether anything in this bucket or below it changed since the last
    /// [`RestorationBucket::finalize`].
    pub fn needs_serialization(&self) -> bool {
        self.inner.borrow().needs_serialization
    }

    /// This bucket and everything under it, as data.
    ///
    /// Children are folded in live rather than read from `data.children`, so a
    /// write into a child that was claimed this frame is in the blob without
    /// the child having to push it up.
    pub fn to_data(&self) -> BucketData {
        let inner = self.inner.borrow();
        let mut data = BucketData {
            values: inner.data.values.clone(),
            children: inner.data.children.clone(),
        };
        for (id, child) in &inner.children {
            data.children.insert(id.clone(), child.to_data());
        }
        data
    }

    /// Ends the frame: the claim set is cleared and the dirty flag with it.
    ///
    /// Upstream does this per frame around its integrity assertion. Answers the
    /// ids that were claimed, which is what an assertion would compare.
    pub fn finalize(&self) -> Vec<String> {
        let inner = &mut *self.inner.borrow_mut();
        inner.needs_serialization = false;
        std::mem::take(&mut inner.claimed)
    }
}

impl std::fmt::Debug for RestorationBucket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RestorationBucket({})", self.restoration_id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn int(n: i64) -> Value {
        Value::I64(n)
    }

    // -- Reading and writing --------------------------------------------------------

    #[test]
    fn what_was_written_is_what_comes_back() {
        let bucket = RestorationBucket::empty("root");
        assert!(!bucket.contains("tab"));
        bucket.write("tab", int(2));
        assert_eq!(bucket.read("tab"), Some(int(2)));
        assert!(bucket.contains("tab"));
    }

    #[test]
    fn writing_the_same_value_again_is_not_a_change() {
        // The whole tree is serialised and sent whenever anything changed, so a
        // widget rewriting its state every frame -- the ordinary case for one
        // driven by a controller -- would otherwise send the blob sixty times a
        // second for nothing.
        let bucket = RestorationBucket::empty("root");
        bucket.write("tab", int(2));
        bucket.finalize();
        assert!(!bucket.needs_serialization());

        bucket.write("tab", int(2));
        assert!(!bucket.needs_serialization(), "same value, nothing to send");

        bucket.write("tab", int(3));
        assert!(bucket.needs_serialization());
    }

    #[test]
    fn removing_something_that_was_never_there_is_not_a_change() {
        let bucket = RestorationBucket::empty("root");
        bucket.finalize();
        assert_eq!(bucket.remove("absent"), None);
        assert!(!bucket.needs_serialization());

        bucket.write("tab", int(1));
        bucket.finalize();
        assert_eq!(bucket.remove("tab"), Some(int(1)));
        assert!(bucket.needs_serialization());
    }

    // -- The three claim cases -------------------------------------------------------

    #[test]
    fn claiming_an_id_with_data_gives_back_the_data() {
        // Case 3, and the case restoration exists for: the reader comes back
        // and the widget finds what it wrote.
        let mut data = BucketData::default();
        let mut child = BucketData::default();
        child.values.insert("scroll".into(), int(420));
        data.children.insert("list".into(), child);

        let root = RestorationBucket::from_data("root", data);
        let claimed = root.claim_child("list");
        assert_eq!(claimed.read("scroll"), Some(int(420)));
    }

    #[test]
    fn claiming_an_id_with_no_data_gives_back_an_empty_bucket() {
        // Case 2: a widget appearing for the first time.
        let root = RestorationBucket::empty("root");
        let claimed = root.claim_child("list");
        assert_eq!(claimed.read("scroll"), None);
        assert_eq!(claimed.restoration_id(), "list");
    }

    #[test]
    fn claiming_an_id_twice_gives_the_second_claimant_an_empty_bucket() {
        // Case 1, and it is the surprising one: the second claimant does *not*
        // get the existing bucket. Upstream grants the claim in anticipation of
        // the first owner giving the id up later in the same frame -- a list
        // rebuilding its rows claims and releases in whatever order the build
        // happens to take.
        let mut data = BucketData::default();
        let mut child = BucketData::default();
        child.values.insert("scroll".into(), int(420));
        data.children.insert("list".into(), child);
        let root = RestorationBucket::from_data("root", data);

        let first = root.claim_child("list");
        assert_eq!(first.read("scroll"), Some(int(420)), "case 3");

        let second = root.claim_child("list");
        assert_eq!(
            second.read("scroll"),
            None,
            "case 1: empty, not the same data twice"
        );
    }

    #[test]
    fn a_claim_is_recorded_until_the_frame_ends() {
        // Upstream keeps the set to assert against at the end of a frame: the
        // previous owner has to have surrendered the id by then.
        let root = RestorationBucket::empty("root");
        assert!(!root.is_claimed("list"));
        root.claim_child("list");
        assert!(root.is_claimed("list"));

        assert_eq!(root.finalize(), vec!["list".to_string()]);
        assert!(!root.is_claimed("list"), "a new frame, a fresh set");
    }

    // -- The tree ---------------------------------------------------------------------

    #[test]
    fn a_write_at_a_leaf_reaches_the_root() {
        // The root is what gets sent, so a change anywhere below has to mark it.
        let root = RestorationBucket::empty("root");
        let list = root.claim_child("list");
        let row = list.claim_child("row-3");
        root.finalize();
        list.finalize();
        row.finalize();
        assert!(!root.needs_serialization());

        row.write("expanded", Value::Bool(true));
        assert!(row.needs_serialization());
        assert!(list.needs_serialization(), "and every step up");
        assert!(root.needs_serialization());
    }

    #[test]
    fn a_childs_values_are_in_the_roots_blob() {
        let root = RestorationBucket::empty("root");
        let list = root.claim_child("list");
        list.write("scroll", int(420));

        let data = root.to_data();
        assert_eq!(
            data.children["list"].values["scroll"],
            int(420),
            "folded in live, without the child pushing it up"
        );
    }

    #[test]
    fn a_dropped_child_is_out_of_the_blob() {
        let root = RestorationBucket::empty("root");
        let list = root.claim_child("list");
        list.write("scroll", int(420));
        assert!(root.to_data().children.contains_key("list"));

        root.drop_child(&list);
        assert!(!root.to_data().children.contains_key("list"));
        assert!(
            !root.is_claimed("list"),
            "and the id is free for somebody else this frame"
        );
    }

    #[test]
    fn adopting_a_child_under_an_id_that_is_taken_replaces_it() {
        let root = RestorationBucket::empty("root");
        let first = root.claim_child("slot");
        first.write("who", Value::String("first".into()));

        let second = RestorationBucket::empty("slot");
        second.write("who", Value::String("second".into()));
        root.adopt_child(&second);

        assert_eq!(
            root.to_data().children["slot"].values["who"],
            Value::String("second".into())
        );
    }

    // -- The wire form -----------------------------------------------------------------

    #[test]
    fn the_two_well_known_keys_are_one_character_each() {
        // The whole tree crosses a channel on every change.
        assert_eq!(CHILDREN_KEY, "c");
        assert_eq!(VALUES_KEY, "v");
    }

    #[test]
    fn an_empty_map_is_left_out_rather_than_written_empty() {
        // Upstream drops the values key once the last value goes. A blob sent
        // on every change does not want an empty map per bucket.
        let empty = BucketData::default();
        assert_eq!(empty.to_value(), Value::Map(Vec::new()));

        let mut values_only = BucketData::default();
        values_only.values.insert("a".into(), int(1));
        let Value::Map(pairs) = values_only.to_value() else {
            panic!("a map");
        };
        assert_eq!(pairs.len(), 1, "no children key");
        assert_eq!(pairs[0].0, Value::String(VALUES_KEY.into()));
    }

    #[test]
    fn children_come_before_values_in_the_wire_form() {
        let mut data = BucketData::default();
        data.values.insert("a".into(), int(1));
        data.children.insert("kid".into(), BucketData::default());
        let Value::Map(pairs) = data.to_value() else {
            panic!("a map");
        };
        assert_eq!(pairs[0].0, Value::String(CHILDREN_KEY.into()));
        assert_eq!(pairs[1].0, Value::String(VALUES_KEY.into()));
    }

    #[test]
    fn the_wire_form_round_trips() {
        let mut child = BucketData::default();
        child.values.insert("scroll".into(), int(420));
        let mut data = BucketData::default();
        data.values.insert("tab".into(), int(2));
        data.children.insert("list".into(), child);

        assert_eq!(BucketData::from_value(&data.to_value()), data);
    }

    #[test]
    fn a_blob_in_a_shape_this_does_not_recognise_reads_as_empty() {
        // Upstream is equally forgiving, and for a good reason: the blob may
        // have been written by an older version of the application, and losing
        // the reader's place is better than refusing to start.
        assert_eq!(BucketData::from_value(&Value::Null), BucketData::default());
        assert_eq!(
            BucketData::from_value(&Value::String("nonsense".into())),
            BucketData::default()
        );
        assert_eq!(
            BucketData::from_value(&Value::Map(vec![(
                Value::String("v".into()),
                Value::I64(7)
            )])),
            BucketData::default(),
            "the values key holding something that is not a map"
        );
    }

    #[test]
    fn a_blob_the_platform_handed_back_is_claimable() {
        // The end-to-end shape, minus the platform: decode, claim, read.
        let mut child = BucketData::default();
        child.values.insert("scroll".into(), int(420));
        let mut data = BucketData::default();
        data.children.insert("list".into(), child);
        let wire = data.to_value();

        let root = RestorationBucket::from_data("root", BucketData::from_value(&wire));
        assert_eq!(root.claim_child("list").read("scroll"), Some(int(420)));
    }
}
