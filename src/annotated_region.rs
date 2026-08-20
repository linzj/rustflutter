// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Leaving a value behind for whoever is above to find.
//!
//! Upstream's `widgets/annotated_region.dart` and the `AnnotatedRegionLayer` in
//! `rendering/layer.dart`.
//!
//! # It answers a question inheritance cannot
//!
//! An inherited value flows **down**: a widget asks its ancestors. An
//! annotation flows **up** by position: something outside the tree asks "what
//! is at this point on the screen", and gets what the innermost widget covering
//! that point left there.
//!
//! The status bar style is the case it exists for. Whether the clock and battery
//! are drawn light or dark depends on what is underneath them, and what is
//! underneath them is whatever widget happens to be there -- which the shell
//! cannot reach by walking down from the root, because "underneath the status
//! bar" is a position and not a place in the tree.
//!
//! # Innermost wins, and the search stops where it is told
//!
//! The walk is back-to-front, so the last thing painted is asked first, and it
//! stops at the first answer when only one was wanted. An **opaque** region
//! stops it whatever was wanted -- it says "everything below me is covered", and
//! a region that did not could hand back a status bar style from a page hidden
//! behind a dialog.

use crate::engine::Rect;
use crate::render::{Offset, Size};

/// One thing found at a point, and where the point was inside it.
///
/// Upstream `AnnotationEntry`. The local position rides along because the
/// searcher asked in root coordinates and each region knows its own offset --
/// so the answer says *where in this region* the point fell, which is what a
/// caller wanting to act on the hit needs.
#[derive(Clone, Debug, PartialEq)]
pub struct AnnotationEntry<T> {
    pub annotation: T,
    pub local_position: Offset,
}

/// Upstream `AnnotationResult`: what a search collected, innermost first.
#[derive(Clone, Debug)]
pub struct AnnotationResult<T> {
    pub entries: Vec<AnnotationEntry<T>>,
}

impl<T> Default for AnnotationResult<T> {
    fn default() -> AnnotationResult<T> {
        AnnotationResult {
            entries: Vec::new(),
        }
    }
}

impl<T> AnnotationResult<T> {
    pub fn new() -> AnnotationResult<T> {
        AnnotationResult::default()
    }

