// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Where a list is scrolled to, and what keeps it moving.
//!
//! A scroll offset looks like a number that a drag handler can add to, and for
//! as long as the finger is down that is all it is. The moment it lifts, the
//! offset becomes something else: a value with momentum, moving on its own,
//! for a while, and stopping where physics says rather than where the finger
//! left it. Without that, content stops dead the instant the finger does --
//! which is what a list feels like when nobody has written this file, and is
//! immediately obvious next to any other application on the device.
//!
//! [`Scroll`] is both halves. It holds the offset, it clamps against the
//! content, and it owns the fling: [`Scroll::fling`] starts one and
//! [`Scroll::advance`] moves it along, once per frame, returning whether it
//! wants another.
//!
//! # What upstream splits up
//!
//! Upstream this is three objects. `ScrollPosition` holds the offset and the
//! extents; `ScrollActivity` is what is currently in charge of it -- a
//! `DragScrollActivity` while a finger is down, a `BallisticScrollActivity`
//! after it lifts, an `IdleScrollActivity` when nothing is happening; and
//! `ScrollPhysics` decides which activity comes next and with what simulation.
//! The split earns its keep there because activities are pluggable: page
//! snapping, `ScrollController.animateTo` and overscroll bouncing are each
//! another activity. Here there are two states -- dragging and flinging --
//! and one physics, so they are fields on one struct, and the day a third
//! activity is wanted is the day this becomes an enum.
//!
//! # Which way is positive
//!
//! `offset` grows as the reader goes further into the content, exactly as
//! upstream's `pixels` does, and a fling's velocity is in the same direction.
//! That is *opposite* to the finger: dragging down reveals earlier content, so
//! it decreases the offset. Handlers negate, and it is worth doing in the one
//! place they do it rather than here, because a wheel does not need negating
//! and a scrollbar drag does not either.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::physics::{ClampingScrollSimulation, Simulation};

/// A scroll offset, its limit, and any fling in progress.
///
/// The limit lives behind an [`Rc<Cell>`](std::cell::Cell) because it is not
/// known when the offset is set: how far a list can scroll depends on how tall
/// its content turned out to be, which is settled during layout, a frame after
/// whoever holds the offset needed it. [`crate::widgets::ListView::with_extent_sink`]
/// fills it in from the other side.
#[derive(Clone)]
pub struct Scroll {
    /// How far into the content the view is, in logical pixels. Always within
    /// `0..=`[`max_extent`](Scroll::max_extent) as of the last thing that
    /// moved it.
    pub offset: f32,
    /// How far this list can scroll, filled in at layout.
    pub extent: Rc<Cell<f32>>,
    /// The fling in flight, if any.
    ballistic: Option<Ballistic>,
}

/// A fling being played out.
#[derive(Clone, Copy)]
struct Ballistic {
    simulation: ClampingScrollSimulation,
    /// When it started, in frame-clock microseconds. Not known when the fling
    /// is created -- the finger lifts between frames -- so it is taken from
    /// the first frame that advances it, which is also the first frame that
    /// could draw it. Upstream a `Ticker` does the same thing: its elapsed
    /// duration is measured from its first tick, not from `start`.
    started_micros: Option<i64>,
}

impl Default for Scroll {
    fn default() -> Scroll {
        Scroll { offset: 0.0, extent: Rc::new(Cell::new(0.0)), ballistic: None }
    }
}

impl Scroll {
    pub fn new() -> Scroll {
        Scroll::default()
    }

    /// How far this list can scroll. Zero until something has measured it.
    pub fn max_extent(&self) -> f32 {
        self.extent.get().max(0.0)
    }

    /// Records how far the list can scroll, for callers that measure the
    /// content themselves rather than handing [`extent`](Scroll::extent) to a
    /// [`ListView`](crate::widgets::ListView).
    ///
    /// Takes `&self` because a build is handed its state by shared reference
    /// and the limit is discovered during the build that lays the content out.
    pub fn set_extent(&self, extent: f32) {
        self.extent.set(extent);
    }

    /// Moves by `delta` and stays inside the content.
    ///
    /// Clamping here rather than in the viewport is what stops an overscroll
    /// from banking travel: without it, flinging past the end and dragging
    /// back would do nothing until the imaginary distance had been paid off.
    ///
    /// A drag or a wheel also ends any fling. Upstream the drag *replaces* the
    /// ballistic activity, which is the same thing said in objects: whatever
    /// the reader is doing now wins over what they did a moment ago.
    pub fn scroll_by(&mut self, delta: f32) {
        self.ballistic = None;
        self.offset = (self.offset + delta).clamp(0.0, self.max_extent());
    }

    /// Puts the offset somewhere, without any physics. For jumping to a
    /// position rather than travelling to it.
    pub fn jump_to(&mut self, offset: f32) {
        self.ballistic = None;
        self.offset = offset.clamp(0.0, self.max_extent());
    }

    /// Stops a fling where it is. What a finger touching the content does.
    pub fn stop(&mut self) {
        self.ballistic = None;
    }

    /// Whether a fling is in flight.
    pub fn is_ballistic(&self) -> bool {
        self.ballistic.is_some()
    }

    /// Starts a fling at `velocity` logical pixels per second, in offset
    /// space -- positive meaning further into the content.
    ///
    /// Does nothing when there is nowhere to go, which is upstream's
    /// `ClampingScrollPhysics.createBallisticSimulation` returning null: no
    /// velocity, or already at the end the fling is heading for. Starting one
    /// anyway would cost a run of frames that each clamp to the same number.
    pub fn fling(&mut self, velocity: f32) {
        self.ballistic = None;
        if velocity == 0.0 {
            return;
        }
        if velocity > 0.0 && self.offset >= self.max_extent() {
            return;
        }
        if velocity < 0.0 && self.offset <= 0.0 {
            return;
        }
        self.ballistic = Some(Ballistic {
            simulation: ClampingScrollSimulation::new(self.offset, velocity),
            started_micros: None,
        });
    }

