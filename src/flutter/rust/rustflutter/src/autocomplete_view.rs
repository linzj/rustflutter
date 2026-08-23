//! The options view: `autocomplete.rs`'s list, hosted under the field.
//!
//! `autocomplete.rs` decides everything about *whether* a list is showing --
//! focus, whether there is anything to show, the highlight, the keyboard
//! intents, and that losing focus always closes it -- and its own module header
//! said what was missing: the entry list and the portal's z-ordering existed,
//! and nothing hosted the widgets.
//!
//! # It is a portal, and it has to be
//!
//! The options list belongs to the field. It has to build where the field is,
//! so that it inherits the field's `Theme` and `Directionality` -- and it has to
//! *render* at the root, so it is not clipped by whatever the field is inside.
//! That is `OverlayPortal` and nothing else, which is what upstream uses too.
//!
//! # Below the field, or above it
//!
//! Upstream's `optionsViewOpenDirection` is the choice, and the reason it is a
//! choice rather than a calculation is that a field near the bottom of a scroll
//! view is a perfectly ordinary place to be. The list is anchored to the
//! field's rectangle, so "below" means below *where the field ended up*, which
//! is a question only [`crate::render::RenderRef::transform_to`] can answer.

use std::cell::RefCell;
use std::rc::Rc;

use crate::autocomplete::OptionsViewOpenDirection;
use crate::framework::{AnyWidget, many};
use crate::render::{BoxConstraints, Offset, Size};
use crate::theatre::{Anchor, Placement, PortalController, anchored, overlay_portal};

/// Upstream `Autocomplete.optionsMaxHeight`.
pub const DEFAULT_OPTIONS_MAX_HEIGHT: f32 = 200.0;

/// Where an options list goes against its field.
///
/// Below the field by default, aligned to its left edge and matching its width
/// -- upstream's options view is wrapped in a `SizedBox(width: fieldWidth)` for
/// exactly that reason: a list of completions that was wider or narrower than
/// the thing it completes reads as a different control.
pub fn options_placement(direction: OptionsViewOpenDirection, max_height: f32) -> Placement {
    Rc::new(
        move |field: crate::engine::Rect, list: Size, overlay: Size| {
            let height = list.height.min(max_height);
            let x = field.left.clamp(0.0, (overlay.width - list.width).max(0.0));
            // Upstream measures the room in the field's own coordinates --
            // `spaceAbove = -overlayRectInField.top` and `spaceBelow =
            // overlayRectInField.bottom - fieldSize.height`. In this port's
            // overlay coordinates those are the gap above the field and the
            // gap below it, which is what `MostSpace` compares.
            let space_above = field.top;
            let space_below = overlay.height - field.bottom;
            let y = if direction.opens_upward(space_above, space_below) {
                field.top - height
            } else {
                field.bottom
            };
            // Kept on screen either way. A list that ran off the bottom would show
            // the caller the one completion they could already see.
            let y = y.clamp(0.0, (overlay.height - height).max(0.0));
            Offset::new(x, y)
        },
    )
}

/// The field, and its options list above or below it.
pub struct AutocompleteView {
    controller: PortalController,
    anchor: Anchor,
    field: RefCell<Option<AnyWidget>>,
    options: Rc<dyn Fn() -> AnyWidget>,
    direction: OptionsViewOpenDirection,
    max_height: f32,
}

impl AutocompleteView {
    pub fn new(field: AnyWidget, options: impl Fn() -> AnyWidget + 'static) -> AutocompleteView {
        AutocompleteView {
            controller: PortalController::new(),
            anchor: Anchor::new(),
            field: RefCell::new(Some(field)),
            options: Rc::new(options),
            direction: OptionsViewOpenDirection::Down,
            max_height: DEFAULT_OPTIONS_MAX_HEIGHT,
        }
    }

