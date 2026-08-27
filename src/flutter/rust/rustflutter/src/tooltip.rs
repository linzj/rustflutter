//! The live tooltip: `raw_tooltip.rs`'s clock and geometry, hosted.
//!
//! `raw_tooltip.rs` has had the whole of a tooltip's behaviour for a long time
//! -- the wait before showing, the stay after, the exit delay, the touch path,
//! the announcement, and `position_dependent_box` for where the bubble goes --
//! and nothing to put it in. This is the part that was missing: an
//! `OverlayPortal` for the bubble, and the target's position in the overlay's
//! coordinates to place it against.
//!
//! # The two halves of "where"
//!
//! `position_dependent_box` wants the target's centre **in global
//! coordinates**, and a widget does not know where it is until layout has run.
//! So the two halves happen in different phases: the portal hands the bubble to
//! the theatre during build, and [`crate::theatre::RenderAnchored`] asks the
//! target where it ended up once layout is over.
//!
//! That is the whole reason this could not be written before L0: without
//! `RenderRef::transform_to` there was no way to ask.

use std::cell::RefCell;
use std::rc::Rc;

use crate::framework::{AnyWidget, many};
use crate::raw_tooltip::{TooltipPositionContext, position_dependent_box};
use crate::render::{BoxConstraints, Offset, RenderBox, RenderRef, Size};
use crate::theatre::{Anchor, Placement, PortalController, anchored, overlay_portal};

/// Upstream's `Tooltip.verticalOffset` default.
pub const DEFAULT_VERTICAL_OFFSET: f32 = 24.0;

/// The placement rule a tooltip uses: upstream's `positionDependentBox`, in the
/// shape [`crate::theatre::RenderAnchored`] asks for.
pub fn tooltip_placement(vertical_offset: f32, prefer_below: bool) -> Placement {
    Rc::new(
        move |target: crate::engine::Rect, bubble: Size, overlay: Size| {
            let context = TooltipPositionContext::new(
                // Upstream passes the target's *centre*.
                (
                    (target.left + target.right) / 2.0,
                    (target.top + target.bottom) / 2.0,
                ),
                (target.width(), target.height()),
                (bubble.width, bubble.height),
            )
            .with_overlay((overlay.width, overlay.height))
            .with_vertical_offset(vertical_offset)
            .with_prefer_below(prefer_below);
            let (x, y) = position_dependent_box(&context);
            Offset::new(x, y)
        },
    )
}

impl crate::framework::Component for Tooltip {
    /// Upstream's `Tooltip.build`, which is where the three-step chain runs.
    ///
    /// This is the path that consults the theme; [`Tooltip::build`] is the same
    /// assembly on upstream's bare defaults, for a caller with no context in
    /// hand.
    fn build(&self, context: &mut crate::framework::BuildContext) -> AnyWidget {
        let resolved = crate::component_themes::ResolvedTooltip::of(context);
        let (vertical_offset, prefer_below) = self.placement_from(&resolved);

        let bubble: Rc<dyn Fn() -> AnyWidget> = match &self.message {
            // Upstream's standard bubble: the decoration, the padding, the
            // text style and a *minimum* height -- a floor, so a long message
            // wraps and grows rather than being squeezed into it.
            Some(message) => {
                let message = message.clone();
                let style = resolved.text_style.clone();
                let align = resolved.text_align;
                let padding = resolved.padding;
                let margin = resolved.margin;
                let height = resolved.height;
                let decoration = resolved.decoration.clone();
                let scheme = crate::theme::ThemeData::of(context).color_scheme;
                Rc::new(move || {
                    let message = message.clone();
                    let style = style.clone();
                    let decoration = decoration.clone();
                    crate::framework::leaf(move || {
                        let mut text = crate::widgets::Text::new(message.clone()).with_align(align);
                        if let Some(style) = &style {
                            text = text.with_style(style.clone());
                        } else {
                            // Upstream's default text style (`tooltip.dart`'s
                            // `defaultTextStyle`): body medium in white on a
                            // light theme's bubble and black on a dark one's,
                            // the pair that reads against the default bubble
                            // colour below.
                            text = text.with_color(match scheme.brightness {
                                crate::platform::Brightness::Dark => crate::engine::Color::BLACK,
                                crate::platform::Brightness::Light => crate::engine::Color::WHITE,
                            });
                        }
                        let mut container = crate::widgets::Container::new()
                            .with_padding(padding)
                            .with_margin(margin)
                            .with_child(text);
                        match &decoration {
                            Some(decoration) => {
                                container = container.with_decoration(decoration.clone());
                            }
                            None => {
                                // Upstream's `defaultDecoration`: grey 700 at
                                // 90% on a light theme, white at 90% on a dark
                                // one, corner radius 4.
                                let fill = match scheme.brightness {
                                    crate::platform::Brightness::Dark => {
                                        crate::engine::Color::WHITE.with_alpha(0xE6)
                                    }
                                    crate::platform::Brightness::Light => {
                                        crate::colors::Colors::GREY
                                            .shade(700)
                                            .expect("grey has a 700")
                                            .with_alpha(0xE6)
                                    }
                                };
                                container = container.with_color(fill).with_corner_radius(4.0);
                            }
                        }
                        crate::render::RenderConstrainedBox::new(crate::render::BoxConstraints {
                            min_width: 0.0,
                            max_width: f32::INFINITY,
                            min_height: height,
                            max_height: f32::INFINITY,
                        })
                        .with_child(container)
                    })
                })
            }
            None => Rc::clone(&self.bubble),
        };

        Tooltip {
            id: self.id,
            controller: self.controller.clone(),
            anchor: self.anchor.clone(),
            child: RefCell::new(self.child.borrow_mut().take()),
            bubble,
            vertical_offset: self.vertical_offset,
            prefer_below: self.prefer_below,
            message: None,
        }
        .assemble(vertical_offset, prefer_below)
    }
}

