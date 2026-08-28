//! The live popup menu: `menu.rs`'s geometry and route logic, hosted.
//!
//! `menu.rs` has had `popup_menu_offset` -- upstream's
//! `_PopupMenuRouteLayout.getPositionForChild` together with `_fitInsideScreen`
//! -- since it was ported, and until now the only thing that ever called it was
//! its own unit test. It wants the button's rectangle **in the overlay's
//! coordinates**, and there was no way to ask for one: nothing hosted the menu,
//! so nothing had an overlay to be in.
//!
//! That is the plan's first named symptom, and this is the part that answers
//! it. The button records itself on an [`Anchor`]; the menu is handed to the
//! overlay by an `OverlayPortal`; and the positioner asks the anchor where the
//! button ended up.
//!
//! # A menu is modal and a tooltip is not
//!
//! The difference shows up in one place: a menu goes up with a barrier under
//! it. A tap outside closes it, and while it is open the page beneath does not
//! take presses. A tooltip has neither -- it is a label, and a label that ate
//! your clicks would be a bug. So a menu is a [`crate::theatre::show_modal`]
//! and a tooltip is a bare portal, and the anchoring is all they share.

use std::cell::RefCell;
use std::rc::Rc;

use crate::direction::TextDirection;
use crate::framework::{AnyWidget, BuildContext, ThemeCapture, many};
use crate::menu::popup_menu_offset;
use crate::modal_barrier::ModalBarrier;
use crate::render::{EdgeInsets, Offset, RenderRef, Size};
use crate::theatre::{Anchor, ModalHandle, OverlayHandle, Placement, anchored, show_modal};

/// The placement rule a popup menu uses, in the shape
/// [`crate::theatre::RenderAnchored`] asks for.
///
/// `padding` is the unsafe-area inset -- upstream's `MediaQuery.padding` --
/// which the menu keeps clear of along with its own screen padding.
pub fn menu_placement(padding: EdgeInsets, direction: TextDirection) -> Placement {
    Rc::new(
        move |anchor: crate::engine::Rect, menu: Size, overlay: Size| {
            popup_menu_offset(overlay, anchor, menu, padding, direction)
        },
    )
}

/// A button that opens a menu against itself.
///
/// The anchor is the button, so the menu grows from wherever the button ended
/// up -- inside a scrolled list, inside a transformed card, anywhere. That is
/// the thing the plan lists first among the costs of having no overlay: the
/// menu used to be pinned to whatever `Stack` the caller had put it in, and cut
/// off by any clip between.
pub struct PopupMenuButton {
    anchor: Anchor,
    child: RefCell<Option<AnyWidget>>,
    menu: Rc<dyn Fn() -> AnyWidget>,
    padding: EdgeInsets,
    direction: TextDirection,
    barrier: ModalBarrier,
    /// Upstream's `tooltip`. `None` is not "no tooltip" -- see
    /// [`PopupMenuButton::tooltip`].
    tooltip: Option<String>,
}

impl PopupMenuButton {
    pub fn new(child: AnyWidget, menu: impl Fn() -> AnyWidget + 'static) -> PopupMenuButton {
        PopupMenuButton {
            anchor: Anchor::new(),
            child: RefCell::new(Some(child)),
            menu: Rc::new(menu),
            padding: EdgeInsets::ZERO,
            direction: TextDirection::Ltr,
            // A menu's barrier paints nothing: it is there to catch the tap
            // that closes the menu, and darkening the page for a menu would be
            // heavy. `modal_barrier.rs` says the same thing on the field.
            //
            // Which is exactly why it needs a name. A dialog's scrim is
            // visibly dimmed and obviously belongs to what is in front of it;
            // this one is invisible, so a reader meeting it has nothing to go
            // on but the label.
            barrier: ModalBarrier::new().with_semantics_label(
                crate::material_app::DefaultMaterialLocalizations::MENU_DISMISS_LABEL,
            ),
            tooltip: None,
        }
    }

    /// Upstream's `PopupMenuButton.tooltip`.
    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// What the button's glyph says it does, from upstream's
    /// `widget.tooltip ?? MaterialLocalizations.of(context).showMenuTooltip`.
    ///
    /// Three dots in a corner are not self-explanatory, and this is the only
    /// thing that explains them.
    pub fn tooltip(&self) -> String {
        self.tooltip.clone().unwrap_or_else(|| {
            crate::material_app::DefaultMaterialLocalizations::SHOW_MENU_TOOLTIP.to_string()
        })
    }