    /// Moves a fling on by one frame, and says whether another is wanted.
    ///
    /// Call once per frame from a
    /// [`StatefulComponent::advance`](crate::framework::StatefulComponent::advance).
    /// Returns false when nothing is moving, which is what lets the frame loop
    /// go back to sleep.
    pub fn advance(&mut self, frame_time_micros: i64) -> bool {
        let max = self.max_extent();
        let Some(ballistic) = &mut self.ballistic else {
            return false;
        };
        let started = *ballistic.started_micros.get_or_insert(frame_time_micros);
        let elapsed = (frame_time_micros - started).max(0) as f32 / 1_000_000.0;
        let position = ballistic.simulation.x(elapsed);
        let done = ballistic.simulation.is_done(elapsed);

        let clamped = position.clamp(0.0, max);
        let moved = clamped != self.offset;
        self.offset = clamped;

        // Hitting either end ends the fling, however much of the simulation is
        // left: the content has run out, and continuing would be a run of
        // frames that each clamp to the same number. Upstream's
        // `BallisticScrollActivity` stops the same way -- `applyMoveTo`
        // returning false means the position could not go where the simulation
        // asked, and the activity goes idle.
        if done || clamped != position {
            self.ballistic = None;
            return moved;
        }
        true
    }
}

// -- Lazy lists ---------------------------------------------------------------

/// How far beyond the visible window items are still built.
///
/// Upstream's `RenderAbstractViewport.defaultCacheExtent`, and the same number.
/// Building exactly what is visible means the first row of a fling is built in
/// the frame it appears, which is the frame that has the least time to spare;
/// a screen's worth of margin at each end costs a few items and removes that.
pub const DEFAULT_CACHE_EXTENT: f32 = 250.0;

/// Which items a fixed-extent list needs to have built.
///
/// The whole point of a fixed extent: the answer is arithmetic rather than
/// measurement, so a list of a hundred thousand rows costs the same as a list
/// of ten. Upstream this is `SliverFixedExtentList`'s
/// `getMinChildIndexForScrollOffset` / `getMaxChildIndexForScrollOffset`, and
/// the rounding is theirs -- an offset that lands within a rounding error of an
/// item boundary counts as being on it, which stops a row from flickering in
/// and out when a fling leaves the offset a ten-thousandth of a pixel short.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemWindow {
    /// First index to build.
    pub first: usize,
    /// Last index to build, inclusive. Equal to `first` for one item.
    pub last: usize,
}

impl ItemWindow {
    pub fn len(&self) -> usize {
        self.last + 1 - self.first
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn contains(&self, index: usize) -> bool {
        index >= self.first && index <= self.last
    }
}

/// Rounding slack, as upstream's `precisionErrorTolerance`.
const PRECISION_ERROR: f32 = 1e-10;

/// Which items to build for a viewport of `viewport` showing `offset`.
///
/// `count` items of `extent` each, plus [`DEFAULT_CACHE_EXTENT`] of margin at
/// both ends. Returns `None` when there is nothing to show at all.
pub fn item_window(
    count: usize,
    extent: f32,
    offset: f32,
    viewport: f32,
    cache_extent: f32,
) -> Option<ItemWindow> {
    if count == 0 || extent <= 0.0 {
        return None;
    }
    let start = (offset - cache_extent).max(0.0);
    let end = offset + viewport + cache_extent;

    let first = {
        let actual = start / extent;
        let round = actual.round();
        if (actual * extent - round * extent).abs() < PRECISION_ERROR {
            round
        } else {
            actual.floor()
        }
    };
    let last = {
        let actual = end / extent - 1.0;
        let round = actual.round();
        let index = if (actual * extent - round * extent).abs() < PRECISION_ERROR {
            round
        } else {
            actual.ceil()
        };
        index.max(0.0)
    };

    let first = (first.max(0.0) as usize).min(count - 1);
    let last = (last.max(0.0) as usize).min(count - 1);
    Some(ItemWindow { first, last: last.max(first) })
}

/// A list that builds only the items it is showing.
///
/// Upstream this is `ListView.builder` over a `SliverFixedExtentList`: the
/// items are all the same height, so which ones are on screen is arithmetic,
/// and the ones that are not are never built. A list of a hundred thousand rows
/// costs what a screenful costs.
///
/// The items that are not built are still *accounted for*: the list reserves
/// their space with a gap at each end, so the scrollbar, the extent and the
/// offsets are all what they would be if every row existed.
///
/// ```ignore
/// component(
///     LazyList::new(photos.len(), 72.0, move |index| row_for(index))
///         .with_offset(state.scroll.offset)
///         .with_viewport(size.height),
/// )
/// ```
///
/// # What a variable extent would need
///
/// Upstream's plain `SliverList` allows items of different heights, and pays
/// for it: it cannot know where item ten thousand is without having laid out
/// the nine thousand nine hundred and ninety-nine before it, so it keeps a
/// running estimate and corrects it as the reader scrolls. That is a different
/// piece of work and it needs render objects that survive a frame. This is the
/// half that does not.
pub struct LazyList {
    count: usize,
    extent: f32,
    offset: f32,
    viewport: f32,
    cache_extent: f32,
    spacing: f32,
    build_item: Rc<dyn Fn(usize) -> crate::framework::AnyWidget>,
}

impl LazyList {
    pub fn new(
        count: usize,
        extent: f32,
        build_item: impl Fn(usize) -> crate::framework::AnyWidget + 'static,
    ) -> LazyList {
        LazyList {
            count,
            extent,
            offset: 0.0,
            viewport: 0.0,
            cache_extent: DEFAULT_CACHE_EXTENT,
            spacing: 0.0,
            build_item: Rc::new(build_item),
        }
    }