/// A tooltip: `child` as it was, and `bubble` above it while the pointer rests
/// on it.
///
/// The trigger here is hover in and hover out. The delays, the touch path and
/// the announcement belong to `raw_tooltip.rs` and are driven by whoever owns
/// the clock -- [`Tooltip::controller`] is how they reach this.
pub struct Tooltip {
    id: u64,
    controller: PortalController,
    anchor: Anchor,
    child: RefCell<Option<AnyWidget>>,
    bubble: Rc<dyn Fn() -> AnyWidget>,
    /// `None` defers to the tooltip theme, then to upstream's default. Both
    /// are three-step chains -- widget, theme, default -- and holding the
    /// widget's step as an `Option` is what makes the first step tellable from
    /// the third.
    vertical_offset: Option<f32>,
    prefer_below: Option<bool>,
    /// Upstream's `message`: the text of a *standard* tooltip, built from the
    /// theme rather than by the caller.
    ///
    /// `Tooltip::new` takes a closure and builds nothing itself, which is the
    /// right shape for a caller with something unusual to show and the wrong
    /// one for the ordinary case -- a caller who wants what every other tooltip
    /// looks like should not have to rebuild it, and would get the padding and
    /// the colours slightly wrong if they did.
    message: Option<String>,
}

impl Tooltip {
    pub fn new(id: u64, child: AnyWidget, bubble: impl Fn() -> AnyWidget + 'static) -> Tooltip {
        Tooltip {
            id,
            controller: PortalController::new(),
            anchor: Anchor::new(),
            child: RefCell::new(Some(child)),
            bubble: Rc::new(bubble),
            vertical_offset: None,
            prefer_below: None,
            message: None,
        }
    }

    pub fn with_vertical_offset(mut self, offset: f32) -> Self {
        self.vertical_offset = Some(offset);
        self
    }

    pub fn with_prefer_below(mut self, prefer_below: bool) -> Self {
        self.prefer_below = Some(prefer_below);
        self
    }

    /// The controller, so a caller with its own clock -- a `RawTooltipState`,
    /// say -- decides when the bubble is up.
    pub fn controller(&self) -> PortalController {
        self.controller.clone()
    }

    pub fn anchor(&self) -> Anchor {
        self.anchor.clone()
    }

    /// The hit-test identity of the target.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The tooltip on upstream's defaults, for a caller with no theme to read
    /// -- `component(tooltip)` is the one that consults it.
    pub fn build(self) -> AnyWidget {
        let vertical_offset = self
            .vertical_offset
            .unwrap_or(crate::component_themes::ResolvedTooltip::VERTICAL_OFFSET);
        let prefer_below = self
            .prefer_below
            .unwrap_or(crate::component_themes::ResolvedTooltip::PREFER_BELOW);
        self.assemble(vertical_offset, prefer_below)
    }