    /// The innermost answer, which is the one a caller asking for "the" value
    /// wants.
    pub fn first(&self) -> Option<&AnnotationEntry<T>> {
        self.entries.first()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Upstream `AnnotatedRegionLayer`: a value left at a place on the screen.
///
/// This crate composites through the engine rather than through a layer tree it
/// owns, so this is not a layer -- it is the region itself, and
/// [`AnnotatedRegions`] is the set of them a frame left behind.
#[derive(Clone, Debug, PartialEq)]
pub struct AnnotatedRegionLayer<T> {
    pub value: T,
    /// Where the region is. `None` means unbounded -- upstream's `size` is
    /// nullable and a null one matches every point, which is how a region that
    /// does not know its own extent still answers.
    pub size: Option<Size>,
    pub offset: Offset,
    /// Upstream's `opaque`: whether this region hides what is behind it.
    pub opaque: bool,
}

impl<T: Clone> AnnotatedRegionLayer<T> {
    pub fn new(value: T) -> AnnotatedRegionLayer<T> {
        AnnotatedRegionLayer {
            value,
            size: None,
            offset: Offset::ZERO,
            opaque: false,
        }
    }

    /// Upstream's `sized: true`, which is `AnnotatedRegion`'s default: the
    /// region covers exactly the box it wraps.
    pub fn sized(mut self, offset: Offset, size: Size) -> Self {
        self.offset = offset;
        self.size = Some(size);
        self
    }

    pub fn with_opaque(mut self, opaque: bool) -> Self {
        self.opaque = opaque;
        self
    }

    /// Whether `position`, in the searcher's coordinates, is inside this region.
    ///
    /// An unbounded region contains everything -- see [`AnnotatedRegionLayer::size`].
    pub fn contains(&self, position: Offset) -> bool {
        let Some(size) = self.size else {
            return true;
        };
        Rect::xywh(self.offset.dx, self.offset.dy, size.width, size.height).contains(position)
    }
}

/// The annotated regions a frame left behind, innermost last.
///
/// Upstream's search walks its layer tree and this walks a list, because the
/// list *is* the order: regions are appended as they are painted, so the last
/// one added is the frontmost.
#[derive(Clone, Debug)]
pub struct AnnotatedRegions<T> {
    regions: Vec<AnnotatedRegionLayer<T>>,
}

impl<T> Default for AnnotatedRegions<T> {
    fn default() -> AnnotatedRegions<T> {
        AnnotatedRegions {
            regions: Vec::new(),
        }
    }
}

impl<T: Clone> AnnotatedRegions<T> {
    pub fn new() -> AnnotatedRegions<T> {
        AnnotatedRegions::default()
    }

    /// Records a region. Painted order, so later means in front.
    pub fn push(&mut self, region: AnnotatedRegionLayer<T>) {
        self.regions.push(region);
    }

    pub fn clear(&mut self) {
        self.regions.clear();
    }

    pub fn len(&self) -> usize {
        self.regions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    /// Upstream's `findAnnotations`.
    ///
    /// Back to front, so the innermost -- the last painted -- answers first.
    /// Two things stop the walk, and they are not the same thing:
    ///
    /// * **`only_first`**, which is the caller saying one answer is enough;
    /// * **an opaque region**, which is the *region* saying nothing behind it
    ///   counts. That one stops the walk even when the caller asked for
    ///   everything, and it is what keeps a dialog's status-bar style from
    ///   being mixed with the page's underneath.
    ///
    /// A region whose rectangle does not contain the point is skipped without
    /// stopping anything, opaque or not: upstream returns before the opacity is
    /// consulted.
    pub fn find(&self, position: Offset, only_first: bool) -> AnnotationResult<T> {
        let mut result = AnnotationResult::new();
        for region in self.regions.iter().rev() {
            if !region.contains(position) {
                continue;
            }
            result.entries.push(AnnotationEntry {
                annotation: region.value.clone(),
                local_position: Offset::new(
                    position.dx - region.offset.dx,
                    position.dy - region.offset.dy,
                ),
            });
            if only_first || region.opaque {
                break;
            }
        }
        result
    }

    /// The innermost value at a point, which is what a caller nearly always
    /// wants. Upstream's `Layer.find`.
    pub fn find_one(&self, position: Offset) -> Option<T> {
        self.find(position, true)
            .entries
            .into_iter()
            .next()
            .map(|entry| entry.annotation)
    }
}

/// Upstream `AnnotatedRegion`: the widget that leaves the value.
///
/// Upstream is a `SingleChildRenderObjectWidget` making a
/// `RenderAnnotatedRegion`; here it is the description, and a caller records it
/// into the frame's [`AnnotatedRegions`] during paint. The `sized` flag is the
/// one decision it carries.
#[derive(Clone, Debug, PartialEq)]
pub struct AnnotatedRegion<T> {
    pub value: T,
    /// Upstream's `sized`, **true by default**: the region is exactly the child
    /// it wraps. False makes it unbounded, which is for a value that describes
    /// the whole screen rather than a part of it.
    pub sized: bool,
}

impl<T: Clone> AnnotatedRegion<T> {
    pub fn new(value: T) -> AnnotatedRegion<T> {
        AnnotatedRegion { value, sized: true }
    }

    pub fn with_sized(mut self, sized: bool) -> Self {
        self.sized = sized;
        self
    }

    /// The region this describes, once the child's geometry is known.
    pub fn to_layer(&self, offset: Offset, size: Size) -> AnnotatedRegionLayer<T> {
        let layer = AnnotatedRegionLayer::new(self.value.clone());
        if self.sized {
            layer.sized(offset, size)
        } else {
            layer
        }
    }
}