    /// How far the list is scrolled.
    pub fn with_offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }

    /// How much of the list is visible. Without this the list builds one
    /// screenful of cache and nothing else, which is what a viewport of zero
    /// means.
    pub fn with_viewport(mut self, viewport: f32) -> Self {
        self.viewport = viewport;
        self
    }

    pub fn with_cache_extent(mut self, cache_extent: f32) -> Self {
        self.cache_extent = cache_extent.max(0.0);
        self
    }

    /// Space between items. Counted in the extent, as upstream counts a
    /// separator: `item_extent` is one item plus one gap.
    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// How tall the whole list is, built or not.
    pub fn content_extent(&self) -> f32 {
        if self.count == 0 {
            0.0
        } else {
            self.count as f32 * (self.extent + self.spacing) - self.spacing
        }
    }

    /// How far this list can scroll, given the viewport it is in.
    pub fn max_scroll_extent(&self) -> f32 {
        (self.content_extent() - self.viewport).max(0.0)
    }

    /// Which items this list would build right now.
    pub fn window(&self) -> Option<ItemWindow> {
        item_window(
            self.count,
            self.extent + self.spacing,
            self.offset,
            self.viewport,
            self.cache_extent,
        )
    }
}

impl crate::framework::Component for LazyList {
    fn build(&self, _context: &mut crate::framework::BuildContext) -> crate::framework::AnyWidget {
        let Some(window) = self.window() else {
            return crate::framework::leaf(|| crate::widgets::Empty);
        };
        let step = self.extent + self.spacing;
        // The items that are not built are still taken up: a gap at each end,
        // as tall as the rows it stands in for. Without it every offset below
        // the window would be wrong by however much was skipped.
        //
        // A spacer that stands for nothing is left out rather than given a
        // height of zero, and one that stands for something is a gap shorter
        // than the rows it replaces. Both because the column puts a gap
        // *between* its children: the gap above the first built row is the
        // column's to add, and counting it here as well made a spaced list
        // two gaps taller than its own content.
        let leading = (window.first as f32 * step - self.spacing).max(0.0);
        let after = self.count - 1 - window.last;
        let trailing = (after as f32 * step - self.spacing).max(0.0);

        let mut children = Vec::with_capacity(window.len() + 2);
        if window.first > 0 {
            children.push(crate::framework::leaf(move || {
                crate::render::RenderConstrainedBox::tight(0.0, leading)
            }));
        }
        for index in window.first..=window.last {
            // Keyed by index, so an item keeps its element -- and therefore
            // its state -- when the window moves. Matching by position would
            // hand row eleven's state to row ten as soon as one scrolled off
            // the top.
            children.push(crate::framework::keyed_single(
                index as u64,
                (self.build_item)(index),
                |child| child,
            ));
        }
        if after > 0 {
            children.push(crate::framework::leaf(move || {
                crate::render::RenderConstrainedBox::tight(0.0, trailing)
            }));
        }

        let spacing = self.spacing;
        crate::framework::many(children, move |rendered| {
            let mut column = crate::render::RenderFlex::column()
                .with_main_axis_size(crate::render::MainAxisSize::Min)
                .with_cross_axis_alignment(crate::render::CrossAxisAlignment::Stretch)
                .with_spacing(spacing);
            for child in rendered {
                column = column.push(child);
            }
            Box::new(column)
        })
    }
}

// -- Lists whose items are not all the same height ----------------------------

/// What a variable-extent list has learned about how tall its items are.
///
/// Upstream's `RenderSliverList` keeps the same knowledge on the children
/// themselves, as `SliverMultiBoxAdaptorParentData.layoutOffset`, and can
/// afford to because it creates its children inside its own `performLayout`:
/// it lays one out, learns its height, and knows where the next one starts.
///
/// Nothing here can do that. Children are elements, and elements are built one
/// step before anything is measured -- so the window has to be chosen from what
/// is already known. The measurements are therefore kept beside the list rather
/// than inside it, and a frame reads what the frame before it wrote. That is
/// already how this framework answers "how far can this scroll" (see
/// [`Scroll::extent`]) and it is the same one-frame lag, for the same reason.
///
/// A book belongs to whoever holds the scroll offset, and lives as long as it
/// does. Sharing is the point: two frames of the same list are the same list.
#[derive(Clone, Default)]
pub struct ExtentBook(Rc<RefCell<Book>>);

#[derive(Default)]
struct Book {
    /// Measured extents by index. `None` for an item that has never been laid
    /// out -- which at the start is all of them.
    measured: Vec<Option<f32>>,
    /// Sum of the measured ones, and how many there are, so the average is not
    /// a walk.
    total: f32,
    known: usize,
    /// Bumped whenever a measurement arrives that was not already the answer.
    /// A caller that wants the correction applied without waiting for the next
    /// thing to happen watches this and asks for a frame.
    revision: u64,
}

impl ExtentBook {
    pub fn new() -> ExtentBook {
        ExtentBook::default()
    }