    pub fn with_direction(mut self, direction: OptionsViewOpenDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Upstream's `optionsMaxHeight`, which is a **cap** and not a height: a
    /// short list is short, and only a long one is cut off and scrolled.
    pub fn with_max_height(mut self, max_height: f32) -> Self {
        self.max_height = max_height;
        self
    }

    /// The controller, which is what `AutocompleteState`'s decisions drive:
    /// [`crate::autocomplete::AutocompleteState::can_show_options_view`] is the
    /// question, and `show`/`hide` are the answer.
    pub fn controller(&self) -> PortalController {
        self.controller.clone()
    }

    pub fn build(self) -> AnyWidget {
        let AutocompleteView {
            controller,
            anchor,
            field,
            options,
            direction,
            max_height,
        } = self;
        let field = field.borrow_mut().take().expect("a view has a field");

        // The anchor is the field, recorded from its own assemble.
        let anchor_for_field = anchor.clone();
        let field = many(vec![field], move |mut rendered| {
            let child = rendered.pop().expect("the field");
            anchor_for_field.set(child.clone());
            crate::theatre::RenderPortal::new(child)
        });

        let place = options_placement(direction, max_height);
        overlay_portal(controller, field, move || {
            let list = capped(max_height, (options)());
            anchored(anchor.clone(), Rc::clone(&place), list)
        })
    }
}

/// The options list, no taller than its cap.
fn capped(max_height: f32, list: AnyWidget) -> AnyWidget {
    many(vec![list], move |mut rendered| {
        crate::render::RenderConstrainedBox::new(BoxConstraints::new(
            0.0,
            f32::INFINITY,
            0.0,
            max_height,
        ))
        .with_child(rendered.pop().expect("the options list"))
    })
}

/// Builds an autocomplete field with its options list.
pub fn autocomplete_view(
    field: AnyWidget,
    options: impl Fn() -> AnyWidget + 'static,
) -> (AnyWidget, PortalController) {
    let view = AutocompleteView::new(field, options);
    let controller = view.controller();
    (view.build(), controller)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{BuildContext, Component, ElementTree};
    use crate::render::{
        Alignment, HitTestResult, RenderAlign, RenderBox, RenderConstrainedBox, RenderPadding,
        RenderRef,
    };
    use crate::theatre::{RenderAnchored, overlay};

    fn leaf(width: f32, height: f32) -> AnyWidget {
        crate::framework::leaf(move || RenderConstrainedBox::tight(width, height))
    }

    fn laid_out(tree: &mut ElementTree) -> RenderRef {
        let root = tree.build_render_tree().expect("a mounted root");
        crate::render::schedule_root_layout(&root, BoxConstraints::tight(800.0, 600.0));
        crate::render::flush_layout();
        let mut discard = HitTestResult::new();
        root.hit_test(Offset::new(1.0, 1.0), &mut discard);
        root
    }

    fn list_offset(root: &RenderRef) -> Option<Offset> {
        fn walk(handle: &RenderRef, found: &mut Option<Offset>) {
            if found.is_some() {
                return;
            }
            let kids: Vec<RenderRef> = handle.with(|object| {
                if let Some(anchored) = object.as_any().downcast_ref::<RenderAnchored>() {
                    *found = Some(anchored.placed());
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
        found
    }

    /// A field 200 x 30, `inset` down and across the page.
    fn mounted(inset: f32, direction: OptionsViewOpenDirection) -> (ElementTree, PortalController) {
        let slot: Rc<RefCell<Option<PortalController>>> = Rc::new(RefCell::new(None));

        struct Page {
            inset: f32,
            direction: OptionsViewOpenDirection,
            slot: Rc<RefCell<Option<PortalController>>>,
        }

        impl Component for Page {
            fn build(&self, _context: &mut BuildContext) -> AnyWidget {
                let view = AutocompleteView::new(leaf(200.0, 30.0), || leaf(200.0, 500.0))
                    .with_direction(self.direction);
                *self.slot.borrow_mut() = Some(view.controller());
                let inset = self.inset;
                many(vec![view.build()], move |mut rendered| {
                    RenderPadding::new(
                        crate::render::EdgeInsets::only(inset, inset, 0.0, 0.0),
                        RenderAlign::new(
                            Alignment::new(-1.0, -1.0),
                            rendered.pop().expect("the field"),
                        ),
                    )
                })
            }
        }

        let mut tree = ElementTree::new();
        tree.rebuild(overlay(crate::framework::component(Page {
            inset,
            direction,
            slot: Rc::clone(&slot),
        })));
        tree.build_render_tree();
        let controller = slot.borrow().clone().expect("a controller");
        (tree, controller)
    }

    #[test]
    fn the_list_is_not_there_until_the_field_asks_for_it() {
        let (mut tree, controller) = mounted(100.0, OptionsViewOpenDirection::Down);
        assert!(!controller.is_showing());
        assert_eq!(list_offset(&laid_out(&mut tree)), None);
    }

    #[test]
    fn the_list_opens_directly_below_the_field() {
        // Field at (100, 100), 200 x 30, so its bottom edge is 130.
        let (mut tree, controller) = mounted(100.0, OptionsViewOpenDirection::Down);
        controller.show();
        tree.rebuild_dirty();
        let at = list_offset(&laid_out(&mut tree)).expect("open");

        assert_eq!(at.dx, 100.0, "aligned to the field's left edge");
        assert_eq!(at.dy, 130.0, "and hanging off its bottom");
    }

    #[test]
    fn opening_upwards_puts_it_above_the_field() {
        // The same field, asked to open up: the list is 200 tall after the cap,
        // so it starts 200 above the field's top.
        let (mut tree, controller) = mounted(300.0, OptionsViewOpenDirection::Up);
        controller.show();
        tree.rebuild_dirty();
        let at = list_offset(&laid_out(&mut tree)).expect("open");

        assert_eq!(at.dy, 100.0, "300 - 200");
        assert!(at.dy < 300.0, "above the field, not below it");
    }

    #[test]
    fn the_list_follows_the_field_across_the_page() {
        // The claim the whole module rests on: "below" means below where the
        // field ended up, which only transform_to can answer.
        let (mut near_tree, near) = mounted(50.0, OptionsViewOpenDirection::Down);
        near.show();
        near_tree.rebuild_dirty();
        let near_at = list_offset(&laid_out(&mut near_tree)).expect("open");

        let (mut far_tree, far) = mounted(250.0, OptionsViewOpenDirection::Down);
        far.show();
        far_tree.rebuild_dirty();
        let far_at = list_offset(&laid_out(&mut far_tree)).expect("open");

        assert_eq!(near_at, Offset::new(50.0, 80.0));
        assert_eq!(far_at, Offset::new(250.0, 280.0));
    }

    #[test]
    fn a_field_near_the_bottom_keeps_its_list_on_screen() {
        // A list that ran off the bottom would show the caller the one
        // completion they could already see.
        let (mut tree, controller) = mounted(560.0, OptionsViewOpenDirection::Down);
        controller.show();
        tree.rebuild_dirty();
        let at = list_offset(&laid_out(&mut tree)).expect("open");

        assert!(at.dy + 200.0 <= 600.0, "pulled back up to fit: {at:?}");
    }

    #[test]
    fn the_max_height_is_a_cap_and_not_a_height() {
        let place = options_placement(OptionsViewOpenDirection::Down, 200.0);
        let field = crate::engine::Rect::xywh(0.0, 100.0, 200.0, 30.0);
        let overlay = Size::new(800.0, 600.0);

        // A short list keeps its own height, so it opens at the field's bottom.
        let short = place(field, Size::new(200.0, 40.0), overlay);
        assert_eq!(short.dy, 130.0);

        // A long one is capped, and still opens at the field's bottom.
        let long = place(field, Size::new(200.0, 900.0), overlay);
        assert_eq!(long.dy, 130.0);
    }

    #[test]
    fn closing_the_field_takes_the_list_with_it() {
        // `AutocompleteState` says losing focus always closes the list; this is
        // that decision reaching the screen.
        let (mut tree, controller) = mounted(100.0, OptionsViewOpenDirection::Down);
        controller.show();
        tree.rebuild_dirty();
        assert!(list_offset(&laid_out(&mut tree)).is_some());

        controller.hide();
        tree.rebuild_dirty();
        assert_eq!(list_offset(&laid_out(&mut tree)), None);
    }

    #[test]
    fn the_states_question_drives_the_controller() {
        // The two halves meeting: autocomplete.rs decides, this shows.
        let mut state = crate::autocomplete::AutocompleteState::<String>::new();
        assert!(!state.can_show_options_view(), "no focus, nothing to show");

        state.set_focus(true);
        assert!(
            !state.can_show_options_view(),
            "focus alone is not enough -- there has to be something in the list"
        );
    }
}

#[cfg(test)]
mod open_direction_tests {
    use super::options_placement;
    use crate::autocomplete::OptionsViewOpenDirection;
    use crate::engine::Rect;
    use crate::render::Size;

    /// A field 20 tall sitting `top` down a 400-tall overlay.
    fn field_at(top: f32) -> Rect {
        Rect {
            left: 0.0,
            top,
            right: 100.0,
            bottom: top + 20.0,
        }
    }

    fn place(direction: OptionsViewOpenDirection, top: f32) -> f32 {
        let placement = options_placement(direction, 200.0);
        placement(
            field_at(top),
            Size::new(100.0, 80.0),
            Size::new(400.0, 400.0),
        )
        .dy
    }

    #[test]
    fn the_two_fixed_directions_ignore_the_room_entirely() {
        // A field told to open upward opens upward with nothing above it, and
        // upstream lets it: `up => true` reads neither argument.
        assert!(OptionsViewOpenDirection::Up.opens_upward(0.0, 400.0));
        assert!(!OptionsViewOpenDirection::Down.opens_upward(400.0, 0.0));
    }

    #[test]
    fn and_most_space_is_a_question_rather_than_an_answer() {
        assert!(OptionsViewOpenDirection::MostSpace.opens_upward(300.0, 100.0));
        assert!(!OptionsViewOpenDirection::MostSpace.opens_upward(100.0, 300.0));
    }

    #[test]
    fn an_even_split_opens_downward() {
        // The comparison is strict, so a tie keeps the default rather than
        // being decided by nothing.
        assert!(!OptionsViewOpenDirection::MostSpace.opens_upward(150.0, 150.0));
        assert_eq!(
            OptionsViewOpenDirection::MostSpace.opens_upward(150.0, 150.0),
            OptionsViewOpenDirection::Down.opens_upward(150.0, 150.0)
        );
    }

    #[test]
    fn the_height_is_the_room_on_the_side_that_won() {
        assert_eq!(
            OptionsViewOpenDirection::MostSpace.max_height(300.0, 100.0),
            300.0
        );
        assert_eq!(
            OptionsViewOpenDirection::MostSpace.max_height(100.0, 300.0),
            300.0
        );
        // Which is to say: most space really does get the most space.
        for (above, below) in [(10.0, 90.0), (90.0, 10.0), (50.0, 50.0)] {
            assert_eq!(
                OptionsViewOpenDirection::MostSpace.max_height(above, below),
                above.max(below),
                "{above} {below}"
            );
        }
        // A fixed direction takes what its side has, however little.
        assert_eq!(OptionsViewOpenDirection::Up.max_height(10.0, 390.0), 10.0);
    }

    #[test]
    fn and_the_placement_follows_it() {
        // Through the real placement closure, not the predicate alone.
        // A field near the top has more room below, so MostSpace drops the
        // list below it and agrees with Down.
        assert_eq!(
            place(OptionsViewOpenDirection::MostSpace, 10.0),
            place(OptionsViewOpenDirection::Down, 10.0)
        );
        // Near the bottom it agrees with Up instead.
        assert_eq!(
            place(OptionsViewOpenDirection::MostSpace, 370.0),
            place(OptionsViewOpenDirection::Up, 370.0)
        );
        // And those two answers are not the same answer, or the test above
        // would hold for any implementation at all.
        assert_ne!(
            place(OptionsViewOpenDirection::MostSpace, 10.0),
            place(OptionsViewOpenDirection::MostSpace, 370.0)
        );
    }
}
