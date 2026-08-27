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

use crate::framework::{AnyWidget, BuildContext, Component};
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

impl RestorationBucket {
    /// Whether this and `other` are the same bucket.
    ///
    /// Identity, because a bucket is a handle and two handles onto one node are
    /// the same node. This is what [`UnmanagedRestorationScope`]'s equality
    /// asks, and it is deliberately not a comparison of contents -- see there.
    pub fn is(&self, other: &RestorationBucket) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl std::fmt::Debug for RestorationBucket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RestorationBucket({})", self.restoration_id())
    }
}

// -- The values a widget remembers --------------------------------------------

/// Upstream `RestorableProperty<T>`: one thing a widget wants back.
///
/// Four members, and between them they are the whole contract with the bucket:
/// what to be when there is nothing stored, how to read what was stored, how to
/// write it, and whether to bother at all.
///
/// A trait here rather than a class hierarchy, because upstream's four levels
/// (`RestorableProperty` → `RestorableValue` → `_RestorablePrimitiveValue{,N}`
/// → the concrete ones) exist to share Dart field declarations, and the sharing
/// is what a generic does in Rust. [`Restorable`] is that generic and the
/// concrete names are aliases of it.
pub trait RestorableProperty {
    /// What is stored, once decoded.
    type Value;

    /// Upstream's `createDefaultValue`: what to be when the bucket has nothing.
    fn default_value(&self) -> Self::Value;

    /// Upstream's `fromPrimitives`: what the bucket held, as a value.
    fn from_primitives(&self, data: &Value) -> Self::Value;

    /// Upstream's `toPrimitives`: this value, as something the bucket can hold.
    fn to_primitives(&self) -> Value;

    /// Upstream's `enabled`, true by default.
    ///
    /// A property that answers false is **not written to the bucket at all** --
    /// not written as null, not left at its old value. What it is for is a
    /// widget whose state is only worth remembering under some condition; a
    /// text field inside a form that has not been touched has nothing to say,
    /// and saying nothing keeps it out of a blob that crosses a channel.
    fn enabled(&self) -> bool {
        true
    }
}

/// How a value converts to and from what a bucket can hold.
///
/// The seam between [`Restorable`] and the primitive types. Upstream gets this
/// from Dart's dynamic typing -- `serialized as T` -- which is one cast and no
/// declaration; here each kind says how it crosses.
pub trait RestorableCodec: Sized {
    fn encode(&self) -> Value;
    /// `None` when the stored form is not this kind, which is what an
    /// application updated since the blob was written will see.
    fn decode(data: &Value) -> Option<Self>;
}

impl RestorableCodec for i64 {
    fn encode(&self) -> Value {
        Value::I64(*self)
    }
    fn decode(data: &Value) -> Option<i64> {
        match data {
            Value::I64(n) => Some(*n),
            // The standard codec narrows small integers to 32 bits, so a value
            // written as an `int` may come back either way. Upstream's `as int`
            // does not care because a Dart `int` has no width.
            Value::I32(n) => Some(*n as i64),
            _ => None,
        }
    }
}

impl RestorableCodec for f64 {
    fn encode(&self) -> Value {
        Value::F64(*self)
    }
    fn decode(data: &Value) -> Option<f64> {
        match data {
            Value::F64(n) => Some(*n),
            // A whole number written as a double may come back as an integer.
            Value::I64(n) => Some(*n as f64),
            Value::I32(n) => Some(*n as f64),
            _ => None,
        }
    }
}