    /// Records how tall item `index` turned out. Called from layout.
    pub fn record(&self, index: usize, extent: f32) {
        if !extent.is_finite() || extent < 0.0 {
            return;
        }
        let mut book = self.0.borrow_mut();
        if book.measured.len() <= index {
            book.measured.resize(index + 1, None);
        }
        match book.measured[index] {
            Some(before) if before == extent => return,
            Some(before) => book.total += extent - before,
            None => {
                book.total += extent;
                book.known += 1;
            }
        }
        book.measured[index] = Some(extent);
        book.revision += 1;
    }

    /// What was measured for `index`, if it has ever been on screen.
    pub fn measured(&self, index: usize) -> Option<f32> {
        self.0.borrow().measured.get(index).copied().flatten()
    }

    /// How many items have been measured.
    pub fn known(&self) -> usize {
        self.0.borrow().known
    }

    /// Changes so far. Only useful compared against an earlier reading.
    pub fn revision(&self) -> u64 {
        self.0.borrow().revision
    }

    //--------------------------------------------------------------------------
    /// The height to assume for an item nobody has measured.
    ///
    /// The average of the ones that have been, which is upstream's
    /// `_extrapolateMaxScrollOffset`: it takes the extent of the children it
    /// currently has and spreads it over the ones it does not. The difference
    /// is that this averages over everything ever measured rather than only
    /// what is alive now, which is strictly better informed and converges to
    /// the same number.
    ///
    /// `fallback` is the answer before anything at all has been measured, and
    /// it is the caller's guess -- upstream has no equivalent because its first
    /// layout is a real one.
    pub fn average(&self, fallback: f32) -> f32 {
        let book = self.0.borrow();
        if book.known == 0 {
            fallback.max(0.0)
        } else {
            book.total / book.known as f32
        }
    }

    /// Where item `index` starts, counting from the top of the content.
    fn offset_of(&self, index: usize, fallback: f32, spacing: f32) -> f32 {
        let average = self.average(fallback);
        let book = self.0.borrow();
        let mut at = 0.0;
        for item in 0..index {
            at += book.measured.get(item).copied().flatten().unwrap_or(average) + spacing;
        }
        at
    }

    /// How tall the whole list is, measured where it can be and estimated
    /// where it cannot.
    fn content_extent(&self, count: usize, fallback: f32, spacing: f32) -> f32 {
        if count == 0 {
            return 0.0;
        }
        let average = self.average(fallback);
        let book = self.0.borrow();
        let mut total = 0.0;
        for index in 0..count {
            total += book.measured.get(index).copied().flatten().unwrap_or(average);
        }
        total + spacing * (count - 1) as f32
    }

    //--------------------------------------------------------------------------
    /// Which items a viewport at `offset` needs built.
    ///
    /// The fixed-extent answer is arithmetic ([`item_window`]); this one is a
    /// walk, because the only way to know where item ten thousand is, is to
    /// add up the nine thousand nine hundred and ninety-nine before it.
    /// Upstream pays the same price and manages it the same way -- it walks
    /// from the children it already has rather than from zero. Here the walk is
    /// over a vector of floats, which is cheap enough that starting from zero
    /// is not worth avoiding until a list is long enough to prove otherwise.
    fn window(
        &self,
        count: usize,
        offset: f32,
        viewport: f32,
        cache_extent: f32,
        fallback: f32,
        spacing: f32,
    ) -> Option<ItemWindow> {
        if count == 0 {
            return None;
        }
        let average = self.average(fallback);
        if average <= 0.0 && spacing <= 0.0 {
            return None;
        }
        let start = (offset - cache_extent).max(0.0);
        let end = offset + viewport + cache_extent;

        let book = self.0.borrow();
        let mut at = 0.0;
        let mut first = None;
        let mut last = 0;
        for index in 0..count {
            let extent = book.measured.get(index).copied().flatten().unwrap_or(average);
            let bottom = at + extent;
            // An item that starts at or past the end of the window is below it,
            // and so is everything after it.
            if first.is_some() && at >= end - PRECISION_ERROR {
                break;
            }
            // An item that ends exactly where the window starts is above it,
            // which is the same boundary the fixed-extent arithmetic draws.
            if bottom > start + PRECISION_ERROR {
                if first.is_none() {
                    first = Some(index);
                }
                last = index;
            }
            at = bottom + spacing;
        }
        let first = first.unwrap_or(count - 1);
        Some(ItemWindow { first, last: last.max(first) })
    }
}

/// A list that builds only the items it is showing, without being told how
/// tall they are.
///
/// Upstream this is `ListView.builder` over a plain `SliverList`, as opposed to
/// the `SliverFixedExtentList` that [`LazyList`] is. The difference is the
/// whole of it: a fixed extent makes "which items are on screen" arithmetic,
/// and without one the list has to remember what it measured and estimate the
/// rest. So this needs two things [`LazyList`] does not -- an [`ExtentBook`]
/// that outlives the frame, and render objects that report their height into it
/// as they are laid out.
///
/// ```ignore
/// component(
///     VariableExtentList::new(messages.len(), state.extents.clone(), move |index| {
///         row_for(index)
///     })
///     .with_estimate(72.0)
///     .with_offset(state.scroll.offset)
///     .with_viewport(size.height),
/// )
/// ```
pub struct VariableExtentList {
    count: usize,
    book: ExtentBook,
    /// What to assume before anything has been measured. Only ever used for
    /// the first frame; after that the average of what was measured is a
    /// better guess than any constant.
    estimate: f32,
    offset: f32,
    viewport: f32,
    cache_extent: f32,
    spacing: f32,
    build_item: Rc<dyn Fn(usize) -> crate::framework::AnyWidget>,
}

