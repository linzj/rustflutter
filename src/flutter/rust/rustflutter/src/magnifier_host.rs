// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The loupe, put on the screen: upstream's `MagnifierController`.
//!
//! `magnifier.rs` ports every decision -- where the loupe goes, when it hides,
//! how far the focal point is pulled in, which of the two platforms is being
//! imitated -- as pure functions of the gesture and the screen. What it says it
//! cannot do is own one:
//!
//! > the rest of upstream's controller is an `OverlayEntry`'s lifetime [...]
//! > nothing hosts the widget yet
//!
//! This is the lifetime. A [`MagnifierHost`] holds an overlay entry, moves it
//! as the gesture moves, hides it when the platform says to, and takes it down
//! at the end.
//!
//! # Global in, overlay-local out
//!
//! Every placement in `magnifier.rs` answers in **global** coordinates -- the
//! gesture arrives global, the screen bounds are global, and
//! `MagnifierPlacement::position` is global. An overlay entry is laid out in
//! the *overlay's* coordinates. The two are the same only when the overlay
//! fills the window, so the host converts, once, at the point of placing:
//! [`RenderRef::global_to_local`](crate::render::RenderRef::global_to_local).
//!
//! Getting this wrong is invisible on a full-screen overlay and puts the loupe
//! at a constant wrong offset on any other, which is why the conversion is here
//! and not left to callers.
//!
//! # What the loupe cannot yet show
//!
//! A magnifier magnifies by re-sampling what is behind it, which is a backdrop
//! read with a scale. The paint bridge has one backdrop operation and it is a
//! blur (see [`crate::render::RenderBackdropFilter`]), so **the loupe's body is
//! drawn -- size, corner radius, border, shadow -- and its contents are not
//! magnified**. Everything this module decides is the part that is not the
//! pixels: where it is, whether it is up, and when it animates. The
//! magnification is one missing engine operation away, and nothing here would
//! change when it arrives.

use std::cell::Cell;
use std::rc::Rc;

use crate::engine::Rect;
use crate::framework::{AnyWidget, BuildContext, StateHandle, StatefulComponent};
use crate::magnifier::{
    CupertinoMagnifier, CupertinoTextMagnifier, Magnifier, MagnifierInfo, MagnifierPlacement,
    RawMagnifier, TextMagnifier,
};
use crate::render::{
    Offset, RenderConstrainedBox, RenderDecoratedBox, RenderRef, RenderStack, Size, StackPosition,
};
use crate::theatre::{EntryRefresh, OverlayHandle};

/// Which platform's loupe. Upstream picks between them in
/// `TextMagnifierConfiguration`, per `TargetPlatform`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MagnifierStyle {
    /// `material/magnifier.dart`'s `TextMagnifier`.
    Material,
    /// `cupertino/magnifier.dart`'s `CupertinoTextMagnifier`.
    Cupertino,
}

impl MagnifierStyle {
    /// The loupe's own size, which the two platforms do not agree on.
    pub fn size(self) -> Size {
        match self {
            MagnifierStyle::Material => Magnifier::DEFAULT_SIZE,
            MagnifierStyle::Cupertino => CupertinoMagnifier::DEFAULT_SIZE,
        }
    }

    /// Runs the platform's placement. `previous` is where the loupe was last
    /// frame; the Material one uses it to decide whether to animate, and the
    /// Cupertino one has no such rule.
    pub fn place(
        self,
        info: MagnifierInfo,
        screen: Rect,
        previous: Option<Offset>,
    ) -> MagnifierPlacement {
        match self {
            MagnifierStyle::Material => TextMagnifier::place(info, screen, previous),
            MagnifierStyle::Cupertino => CupertinoTextMagnifier::place(info, screen),
        }
    }