    /// What the **opened menu** is announced as, from upstream's
    /// `semanticLabel ??= MaterialLocalizations.of(context).popupMenuLabel`.
    ///
    /// A different string from the button's tooltip, and a different listener
    /// moment: the tooltip explains the glyph before it is pressed, this names
    /// the thing that appeared after it was.
    pub fn menu_semantic_label(&self) -> &'static str {
        crate::material_app::DefaultMaterialLocalizations::POPUP_MENU_LABEL
    }

    /// The unsafe-area inset the menu keeps clear of.
    pub fn with_padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = padding;
        self
    }

    pub fn with_direction(mut self, direction: TextDirection) -> Self {
        self.direction = direction;
        self
    }

    pub fn anchor(&self) -> Anchor {
        self.anchor.clone()
    }

    /// The widget, and the way to open it.
    ///
    /// Opening is a call rather than a flag because a menu is a modal: it goes
    /// up over everything, it takes the presses, and it comes down when it is
    /// dismissed. [`ModalHandle`] is what a caller keeps.
    ///
    /// `context` is the button's, and is read for one thing: the themes in
    /// scope here. The menu is built in the overlay, which is not below them --
    /// a light page inside a dark application would otherwise open a dark menu
    /// over itself. Upstream's `showMenu` takes the same capture at the same
    /// place (`InheritedTheme.capture(from: context, ...)`).
    pub fn build(self, context: &BuildContext) -> (AnyWidget, PopupMenuOpener) {
        let PopupMenuButton {
            anchor,
            child,
            menu,
            padding,
            direction,
            barrier,
            // The tooltip is the button's own; the opener never needs it.
            tooltip: _,
        } = self;
        let child = child
            .borrow_mut()
            .take()
            .expect("a menu button has a child");

        // Recorded from the button's own assemble, which is where its render
        // object first exists.
        let anchor_for_button = anchor.clone();
        let button = many(vec![child], move |mut rendered| {
            let child = rendered.pop().expect("the button");
            anchor_for_button.set(child.clone());
            crate::theatre::RenderPortal::new(child)
        });

        let opener = PopupMenuOpener {
            anchor,
            menu,
            padding,
            direction,
            barrier,
            themes: context.capture_themes(),
            open: Rc::new(RefCell::new(None)),
        };
        (button, opener)
    }
}

/// Opens and closes a [`PopupMenuButton`]'s menu.
#[derive(Clone)]
pub struct PopupMenuOpener {
    anchor: Anchor,
    menu: Rc<dyn Fn() -> AnyWidget>,
    padding: EdgeInsets,
    direction: TextDirection,
    barrier: ModalBarrier,
    /// The themes the button was built under, to be put back around the menu
    /// in the overlay. Upstream's `CapturedThemes`, held by the route.
    themes: ThemeCapture,
    open: Rc<RefCell<Option<ModalHandle>>>,
}

impl PopupMenuOpener {
    /// Puts the menu up over `overlay`, anchored to the button.
    ///
    /// Opening a menu that is already open does nothing rather than stacking a
    /// second copy -- upstream's route would be pushed twice and the button
    /// would need two dismissals.
    pub fn open(&self, overlay: Rc<OverlayHandle>) -> bool {
        if self.is_open() {
            return false;
        }
        let anchor = self.anchor.clone();
        let menu = Rc::clone(&self.menu);
        let place = menu_placement(self.padding, self.direction);
        // Inside the anchoring, so the themes wrap the menu itself and not the
        // positioner: what is wrapped is what upstream's route wraps, the thing
        // being built in the overlay.
        let themes = self.themes.clone();
        let handle = show_modal(overlay, self.barrier.clone(), move || {
            anchored(anchor.clone(), Rc::clone(&place), themes.wrap((menu)()))
        });
        let opened = handle.is_some();
        *self.open.borrow_mut() = handle;
        opened
    }