impl VariableExtentList {
    pub fn new(
        count: usize,
        book: ExtentBook,
        build_item: impl Fn(usize) -> crate::framework::AnyWidget + 'static,
    ) -> VariableExtentList {
        VariableExtentList {
            count,
            book,
            estimate: DEFAULT_ITEM_ESTIMATE,
            offset: 0.0,
            viewport: 0.0,
            cache_extent: DEFAULT_CACHE_EXTENT,
            spacing: 0.0,
            build_item: Rc::new(build_item),
        }
    }

    /// What to assume an unmeasured item is worth on the very first frame.
    pub fn with_estimate(mut self, estimate: f32) -> Self {
        self.estimate = estimate.max(0.0);
        self
    }

    pub fn with_offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }

    pub fn with_viewport(mut self, viewport: f32) -> Self {
        self.viewport = viewport;
        self
    }

    pub fn with_cache_extent(mut self, cache_extent: f32) -> Self {
        self.cache_extent = cache_extent.max(0.0);
        self
    }

    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// How tall the whole list is, as well as it is currently known.
    pub fn content_extent(&self) -> f32 {
        self.book.content_extent(self.count, self.estimate, self.spacing)
    }

    /// How far this list can scroll, given the viewport it is in.
    pub fn max_scroll_extent(&self) -> f32 {
        (self.content_extent() - self.viewport).max(0.0)
    }

    /// Which items this list would build right now.
    pub fn window(&self) -> Option<ItemWindow> {
        self.book.window(
            self.count,
            self.offset,
            self.viewport,
            self.cache_extent,
            self.estimate,
            self.spacing,
        )
    }
}

/// What an unmeasured item is worth before anything better is known.
///
/// A row of one line of text with the padding a list tile has. Upstream has no
/// such number because its first layout measures for real; this is the price of
/// choosing the window a step earlier, and it is paid for exactly one frame.
pub const DEFAULT_ITEM_ESTIMATE: f32 = 56.0;

impl crate::framework::Component for VariableExtentList {
    fn build(&self, _context: &mut crate::framework::BuildContext) -> crate::framework::AnyWidget {
        let Some(window) = self.window() else {
            return crate::framework::leaf(|| crate::widgets::Empty);
        };

        // The items outside the window are still accounted for: a gap at each
        // end as tall as the rows it stands in for, measured where they have
        // been and estimated where they have not. A gap short of the rows it
        // replaces, and left out entirely when there are none, because the
        // column supplies the space between its own children -- see the same
        // arithmetic in `LazyList`.
        let leading =
            (self.book.offset_of(window.first, self.estimate, self.spacing) - self.spacing)
                .max(0.0);
        let after_last =
            self.book.offset_of(window.last + 1, self.estimate, self.spacing);
        let trailing = (self.content_extent() - after_last).max(0.0);
        let more_below = window.last + 1 < self.count;

        let mut children = Vec::with_capacity(window.len() + 2);
        if window.first > 0 {
            children.push(crate::framework::leaf(move || {
                crate::render::RenderConstrainedBox::tight(0.0, leading)
            }));
        }
        for index in window.first..=window.last {
            let book = self.book.clone();
            // Keyed by index, as the fixed-extent list is and for the same
            // reason: an item keeps its element, and therefore its state, when
            // the window moves past it.
            children.push(crate::framework::keyed_single(
                index as u64,
                (self.build_item)(index),
                move |child| RenderMeasuredItem::new(index, book.clone(), child),
            ));
        }
        if more_below {
            children.push(crate::framework::leaf(move || {
                crate::render::RenderConstrainedBox::tight(0.0, trailing)
            }));
        }

        let spacing = self.spacing;
        crate::framework::many(children, move |rendered| {
            let mut column = crate::render::RenderFlex::column()
                .with_main_axis_size(crate::render::MainAxisSize::Min)
                .with_cross_axis_alignment(crate::render::CrossAxisAlignment::Stretch)
                .with_spacing(spacing);
            for child in rendered {
                column = column.push(child);
            }
            Box::new(column)
        })
    }
}

/// Reports how tall its child turned out, into the book its list reads.
///
/// Draws nothing and changes no layout: it is the same box as its child, with
/// one number written down on the way out. Upstream needs no equivalent because
/// the sliver that laid the child out is the thing that wanted the number.
pub struct RenderMeasuredItem {
    index: usize,
    book: ExtentBook,
    child: crate::render::BoxedRender,
    size: crate::render::Size,
}

impl RenderMeasuredItem {
    pub fn new(
        index: usize,
        book: ExtentBook,
        child: impl crate::render::RenderBox + 'static,
    ) -> RenderMeasuredItem {
        RenderMeasuredItem {
            index,
            book,
            child: crate::render::BoxedRender::new(child),
            size: crate::render::Size::ZERO,
        }
    }
}

impl crate::render::RenderBox for RenderMeasuredItem {
    fn layout(&mut self, constraints: crate::render::BoxConstraints) -> crate::render::Size {
        self.size = self.child.layout(constraints);
        self.book.record(self.index, self.size.height);
        self.size
    }

    fn size(&self) -> crate::render::Size {
        self.size
    }

    fn paint(&self, context: &mut crate::render::PaintContext, offset: crate::render::Offset) {
        context.paint_child(&self.child, offset);
    }

    fn hit_test(
        &self,
        position: crate::render::Offset,
        result: &mut crate::render::HitTestResult,
    ) -> bool {
        self.child.hit_test(position, result)
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.child.min_intrinsic_width(height)
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.child.max_intrinsic_width(height)
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.child.min_intrinsic_height(width)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.child.max_intrinsic_height(width)
    }