    /// The loupe widget itself at this platform's proportions.
    fn raw(self, extra_focal_point_offset: Offset) -> RawMagnifier {
        match self {
            MagnifierStyle::Material => RawMagnifier::new(Magnifier::DEFAULT_SIZE)
                .with_magnification_scale(Magnifier::MAGNIFICATION_SCALE)
                .with_focal_point_offset(Offset::new(
                    extra_focal_point_offset.dx,
                    extra_focal_point_offset.dy + Magnifier::STANDARD_VERTICAL_FOCAL_POINT_SHIFT,
                )),
            MagnifierStyle::Cupertino => RawMagnifier::new(CupertinoMagnifier::DEFAULT_SIZE)
                .with_magnification_scale(CupertinoMagnifier::MAGNIFICATION)
                .with_focal_point_offset(extra_focal_point_offset),
        }
    }
}

/// What the entry is showing. Shared with the host so a gesture repositions the
/// loupe without rebuilding anything above it.
#[derive(Clone, Default)]
struct LoupeState {
    at: Rc<Cell<Offset>>,
    focal: Rc<Cell<Offset>>,
    shown: Rc<Cell<bool>>,
    /// What tells the entry that the cells above changed. Without it the entry
    /// is never dirty, never rebuilds, and never reads them.
    refresh: EntryRefresh,
}

struct LoupeEntry {
    style: MagnifierStyle,
    state: LoupeState,
}

impl StatefulComponent for LoupeEntry {
    type State = u64;

    fn initial_state(&self) -> u64 {
        self.state.refresh.revision()
    }

    fn build(
        &self,
        _state: &u64,
        handle: StateHandle<u64>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        self.state.refresh.attach(handle);
        if !self.state.shown.get() {
            // Hidden is a state the entry stays mounted in, not one it is
            // removed for. Upstream's Cupertino loupe hides and un-hides during
            // a single drag -- pulled below the line and brought back up -- and
            // an entry that came and went would restart the animation each
            // time.
            return crate::framework::leaf(|| RenderConstrainedBox::tight(0.0, 0.0));
        }

        let at = self.state.at.get();
        let raw = self.style.raw(self.state.focal.get());
        crate::framework::leaf(move || {
            RenderStack::new().push_positioned(
                loupe_body(&raw),
                StackPosition {
                    left: Some(at.dx),
                    top: Some(at.dy),
                    ..StackPosition::default()
                },
            )
        })
    }
}

/// A loupe that is up. Upstream's `MagnifierController` with an entry in it.
pub struct MagnifierHost {
    style: MagnifierStyle,
    overlay: Rc<OverlayHandle>,
    entry: u64,
    state: LoupeState,
    /// Upstream's `_magnifierPosition`, null for exactly one frame so the
    /// *first* appearance is not animated. Kept here rather than in the entry
    /// because it is the controller's memory, not the widget's.
    previous: Option<Offset>,
    /// What the last placement decided, so a caller can ask without repeating
    /// the arithmetic.
    last: Option<MagnifierPlacement>,
}

impl MagnifierHost {
    /// Where the loupe is, in the overlay's coordinates.
    pub fn overlay_position(&self) -> Offset {
        self.state.at.get()
    }

    /// Whether the platform's rule says it should be on screen at all.
    pub fn is_shown(&self) -> bool {
        self.state.shown.get()
    }

    /// The last placement, or `None` before the first gesture.
    pub fn placement(&self) -> Option<MagnifierPlacement> {
        self.last
    }

    /// Upstream's `MagnifierController.show` followed by the info listener:
    /// run the platform's placement for this gesture and move the loupe there.
    ///
    /// `overlay` is the theatre's render object, which is what the global
    /// answer is converted against.
    pub fn update(&mut self, info: MagnifierInfo, screen: Rect, overlay: &RenderRef) {
        let placement = self.style.place(info, screen, self.previous);
        self.last = Some(placement);
        self.state.shown.set(placement.shown);

        if placement.shown {
            // The one conversion: global out of `place`, overlay-local into the
            // entry.
            let local = overlay.global_to_local(placement.position, None);
            self.state.at.set(local);
            self.state.focal.set(placement.extra_focal_point_offset);
            // Only a shown position is remembered: a hidden placement's
            // position is `Offset::ZERO`, and a loupe that came back would
            // otherwise ease across the screen from the origin.
            //
            // **No test can currently fail if this guard is removed**, and it
            // is worth saying so rather than leaving a confident comment over
            // unfalsifiable code: the only style that hides is Cupertino, and
            // Cupertino's placements always answer `animate: false`, so nothing
            // reads `previous` on the path that could be wrong. It stays
            // because the pairing is a fact about today's two platforms and not
            // about the rule.
            self.previous = Some(placement.position);
        }
        self.state.refresh.refresh();
    }