    /// Takes the menu down. Upstream's `Navigator.pop` on the menu route.
    pub fn close(&self) -> bool {
        let handle = self.open.borrow_mut().take();
        match handle {
            Some(handle) => handle.dismiss(),
            None => false,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
            .borrow()
            .as_ref()
            .is_some_and(|handle| handle.is_showing())
    }
}

/// Where a menu of this size would go, against a button at this rectangle.
///
/// Exposed because it is the question `menu.rs` could always answer and nobody
/// could ask: a caller with a rectangle can now get the answer without building
/// anything.
pub fn menu_offset_for(
    overlay: Size,
    button: crate::engine::Rect,
    menu: Size,
    padding: EdgeInsets,
    direction: TextDirection,
) -> Offset {
    popup_menu_offset(overlay, button, menu, padding, direction)
}

/// The anchor a caller can hand to something else -- a magnifier, a selection
/// toolbar -- that wants to sit against the same target.
pub fn anchor_of(target: RenderRef) -> Anchor {
    let anchor = Anchor::new();
    anchor.set(target);
    anchor
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{BuildContext, Component, ElementTree};
    use crate::render::{
        Alignment, BoxConstraints, HitTestResult, RenderAlign, RenderBox, RenderConstrainedBox,
        RenderPadding,
    };
    use crate::theatre::{RenderAnchored, overlay};

    fn leaf(width: f32, height: f32) -> AnyWidget {
        crate::framework::leaf(move || RenderConstrainedBox::tight(width, height))
    }

    fn laid_out(tree: &mut ElementTree) -> RenderRef {
        let root = tree.build_render_tree().expect("a mounted root");
        crate::render::schedule_root_layout(&root, BoxConstraints::tight(800.0, 600.0));
        crate::render::flush_layout();
        // The placement is worked out in a &self phase, as a frame does it.
        let mut discard = HitTestResult::new();
        root.hit_test(Offset::new(1.0, 1.0), &mut discard);
        root
    }

    fn menu_offset(root: &RenderRef) -> Option<Offset> {
        fn walk(handle: &RenderRef, found: &mut Option<Offset>) {
            if found.is_some() {
                return;
            }
            let children: Vec<RenderRef> = handle.with(|object| {
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
            for child in children {
                walk(&child, found);
            }
        }
        let mut found = None;
        walk(root, &mut found);
        found
    }

    /// A page with a button at `inset`, and the opener for its menu.
    fn mounted(inset: f32) -> (ElementTree, PopupMenuOpener, Rc<OverlayHandle>) {
        let opener_slot: Rc<RefCell<Option<PopupMenuOpener>>> = Rc::new(RefCell::new(None));
        let overlay_slot: Rc<RefCell<Option<Rc<OverlayHandle>>>> = Rc::new(RefCell::new(None));

        struct Page {
            inset: f32,
            opener: Rc<RefCell<Option<PopupMenuOpener>>>,
            overlay: Rc<RefCell<Option<Rc<OverlayHandle>>>>,
        }

        impl Component for Page {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.overlay.borrow_mut() = OverlayHandle::of(context);
                let (button, opener) =
                    PopupMenuButton::new(leaf(40.0, 20.0), || leaf(120.0, 90.0)).build(context);
                *self.opener.borrow_mut() = Some(opener);
                let inset = self.inset;
                many(vec![button], move |mut rendered| {
                    RenderPadding::new(
                        EdgeInsets::only(inset, inset, 0.0, 0.0),
                        RenderAlign::new(
                            Alignment::new(-1.0, -1.0),
                            rendered.pop().expect("the button"),
                        ),
                    )
                })
            }
        }

        let mut tree = ElementTree::new();
        tree.rebuild(overlay(crate::framework::component(Page {
            inset,
            opener: Rc::clone(&opener_slot),
            overlay: Rc::clone(&overlay_slot),
        })));
        tree.build_render_tree();
        let opener = opener_slot.borrow().clone().expect("an opener");
        let overlay = overlay_slot.borrow().clone().expect("an overlay");
        (tree, opener, overlay)
    }

    #[test]
    fn a_menu_is_not_up_until_it_is_opened() {
        let (mut tree, opener, _overlay) = mounted(0.0);
        assert!(!opener.is_open());
        assert_eq!(menu_offset(&laid_out(&mut tree)), None);
    }

    #[test]
    fn opening_puts_it_in_the_overlay() {
        let (mut tree, opener, overlay) = mounted(0.0);
        assert!(opener.open(overlay));
        tree.rebuild_dirty();
        assert!(opener.is_open());
        assert!(menu_offset(&laid_out(&mut tree)).is_some());
    }

    #[test]
    fn the_menu_is_drawn_in_the_button_s_theme_not_the_overlay_s() {
        // The bug this answers, seen on a device: the gallery's demo pages
        // publish a light theme *below* the application's overlay, so a menu
        // pushed into that overlay was built under the application's dark
        // theme and opened as a dark card over a light page. Upstream's
        // `showMenu` captures at the button (`InheritedTheme.capture`) and
        // wraps the route in what it caught; so does `PopupMenuButton::build`.
        let seen: Rc<RefCell<Option<crate::engine::Color>>> = Rc::new(RefCell::new(None));
        let opener_slot: Rc<RefCell<Option<PopupMenuOpener>>> = Rc::new(RefCell::new(None));
        let overlay_slot: Rc<RefCell<Option<Rc<OverlayHandle>>>> = Rc::new(RefCell::new(None));

        /// The menu's content, which reports the theme it was built under.
        struct Probe(Rc<RefCell<Option<crate::engine::Color>>>);

        impl Component for Probe {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.0.borrow_mut() = Some(crate::components::theme_of(context).background);
                leaf(120.0, 90.0)
            }
        }

        struct Page {
            seen: Rc<RefCell<Option<crate::engine::Color>>>,
            opener: Rc<RefCell<Option<PopupMenuOpener>>>,
            overlay: Rc<RefCell<Option<Rc<OverlayHandle>>>>,
        }

        impl Component for Page {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.overlay.borrow_mut() = OverlayHandle::of(context);
                let seen = Rc::clone(&self.seen);
                let (button, opener) = PopupMenuButton::new(leaf(40.0, 20.0), move || {
                    crate::framework::component(Probe(Rc::clone(&seen)))
                })
                .build(context);
                *self.opener.borrow_mut() = Some(opener);
                button
            }
        }

        // The overlay is above the theme, exactly as the application's is
        // above a demo page's.
        let mut tree = ElementTree::new();
        tree.rebuild(overlay(crate::framework::provide(
            crate::components::Theme::light(),
            crate::framework::component(Page {
                seen: Rc::clone(&seen),
                opener: Rc::clone(&opener_slot),
                overlay: Rc::clone(&overlay_slot),
            }),
        )));
        tree.build_render_tree();

        let opener = opener_slot.borrow().clone().expect("an opener");
        let overlay_handle = overlay_slot.borrow().clone().expect("an overlay");
        assert!(opener.open(overlay_handle));
        tree.rebuild_dirty();

        assert_eq!(
            *seen.borrow(),
            Some(crate::components::Theme::light().background),
            "the menu was built under the page's theme, not the default one \
             the overlay sits in"
        );
    }