    /// Upstream's `Tooltip(message:)`: the ordinary tooltip, whose bubble this
    /// builds from the theme. Only reachable through the `Component`
    /// implementation, because the theme needs a context to be read from.
    pub fn message(id: u64, child: AnyWidget, message: impl Into<String>) -> Tooltip {
        let mut tooltip = Tooltip::new(id, child, || {
            crate::framework::leaf(|| crate::widgets::Empty)
        });
        tooltip.message = Some(message.into());
        tooltip
    }

    /// The widget's step of the three-step chain: its own numbers where it has
    /// them, the resolution's where it does not.
    ///
    /// A method rather than two lines inside `build` so that it can be asked as
    /// well as used -- where a bubble ends up is decided inside an overlay this
    /// harness cannot reach into, and a test of `build` alone could only check
    /// that it did not crash.
    pub fn placement_from(
        &self,
        resolved: &crate::component_themes::ResolvedTooltip,
    ) -> (f32, bool) {
        (
            self.vertical_offset.unwrap_or(resolved.vertical_offset),
            self.prefer_below.unwrap_or(resolved.prefer_below),
        )
    }

    /// The tooltip with its placement decided. `build` and the `Component`
    /// implementation differ only in where the two numbers come from.
    fn assemble(self, vertical_offset: f32, prefer_below: bool) -> AnyWidget {
        let Tooltip {
            id,
            controller,
            anchor,
            child,
            bubble,
            ..
        } = self;
        crate::framework::stateful(TooltipHost {
            id,
            controller,
            anchor,
            child,
            bubble,
            vertical_offset,
            prefer_below,
        })
    }
}

/// The tooltip as upstream has it: a `StatefulWidget`. The widget is thrown
/// away every time the demo around it rebuilds, and everything the bubble's
/// lifetime depends on -- the controller above all -- belongs to the `State`,
/// which is not.
///
/// This is the difference that keeps a bubble hideable: a rebuild hands down a
/// fresh widget with a fresh controller, and it is still the **state's**
/// controller the hover handlers and the portal talk to, so the entry the
/// first build showed is the entry the next build's hover-out takes down.
/// Found by tapping anywhere in a demo while a tooltip was up: the tap
/// rebuilt the demo, the rebuilt tooltip answered "not showing" for the rest
/// of the session, and the first controller's bubble had nobody left to hide
/// it.
struct TooltipHost {
    id: u64,
    controller: PortalController,
    anchor: Anchor,
    child: RefCell<Option<AnyWidget>>,
    bubble: Rc<dyn Fn() -> AnyWidget>,
    vertical_offset: f32,
    prefer_below: bool,
}

/// Upstream's `TooltipState` field, `final _controller =
/// OverlayPortalController()`. An `Option` only because `Default` cannot come
/// from the widget; [`TooltipHost::initial_state`] fills it before the first
/// build.
#[derive(Default)]
struct TooltipHostState {
    controller: Option<PortalController>,
}

impl crate::framework::StatefulComponent for TooltipHost {
    type State = TooltipHostState;

    /// The widget's controller is the seed of the state's, so a caller that
    /// took [`Tooltip::controller`] before building drives the same object the
    /// portal does. After that the widget's is ignored: upstream initialises
    /// the controller in the State, and a rebuilt widget bringing a new one is
    /// the ordinary case, not a change to react to.
    fn initial_state(&self) -> TooltipHostState {
        TooltipHostState {
            controller: Some(self.controller.clone()),
        }
    }