impl RestorableCodec for bool {
    fn encode(&self) -> Value {
        Value::Bool(*self)
    }
    fn decode(data: &Value) -> Option<bool> {
        match data {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

impl RestorableCodec for String {
    fn encode(&self) -> Value {
        Value::String(self.clone())
    }
    fn decode(data: &Value) -> Option<String> {
        match data {
            Value::String(s) => Some(s.clone()),
            _ => None,
        }
    }
}

/// Upstream `RestorableValue<T>` and the two primitive bases under it.
///
/// # The setter fires only on a change
///
/// Upstream's `set value` compares before assigning and calls
/// `didUpdateValue` only if it differs. That is the same rule
/// [`RestorationBucket::write`] has one level down, and it is here for the same
/// reason and one more: the listeners are what rebuild the widget, so a
/// property reassigned to what it already held would rebuild for nothing.
#[derive(Clone, Debug)]
pub struct Restorable<T> {
    value: T,
    default_value: T,
    enabled: bool,
}

impl<T: Clone + PartialEq + RestorableCodec> Restorable<T> {
    /// Upstream's constructors, which all take the default value.
    pub fn new(default_value: T) -> Restorable<T> {
        Restorable {
            value: default_value.clone(),
            default_value,
            enabled: true,
        }
    }

    pub fn value(&self) -> &T {
        &self.value
    }

    /// Upstream's `set value`. Answers whether anything changed, which is what
    /// upstream signals by calling `notifyListeners`.
    pub fn set(&mut self, value: T) -> bool {
        if self.value == value {
            return false;
        }
        self.value = value;
        true
    }

    /// Upstream's `initWithValue`, which assigns **without** the change check
    /// and without notifying: this is the bucket handing back what it stored,
    /// not the application changing its mind.
    pub fn init_with_value(&mut self, value: T) {
        self.value = value;
    }

    /// Turns writing off. See [`RestorableProperty::enabled`].
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Reads this property out of `bucket`, or takes the default when it holds
    /// nothing this can read.
    ///
    /// **A stored value of the wrong kind is treated as absent**, which is what
    /// an application updated since the blob was written will meet. Upstream's
    /// `serialized as T` would throw; the honest end of that in Rust is the
    /// default, because the alternative is refusing to start over a value the
    /// reader does not know exists.
    pub fn restore(&mut self, bucket: &RestorationBucket, restoration_id: &str) {
        let stored = bucket.read(restoration_id);
        self.value = match stored {
            Some(data) => self.from_primitives(&data),
            None => self.default_value(),
        };
    }

    /// Writes this property into `bucket`, if it is enabled.
    pub fn save(&self, bucket: &RestorationBucket, restoration_id: &str) {
        if self.enabled {
            bucket.write(restoration_id, self.to_primitives());
        }
    }
}

impl<T: Clone + PartialEq + RestorableCodec> RestorableProperty for Restorable<T> {
    type Value = T;

    fn default_value(&self) -> T {
        self.default_value.clone()
    }

    fn from_primitives(&self, data: &Value) -> T {
        T::decode(data).unwrap_or_else(|| self.default_value.clone())
    }

    fn to_primitives(&self) -> Value {
        self.value.encode()
    }

    fn enabled(&self) -> bool {
        self.enabled
    }
}

/// Upstream `RestorableValue<T>`.
///
/// Upstream's is the abstract middle of a four-level hierarchy: it adds the
/// held value and the change-checking setter to [`RestorableProperty`], and
/// leaves `didUpdateValue` for a subclass. [`Restorable`] is that layer, and is
/// concrete because the only thing the subclasses were varying is how the value
/// converts -- which is [`RestorableCodec`] here, a separate trait rather than
/// another rung.
///
/// An alias rather than a ledger entry, so that a reader who comes looking for
/// upstream's name lands on the thing that plays its part.
pub type RestorableValue<T> = Restorable<T>;

/// Upstream `RestorableInt`.
pub type RestorableInt = Restorable<i64>;
/// Upstream `RestorableDouble`.
pub type RestorableDouble = Restorable<f64>;
/// Upstream `RestorableBool`.
pub type RestorableBool = Restorable<bool>;
/// Upstream `RestorableString`.
pub type RestorableString = Restorable<String>;
/// Upstream `RestorableNum<T>`, whose `T extends num`.
///
/// Dart's `num` is the supertype of `int` and `double`, and a Rust alias cannot
/// be generic over "either of two concrete types". `f64` is the one that can
/// hold both without loss for the range restoration deals in, so this is that.
pub type RestorableNum = Restorable<f64>;

/// The nullable half of the family: upstream's `RestorableIntN` and friends,
/// built on `_RestorablePrimitiveValueN`.
///
/// Upstream needs two class hierarchies for this because Dart's nullability is
/// in the type parameter and the non-nullable setter has to be narrowed.
/// `Option<T>` needs neither.
impl<T: Clone + PartialEq + RestorableCodec> RestorableCodec for Option<T> {
    fn encode(&self) -> Value {
        match self {
            Some(value) => value.encode(),
            None => Value::Null,
        }
    }

    fn decode(data: &Value) -> Option<Option<T>> {
        match data {
            Value::Null => Some(None),
            other => T::decode(other).map(Some),
        }
    }
}

/// Upstream `RestorableIntN`.
pub type RestorableIntN = Restorable<Option<i64>>;
/// Upstream `RestorableDoubleN`.
pub type RestorableDoubleN = Restorable<Option<f64>>;
/// Upstream `RestorableBoolN`.
pub type RestorableBoolN = Restorable<Option<bool>>;
/// Upstream `RestorableStringN`.
pub type RestorableStringN = Restorable<Option<String>>;
/// Upstream `RestorableNumN`.
pub type RestorableNumN = Restorable<Option<f64>>;

/// Upstream `RestorableDateTime`: a moment, stored as milliseconds since the
/// epoch.
///
/// Upstream's `toPrimitives` is `value.millisecondsSinceEpoch` and its
/// `fromPrimitives` is `DateTime.fromMillisecondsSinceEpoch(data as int)`. This
/// crate has no `DateTime`, so the millisecond count *is* the type -- which is
/// what crosses the channel either way, and what a caller would have had to
/// convert to.
pub type RestorableDateTime = Restorable<i64>;
/// Upstream `RestorableDateTimeN`.
pub type RestorableDateTimeN = Restorable<Option<i64>>;

/// Upstream `RestorableEnum<T>`: one of a known set, stored by **name**.
///
/// # Stored by name, and the set is checked on the way back
///
/// Upstream stores `value.name` rather than the index, and the reason shows up
/// on the way back: an application whose enum gained a value in the middle
/// would restore every stored index to the wrong member. A name survives
/// reordering.
///
/// What a name does not survive is being *removed*. Upstream's `fromPrimitives`
/// walks the allowed set and, finding no match, asserts in debug and answers
/// the default in release -- so an application that dropped an enum value
/// restores to its default rather than to nothing. Kept, minus the assert:
/// there is no "debug only" behaviour to hang it on here, and answering the
/// default is what actually happens in a shipped build either way.
#[derive(Clone, Debug)]
pub struct RestorableEnum {
    value: String,
    default_value: String,
    /// The names this may hold. Upstream takes the enum's `values`.
    allowed: Vec<String>,
    enabled: bool,
}

impl RestorableEnum {
    /// Upstream asserts the default is in the set, and it is worth keeping as
    /// one: a default outside the set is a value the property would restore to
    /// and then refuse to be set to.
    pub fn new(default_value: impl Into<String>, allowed: Vec<String>) -> RestorableEnum {
        let default_value = default_value.into();
        debug_assert!(
            allowed.contains(&default_value),
            "a default outside the allowed set is a value this can restore to and not be set to"
        );
        RestorableEnum {
            value: default_value.clone(),
            default_value,
            allowed,
            enabled: true,
        }
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    /// Upstream asserts the new value is in the set. Answers whether it was
    /// taken, so a caller can tell a refusal from a no-op.
    pub fn set(&mut self, value: impl Into<String>) -> bool {
        let value = value.into();
        debug_assert!(
            self.allowed.contains(&value),
            "an enum value outside the allowed set"
        );
        if !self.allowed.contains(&value) || self.value == value {
            return false;
        }
        self.value = value;
        true
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn restore(&mut self, bucket: &RestorationBucket, restoration_id: &str) {
        self.value = match bucket.read(restoration_id) {
            Some(data) => self.from_primitives(&data),
            None => self.default_value.clone(),
        };
    }

    pub fn save(&self, bucket: &RestorationBucket, restoration_id: &str) {
        if self.enabled {
            bucket.write(restoration_id, self.to_primitives());
        }
    }
}

impl RestorableProperty for RestorableEnum {
    type Value = String;

    fn default_value(&self) -> String {
        self.default_value.clone()
    }

    fn from_primitives(&self, data: &Value) -> String {
        match data {
            Value::String(name) if self.allowed.contains(name) => name.clone(),
            // A name the application no longer has: the default, not nothing.
            _ => self.default_value.clone(),
        }
    }

    fn to_primitives(&self) -> Value {
        Value::String(self.value.clone())
    }

    fn enabled(&self) -> bool {
        self.enabled
    }
}

/// Upstream `RestorableEnumN`: the same, and `None` is a value it may hold.
///
/// Upstream's `fromPrimitives` answers `null` for stored null **before** it
/// consults the allowed set, so a null survives whatever the set says.
#[derive(Clone, Debug)]
pub struct RestorableEnumN {
    inner: RestorableEnum,
    is_null: bool,
}

impl RestorableEnumN {
    pub fn new(default_value: Option<String>, allowed: Vec<String>) -> RestorableEnumN {
        let is_null = default_value.is_none();
        let seed = default_value.unwrap_or_else(|| {
            allowed
                .first()
                .cloned()
                .unwrap_or_else(|| String::from("<none>"))
        });
        RestorableEnumN {
            // The seed only matters when this is not null, and `new`'s assert
            // would otherwise fire on an empty allowed set with a null default.
            inner: RestorableEnum {
                value: seed.clone(),
                default_value: seed,
                allowed,
                enabled: true,
            },
            is_null,
        }
    }

    pub fn value(&self) -> Option<&str> {
        if self.is_null {
            None
        } else {
            Some(self.inner.value())
        }
    }

    pub fn set(&mut self, value: Option<String>) -> bool {
        match value {
            None => {
                if self.is_null {
                    return false;
                }
                self.is_null = true;
                true
            }
            Some(value) => {
                let was_null = self.is_null;
                let changed = self.inner.set(value);
                if changed || was_null {
                    self.is_null = false;
                }
                changed || was_null
            }
        }
    }

    pub fn restore(&mut self, bucket: &RestorationBucket, restoration_id: &str) {
        match bucket.read(restoration_id) {
            // Null first, before the set is consulted.
            Some(Value::Null) | None => self.is_null = true,
            Some(data) => {
                self.inner.value = self.inner.from_primitives(&data);
                self.is_null = false;
            }
        }
    }

    pub fn save(&self, bucket: &RestorationBucket, restoration_id: &str) {
        if !self.inner.enabled {
            return;
        }
        bucket.write(
            restoration_id,
            match self.value() {
                Some(name) => Value::String(name.to_string()),
                None => Value::Null,
            },
        );
    }
}

/// Upstream `RestorableListenable<T>`: a property whose value is an object that
/// announces its own changes.
///
/// The difference from [`Restorable`] is where the notification comes from --
/// upstream's listens to the held object and republishes, rather than firing on
/// assignment. What it holds is not serialised by this type; a subclass says
/// how.
///
/// Modelled here as the callback seam, because this crate has no
/// `ChangeNotifier` base for a value to extend: a caller hands over what to do
/// when the held thing changed, and [`RestorableListenable::changed`] is what
/// the held thing calls.
pub struct RestorableListenable {
    on_change: Option<Rc<dyn Fn()>>,
}

impl RestorableListenable {
    pub fn new() -> RestorableListenable {
        RestorableListenable { on_change: None }
    }

    pub fn with_on_change(mut self, on_change: impl Fn() + 'static) -> Self {
        self.on_change = Some(Rc::new(on_change));
        self
    }

    /// What the held object calls. Upstream's republished notification.
    pub fn changed(&self) {
        if let Some(on_change) = &self.on_change {
            on_change();
        }
    }
}

impl Default for RestorableListenable {
    fn default() -> RestorableListenable {
        RestorableListenable::new()
    }
}

/// Upstream `RestorableChangeNotifier<T>`: a [`RestorableListenable`] that also
/// owns what it holds, and disposes it.
///
/// Upstream's addition over its parent is the lifetime: it creates the notifier
/// in `initWithValue` and disposes the old one when a new value replaces it, so
/// a widget restoring twice does not leak the first. There is nothing to leak
/// here -- a replaced value is dropped -- so what survives is the shape.
pub struct RestorableChangeNotifier {
    listenable: RestorableListenable,
}

impl RestorableChangeNotifier {
    pub fn new() -> RestorableChangeNotifier {
        RestorableChangeNotifier {
            listenable: RestorableListenable::new(),
        }
    }

    pub fn with_on_change(mut self, on_change: impl Fn() + 'static) -> Self {
        self.listenable = self.listenable.with_on_change(on_change);
        self
    }

    pub fn changed(&self) {
        self.listenable.changed();
    }
}

impl Default for RestorableChangeNotifier {
    fn default() -> RestorableChangeNotifier {
        RestorableChangeNotifier::new()
    }
}

/// Upstream `RestorableTextEditingController`: a text field's contents and
/// selection, remembered.
///
/// Upstream stores **the text alone**, not the selection -- `toPrimitives` is
/// `value.text` and `fromPrimitives` builds a fresh controller from it. So a
/// restored field has the reader's words back and the caret at the end, which
/// is upstream's judgement: the words are what was lost and the caret is where
/// a reader would put it anyway.
#[derive(Clone, Debug)]
pub struct RestorableTextEditingController {
    text: Restorable<String>,
}

impl RestorableTextEditingController {
    pub fn new(text: impl Into<String>) -> RestorableTextEditingController {
        RestorableTextEditingController {
            text: Restorable::new(text.into()),
        }
    }

    pub fn text(&self) -> &str {
        self.text.value()
    }

    pub fn set_text(&mut self, text: impl Into<String>) -> bool {
        self.text.set(text.into())
    }

    /// Where the caret lands on a restore: the end of the text, because the
    /// selection is not stored.
    pub fn restored_selection(&self) -> usize {
        self.text.value().len()
    }

    pub fn restore(&mut self, bucket: &RestorationBucket, restoration_id: &str) {
        self.text.restore(bucket, restoration_id);
    }

    pub fn save(&self, bucket: &RestorationBucket, restoration_id: &str) {
        self.text.save(bucket, restoration_id);
    }
}

// -- Where a widget finds its bucket ------------------------------------------

/// Upstream `UnmanagedRestorationScope`: the bucket, published to the subtree.
///
/// "Unmanaged" because it does not claim or release anything -- it carries a
/// bucket somebody else is looking after. [`RestorationScope`] is the managed
/// one, and it is built out of this.
///
/// The bucket is optional and the `None` is meaningful: restoration is off, or
/// the root has not arrived from the platform yet, and a widget below finds
/// nothing to write into rather than a bucket that goes nowhere.
#[derive(Clone, Debug)]
pub struct UnmanagedRestorationScope {
    pub bucket: Option<RestorationBucket>,
}

impl UnmanagedRestorationScope {
    pub fn new(bucket: Option<RestorationBucket>) -> UnmanagedRestorationScope {
        UnmanagedRestorationScope { bucket }
    }
}

/// Upstream's `updateShouldNotify`, which is `oldWidget.bucket != bucket`.
///
/// **Identity, not contents.** A bucket whose values changed is the same bucket
/// and the subtree does not need rebuilding for it -- the properties that care
/// are listening to themselves. What a rebuild is for is the bucket being
/// *replaced*, which is restoration arriving or going away.
impl PartialEq for UnmanagedRestorationScope {
    fn eq(&self, other: &UnmanagedRestorationScope) -> bool {
        match (&self.bucket, &other.bucket) {
            (None, None) => true,
            (Some(mine), Some(theirs)) => mine.is(theirs),
            _ => false,
        }
    }
}

/// Upstream `RestorationScope`: claims a child bucket and publishes it.
///
/// A widget wanting to remember something looks up the nearest one of these,
/// claims a child of *its* bucket, and writes there -- so the tree of buckets
/// mirrors the tree of scopes, and a widget's place in the blob is its place on
/// screen.
///
/// # A null id turns it off for the subtree
///
/// Upstream's `restorationId` is nullable, and a null one publishes no bucket
/// rather than claiming one called "null". That is how a subtree opts out:
/// everything below finds nothing to write into and quietly does not remember.
pub struct RestorationScope {
    restoration_id: Option<String>,
    child: RefCell<Option<AnyWidget>>,
}

impl RestorationScope {
    pub fn new(restoration_id: impl Into<String>, child: AnyWidget) -> RestorationScope {
        RestorationScope {
            restoration_id: Some(restoration_id.into()),
            child: RefCell::new(Some(child)),
        }
    }

    /// A scope that publishes nothing, turning restoration off below it.
    pub fn disabled(child: AnyWidget) -> RestorationScope {
        RestorationScope {
            restoration_id: None,
            child: RefCell::new(Some(child)),
        }
    }

    /// Upstream's `RestorationScope.maybeOf`: the bucket in scope, if there is
    /// one.
    pub fn maybe_of(context: &mut BuildContext) -> Option<RestorationBucket> {
        context
            .inherited::<UnmanagedRestorationScope>()
            .and_then(|scope| scope.bucket.clone())
    }

    /// Upstream's `RestorationScope.of`, which asserts.
    ///
    /// Upstream's error names the fix rather than the fault -- "state
    /// restoration must be enabled for a RestorationScope to exist... by
    /// passing a restorationScopeId to MaterialApp... or by wrapping the widget
    /// tree in a RootRestorationScope". Answering `None` here and letting the
    /// caller decide, because a missing scope is the ordinary state of an
    /// application that does not restore, and this crate has no host that does.
    pub fn of(context: &mut BuildContext) -> Option<RestorationBucket> {
        RestorationScope::maybe_of(context)
    }
}

impl Component for RestorationScope {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let parent = RestorationScope::maybe_of(context);
        let child = self
            .child
            .borrow()
            .clone()
            .unwrap_or_else(|| crate::framework::leaf(|| crate::widgets::Empty));

        // A null id, or no parent bucket to claim from, and nothing is
        // published -- the subtree finds no bucket and does not remember.
        let bucket = match (&self.restoration_id, parent) {
            (Some(id), Some(parent)) => Some(parent.claim_child(id.clone())),
            _ => None,
        };
        crate::framework::provide(UnmanagedRestorationScope::new(bucket), child)
    }
}

/// Upstream `RootRestorationScope`: the top of the tree, which waits.
///
/// The root bucket does not exist until the platform hands it over, and
/// upstream's scope **builds nothing until it arrives** -- its state holds the
/// child back rather than showing a subtree that would claim buckets it then
/// had to throw away.
///
/// That wait is the whole difference from [`RestorationScope`], and it is why
/// this is a separate class rather than the same one at the top.
pub struct RootRestorationScope {
    restoration_id: Option<String>,
    root: Option<RestorationBucket>,
    child: RefCell<Option<AnyWidget>>,
}

impl RootRestorationScope {
    pub fn new(
        restoration_id: impl Into<String>,
        root: Option<RestorationBucket>,
        child: AnyWidget,
    ) -> RootRestorationScope {
        RootRestorationScope {
            restoration_id: Some(restoration_id.into()),
            root,
            child: RefCell::new(Some(child)),
        }
    }

    /// Whether the root has arrived. Upstream's `_okToRenderBlankContainer`
    /// inverted: while this is false, upstream renders an empty box.
    pub fn is_ready(&self) -> bool {
        self.restoration_id.is_none() || self.root.is_some()
    }
}

impl Component for RootRestorationScope {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        let child = self
            .child
            .borrow()
            .clone()
            .unwrap_or_else(|| crate::framework::leaf(|| crate::widgets::Empty));
        if !self.is_ready() {
            // Upstream's blank container. Not the child without a bucket: a
            // subtree built now would claim buckets and have to give them up
            // the moment the real root landed.
            return crate::framework::leaf(|| crate::widgets::Empty);
        }
        crate::framework::provide(UnmanagedRestorationScope::new(self.root.clone()), child)
    }
}

/// Upstream `RestorationMixin`: what a widget's state uses to register its
/// properties.
///
/// Upstream is a mixin on `State` because that is where the lifecycle hooks
/// are. This crate's state is a plain value owned by the element tree, so this
/// is a value the state holds -- and the hooks it needs (`didUpdateWidget`,
/// `dispose`) are the two `State` members the ledger records as absent, so the
/// owner drives it explicitly.
#[derive(Default)]
pub struct RestorationMixin {
    bucket: Option<RestorationBucket>,
    /// Which ids are taken, so the two assertions below have something to
    /// check. Upstream keeps the properties themselves; the ids are what the
    /// assertions actually compare.
    registered: Vec<String>,
}

impl RestorationMixin {
    pub fn new() -> RestorationMixin {
        RestorationMixin::default()
    }

    /// The bucket this state writes into, once it has one.
    pub fn bucket(&self) -> Option<&RestorationBucket> {
        self.bucket.as_ref()
    }

    /// Upstream's `didToggleBucket` path: the state is given a bucket, or given
    /// a different one, or given none.
    ///
    /// Answers whether this is the **initial** restore -- upstream passes that
    /// to `restoreState` as `initialRestore`, and it is the difference between
    /// "you are being set up" and "the platform replaced your data underneath
    /// you", which a widget with a controller has to handle differently.
    pub fn set_bucket(&mut self, bucket: Option<RestorationBucket>) -> bool {
        let initial = self.bucket.is_none();
        self.bucket = bucket;
        self.registered.clear();
        initial
    }

    /// Upstream's `registerForRestoration`.
    ///
    /// **The stored value wins over the default**, which is the whole of
    /// restoration in one line: `hasSerializedValue ? fromPrimitives(...) :
    /// createDefaultValue()`. The property is then written back, so a value
    /// that came from the default is in the blob for next time.
    ///
    /// Upstream asserts twice, and both are about the same mistake from two
    /// sides -- a property registered under two ids, and an id used by two
    /// properties. Either would make one of them silently overwrite the other,
    /// which the reader would see as a control that does not come back.
    pub fn register_for_restoration<T>(
        &mut self,
        property: &mut Restorable<T>,
        restoration_id: impl Into<String>,
    ) where
        T: Clone + PartialEq + RestorableCodec,
    {
        let restoration_id = restoration_id.into();
        debug_assert!(
            !self.registered.contains(&restoration_id),
            "\"{restoration_id}\" is already registered to another property"
        );
        let Some(bucket) = &self.bucket else {
            // No bucket: the property keeps its default and nothing is stored.
            return;
        };
        property.restore(bucket, &restoration_id);
        property.save(bucket, &restoration_id);
        self.registered.push(restoration_id);
    }

    /// Upstream's `unregisterFromRestoration`: the property stops being written
    /// **and its stored value is removed**.
    ///
    /// The removal is the part worth noticing. A property that merely stopped
    /// being written would leave its last value in the blob, and a later widget
    /// claiming that id would restore somebody else's state.
    pub fn unregister_from_restoration(&mut self, restoration_id: &str) {
        if let Some(bucket) = &self.bucket {
            bucket.remove(restoration_id);
        }
        self.registered.retain(|id| id != restoration_id);
    }

    /// Which ids this state has registered.
    pub fn registered_ids(&self) -> &[String] {
        &self.registered
    }
}

/// Upstream `RestorationManager`: the seam to the platform.
///
/// # This is the one class in the family that genuinely needs the engine
///
/// Everything else in this file is a tree over a map. The manager is where the
/// platform comes in, through two calls on `flutter/restoration`: asking for
/// the root bucket at startup, and pushing the serialised tree back whenever it
/// changed. Nothing in this repository serves that channel -- see the module
/// docs -- so [`RestorationManager::send_to_engine`] is a seam a host or a test
/// fills in, which is what upstream documents its own as being for.
pub struct RestorationManager {
    root: Option<RestorationBucket>,
    /// Upstream's `isReplacing`: whether the data currently being restored came
    /// from the platform replacing what was there, rather than from startup.
    is_replacing: bool,
    send: Option<Rc<dyn Fn(&Value)>>,
}

impl RestorationManager {
    pub fn new() -> RestorationManager {
        RestorationManager {
            root: None,
            is_replacing: false,
            send: None,
        }
    }

    /// What to do with the serialised tree. Upstream's `sendToEngine`, which it
    /// documents as overridable for exactly this.
    pub fn with_send(mut self, send: impl Fn(&Value) + 'static) -> Self {
        self.send = Some(Rc::new(send));
        self
    }

    /// Upstream's `rootBucket`, which is null until the platform answers.
    pub fn root_bucket(&self) -> Option<&RestorationBucket> {
        self.root.as_ref()
    }

    pub fn is_replacing(&self) -> bool {
        self.is_replacing
    }

    /// The platform handed over restoration data. Upstream's
    /// `handleRestorationUpdateFromEngine`.
    ///
    /// `enabled` false means the platform has turned restoration off, and
    /// upstream drops the root rather than keeping a bucket nothing will
    /// collect -- a widget that kept writing into it would be writing into
    /// nothing, slowly.
    pub fn handle_update_from_engine(&mut self, enabled: bool, data: Option<&Value>) {
        self.is_replacing = self.root.is_some();
        self.root = if enabled {
            Some(RestorationBucket::from_data(
                "root",
                data.map(BucketData::from_value).unwrap_or_default(),
            ))
        } else {
            None
        };
    }

    /// Upstream's `scheduleSerializationFor` reaching its end: the tree goes to
    /// the platform if anything in it changed.
    ///
    /// Answers whether anything was sent, and clears the dirty flag either way
    /// -- upstream's per-frame `_doSerialization`.
    pub fn flush(&mut self) -> bool {
        let Some(root) = &self.root else {
            return false;
        };
        if !root.needs_serialization() {
            root.finalize();
            return false;
        }
        let data = root.to_data().to_value();
        root.finalize();
        if let Some(send) = &self.send {
            send(&data);
        }
        self.is_replacing = false;
        true
    }
}

impl Default for RestorationManager {
    fn default() -> RestorationManager {
        RestorationManager::new()
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

    // -- Restorable values ----------------------------------------------------------

    #[test]
    fn setting_a_value_to_what_it_already_is_is_not_a_change() {
        // The same rule the bucket has one level down, and here it also decides
        // whether the widget rebuilds -- a property reassigned to what it held
        // would rebuild for nothing.
        let mut tab = RestorableInt::new(0);
        assert!(tab.set(2));
        assert!(!tab.set(2), "same value");
        assert!(tab.set(3));
        assert_eq!(*tab.value(), 3);
    }

    #[test]
    fn a_property_with_nothing_stored_takes_its_default() {
        let bucket = RestorationBucket::empty("root");
        let mut tab = RestorableInt::new(7);
        tab.set(2);
        tab.restore(&bucket, "tab");
        assert_eq!(*tab.value(), 7, "the default, not what it happened to hold");
    }

    #[test]
    fn a_property_reads_back_what_it_wrote() {
        let bucket = RestorationBucket::empty("root");
        let mut tab = RestorableInt::new(0);
        tab.set(2);
        tab.save(&bucket, "tab");

        let mut restored = RestorableInt::new(0);
        restored.restore(&bucket, "tab");
        assert_eq!(*restored.value(), 2);
    }

    #[test]
    fn a_disabled_property_is_not_written_at_all() {
        // Not written as null, not left at its old value -- absent. A widget
        // whose state is only worth remembering under some condition keeps out
        // of a blob that crosses a channel.
        let bucket = RestorationBucket::empty("root");
        let mut draft = RestorableString::new(String::new()).with_enabled(false);
        draft.set("half-typed".to_string());
        draft.save(&bucket, "draft");
        assert!(!bucket.contains("draft"));
    }

    #[test]
    fn a_stored_value_of_the_wrong_kind_is_treated_as_absent() {
        // What an application updated since the blob was written meets.
        // Upstream's `serialized as T` would throw; answering the default is
        // the honest end of that, because refusing to start over a value the
        // reader does not know exists is worse.
        let bucket = RestorationBucket::empty("root");
        bucket.write("tab", Value::String("not a number".into()));

        // Moved off the default first, or the assertion cannot tell "took the
        // default" from "kept what it was holding" -- they are the same number
        // on a freshly built property, and the first version of this test could
        // not tell them apart.
        let mut tab = RestorableInt::new(7);
        tab.set(2);
        tab.restore(&bucket, "tab");
        assert_eq!(*tab.value(), 7, "the default, not the 2 it was holding");
    }

    #[test]
    fn an_integer_written_narrow_comes_back() {
        // The standard codec narrows small integers to 32 bits, so a value
        // written as an int may come back either way. Upstream's `as int` does
        // not care, because a Dart int has no width.
        let bucket = RestorationBucket::empty("root");
        bucket.write("tab", Value::I32(2));
        let mut tab = RestorableInt::new(0);
        tab.restore(&bucket, "tab");
        assert_eq!(*tab.value(), 2);
    }

    #[test]
    fn a_whole_number_read_as_a_double_survives() {
        let bucket = RestorationBucket::empty("root");
        bucket.write("scroll", Value::I64(420));
        let mut scroll = RestorableDouble::new(0.0);
        scroll.restore(&bucket, "scroll");
        assert_eq!(*scroll.value(), 420.0);
    }

    #[test]
    fn init_with_value_assigns_without_calling_it_a_change() {
        // The bucket handing back what it stored is not the application
        // changing its mind, so upstream's `initWithValue` skips the check.
        let mut tab = RestorableInt::new(0);
        tab.init_with_value(5);
        assert_eq!(*tab.value(), 5);
        assert!(!tab.set(5), "and it is now the current value");
    }

    // -- The nullable half ------------------------------------------------------------

    #[test]
    fn a_nullable_property_round_trips_both_states() {
        let bucket = RestorationBucket::empty("root");
        let mut picked = RestorableIntN::new(None);
        picked.set(Some(3));
        picked.save(&bucket, "picked");

        let mut back = RestorableIntN::new(None);
        back.restore(&bucket, "picked");
        assert_eq!(*back.value(), Some(3));

        picked.set(None);
        picked.save(&bucket, "picked");
        back.restore(&bucket, "picked");
        assert_eq!(*back.value(), None, "and null is a value it may hold");
    }

    #[test]
    fn null_is_stored_as_null_and_not_as_a_missing_key() {
        // Absent means "take the default"; null means "the reader chose
        // nothing". A property that stored null by omitting the key could not
        // tell the two apart on the way back.
        let bucket = RestorationBucket::empty("root");
        let mut picked = RestorableIntN::new(Some(9));
        picked.set(None);
        picked.save(&bucket, "picked");
        assert_eq!(bucket.read("picked"), Some(Value::Null));

        let mut back = RestorableIntN::new(Some(9));
        back.restore(&bucket, "picked");
        assert_eq!(*back.value(), None, "not the default of 9");
    }

    // -- Enums ------------------------------------------------------------------------

    fn colours() -> Vec<String> {
        vec!["red".into(), "green".into(), "blue".into()]
    }

    #[test]
    fn an_enum_is_stored_by_name_and_not_by_index() {
        // An application whose enum gained a value in the middle would restore
        // every stored index to the wrong member. A name survives reordering.
        let bucket = RestorationBucket::empty("root");
        let mut colour = RestorableEnum::new("red", colours());
        colour.set("blue");
        colour.save(&bucket, "colour");
        assert_eq!(bucket.read("colour"), Some(Value::String("blue".into())));

        // The same blob against a set whose order changed.
        let mut reordered =
            RestorableEnum::new("red", vec!["blue".into(), "red".into(), "green".into()]);
        reordered.restore(&bucket, "colour");
        assert_eq!(reordered.value(), "blue");
    }

    #[test]
    fn a_name_the_application_no_longer_has_restores_to_the_default() {
        // What a name does not survive is being removed. Upstream asserts in
        // debug and answers the default in release; the default is what
        // actually happens in a shipped build either way.
        let bucket = RestorationBucket::empty("root");
        bucket.write("colour", Value::String("chartreuse".into()));

        let mut colour = RestorableEnum::new("green", colours());
        colour.restore(&bucket, "colour");
        assert_eq!(colour.value(), "green");
    }

    #[test]
    fn a_nullable_enum_answers_null_before_it_consults_the_set() {
        // Upstream's `fromPrimitives` returns null for stored null first, so a
        // null survives whatever the allowed set says.
        let bucket = RestorationBucket::empty("root");
        bucket.write("colour", Value::Null);

        let mut colour = RestorableEnumN::new(Some("red".into()), colours());
        colour.restore(&bucket, "colour");
        assert_eq!(colour.value(), None);
    }

    #[test]
    fn a_nullable_enum_moves_between_null_and_a_name() {
        let mut colour = RestorableEnumN::new(None, colours());
        assert_eq!(colour.value(), None);
        assert!(colour.set(Some("red".into())));
        assert_eq!(colour.value(), Some("red"));
        assert!(!colour.set(Some("red".into())), "no change");
        assert!(colour.set(None));
        assert_eq!(colour.value(), None);
        assert!(!colour.set(None), "no change");
    }

    // -- DateTime ----------------------------------------------------------------------

    #[test]
    fn a_moment_is_stored_as_milliseconds_since_the_epoch() {
        // Which is what crosses the channel upstream too --
        // `value.millisecondsSinceEpoch` -- so the count is the type here.
        let bucket = RestorationBucket::empty("root");
        let mut when = RestorableDateTime::new(0);
        when.set(1_700_000_000_000);
        when.save(&bucket, "when");
        assert_eq!(bucket.read("when"), Some(Value::I64(1_700_000_000_000)));
    }

    // -- The text controller --------------------------------------------------------------

    #[test]
    fn a_text_controller_stores_the_words_and_not_the_selection() {
        // Upstream's `toPrimitives` is `value.text`. A restored field has the
        // reader's words back and the caret at the end, which is upstream's
        // judgement: the words are what was lost.
        let bucket = RestorationBucket::empty("root");
        let mut field = RestorableTextEditingController::new("");
        field.set_text("half a sentence");
        field.save(&bucket, "field");
        assert_eq!(
            bucket.read("field"),
            Some(Value::String("half a sentence".into()))
        );

        let mut back = RestorableTextEditingController::new("");
        back.restore(&bucket, "field");
        assert_eq!(back.text(), "half a sentence");
        assert_eq!(back.restored_selection(), "half a sentence".len());
    }

    // -- Listenables -----------------------------------------------------------------------

    #[test]
    fn a_listenable_republishes_what_it_holds() {
        use std::cell::Cell;
        let heard = Rc::new(Cell::new(0));
        let counter = Rc::clone(&heard);
        let listenable = RestorableListenable::new().with_on_change(move || {
            counter.set(counter.get() + 1);
        });
        listenable.changed();
        listenable.changed();
        assert_eq!(heard.get(), 2);
    }

    #[test]
    fn a_listenable_with_nobody_listening_is_not_an_error() {
        RestorableListenable::new().changed();
        RestorableChangeNotifier::new().changed();
    }

    // -- Through the bucket tree -------------------------------------------------------------

    #[test]
    fn a_property_saved_into_a_child_is_in_the_roots_blob() {
        // The end-to-end shape: a widget deep in the tree writes, and the root
        // is what gets sent.
        let root = RestorationBucket::empty("root");
        let list = root.claim_child("list");
        let mut scroll = RestorableDouble::new(0.0);
        scroll.set(420.0);
        scroll.save(&list, "scroll");

        assert_eq!(
            root.to_data().children["list"].values["scroll"],
            Value::F64(420.0)
        );
    }

    #[test]
    fn a_blob_from_the_platform_restores_a_property_deep_in_the_tree() {
        let root = RestorationBucket::empty("root");
        let list = root.claim_child("list");
        let mut scroll = RestorableDouble::new(0.0);
        scroll.set(420.0);
        scroll.save(&list, "scroll");
        let wire = root.to_data().to_value();

        // A fresh run, given what the platform kept.
        let restored_root = RestorationBucket::from_data("root", BucketData::from_value(&wire));
        let restored_list = restored_root.claim_child("list");
        let mut restored_scroll = RestorableDouble::new(0.0);
        restored_scroll.restore(&restored_list, "scroll");
        assert_eq!(*restored_scroll.value(), 420.0);
    }

    // -- Scopes -----------------------------------------------------------------------

    #[test]
    fn two_handles_onto_one_bucket_are_the_same_bucket() {
        let bucket = RestorationBucket::empty("root");
        assert!(bucket.is(&bucket.clone()));
        assert!(!bucket.is(&RestorationBucket::empty("root")));
    }

    #[test]
    fn a_scope_rebuilds_the_subtree_when_the_bucket_is_replaced_and_not_when_it_changes() {
        // Identity, not contents. A bucket whose values changed is the same
        // bucket, and the properties that care are watching themselves; what a
        // rebuild is for is restoration arriving or going away.
        let one = RestorationBucket::empty("root");
        let same = UnmanagedRestorationScope::new(Some(one.clone()));
        one.write("tab", Value::I64(2));
        assert_eq!(
            same,
            UnmanagedRestorationScope::new(Some(one.clone())),
            "the values moved; the bucket did not"
        );

        // Two *different* buckets holding exactly the same thing. This is the
        // pair that tells identity from contents, and without it the assertion
        // passes either way -- the first version of this test compared a bucket
        // with itself and an empty one, which contents-comparison also gets
        // right.
        let twin = RestorationBucket::empty("root");
        twin.write("tab", Value::I64(2));
        assert_eq!(one.to_data(), twin.to_data(), "identical contents");
        assert_ne!(
            same,
            UnmanagedRestorationScope::new(Some(twin)),
            "and still a different bucket"
        );

        let other = RestorationBucket::empty("root");
        assert_ne!(same, UnmanagedRestorationScope::new(Some(other)));
        assert_ne!(same, UnmanagedRestorationScope::new(None));
        assert_eq!(
            UnmanagedRestorationScope::new(None),
            UnmanagedRestorationScope::new(None)
        );
    }

    #[test]
    fn a_scope_claims_a_child_of_the_bucket_above_it() {
        use crate::framework::{ElementTree, leaf};
        use crate::widgets::Empty;
        use std::cell::RefCell as Cell2;

        let seen: Rc<Cell2<Option<RestorationBucket>>> = Rc::new(Cell2::new(None));

        struct Probe(Rc<Cell2<Option<RestorationBucket>>>);
        impl Component for Probe {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.0.borrow_mut() = RestorationScope::maybe_of(context);
                leaf(|| Empty)
            }
        }

        let root = RestorationBucket::empty("root");
        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::provide(
            UnmanagedRestorationScope::new(Some(root.clone())),
            crate::framework::component(RestorationScope::new(
                "list",
                crate::framework::component(Probe(Rc::clone(&seen))),
            )),
        ));
        tree.build_render_tree();

        let claimed = seen.borrow().clone().expect("a bucket in scope");
        assert_eq!(claimed.restoration_id(), "list");
        assert!(root.is_claimed("list"), "claimed from the one above");
    }

    #[test]
    fn a_scope_with_no_id_publishes_nothing_and_the_subtree_stops_remembering() {
        use crate::framework::{ElementTree, leaf};
        use crate::widgets::Empty;
        use std::cell::RefCell as Cell2;

        let seen: Rc<Cell2<Option<RestorationBucket>>> = Rc::new(Cell2::new(None));
        struct Probe(Rc<Cell2<Option<RestorationBucket>>>);
        impl Component for Probe {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.0.borrow_mut() = RestorationScope::maybe_of(context);
                leaf(|| Empty)
            }
        }

        let root = RestorationBucket::empty("root");
        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::provide(
            UnmanagedRestorationScope::new(Some(root.clone())),
            crate::framework::component(RestorationScope::disabled(crate::framework::component(
                Probe(Rc::clone(&seen)),
            ))),
        ));
        tree.build_render_tree();

        assert!(seen.borrow().is_none(), "nothing to write into");
        assert!(root.to_data().children.is_empty(), "and nothing claimed");
    }