    // -- The plan's first symptom, answered ---------------------------------------

    #[test]
    fn the_menu_grows_from_wherever_the_button_ended_up() {
        // popup_menu_offset wants the button's rectangle in the overlay's
        // coordinates. Before there was an overlay, nothing could produce one --
        // the plan lists this function as dead for exactly that reason.
        let (mut near_tree, near, near_overlay) = mounted(0.0);
        near.open(near_overlay);
        near_tree.rebuild_dirty();
        let near_at = menu_offset(&laid_out(&mut near_tree)).expect("open");

        let (mut far_tree, far, far_overlay) = mounted(200.0);
        far.open(far_overlay);
        far_tree.rebuild_dirty();
        let far_at = menu_offset(&laid_out(&mut far_tree)).expect("open");

        assert_ne!(
            near_at, far_at,
            "a menu that did not move with its button read zero, not the button"
        );
        assert!(far_at.dy > near_at.dy, "{near_at:?} -> {far_at:?}");
    }

    #[test]
    fn a_button_near_the_left_grows_its_menu_rightwards_from_its_own_edge() {
        // Upstream's rule: grow towards whichever edge has more room, aligned
        // to the anchor's near edge.
        let (mut tree, opener, overlay) = mounted(100.0);
        opener.open(overlay);
        tree.rebuild_dirty();
        let at = menu_offset(&laid_out(&mut tree)).expect("open");

        assert_eq!(at.dx, 100.0, "aligned to the button's left edge");
        assert_eq!(at.dy, 100.0, "and starting at its top");
    }

    #[test]
    fn a_button_near_the_right_grows_its_menu_leftwards() {
        // Button at x = 700, 40 wide, so its right edge is 740 and the menu is
        // 120 wide: aligned to the right edge it starts at 620.
        let (mut tree, opener, overlay) = mounted(700.0);
        opener.open(overlay);
        tree.rebuild_dirty();
        let at = menu_offset(&laid_out(&mut tree)).expect("open");

        assert!(
            at.dx < 700.0,
            "it grew back towards the room it had: {at:?}"
        );
        assert!(at.dx + 120.0 <= 800.0, "and stayed on screen: {at:?}");
    }

    #[test]
    fn a_button_near_the_bottom_keeps_its_menu_on_screen() {
        let (mut tree, opener, overlay) = mounted(560.0);
        opener.open(overlay);
        tree.rebuild_dirty();
        let at = menu_offset(&laid_out(&mut tree)).expect("open");

        assert!(
            at.dy + 90.0 <= 600.0,
            "_fitInsideScreen pulled it back up: {at:?}"
        );
    }