    fn build(
        &self,
        state: &TooltipHostState,
        _handle: crate::framework::StateHandle<TooltipHostState>,
        _context: &mut crate::framework::BuildContext,
    ) -> AnyWidget {
        let controller = state
            .controller
            .clone()
            .expect("initial_state ran before the first build");
        let child = self
            .child
            .borrow_mut()
            .take()
            .expect("a tooltip has a child");

        let show = controller.clone();
        let hide = controller.clone();
        let handlers = crate::gestures::PointerHandlers::new().with_hover_change(move |inside| {
            if inside {
                show.show();
            } else {
                hide.hide();
            }
        });

        // The anchor is filled in from the target's own assemble, which runs
        // before any layout -- so by the time the bubble needs a position there
        // is a handle to ask.
        let id = self.id;
        let anchor_for_target = self.anchor.clone();
        let target = many(vec![child], move |mut rendered| {
            let child = rendered.pop().expect("the target");
            let region = crate::render::RenderPointerRegion::new(id, child.clone())
                .with_handlers(handlers.clone());
            anchor_for_target.set(child);
            region
        });

        let place = tooltip_placement(self.vertical_offset, self.prefer_below);
        let anchor = self.anchor.clone();
        let bubble = Rc::clone(&self.bubble);
        overlay_portal(controller, target, move || {
            anchored(anchor.clone(), Rc::clone(&place), (bubble)())
        })
    }
}

/// A tooltip with the defaults.
pub fn tooltip(id: u64, child: AnyWidget, bubble: impl Fn() -> AnyWidget + 'static) -> AnyWidget {
    Tooltip::new(id, child, bubble).build()
}