    /// Whether the last move should be animated rather than jumped.
    ///
    /// Upstream animates only a jump *between lines*, over
    /// `TextMagnifier::JUMP_BETWEEN_LINES_MICROS`; sliding along one line
    /// tracks the finger directly, because a loupe that eased towards the
    /// finger would lag behind it and read as unresponsive.
    pub fn animating(&self) -> bool {
        self.last.is_some_and(|placement| placement.animate)
    }

    /// Upstream's `MagnifierController.hide`, which keeps the entry: see
    /// [`LoupeEntry::build`].
    pub fn hide(&mut self) {
        self.state.shown.set(false);
        self.state.refresh.refresh();
    }

    /// Upstream's `MagnifierController.dispose`: the entry goes.
    pub fn dismiss(self) -> bool {
        self.overlay.remove(self.entry)
    }
}

/// Puts a loupe up over `overlay`. It starts hidden -- upstream's controller
/// shows nothing until the first `MagnifierInfo` arrives, because there is
/// nowhere to put a loupe before there is a gesture to follow.
pub fn show_magnifier(overlay: Rc<OverlayHandle>, style: MagnifierStyle) -> Option<MagnifierHost> {
    let state = LoupeState::default();
    let entry = {
        let state = state.clone();
        overlay.insert(move || {
            crate::framework::stateful(LoupeEntry {
                style,
                state: state.clone(),
            })
        })?
    };
    Some(MagnifierHost {
        style,
        overlay,
        entry,
        state,
        previous: None,
        last: None,
    })
}

/// How far below the line a Cupertino gesture may go before the loupe gives
/// up, re-exported here because it is the rule a caller most often needs when
/// deciding whether to keep a host alive at all.
pub const CUPERTINO_HIDE_BELOW: f32 = CupertinoTextMagnifier::HIDE_BELOW_THRESHOLD;

/// A placement's position expressed where an entry will be laid out.
///
/// Split out from [`MagnifierHost::update`] so the conversion can be tested
/// against an overlay that is deliberately not at the window's origin, which is
/// the case the two coordinate systems differ in.
pub fn in_overlay(placement: MagnifierPlacement, overlay: &RenderRef) -> Offset {
    overlay.global_to_local(placement.position, None)
}