    #[test]
    fn the_unsafe_area_is_kept_clear() {
        // Two placements of the same button, one with a bottom inset: the menu
        // has to end higher when there is a system bar in the way.
        let button = crate::engine::Rect::xywh(100.0, 540.0, 40.0, 20.0);
        let overlay = Size::new(800.0, 600.0);
        let menu = Size::new(120.0, 90.0);

        let bare = menu_offset_for(overlay, button, menu, EdgeInsets::ZERO, TextDirection::Ltr);
        let inset = menu_offset_for(
            overlay,
            button,
            menu,
            EdgeInsets::only(0.0, 0.0, 0.0, 60.0),
            TextDirection::Ltr,
        );
        assert!(
            inset.dy < bare.dy,
            "the menu moved up out of the system bar: {bare:?} -> {inset:?}"
        );
    }

    // -- A menu is modal ----------------------------------------------------------

    #[test]
    fn an_open_menu_puts_a_barrier_under_itself() {
        let (mut tree, opener, overlay) = mounted(100.0);
        assert_eq!(crate::theatre::modal_count(), 0);
        opener.open(overlay);
        tree.rebuild_dirty();
        assert_eq!(
            crate::theatre::modal_count(),
            1,
            "a menu is a modal, unlike a tooltip"
        );
        opener.close();
        assert_eq!(crate::theatre::modal_count(), 0);
    }

    #[test]
    fn opening_a_menu_that_is_already_open_does_nothing() {
        // Upstream would push a second route and the button would need two
        // dismissals to get back.
        let (mut tree, opener, overlay) = mounted(100.0);
        assert!(opener.open(Rc::clone(&overlay)));
        tree.rebuild_dirty();
        assert!(!opener.open(overlay), "already up");
        assert_eq!(crate::theatre::modal_count(), 1);
        opener.close();
    }

    #[test]
    fn closing_a_menu_that_is_not_open_does_nothing() {
        let (_tree, opener, _overlay) = mounted(100.0);
        assert!(!opener.close());
    }

    #[test]
    fn escape_closes_an_open_menu() {
        let (mut tree, opener, overlay) = mounted(100.0);
        opener.open(overlay);
        tree.rebuild_dirty();
        assert!(crate::theatre::dismiss_topmost_modal());
        assert!(!opener.is_open());
    }
}

#[cfg(test)]
mod menu_label_tests {
    use super::PopupMenuButton;
    use crate::framework::leaf;
    use crate::material_app::DefaultMaterialLocalizations as L10n;
    use crate::render::RenderConstrainedBox;

    fn button() -> PopupMenuButton {
        PopupMenuButton::new(leaf(|| RenderConstrainedBox::tight(10.0, 10.0)), || {
            leaf(|| RenderConstrainedBox::tight(10.0, 10.0))
        })
    }

    #[test]
    fn three_dots_in_a_corner_explain_themselves() {
        assert_eq!(button().tooltip(), "Show menu");
    }

    #[test]
    fn and_a_button_that_said_what_it_opens_says_that_instead() {
        assert_eq!(
            button().with_tooltip("Sort and filter").tooltip(),
            "Sort and filter"
        );
    }

    #[test]
    fn the_menu_and_the_button_are_named_separately() {
        // Two strings for two moments: the tooltip explains the glyph before
        // it is pressed, the label names what appeared after it was. Even a
        // button that renamed its tooltip leaves the menu's name alone.
        let renamed = button().with_tooltip("Sort and filter");
        assert_eq!(renamed.menu_semantic_label(), "Popup menu");
        assert_ne!(renamed.tooltip(), renamed.menu_semantic_label());
    }

    #[test]
    fn a_menus_invisible_barrier_says_which_thing_goes_away() {
        // "Dismiss menu", not the dialog scrim's "Dismiss". A dialog's is
        // visibly dimmed and belongs to what is in front of it; this one
        // paints nothing, so the label is all a reader has.
        let button = button();
        let barrier = &button.barrier;
        assert!(!barrier.paints(), "invisible, which is why it needs a name");
        assert_eq!(barrier.semantics_label.as_deref(), Some("Dismiss menu"));
        assert_ne!(
            L10n::MENU_DISMISS_LABEL,
            L10n::MODAL_BARRIER_DISMISS_LABEL,
            "two different strings for two different scrims"
        );
    }
}