/// Whether a pointer that has rested this long should show the tooltip.
/// Upstream's `waitDuration`, asked of the clock in `raw_tooltip.rs` rather
/// than kept here.
pub fn should_show_after(rested_ms: f32, wait_ms: f32) -> bool {
    rested_ms >= wait_ms
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::ElementTree;
    use crate::render::{RenderConstrainedBox, RenderPadding};
    use crate::theatre::RenderAnchored;
    use crate::theatre::overlay;

    fn leaf(width: f32, height: f32) -> AnyWidget {
        crate::framework::leaf(move || RenderConstrainedBox::tight(width, height))
    }

    /// The page: a target pushed well away from the origin, so that "it read
    /// the target's global position" is distinguishable from "it read zero".
    fn page_with_tooltip(
        inset: f32,
        controller_out: &Rc<RefCell<Option<PortalController>>>,
    ) -> AnyWidget {
        let tip = Tooltip::new(9001, leaf(60.0, 20.0), || leaf(100.0, 30.0));
        *controller_out.borrow_mut() = Some(tip.controller());
        let anchored = tip.build();
        many(vec![anchored], move |mut rendered| {
            // Aligned inside the padding, so the target keeps its own 60 x 20.
            // Without this the tight constraints reach all the way down and
            // stretch it to fill the page -- which it did, and the arithmetic
            // in these tests was written against the size it was asked for
            // rather than the size it got.
            RenderPadding::new(
                crate::render::EdgeInsets::only(inset, inset, 0.0, 0.0),
                crate::render::RenderAlign::new(
                    crate::render::Alignment::new(-1.0, -1.0),
                    rendered.pop().expect("the tooltip target"),
                ),
            )
        })
    }

    fn laid_out(tree: &mut ElementTree) -> RenderRef {
        let root = tree.build_render_tree().expect("a mounted root");
        crate::render::schedule_root_layout(&root, BoxConstraints::tight(800.0, 600.0));
        crate::render::flush_layout();
        // A frame lays out, then paints and hit-tests -- and the bubble's
        // position is worked out in those phases, not in layout, because that
        // is where asking an ancestor is legal. A harness that stopped after
        // layout would read the position from before the target had one.
        let mut discard = crate::render::HitTestResult::new();
        root.hit_test(Offset::new(1.0, 1.0), &mut discard);
        root
    }

    /// Where the bubble ended up, by finding the positioner in the render tree.
    fn bubble_offset(root: &RenderRef) -> Option<Offset> {
        fn walk(handle: &RenderRef, found: &mut Option<Offset>) {
            if found.is_some() {
                return;
            }
            let children: Vec<RenderRef> = handle.with(|object| {
                if let Some(position) = object.as_any().downcast_ref::<RenderAnchored>() {
                    *found = Some(position.placed());
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

    fn mounted(inset: f32) -> (ElementTree, PortalController) {
        let slot: Rc<RefCell<Option<PortalController>>> = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(overlay(page_with_tooltip(inset, &slot)));
        tree.build_render_tree();
        let controller = slot.borrow().clone().expect("a controller");
        (tree, controller)
    }

    #[test]
    fn a_tooltip_shows_nothing_until_it_is_asked() {
        let (mut tree, controller) = mounted(0.0);
        let root = laid_out(&mut tree);
        assert!(!controller.is_showing());
        assert_eq!(bubble_offset(&root), None, "no bubble in the tree");
    }

    #[test]
    fn showing_it_puts_a_bubble_in_the_overlay() {
        let (mut tree, controller) = mounted(0.0);
        controller.show();
        tree.rebuild_dirty();
        let root = laid_out(&mut tree);
        assert!(bubble_offset(&root).is_some(), "the bubble is hosted");
    }

    // -- The closed loop: L0 reaches L2 ------------------------------------------

    #[test]
    fn the_bubble_follows_the_target_across_the_page() {
        // The same tooltip, once near the origin and once pushed 300 across and
        // 200 down. If the bubble moves with it, the position came from the
        // target's *global* rectangle -- which is transform_to, reached through
        // an overlay, from a widget built somewhere else entirely.
        let (mut near_tree, near) = mounted(0.0);
        near.show();
        near_tree.rebuild_dirty();
        let near_at = bubble_offset(&laid_out(&mut near_tree)).expect("shown");

        let (mut far_tree, far) = mounted(300.0);
        far.show();
        far_tree.rebuild_dirty();
        let far_at = bubble_offset(&laid_out(&mut far_tree)).expect("shown");

        assert_ne!(
            near_at, far_at,
            "a bubble that did not move with its target read zero, not the target"
        );
        assert!(
            far_at.dx > near_at.dx && far_at.dy > near_at.dy,
            "and it moved the way the target did: {near_at:?} -> {far_at:?}"
        );
    }

    #[test]
    fn the_bubble_is_centred_on_the_target_and_below_it() {
        // Target: 60 x 20 at (300, 300), so its centre is (330, 310).
        // Bubble: 100 wide, so centred means x = 330 - 50 = 280.
        // Below means y = centre + the vertical offset.
        let (mut tree, controller) = mounted(300.0);
        controller.show();
        tree.rebuild_dirty();
        let at = bubble_offset(&laid_out(&mut tree)).expect("shown");

        assert_eq!(at.dx, 280.0, "centred on the target");
        assert_eq!(at.dy, 310.0 + 24.0, "and the default offset below it");
    }

    #[test]
    fn a_target_near_the_bottom_puts_its_bubble_above_itself() {
        // The preference is a preference: below if it fits, above if it does
        // not. 560 down a 600-tall overlay leaves no room underneath.
        let (mut tree, controller) = mounted(560.0);
        controller.show();
        tree.rebuild_dirty();
        let at = bubble_offset(&laid_out(&mut tree)).expect("shown");

        assert!(
            at.dy < 560.0,
            "the bubble went above the target rather than off the screen: {at:?}"
        );
    }

    #[test]
    fn a_target_at_the_right_edge_keeps_its_bubble_on_screen() {
        // 780 across an 800-wide overlay: centring a 100-wide bubble on it
        // would put half of it past the edge.
        let (mut tree, controller) = mounted(760.0);
        controller.show();
        tree.rebuild_dirty();
        let at = bubble_offset(&laid_out(&mut tree)).expect("shown");

        assert!(
            at.dx + 100.0 <= 800.0,
            "a tooltip off the edge is worse than one not quite where it was asked: {at:?}"
        );
    }

    #[test]
    fn hiding_it_takes_the_bubble_away() {
        let (mut tree, controller) = mounted(100.0);
        controller.show();
        tree.rebuild_dirty();
        assert!(bubble_offset(&laid_out(&mut tree)).is_some());

        controller.hide();
        tree.rebuild_dirty();
        assert_eq!(bubble_offset(&laid_out(&mut tree)), None);
    }

    /// A mouse at a position, hovering rather than pressing.
    fn hover_at(x: f32, y: f32) -> crate::gestures::PointerEvent {
        crate::gestures::PointerEvent {
            view_id: 0,
            device: 0,
            pointer_id: 1,
            change: crate::gestures::PointerChange::Hover,
            kind: crate::gestures::PointerKind::Mouse,
            signal_kind: crate::gestures::SignalKind::None,
            buttons: 0,
            time_stamp_micros: 0,
            position: Offset::new(x, y),
            delta: Offset::ZERO,
            scroll_delta: Offset::ZERO,
            pressure: 1.0,
            local_position: Offset::new(x, y),
        }
    }

    #[test]
    fn a_rebuild_while_showing_does_not_orphan_the_bubble() {
        // What the gallery's tooltip demo does on every tap: the demo's build
        // runs again, and the rebuilt tooltip is a fresh widget whose
        // controller is *not* the one that showed the bubble. Upstream
        // survives this because the controller is `TooltipState`'s, and so
        // does this: hover in, rebuild, hover out -- the bubble has to go.
        let slot: Rc<RefCell<Option<PortalController>>> = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(overlay(page_with_tooltip(100.0, &slot)));
        tree.build_render_tree();

        let mut router = crate::gestures::GestureRouter::new();
        // The 60x20 target sits at (100, 100).
        let root = laid_out(&mut tree);
        router.dispatch(&root, &hover_at(130.0, 110.0));
        tree.rebuild_dirty();
        assert!(
            bubble_offset(&laid_out(&mut tree)).is_some(),
            "hovering the target shows the bubble"
        );

        // The tap's rebuild: a brand-new Tooltip at the same position. The
        // slot gets the new widget's controller, which -- like the demo's --
        // nobody will ever call.
        let other: Rc<RefCell<Option<PortalController>>> = Rc::new(RefCell::new(None));
        tree.rebuild(overlay(page_with_tooltip(100.0, &other)));
        let root = laid_out(&mut tree);
        assert!(
            bubble_offset(&root).is_some(),
            "and the bubble is still up after the rebuild"
        );

        router.dispatch(&root, &hover_at(700.0, 500.0));
        tree.rebuild_dirty();
        assert_eq!(
            bubble_offset(&laid_out(&mut tree)),
            None,
            "hovering away hides the bubble the previous build showed"
        );
    }

    #[test]
    fn the_wait_is_the_clocks_question_not_the_widgets() {
        assert!(!should_show_after(100.0, 500.0));
        assert!(should_show_after(500.0, 500.0));
        assert!(should_show_after(900.0, 500.0));
    }
}

#[cfg(test)]
mod tooltip_theme_tests {
    use super::*;
    use crate::component_themes::{ResolvedTooltip, TooltipTheme, TooltipThemeData};
    use crate::editable_text::TargetPlatform;
    use crate::framework::{BuildContext, Component, ElementTree, component, provide};
    use crate::theme::ThemeData;

    struct Reader(Rc<RefCell<Option<ResolvedTooltip>>>);

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.0.borrow_mut() = Some(ResolvedTooltip::of(context));
            crate::framework::leaf(|| crate::widgets::Empty)
        }
    }

    fn resolve_on(platform: TargetPlatform, data: TooltipThemeData) -> ResolvedTooltip {
        let seen = Rc::new(RefCell::new(None));
        let mut theme = ThemeData::light();
        theme.platform = platform;
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            theme,
            TooltipTheme::new(data, component(Reader(Rc::clone(&seen)))),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    fn resolve(data: TooltipThemeData) -> ResolvedTooltip {
        resolve_on(TargetPlatform::Windows, data)
    }

    #[test]
    fn a_touch_tooltip_is_taller_and_wider_than_a_desktop_one() {
        // Not generosity: the same tooltip at the distance it will actually be
        // read from. A desktop one is summoned by a mouse resting exactly on
        // something; a touch one appears under a hand and is read at arm's
        // length.
        let desktop = resolve_on(TargetPlatform::Windows, TooltipThemeData::new());
        let touch = resolve_on(TargetPlatform::Android, TooltipThemeData::new());
        assert_eq!(desktop.height, 24.0);
        assert_eq!(touch.height, 32.0);
        assert_eq!(desktop.padding.left, 8.0);
        assert_eq!(touch.padding.left, 16.0);
    }

    #[test]
    fn only_the_horizontal_padding_changes_with_the_platform() {
        // The height is what gives a touch tooltip its room; vertical padding
        // on top of that would fight it.
        for platform in [TargetPlatform::Windows, TargetPlatform::IOS] {
            assert_eq!(
                resolve_on(platform, TooltipThemeData::new()).padding.top,
                4.0
            );
        }
    }

    #[test]
    fn every_desktop_agrees_and_so_does_every_phone() {
        for platform in [
            TargetPlatform::Windows,
            TargetPlatform::MacOS,
            TargetPlatform::Linux,
        ] {
            assert_eq!(resolve_on(platform, TooltipThemeData::new()).height, 24.0);
        }
        for platform in [
            TargetPlatform::Android,
            TargetPlatform::IOS,
            TargetPlatform::Fuchsia,
        ] {
            assert_eq!(resolve_on(platform, TooltipThemeData::new()).height, 32.0);
        }
    }

    #[test]
    fn a_theme_that_sets_the_height_takes_the_platform_out_of_it() {
        let mut data = TooltipThemeData::new();
        data.height = Some(50.0);
        assert_eq!(
            resolve_on(TargetPlatform::Windows, data.clone()).height,
            50.0
        );
        assert_eq!(resolve_on(TargetPlatform::Android, data).height, 50.0);
    }

    #[test]
    fn the_defaults_are_upstreams_and_not_invented() {
        let resolved = resolve(TooltipThemeData::new());
        assert_eq!(resolved.vertical_offset, 24.0);
        assert!(resolved.prefer_below, "below unless there is no room");
        assert_eq!(
            resolved.margin,
            crate::render::EdgeInsets::ZERO,
            "a tooltip is placed against its target; a margin is a second \
             opinion about where that is"
        );
        assert_eq!(
            resolved.wait_duration,
            std::time::Duration::ZERO,
            "a tooltip summoned by a long press has already been waited for"
        );
        assert_eq!(
            resolved.show_duration,
            std::time::Duration::from_millis(1500)
        );
        assert!(!resolved.exclude_from_semantics);
    }

    #[test]
    fn the_theme_beats_every_default_it_sets() {
        let mut data = TooltipThemeData::new();
        data.vertical_offset = Some(9.0);
        data.prefer_below = Some(false);
        data.show_duration = Some(std::time::Duration::from_secs(9));
        let resolved = resolve(data);
        assert_eq!(resolved.vertical_offset, 9.0);
        assert!(!resolved.prefer_below);
        assert_eq!(resolved.show_duration, std::time::Duration::from_secs(9));
        assert_eq!(
            resolved.wait_duration,
            std::time::Duration::ZERO,
            "and leaves alone what it did not set"
        );
    }

    #[test]
    fn the_widgets_own_numbers_beat_the_themes() {
        // Three steps, and the widget is the first. Asked through the method
        // `build` itself calls.
        let mut data = TooltipThemeData::new();
        data.vertical_offset = Some(100.0);
        data.prefer_below = Some(false);
        let resolved = resolve(data);

        let plain = Tooltip::new(1, crate::framework::leaf(|| crate::widgets::Empty), || {
            crate::framework::leaf(|| crate::widgets::Empty)
        });
        assert_eq!(plain.placement_from(&resolved), (100.0, false));

        let mine = Tooltip::new(1, crate::framework::leaf(|| crate::widgets::Empty), || {
            crate::framework::leaf(|| crate::widgets::Empty)
        })
        .with_vertical_offset(5.0)
        .with_prefer_below(true);
        assert_eq!(mine.placement_from(&resolved), (5.0, true));
    }

    #[test]
    fn an_unset_widget_offset_is_told_apart_from_one_set_to_the_default() {
        // Which is the whole reason the widget's step is an Option: a caller
        // asking for exactly 24 must not be overruled by a theme.
        let plain = Tooltip::new(1, crate::framework::leaf(|| crate::widgets::Empty), || {
            crate::framework::leaf(|| crate::widgets::Empty)
        });
        assert_eq!(plain.vertical_offset, None);

        let explicit = Tooltip::new(1, crate::framework::leaf(|| crate::widgets::Empty), || {
            crate::framework::leaf(|| crate::widgets::Empty)
        })
        .with_vertical_offset(ResolvedTooltip::VERTICAL_OFFSET);
        assert_eq!(
            explicit.vertical_offset,
            Some(ResolvedTooltip::VERTICAL_OFFSET)
        );
    }

    #[test]
    fn a_message_tooltip_builds_its_own_bubble_and_a_closure_one_does_not() {
        assert!(
            Tooltip::message(1, crate::framework::leaf(|| crate::widgets::Empty), "Copy")
                .message
                .is_some()
        );
        assert!(
            Tooltip::new(1, crate::framework::leaf(|| crate::widgets::Empty), || {
                crate::framework::leaf(|| crate::widgets::Empty)
            })
            .message
            .is_none()
        );
    }
}