    fn distance_to_baseline(&self) -> Option<f32> {
        self.child.distance_to_baseline()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scroll with room to move, for the tests below.
    fn scroll(extent: f32) -> Scroll {
        let scroll = Scroll::new();
        scroll.set_extent(extent);
        scroll
    }

    /// Runs frames at 60Hz until nothing is moving, and returns how many.
    fn settle(scroll: &mut Scroll) -> u32 {
        let mut frames = 0;
        let mut now = 1_000_000;
        while scroll.advance(now) {
            now += 16_667;
            frames += 1;
            assert!(frames < 600, "a fling should not last ten seconds");
        }
        frames
    }

    #[test]
    fn dragging_moves_by_the_delta_and_stops_at_the_ends() {
        let mut scroll = scroll(500.0);
        scroll.scroll_by(120.0);
        assert_eq!(scroll.offset, 120.0);
        scroll.scroll_by(-400.0);
        assert_eq!(scroll.offset, 0.0);
        scroll.scroll_by(9000.0);
        assert_eq!(scroll.offset, 500.0);
    }

    #[test]
    fn a_fling_keeps_going_after_the_finger_stops() {
        let mut scroll = scroll(5000.0);
        scroll.fling(2000.0);
        assert!(scroll.is_ballistic());

        // The first frame only starts the clock, exactly as a ticker does.
        assert!(scroll.advance(1_000_000));
        assert_eq!(scroll.offset, 0.0);

        assert!(scroll.advance(1_100_000));
        let after_a_tenth = scroll.offset;
        assert!(after_a_tenth > 100.0, "a tenth of a second in: {after_a_tenth}");

        settle(&mut scroll);
        assert!(!scroll.is_ballistic());
        // The whole 2000 px/s fling, which the simulation puts at ~647px.
        assert!(
            (scroll.offset - 647.0).abs() < 10.0,
            "should have travelled the simulation's distance, not {}",
            scroll.offset
        );
    }

    #[test]
    fn a_fling_takes_more_than_a_frame_or_two() {
        // The bug this file was written for: a swipe that moved the content
        // and then stopped dead. Whatever else changes, a fling has to be
        // something a person can watch.
        let mut scroll = scroll(5000.0);
        scroll.fling(2000.0);
        let frames = settle(&mut scroll);
        assert!(frames > 30, "a fling should last some frames, not {frames}");
    }

    #[test]
    fn a_fling_stops_at_the_end_of_the_content() {
        let mut scroll = scroll(200.0);
        scroll.fling(4000.0);
        settle(&mut scroll);
        assert_eq!(scroll.offset, 200.0);
        assert!(!scroll.is_ballistic(), "and does not keep asking for frames");
    }

    #[test]
    fn a_fling_from_the_end_does_not_start() {
        let mut scroll = scroll(200.0);
        scroll.jump_to(200.0);
        scroll.fling(3000.0);
        assert!(!scroll.is_ballistic());

        // But back the other way it does.
        scroll.fling(-3000.0);
        assert!(scroll.is_ballistic());
    }

    #[test]
    fn touching_the_content_stops_the_fling() {
        let mut scroll = scroll(5000.0);
        scroll.fling(3000.0);
        scroll.advance(1_000_000);
        scroll.advance(1_050_000);
        let caught = scroll.offset;
        assert!(caught > 0.0);

        scroll.stop();
        assert!(!scroll.is_ballistic());
        assert!(!scroll.advance(1_100_000), "a stopped fling asks for nothing");
        assert_eq!(scroll.offset, caught, "and leaves the offset where it was");
    }

    #[test]
    fn dragging_during_a_fling_takes_over() {
        let mut scroll = scroll(5000.0);
        scroll.fling(3000.0);
        scroll.advance(1_000_000);
        scroll.advance(1_050_000);
        let caught = scroll.offset;

        scroll.scroll_by(-20.0);
        assert!(!scroll.is_ballistic());
        assert_eq!(scroll.offset, caught - 20.0);
    }

    #[test]
    fn a_fling_with_no_velocity_does_nothing() {
        let mut scroll = scroll(500.0);
        scroll.fling(0.0);
        assert!(!scroll.is_ballistic());
        assert!(!scroll.advance(1_000_000));
    }

    #[test]
    fn a_fling_survives_a_late_frame() {
        // Frames are on demand and the device is not always fast. A gap does
        // not break the fling; it advances by the time that actually passed.
        let mut scroll = scroll(5000.0);
        scroll.fling(2000.0);
        scroll.advance(1_000_000);
        assert!(scroll.advance(1_300_000));
        let late = scroll.offset;

        let mut steady = self::scroll(5000.0);
        steady.fling(2000.0);
        steady.advance(1_000_000);
        for step in 1..=18 {
            steady.advance(1_000_000 + step * 16_667);
        }
        assert!(
            (late - steady.offset).abs() < 20.0,
            "a late frame should land where the steady ones did: {late} against {}",
            steady.offset
        );
    }

    // -- Lazy lists -----------------------------------------------------------

    #[test]
    fn a_lazy_list_builds_a_screenful_and_not_a_hundred_thousand() {
        let window = item_window(100_000, 50.0, 0.0, 800.0, DEFAULT_CACHE_EXTENT)
            .expect("a list with items in it");
        assert_eq!(window.first, 0);
        // 800 of viewport plus 250 of cache, in 50-pixel rows.
        assert_eq!(window.last, 20);
        assert_eq!(window.len(), 21);
    }

    #[test]
    fn scrolling_moves_the_window_rather_than_growing_it() {
        let top = item_window(1000, 50.0, 0.0, 500.0, 0.0).expect("items");
        let down = item_window(1000, 50.0, 5000.0, 500.0, 0.0).expect("items");
        assert_eq!(top.first, 0);
        assert_eq!(down.first, 100);
        assert_eq!(down.len(), top.len(), "the same number of rows, further down");
    }

    #[test]
    fn the_cache_reaches_behind_as_well_as_ahead() {
        let plain = item_window(1000, 50.0, 5000.0, 500.0, 0.0).expect("items");
        let cached = item_window(1000, 50.0, 5000.0, 500.0, 250.0).expect("items");
        assert!(cached.first < plain.first, "nothing built above the fold");
        assert!(cached.last > plain.last, "nothing built below it either");
    }

    #[test]
    fn the_window_stops_at_the_ends_of_the_list() {
        let window = item_window(5, 50.0, 0.0, 800.0, 250.0).expect("items");
        assert_eq!(window.first, 0);
        assert_eq!(window.last, 4, "there is no item five to build");

        let past_the_end = item_window(5, 50.0, 10_000.0, 800.0, 250.0).expect("items");
        assert_eq!(past_the_end.last, 4);
        assert!(past_the_end.first <= 4);
    }

    #[test]
    fn an_empty_list_has_no_window() {
        assert!(item_window(0, 50.0, 0.0, 800.0, 250.0).is_none());
        assert!(item_window(10, 0.0, 0.0, 800.0, 250.0).is_none(), "no extent, no arithmetic");
    }

    #[test]
    fn an_offset_a_hair_off_a_boundary_does_not_flicker() {
        // What upstream's precisionErrorTolerance is for: a fling leaves the
        // offset a rounding error away from an item boundary, and the window
        // must not gain and lose a row between one frame and the next.
        let exact = item_window(100, 50.0, 500.0, 500.0, 0.0).expect("items");
        let hair = item_window(100, 50.0, 500.000_01, 500.0, 0.0).expect("items");
        assert_eq!(exact, hair);
    }

    #[test]
    fn a_lazy_list_reserves_the_space_of_what_it_did_not_build() {
        use crate::framework::{ElementTree, component, leaf};
        use crate::render::{BoxConstraints, RenderBox};
        use crate::widgets::SizedBox;

        let mut tree = ElementTree::new();
        tree.rebuild(component(
            LazyList::new(1000, 50.0, |_| leaf(|| SizedBox::new(100.0, 50.0)))
                .with_offset(0.0)
                .with_viewport(500.0),
        ));
        let mut root = tree.build_render_tree().expect("a mounted root");
        let size = root.layout(BoxConstraints::new(0.0, 300.0, 0.0, f32::INFINITY));
        // Fifty thousand pixels of list, whatever was actually built.
        assert_eq!(size.height, 50_000.0);
    }

    #[test]
    fn a_spaced_list_is_as_tall_as_its_own_content() {
        // The gap between two rows belongs to the column that puts it there.
        // Counting it in the spacers as well made a list two gaps taller than
        // the sum of what is in it, which is invisible until the scrollbar is
        // asked where the bottom is.
        use crate::framework::{ElementTree, component, leaf};
        use crate::render::{BoxConstraints, RenderBox};
        use crate::widgets::SizedBox;

        let mut tree = ElementTree::new();
        tree.rebuild(component(
            LazyList::new(100, 40.0, |_| leaf(|| SizedBox::new(100.0, 40.0)))
                .with_spacing(8.0)
                .with_offset(1000.0)
                .with_viewport(400.0),
        ));
        let mut root = tree.build_render_tree().expect("a mounted root");
        let size = root.layout(BoxConstraints::new(0.0, 300.0, 0.0, f32::INFINITY));
        // A hundred rows of forty, with ninety-nine gaps of eight between them.
        assert_eq!(size.height, 100.0 * 40.0 + 99.0 * 8.0);
    }

    // -- Items that are not all the same height ----------------------------

    #[test]
    fn a_book_with_nothing_in_it_falls_back_to_the_guess() {
        let book = ExtentBook::new();
        assert_eq!(book.known(), 0);
        assert_eq!(book.average(72.0), 72.0);
        assert_eq!(book.content_extent(10, 72.0, 0.0), 720.0);
    }

    #[test]
    fn an_unmeasured_item_is_worth_the_average_of_the_measured_ones() {
        // Upstream's `_extrapolateMaxScrollOffset`: take what the children you
        // have are worth and spread it over the ones you do not.
        let book = ExtentBook::new();
        book.record(0, 100.0);
        book.record(1, 50.0);
        assert_eq!(book.average(999.0), 75.0, "the guess stops mattering");
        // Two measured, eight at the average.
        assert_eq!(book.content_extent(10, 999.0, 0.0), 100.0 + 50.0 + 8.0 * 75.0);
    }

    #[test]
    fn a_measurement_that_says_the_same_thing_is_not_a_change() {
        let book = ExtentBook::new();
        book.record(3, 40.0);
        let after_first = book.revision();
        book.record(3, 40.0);
        assert_eq!(book.revision(), after_first, "nothing was learned");
        book.record(3, 41.0);
        assert!(book.revision() > after_first, "and something was");
        assert_eq!(book.known(), 1, "the same item, remeasured");
        assert_eq!(book.average(0.0), 41.0, "and the total followed it");
    }

    #[test]
    fn the_window_walks_past_the_tall_ones() {
        // The whole difference from a fixed extent: which items are on screen
        // is not arithmetic, because item zero being tall pushes item one down.
        let book = ExtentBook::new();
        book.record(0, 300.0);
        book.record(1, 20.0);
        book.record(2, 20.0);
        let window = book.window(10, 0.0, 100.0, 0.0, 50.0, 0.0).expect("items");
        assert_eq!(window.first, 0);
        assert_eq!(window.last, 0, "one very tall row fills the viewport");

        // Below the tall one, the short ones fit several to a screen.
        let window = book.window(10, 300.0, 100.0, 0.0, 50.0, 0.0).expect("items");
        assert_eq!(window.first, 1);
        assert!(window.last >= 3, "several short rows fit where one tall one did");
    }

    #[test]
    fn an_empty_variable_list_has_no_window() {
        let book = ExtentBook::new();
        assert!(book.window(0, 0.0, 800.0, 250.0, 50.0, 0.0).is_none());
    }

    #[test]
    fn a_variable_list_measures_what_it_built() {
        use crate::framework::{ElementTree, component, leaf};
        use crate::render::{BoxConstraints, RenderBox};
        use crate::widgets::SizedBox;

        // Every third row is twice as tall.
        let height = |index: usize| if index % 3 == 0 { 80.0 } else { 40.0 };
        let book = ExtentBook::new();
        let mut tree = ElementTree::new();
        tree.rebuild(component(
            VariableExtentList::new(60, book.clone(), move |index| {
                leaf(move || SizedBox::new(100.0, height(index)))
            })
            .with_estimate(50.0)
            .with_offset(0.0)
            .with_viewport(200.0)
            .with_cache_extent(0.0),
        ));
        let mut root = tree.build_render_tree().expect("a mounted root");
        root.layout(BoxConstraints::new(0.0, 300.0, 0.0, f32::INFINITY));

        assert!(book.known() > 0, "nothing was measured");
        assert!(book.known() < 60, "everything was built, so it was not lazy");
        assert_eq!(book.measured(0), Some(80.0));
        assert_eq!(book.measured(1), Some(40.0));
    }

    #[test]
    fn a_variable_list_learns_the_whole_list_by_being_scrolled_through_it() {
        // The list starts with one guess for every row and ends holding the
        // measurements. Nothing corrects it inside a single frame -- upstream
        // corrects mid-layout with `scrollOffsetCorrection`, and this cannot,
        // because the window is chosen before anything is measured. What it
        // does instead is be right by the next frame, which is the lag this
        // framework already has for `Scroll::extent`.
        //
        // The estimate is not monotonic on the way there, and neither is
        // upstream's: a sample that happens to be all short rows says the list
        // is shorter than it is, and the next tall row it meets corrects it
        // upwards. What is monotonic is what has been measured.
        use crate::framework::{ElementTree, component, leaf};
        use crate::render::{BoxConstraints, RenderBox};
        use crate::widgets::SizedBox;

        let height = |index: usize| if index % 3 == 0 { 80.0 } else { 40.0 };
        let truth: f32 = (0..60).map(height).sum();

        let book = ExtentBook::new();
        let list = |offset: f32, book: ExtentBook| {
            component(
                VariableExtentList::new(60, book, move |index| {
                    leaf(move || SizedBox::new(100.0, height(index)))
                })
                .with_estimate(50.0)
                .with_offset(offset)
                .with_viewport(200.0)
                .with_cache_extent(0.0),
            )
        };

        let mut tree = ElementTree::new();
        let mut known = 0;
        let mut offset = 0.0;
        for _ in 0..40 {
            tree.rebuild(list(offset, book.clone()));
            let mut root = tree.build_render_tree().expect("a mounted root");
            root.layout(BoxConstraints::new(0.0, 300.0, 0.0, f32::INFINITY));
            assert!(book.known() >= known, "a measurement was forgotten");
            known = book.known();
            // Follow the list down as it is currently understood, which is what
            // a reader dragging the scrollbar to the bottom would do.
            offset = (offset + 120.0).min(book.content_extent(60, 50.0, 0.0) - 200.0);
        }

        assert_eq!(book.known(), 60, "the whole list was scrolled past");
        assert_eq!(
            book.content_extent(60, 50.0, 0.0),
            truth,
            "with nothing left to estimate the answer is not an estimate"
        );
    }

    #[test]
    fn a_variable_list_reserves_the_space_of_what_it_did_not_build() {
        use crate::framework::{ElementTree, component, leaf};
        use crate::render::{BoxConstraints, RenderBox};
        use crate::widgets::SizedBox;

        // All the same height, so the answer is one nobody has to estimate.
        let book = ExtentBook::new();
        let mut tree = ElementTree::new();
        tree.rebuild(component(
            VariableExtentList::new(500, book.clone(), |_| {
                leaf(|| SizedBox::new(100.0, 40.0))
            })
            .with_estimate(40.0)
            .with_offset(0.0)
            .with_viewport(400.0),
        ));
        let mut root = tree.build_render_tree().expect("a mounted root");
        let size = root.layout(BoxConstraints::new(0.0, 300.0, 0.0, f32::INFINITY));
        assert_eq!(size.height, 500.0 * 40.0);
    }

    #[test]
    fn an_item_keeps_its_state_when_the_window_moves() {
        use crate::framework::{ElementTree, component, leaf};
        use crate::widgets::SizedBox;

        let build = |offset: f32| {
            component(
                LazyList::new(100, 50.0, |_| leaf(|| SizedBox::new(10.0, 50.0)))
                    .with_offset(offset)
                    .with_viewport(200.0)
                    .with_cache_extent(0.0),
            )
        };
        let mut tree = ElementTree::new();
        // Below the first row, so that both windows have a spacer standing in
        // for what is above them -- at the very top there is nothing to stand
        // in for and the list does not build one.
        tree.rebuild(build(50.0));
        let before = tree.len();

        // One row further down: three of the four rows are the same rows.
        tree.rebuild(build(100.0));
        assert_eq!(tree.len(), before, "the window is the same size, so is the tree");
    }
}