/// The loupe's body: its size, its corners, its border and its shadows.
///
/// Everything a [`RawMagnifier`] says about how it looks *except* what shows
/// through it, which is the missing engine operation the module docs name.
pub fn loupe_body(raw: &RawMagnifier) -> RenderDecoratedBox {
    let mut body = RenderDecoratedBox::new()
        .with_corner_radius(raw.decoration.corner_radius)
        .with_border(1.0, crate::magnifier::MAGNIFIER_DEFAULT_BORDER)
        .with_child(RenderConstrainedBox::tight(raw.size.width, raw.size.height));
    if let Some(shadows) = &raw.decoration.shadows {
        body = body.with_shadows(shadows.clone());
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{Component, ElementTree};
    use crate::render::{BoxConstraints, EdgeInsets, HitTestResult, RenderBox, RenderPadding};
    use crate::theatre::{RenderTheatre, overlay};
    use std::cell::RefCell;

    /// The overlay is 40 across and 30 down from the window's origin. Without
    /// that the global answer and the overlay-local one are the same number and
    /// the conversion this module exists for cannot fail.
    const OVERLAY_ORIGIN: Offset = Offset { dx: 40.0, dy: 30.0 };

    fn screen() -> Rect {
        Rect::ltrb(0.0, 0.0, 800.0, 600.0)
    }

    fn mounted() -> (ElementTree, Rc<OverlayHandle>) {
        let slot: Rc<RefCell<Option<Rc<OverlayHandle>>>> = Rc::new(RefCell::new(None));

        struct Grab {
            slot: Rc<RefCell<Option<Rc<OverlayHandle>>>>,
        }
        impl Component for Grab {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.slot.borrow_mut() = OverlayHandle::of(context);
                crate::framework::leaf(|| RenderConstrainedBox::tight(400.0, 300.0))
            }
        }

        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::many(
            vec![overlay(crate::framework::component(Grab {
                slot: Rc::clone(&slot),
            }))],
            |mut rendered| {
                RenderPadding::new(
                    EdgeInsets::only(OVERLAY_ORIGIN.dx, OVERLAY_ORIGIN.dy, 0.0, 0.0),
                    rendered.pop().expect("the overlay"),
                )
            },
        ));
        tree.build_render_tree();
        let handle = slot.borrow().clone().expect("an overlay in scope");
        (tree, handle)
    }

    fn laid_out(tree: &mut ElementTree) -> RenderRef {
        let root = tree.build_render_tree().expect("a mounted root");
        crate::render::schedule_root_layout(&root, BoxConstraints::tight(800.0, 600.0));
        crate::render::flush_layout();
        let mut discard = HitTestResult::new();
        root.hit_test(Offset::new(1.0, 1.0), &mut discard);
        root
    }

    fn theatre_of(root: &RenderRef) -> RenderRef {
        fn walk(handle: &RenderRef, found: &mut Option<RenderRef>) {
            if found.is_some() {
                return;
            }
            let kids: Vec<RenderRef> = handle.with(|object| {
                if object.as_any().downcast_ref::<RenderTheatre>().is_some() {
                    *found = Some(handle.clone());
                }
                let mut kids = Vec::new();
                object.visit_children(&mut |child, _| {
                    if let Some(child) = child.as_any().downcast_ref::<RenderRef>() {
                        kids.push(child.clone());
                    }
                });
                kids
            });
            for child in kids {
                walk(&child, found);
            }
        }
        let mut found = None;
        walk(root, &mut found);
        found.expect("a theatre under the root")
    }

    /// A finger at `x`, `y`, with the caret on a line centred at `line`.
    fn info_at(x: f32, y: f32, line: f32) -> MagnifierInfo {
        MagnifierInfo::new(
            Offset::new(x, y),
            Rect::ltrb(x, line - 8.0, x + 2.0, line + 8.0),
            Rect::ltrb(50.0, line - 8.0, 400.0, line + 8.0),
            Rect::ltrb(50.0, 100.0, 400.0, 500.0),
        )
    }

    // -- The conversion this module exists for ------------------------------------

    #[test]
    fn the_placement_is_global_and_the_entry_is_not() {
        let (mut tree, overlay) = mounted();
        let mut host = show_magnifier(Rc::clone(&overlay), MagnifierStyle::Material)
            .expect("an overlay to put it in");
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);

        host.update(info_at(200.0, 320.0, 300.0), screen(), &theatre);
        let placement = host.placement().expect("a placement");

        assert_eq!(
            host.overlay_position(),
            Offset::new(
                placement.position.dx - OVERLAY_ORIGIN.dx,
                placement.position.dy - OVERLAY_ORIGIN.dy
            ),
            "the entry is placed in the overlay's coordinates, not the window's"
        );
        assert_ne!(
            host.overlay_position(),
            placement.position,
            "and they are different numbers, which is the whole point"
        );
    }

    #[test]
    fn in_overlay_is_the_same_conversion_the_host_does() {
        let (mut tree, overlay) = mounted();
        let mut host =
            show_magnifier(Rc::clone(&overlay), MagnifierStyle::Material).expect("an overlay");
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);

        host.update(info_at(200.0, 320.0, 300.0), screen(), &theatre);
        let placement = host.placement().expect("a placement");
        assert_eq!(in_overlay(placement, &theatre), host.overlay_position());
    }

    // -- What the placement decides, still decided by magnifier.rs -----------------

    #[test]
    fn the_loupe_tracks_the_finger_across_the_line() {
        let (mut tree, overlay) = mounted();
        let mut host =
            show_magnifier(Rc::clone(&overlay), MagnifierStyle::Material).expect("an overlay");
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);

        host.update(info_at(150.0, 320.0, 300.0), screen(), &theatre);
        let left = host.overlay_position();
        host.update(info_at(250.0, 320.0, 300.0), screen(), &theatre);
        let right = host.overlay_position();

        assert_eq!(right.dx - left.dx, 100.0, "it followed exactly");
        assert_eq!(right.dy, left.dy, "and stayed on the line");
    }

    #[test]
    fn a_cupertino_loupe_dragged_far_below_the_line_goes_away() {
        let (mut tree, overlay) = mounted();
        let mut host =
            show_magnifier(Rc::clone(&overlay), MagnifierStyle::Cupertino).expect("an overlay");
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);

        host.update(info_at(200.0, 320.0, 300.0), screen(), &theatre);
        assert!(host.is_shown(), "20 below the line is still aiming at text");

        host.update(
            info_at(200.0, 300.0 + CUPERTINO_HIDE_BELOW + 10.0, 300.0),
            screen(),
            &theatre,
        );
        assert!(!host.is_shown(), "past the threshold it has stopped aiming");
    }

    #[test]
    fn a_hidden_loupe_keeps_its_entry() {
        // It hides and un-hides within one drag; an entry that came and went
        // would restart from nothing each time.
        let (mut tree, overlay) = mounted();
        let before = overlay.entry_count();
        let mut host =
            show_magnifier(Rc::clone(&overlay), MagnifierStyle::Cupertino).expect("an overlay");
        assert_eq!(overlay.entry_count(), before + 1);
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);

        host.update(
            info_at(200.0, 300.0 + CUPERTINO_HIDE_BELOW + 10.0, 300.0),
            screen(),
            &theatre,
        );
        assert!(!host.is_shown());
        assert_eq!(overlay.entry_count(), before + 1, "hidden, not removed");

        host.update(info_at(200.0, 310.0, 300.0), screen(), &theatre);
        assert!(host.is_shown(), "and it came back");
    }

    /// How many loupe bodies the tree is actually drawing.
    ///
    /// `entry_count` alone cannot tell a hidden loupe from a visible one -- the
    /// entry is there either way, which is the point of the previous test. This
    /// walks the render tree instead, because "hidden" has to mean nothing is
    /// on the screen and not merely that a flag says so.
    fn bodies_drawn(root: &RenderRef) -> usize {
        fn walk(handle: &RenderRef, found: &mut usize) {
            let kids: Vec<RenderRef> = handle.with(|object| {
                if object
                    .as_any()
                    .downcast_ref::<RenderDecoratedBox>()
                    .is_some()
                {
                    *found += 1;
                }
                let mut kids = Vec::new();
                object.visit_children(&mut |child, _| {
                    if let Some(child) = child.as_any().downcast_ref::<RenderRef>() {
                        kids.push(child.clone());
                    }
                });
                kids
            });
            for child in kids {
                walk(&child, found);
            }
        }
        let mut found = 0;
        walk(root, &mut found);
        found
    }

    #[test]
    fn a_hidden_loupe_draws_nothing() {
        let (mut tree, overlay) = mounted();
        let mut host =
            show_magnifier(Rc::clone(&overlay), MagnifierStyle::Cupertino).expect("an overlay");
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);
        assert_eq!(bodies_drawn(&root), 0, "nothing before the first gesture");

        host.update(info_at(200.0, 310.0, 300.0), screen(), &theatre);
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        assert_eq!(bodies_drawn(&root), 1, "one loupe, on the line");

        host.update(
            info_at(200.0, 300.0 + CUPERTINO_HIDE_BELOW + 10.0, 300.0),
            screen(),
            &theatre,
        );
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        assert_eq!(
            bodies_drawn(&root),
            0,
            "hidden means off the screen, not merely a flag saying so"
        );
    }

    #[test]
    fn a_hidden_position_is_not_remembered() {
        // A loupe that animated from where it was hidden would slide across the
        // screen on reappearing -- the same reason upstream leaves
        // `_magnifierPosition` null for the first frame.
        let (mut tree, overlay) = mounted();
        let mut host =
            show_magnifier(Rc::clone(&overlay), MagnifierStyle::Material).expect("an overlay");
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);

        host.update(info_at(200.0, 320.0, 300.0), screen(), &theatre);
        let shown_at = host.overlay_position();
        host.hide();
        assert!(!host.is_shown());
        assert_eq!(
            host.overlay_position(),
            shown_at,
            "hiding does not move it, it only stops drawing it"
        );
    }

    #[test]
    fn the_first_appearance_is_not_animated() {
        // Upstream keeps `_magnifierPosition` null for exactly one frame so the
        // loupe does not slide in from wherever the last one happened to be.
        let (mut tree, overlay) = mounted();
        let mut host =
            show_magnifier(Rc::clone(&overlay), MagnifierStyle::Material).expect("an overlay");
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);

        host.update(info_at(200.0, 320.0, 300.0), screen(), &theatre);
        assert!(!host.animating(), "the first one appears where it belongs");
    }

    #[test]
    fn a_jump_between_lines_is_animated_and_a_slide_along_one_is_not() {
        let (mut tree, overlay) = mounted();
        let mut host =
            show_magnifier(Rc::clone(&overlay), MagnifierStyle::Material).expect("an overlay");
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        let theatre = theatre_of(&root);

        host.update(info_at(200.0, 320.0, 300.0), screen(), &theatre);
        host.update(info_at(260.0, 320.0, 300.0), screen(), &theatre);
        assert!(
            !host.animating(),
            "along one line it tracks the finger; easing would read as lag"
        );

        host.update(info_at(260.0, 360.0, 340.0), screen(), &theatre);
        assert!(host.animating(), "onto another line it eases across");
    }

    // -- The loupe's own proportions ------------------------------------------------

    #[test]
    fn the_two_platforms_do_not_agree_on_the_size() {
        assert_ne!(
            MagnifierStyle::Material.size(),
            MagnifierStyle::Cupertino.size()
        );
        assert_eq!(MagnifierStyle::Material.size(), Magnifier::DEFAULT_SIZE);
        assert_eq!(
            MagnifierStyle::Cupertino.size(),
            CupertinoMagnifier::DEFAULT_SIZE
        );
    }

    #[test]
    fn the_material_focal_point_carries_the_standing_vertical_shift() {
        // The loupe is drawn above the finger while showing what is under it,
        // so the focal point is pushed down by roughly the distance it was
        // lifted. The extra offset a placement asks for is added to that, not
        // substituted for it.
        let raw = MagnifierStyle::Material.raw(Offset::new(0.0, 5.0));
        assert_eq!(
            raw.focal_point_offset.dy,
            Magnifier::STANDARD_VERTICAL_FOCAL_POINT_SHIFT + 5.0
        );
    }

    #[test]
    fn the_body_is_the_loupes_size_and_its_corners() {
        let raw = RawMagnifier::new(Size::new(80.0, 40.0));
        let body = loupe_body(&raw);
        let mut body = RenderRef::new(body);
        assert_eq!(
            body.layout(BoxConstraints::new(0.0, 800.0, 0.0, 600.0)),
            Size::new(80.0, 40.0)
        );
    }

    #[test]
    fn the_loupe_goes_when_it_is_dismissed() {
        let (tree, overlay) = mounted();
        let before = overlay.entry_count();
        let host =
            show_magnifier(Rc::clone(&overlay), MagnifierStyle::Material).expect("an overlay");
        assert_eq!(overlay.entry_count(), before + 1);
        assert!(host.dismiss());
        assert_eq!(overlay.entry_count(), before);
        drop(tree);
    }

    #[test]
    fn a_loupe_with_no_gesture_yet_has_no_placement() {
        let (tree, overlay) = mounted();
        let host =
            show_magnifier(Rc::clone(&overlay), MagnifierStyle::Material).expect("an overlay");
        assert!(host.placement().is_none());
        assert!(
            !host.is_shown(),
            "nowhere to put it before there is a gesture"
        );
        drop(tree);
    }
}