    #[test]
    fn a_root_scope_shows_nothing_until_the_root_bucket_arrives() {
        // Not the child without a bucket: a subtree built now would claim
        // buckets and have to give them up the moment the real root landed.
        // Built rather than merely asked, because `is_ready` alone cannot tell
        // whether `build` consults it.
        use crate::framework::{ElementTree, leaf};
        use crate::widgets::Empty;
        use std::cell::Cell as Flag;

        let built = Rc::new(Flag::new(false));

        struct Probe(Rc<Flag<bool>>);
        impl Component for Probe {
            fn build(&self, _context: &mut BuildContext) -> AnyWidget {
                self.0.set(true);
                leaf(|| Empty)
            }
        }

        let waiting = RootRestorationScope::new(
            "app",
            None,
            crate::framework::component(Probe(Rc::clone(&built))),
        );
        assert!(!waiting.is_ready());
        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::component(waiting));
        tree.build_render_tree();
        assert!(!built.get(), "the subtree was not built");

        let ready = Rc::new(Flag::new(false));
        let arrived = RootRestorationScope::new(
            "app",
            Some(RestorationBucket::empty("root")),
            crate::framework::component(Probe(Rc::clone(&ready))),
        );
        assert!(arrived.is_ready());
        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::component(arrived));
        tree.build_render_tree();
        assert!(ready.get(), "and once the root arrived, it was");
    }

    // -- Registration --------------------------------------------------------------------

    #[test]
    fn a_stored_value_wins_over_the_default_and_the_default_is_written_back() {
        // The whole of restoration in one line, and the write-back is what puts
        // a first-run default into the blob for next time.
        let bucket = RestorationBucket::empty("state");
        bucket.write("tab", Value::I64(3));

        let mut state = RestorationMixin::new();
        state.set_bucket(Some(bucket.clone()));

        let mut restored = RestorableInt::new(0);
        state.register_for_restoration(&mut restored, "tab");
        assert_eq!(*restored.value(), 3, "the stored value");

        let mut fresh = RestorableInt::new(9);
        state.register_for_restoration(&mut fresh, "other");
        assert_eq!(*fresh.value(), 9, "the default");
        assert_eq!(
            bucket.read("other"),
            Some(Value::I64(9)),
            "and it is in the blob for next time"
        );
    }

    #[test]
    fn registering_without_a_bucket_leaves_the_property_at_its_default() {
        let mut state = RestorationMixin::new();
        let mut tab = RestorableInt::new(7);
        tab.set(2);
        state.register_for_restoration(&mut tab, "tab");
        assert_eq!(*tab.value(), 2, "untouched: there was nowhere to look");
        assert!(state.registered_ids().is_empty());
    }

    #[test]
    fn unregistering_removes_the_stored_value_and_not_only_the_registration() {
        // A property that merely stopped being written would leave its last
        // value in the blob, and a later widget claiming that id would restore
        // somebody else's state.
        let bucket = RestorationBucket::empty("state");
        let mut state = RestorationMixin::new();
        state.set_bucket(Some(bucket.clone()));

        let mut tab = RestorableInt::new(0);
        tab.set(4);
        state.register_for_restoration(&mut tab, "tab");
        assert!(bucket.contains("tab"));

        state.unregister_from_restoration("tab");
        assert!(!bucket.contains("tab"), "gone from the blob too");
        assert!(state.registered_ids().is_empty());
    }

    #[test]
    fn the_first_bucket_is_an_initial_restore_and_a_replacement_is_not() {
        // The difference between "you are being set up" and "the platform
        // replaced your data underneath you", which a widget holding a
        // controller has to handle differently.
        let mut state = RestorationMixin::new();
        assert!(state.set_bucket(Some(RestorationBucket::empty("a"))));
        assert!(!state.set_bucket(Some(RestorationBucket::empty("b"))));
    }

    #[test]
    fn a_new_bucket_clears_what_was_registered_against_the_old_one() {
        let mut state = RestorationMixin::new();
        state.set_bucket(Some(RestorationBucket::empty("a")));
        let mut tab = RestorableInt::new(0);
        state.register_for_restoration(&mut tab, "tab");
        assert_eq!(state.registered_ids(), &["tab".to_string()]);

        state.set_bucket(Some(RestorationBucket::empty("b")));
        assert!(
            state.registered_ids().is_empty(),
            "the ids belonged to the old bucket"
        );
    }

    // -- The manager ----------------------------------------------------------------------

    #[test]
    fn the_manager_has_no_root_until_the_platform_answers() {
        let manager = RestorationManager::new();
        assert!(manager.root_bucket().is_none());
        assert!(!manager.is_replacing());
    }

    #[test]
    fn an_update_from_the_platform_becomes_the_root_bucket() {
        let mut data = BucketData::default();
        data.values.insert("tab".into(), Value::I64(2));

        let mut manager = RestorationManager::new();
        manager.handle_update_from_engine(true, Some(&data.to_value()));
        let root = manager.root_bucket().expect("a root");
        assert_eq!(root.read("tab"), Some(Value::I64(2)));
        assert!(
            !manager.is_replacing(),
            "the first one is not a replacement"
        );
    }

    #[test]
    fn a_second_update_is_a_replacement() {
        let mut manager = RestorationManager::new();
        manager.handle_update_from_engine(true, None);
        manager.handle_update_from_engine(true, None);
        assert!(manager.is_replacing());
    }

    #[test]
    fn the_platform_turning_restoration_off_drops_the_root() {
        // A widget that kept writing into a bucket nothing will collect would
        // be writing into nothing, slowly.
        let mut manager = RestorationManager::new();
        manager.handle_update_from_engine(true, None);
        assert!(manager.root_bucket().is_some());
        manager.handle_update_from_engine(false, None);
        assert!(manager.root_bucket().is_none());
    }

    #[test]
    fn the_tree_goes_to_the_platform_only_when_something_changed() {
        use std::cell::RefCell as Cell2;
        let sent: Rc<Cell2<Vec<Value>>> = Rc::new(Cell2::new(Vec::new()));
        let recorder = Rc::clone(&sent);

        let mut manager = RestorationManager::new()
            .with_send(move |value| recorder.borrow_mut().push(value.clone()));
        manager.handle_update_from_engine(true, None);

        // The root was just built, so nothing is dirty yet.
        manager.flush();
        let before = sent.borrow().len();

        manager
            .root_bucket()
            .expect("a root")
            .write("tab", Value::I64(2));
        assert!(manager.flush(), "something changed");
        assert_eq!(sent.borrow().len(), before + 1);

        assert!(!manager.flush(), "and nothing has changed since");
        assert_eq!(sent.borrow().len(), before + 1);
    }

    #[test]
    fn what_the_platform_is_sent_is_the_whole_tree() {
        use std::cell::RefCell as Cell2;
        let sent: Rc<Cell2<Option<Value>>> = Rc::new(Cell2::new(None));
        let recorder = Rc::clone(&sent);

        let mut manager = RestorationManager::new()
            .with_send(move |value| *recorder.borrow_mut() = Some(value.clone()));
        manager.handle_update_from_engine(true, None);

        let root = manager.root_bucket().expect("a root").clone();
        let list = root.claim_child("list");
        list.write("scroll", Value::I64(420));
        manager.flush();

        let blob = sent.borrow().clone().expect("something was sent");
        let data = BucketData::from_value(&blob);
        assert_eq!(
            data.children["list"].values["scroll"],
            Value::I64(420),
            "a leaf's write reached the platform"
        );
    }

    #[test]
    fn a_manager_with_nowhere_to_send_still_clears_the_dirty_flag() {
        // Nothing serves this channel in this repository, and a manager that
        // stayed dirty for ever would try to serialise the tree every frame.
        let mut manager = RestorationManager::new();
        manager.handle_update_from_engine(true, None);
        manager
            .root_bucket()
            .expect("a root")
            .write("tab", Value::I64(2));
        assert!(manager.flush());
        assert!(!manager.root_bucket().expect("a root").needs_serialization());
    }
}
