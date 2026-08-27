// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Popup menus, ported from `material/popup_menu.dart`.
//!
//! Upstream a menu is a route: `showMenu` pushes a `_PopupMenuRoute` onto the
//! nearest `Navigator` and the route's barrier is what dismisses it. What is
//! ported here is everything except the route: the entries, the menu surface,
//! the button, and the placement math.
//!
//! [`crate::popup`] is the showing. A menu goes up as a modal over the
//! application's `Overlay` -- barrier, focus trap, Escape and dismissal -- and
//! [`popup_menu_offset`] finally has a caller other than its own unit test:
//! anchoring wants the button's rectangle *in the overlay's coordinates*, and
//! before there was a host there was no such thing to ask for.
//!
//! What is still missing, each noted where it would live: the open/close
//! animation (`_kMenuDuration` 300ms and the per-item `Interval` fades -- an
//! entrance animation belongs to a route, and routes are the line after this
//! one), scrolling for menus taller than the screen (`_PopupMenu`'s
//! `SingleChildScrollView`), and the width-step rounding
//! (`IntrinsicWidth(stepWidth:)` -- the crate's
//! [`crate::render::RenderIntrinsicWidth`] has no step).

use std::cell::RefCell;

use crate::components::theme_of;
use crate::direction::TextDirection;
use crate::engine::Rect;
use crate::framework::{AnyWidget, BuildContext, Component, StateHandle, component, leaf, many};
use crate::gestures::PointerHandlers;
use crate::render::{
    Alignment, BoxConstraints, CrossAxisAlignment, EdgeInsets, MainAxisSize, Offset,
    RenderConstrainedBox, RenderFlex, RenderIntrinsicWidth, RenderPadding, Size,
};
use crate::widgets::{Align, Container, Pointer, Text};

// -- Metrics --------------------------------------------------------------------
//
// Upstream's constants at the top of `material/popup_menu.dart`, plus
// `kMinInteractiveDimension` from `material/constants.dart`.

/// Upstream's `_kMenuWidthStep`.
pub const MENU_WIDTH_STEP: f32 = 56.0;
/// Upstream's `_kMenuMinWidth`: `2.0 * _kMenuWidthStep`.
pub const MENU_MIN_WIDTH: f32 = 2.0 * MENU_WIDTH_STEP;
/// Upstream's `_kMenuMaxWidth`: `5.0 * _kMenuWidthStep`.
pub const MENU_MAX_WIDTH: f32 = 5.0 * MENU_WIDTH_STEP;
/// Upstream's `_kMenuScreenPadding`: the closest a menu may sit to a screen edge.
pub const MENU_SCREEN_PADDING: f32 = 8.0;
/// Upstream's `_kMenuDividerHeight`.
pub const MENU_DIVIDER_HEIGHT: f32 = 16.0;
/// Upstream's `kMinInteractiveDimension` (`material/constants.dart`), the
/// default height of a menu item.
pub const K_MIN_INTERACTIVE_DIMENSION: f32 = 48.0;

/// Whether the menu grows over or under its anchor. Upstream's
/// `PopupMenuPosition` (`material/popup_menu_theme.dart`).
///
/// It only matters to the caller's anchor math here: [`PopupMenuPosition::Over`]
/// anchors the menu at the button's own rectangle, [`PopupMenuPosition::Under`]
/// at the rectangle translated down by the button's height -- upstream's
/// `_positionBuilder` switch in `PopupMenuButtonState`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PopupMenuPosition {
    /// The menu appears directly over the anchor. Upstream's default.
    #[default]
    Over,
    /// The menu appears below the anchor.
    Under,
}

/// A base class for entries in a popup menu. Upstream's `PopupMenuEntry<T>`.
///
/// The menu itself only needs to know how tall an entry claims to be and which
/// value it stands for; everything else is the entry's own `build`.
pub trait PopupMenuEntry<T>: Component {
    /// The amount of vertical space occupied by this entry. Upstream's
    /// `PopupMenuEntry.height`.
    fn height(&self) -> f32;

    /// Whether this entry represents `value`. Upstream's
    /// `PopupMenuEntry.represents`, used to find the entry a
    /// [`PopupMenu::with_initial_value`] highlights.
    fn represents(&self, value: Option<&T>) -> bool;
}

/// Lets a boxed entry be built like any other component.
impl<T: 'static> Component for Box<dyn PopupMenuEntry<T>> {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        (**self).build(context)
    }
}

/// A horizontal divider in a popup menu. Upstream's `PopupMenuDivider`.
pub struct PopupMenuDivider {
    height: f32,
}

impl PopupMenuDivider {
    pub fn new() -> PopupMenuDivider {
        PopupMenuDivider {
            height: MENU_DIVIDER_HEIGHT,
        }
    }

    pub fn with_height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
}

impl Default for PopupMenuDivider {
    fn default() -> Self {
        Self::new()
    }
}

/// Upstream's `PopupMenuDivider extends PopupMenuEntry<Never>`, whose
/// `represents` is always false; here it is false for every `T`.
impl<T: 'static> PopupMenuEntry<T> for PopupMenuDivider {
    fn height(&self) -> f32 {
        self.height
    }

    fn represents(&self, _value: Option<&T>) -> bool {
        false
    }
}

impl Component for PopupMenuDivider {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        // `_PopupMenuDividerState.build` is `Divider(height: widget.height)`,
        // and this is this crate's `Divider` (components.rs) with the height
        // carried through: the height tall, a one-pixel line down its middle.
        let color = theme_of(context).outline;
        let height = self.height;
        leaf(move || {
            Container::new().with_height(height).with_child(Align::new(
                Alignment::CENTER,
                Container::new().with_height(1.0).with_color(color),
            ))
        })
    }
}

/// An item in a popup menu. Upstream's `PopupMenuItem<T>`.
///
/// The child upstream takes is a `Text` nine times out of ten, so the label is
/// a string here, as it is on [`crate::components::ListTile`].
pub struct PopupMenuItem<T> {
    id: u64,
    label: String,
    value: Option<T>,
    enabled: bool,
    height: f32,
    handlers: PointerHandlers,
}

impl<T: 'static> PopupMenuItem<T> {
    pub fn new(id: u64, label: impl Into<String>, value: T) -> PopupMenuItem<T> {
        PopupMenuItem {
            id,
            label: label.into(),
            value: Some(value),
            enabled: true,
            height: K_MIN_INTERACTIVE_DIMENSION,
            handlers: PointerHandlers::new(),
        }
    }

    /// Upstream's `enabled`. A disabled item does not react to taps and is
    /// drawn faded.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Upstream's `height`, the minimum height of the item.
    pub fn with_height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// Runs `select` with the item's value when the item is tapped.
    ///
    /// Upstream this is `PopupMenuItemState.handleTap`, whose two steps run in
    /// a documented order (see [`PopupMenuItemState::handle_tap`]): the menu is
    /// popped *first* -- so a callback that pushes a route does not lose that
    /// route to the pop meant for the menu -- and `onTap` runs after. Here the
    /// pop is dismissing the topmost modal, which is the menu the item lives
    /// in by construction.
    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, select: fn(&mut S, T)) -> Self
    where
        T: Clone,
    {
        if self.enabled {
            let value = self.value.clone();
            self.handlers = PointerHandlers::new().with_tap(move |_| {
                crate::theatre::dismiss_topmost_modal();
                if let Some(value) = value.clone() {
                    handle.set_state(move |state| select(state, value));
                }
            });
        }
        self
    }
}

impl<T: PartialEq + 'static> PopupMenuEntry<T> for PopupMenuItem<T> {
    fn height(&self) -> f32 {
        self.height
    }

    /// Upstream's `PopupMenuItem.represents`: `value == this.value`.
    fn represents(&self, value: Option<&T>) -> bool {
        value == self.value.as_ref()
    }
}

impl<T: 'static> Component for PopupMenuItem<T> {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let label = self.label.clone();
        let height = self.height;
        let id = self.id;
        let handlers = self.handlers.clone();
        // Upstream's M3 label style (`_PopupMenuDefaultsM3.labelTextStyle`):
        // onSurface, or onSurface at 0.38 when disabled. The crate's Theme has
        // no onSurface; `text` is the color that reads against `surface`.
        let mut style = theme.body();
        if !self.enabled {
            style.color = theme.text.with_alpha(0x61);
        }
        leaf(move || {
            // `PopupMenuItemState.build`, in its own nesting order:
            // ConstrainedBox(minHeight: height) around Padding around
            // Align(centerStart). The padding is the M3 default,
            // `_PopupMenuDefaultsM3.menuItemPadding`.
            let content = Container::new()
                .with_padding(EdgeInsets::symmetric(12.0, 0.0))
                .with_child(Align::new(
                    Alignment::CENTER_LEFT,
                    Text::new(label.clone()).with_style(style.clone()),
                ));
            let sized = RenderConstrainedBox::new(BoxConstraints::new(
                0.0,
                f32::INFINITY,
                height,
                f32::INFINITY,
            ))
            .with_child(content);
            Pointer::new(id, sized).with_handlers(handlers.clone())
        })
    }
}

/// An item with a checkmark. Upstream's `CheckedPopupMenuItem<T>`.
pub struct CheckedPopupMenuItem<T> {
    item: PopupMenuItem<T>,
    checked: bool,
}

impl<T: 'static> CheckedPopupMenuItem<T> {
    pub fn new(id: u64, label: impl Into<String>, value: T, checked: bool) -> Self {
        CheckedPopupMenuItem {
            item: PopupMenuItem::new(id, label, value),
            checked,
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.item = self.item.with_enabled(enabled);
        self
    }

    pub fn with_height(mut self, height: f32) -> Self {
        self.item = self.item.with_height(height);
        self
    }

    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, select: fn(&mut S, T)) -> Self
    where
        T: Clone,
    {
        self.item = self.item.wired(handle, select);
        self
    }
}

impl<T: PartialEq + 'static> PopupMenuEntry<T> for CheckedPopupMenuItem<T> {
    fn height(&self) -> f32 {
        self.item.height()
    }

    fn represents(&self, value: Option<&T>) -> bool {
        self.item.represents(value)
    }
}

impl<T: 'static> Component for CheckedPopupMenuItem<T> {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let label = self.item.label.clone();
        let height = self.item.height;
        let id = self.item.id;
        let handlers = self.item.handlers.clone();
        let checked = self.checked;
        // The same disabled treatment as PopupMenuItem.
        let mut style = theme.body();
        if !self.item.enabled {
            style.color = theme.text.with_alpha(0x61);
        }
        let mark_color = style.color;
        leaf(move || {
            // `_CheckedPopupMenuItemState.buildChild`: a ListTile whose leading
            // is an `Icons.done` in a 24-square box and whose title is the
            // label. There is no icon font here, so the checkmark is drawn the
            // way `Checkbox` draws its tick -- as a small stroked box rather
            // than a glyph a font might not have. The fade upstream plays on
            // tap (`_fadeDuration`, 150ms) is not ported: it runs while the
            // route the menu lives on is closing, and there is no route.
            let mark = if checked {
                Container::new()
                    .with_size(10.0, 5.0)
                    .with_border(2.0, mark_color)
                    .with_corner_radius(1.0)
            } else {
                Container::new().with_size(10.0, 5.0)
            };
            let row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                // A ListTile's leading-to-title gap, `HORIZONTAL_TITLE_GAP`.
                .with_spacing(16.0)
                .push(Align::new(
                    Alignment::CENTER,
                    crate::widgets::SizedBox::new(24.0, 24.0).with_child(mark),
                ))
                .push(Text::new(label.clone()).with_style(style.clone()));
            let content = Container::new()
                .with_padding(EdgeInsets::symmetric(12.0, 0.0))
                .with_child(row);
            let sized = RenderConstrainedBox::new(BoxConstraints::new(
                0.0,
                f32::INFINITY,
                height,
                f32::INFINITY,
            ))
            .with_child(content);
            Pointer::new(id, sized).with_handlers(handlers.clone())
        })
    }
}

/// The menu surface itself: the styled card the entries are listed in.
/// Upstream's `_PopupMenu`, minus the route animation.
///
/// Put it in a `Stack` over a [`crate::controls::Scrim`], positioned with
/// [`popup_menu_offset`]; which is also to say that closing it is the
/// application clearing the state that put it there.
pub struct PopupMenu<T> {
    entries: RefCell<Vec<Box<dyn PopupMenuEntry<T>>>>,
    initial_value: Option<T>,
}

impl<T: PartialEq + 'static> PopupMenu<T> {
    /// This menu's appearance, with the theme and the defaults folded in.
    pub fn resolved(
        &self,
        context: &mut BuildContext,
    ) -> crate::component_themes::ResolvedPopupMenu {
        crate::component_themes::ResolvedPopupMenu::of(context)
    }

    pub fn new() -> PopupMenu<T> {
        PopupMenu {
            entries: RefCell::new(Vec::new()),
            initial_value: None,
        }
    }

    /// Adds an entry. Upstream the entries are `showMenu`'s `items`, which must
    /// not be empty.
    pub fn push<E: PopupMenuEntry<T>>(self, entry: E) -> Self {
        self.entries.borrow_mut().push(Box::new(entry));
        self
    }

    /// Upstream's `showMenu(initialValue:)`: the first entry that
    /// [`PopupMenuEntry::represents`] it is highlighted.
    pub fn with_initial_value(mut self, value: T) -> Self {
        self.initial_value = Some(value);
        self
    }
}

impl<T: PartialEq + 'static> Default for PopupMenu<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: PartialEq + 'static> Component for PopupMenu<T> {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let entries = std::mem::take(&mut *self.entries.borrow_mut());
        // `_PopupMenuRoute.buildPage`: the first entry representing the
        // initial value is the selected one.
        let selected = self.initial_value.as_ref().and_then(|value| {
            entries
                .iter()
                .position(|entry| entry.represents(Some(value)))
        });
        let children: Vec<AnyWidget> = entries.into_iter().map(component).collect();
        // The M3 surface (`_PopupMenuDefaultsM3`): surfaceContainer, a
        // 4-radius corner, elevation 3, and 8 of vertical menu padding.
        // surfaceContainer has no exact slot in the crate's Theme; `surface`
        // is the card color everything else here pops from.
        let surface = theme.surface;
        // Upstream highlights the selected entry with
        // `ThemeData.highlightColor`, which the Theme has no slot for;
        // `surface_variant` is the closest "same surface, other tone".
        let highlight = theme.surface_variant;

        many(children, move |rendered| {
            // `ListBody` in a vertical `SingleChildScrollView`. The scrolling
            // is not ported: a menu that fits the screen -- every menu the
            // gallery shows -- lays out identically without it, and a menu
            // that does not fit needs a scroll offset owned by someone, which
            // is a decision for the caller, not a default.
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for (index, child) in rendered.into_iter().enumerate() {
                if Some(index) == selected {
                    column = column.push(Container::new().with_color(highlight).with_child(child));
                } else {
                    column = column.push(child);
                }
            }
            // ConstrainedBox(minWidth: _kMenuMinWidth, maxWidth:
            // _kMenuMaxWidth) around IntrinsicWidth around the padded column,
            // in the order `_PopupMenuState.build` nests them. Upstream's
            // IntrinsicWidth has `stepWidth: _kMenuWidthStep`, rounding the
            // width up to a multiple of 56; the crate's RenderIntrinsicWidth
            // has no step, so menu widths are not snapped.
            let padded = RenderPadding::new(EdgeInsets::symmetric(0.0, 8.0), column);
            let intrinsic = RenderIntrinsicWidth::new(padded);
            let constrained = RenderConstrainedBox::new(BoxConstraints::new(
                MENU_MIN_WIDTH,
                MENU_MAX_WIDTH,
                0.0,
                f32::INFINITY,
            ))
            .with_child(intrinsic);
            Box::new(
                Container::new()
                    .with_color(surface)
                    .with_corner_radius(4.0)
                    .with_elevation(3)
                    .with_child(constrained),
            )
        })
    }
}

/// Where on the screen a popup menu goes. Upstream's
/// `_PopupMenuRouteLayout.getPositionForChild` together with its
/// `_fitInsideScreen`.
///
/// `anchor` is the rectangle of the button in the overlay's coordinates (what
/// upstream's `RelativeRect` describes as distances from each edge), `menu` is
/// the menu's laid-out size, and `padding` is the unsafe-area inset, upstream's
/// `MediaQuery.padding`. The returned offset is where the menu's top-left goes
/// in a stack that covers the overlay.
///
/// Upstream first splits the screen around display features
/// (`DisplayFeatureSubScreen.subScreensInBounds`) and fits the menu inside the
/// closest sub-screen; the engine binding here reports no display features, so
/// the one sub-screen is the whole overlay, which is what that function reduces
/// to in that case.
pub fn popup_menu_offset(
    overlay: Size,
    anchor: Rect,
    menu: Size,
    padding: EdgeInsets,
    direction: TextDirection,
) -> Offset {
    // From RelativeRect: how far the anchor's right edge is from the
    // overlay's right edge.
    let anchor_right_inset = overlay.width - anchor.right;

    // Find the ideal horizontal position: grow towards whichever edge has more
    // room, ties broken in the reading direction.
    let mut x;
    if anchor.left > anchor_right_inset {
        // Closer to the right edge: grow to the left, aligned to the anchor's
        // right edge.
        x = anchor.right - menu.width;
    } else if anchor.left < anchor_right_inset {
        // Closer to the left edge: grow to the right, aligned to the anchor's
        // left edge.
        x = anchor.left;
    } else {
        x = match direction {
            TextDirection::Rtl => anchor.right - menu.width,
            TextDirection::Ltr => anchor.left,
        };
    }
    let mut y = anchor.top;

    // `_fitInsideScreen` with the whole overlay as the screen: keep the menu
    // `_kMenuScreenPadding` plus the unsafe-area inset away from every edge.
    if x < MENU_SCREEN_PADDING + padding.left {
        x = MENU_SCREEN_PADDING + padding.left;
    } else if x + menu.width > overlay.width - MENU_SCREEN_PADDING - padding.right {
        x = overlay.width - menu.width - MENU_SCREEN_PADDING - padding.right;
    }
    if y < MENU_SCREEN_PADDING + padding.top {
        y = MENU_SCREEN_PADDING + padding.top;
    } else if y + menu.height > overlay.height - MENU_SCREEN_PADDING - padding.bottom {
        y = overlay.height - menu.height - MENU_SCREEN_PADDING - padding.bottom;
    }
    Offset::new(x, y)
}

/// A button that shows a popup menu when pressed. Upstream's
/// `PopupMenuButton<T>`.
///
/// Upstream the button owns the menu: `itemBuilder` builds the entries,
/// `showButtonMenu` pushes the route, and `onSelected` reports what came back.
/// This one is the button alone: a tap target with the overflow glyph, 8 of
/// padding, and a tooltip the caller adds with
/// [`crate::controls::TooltipTrigger`]. Showing the menu is the caller's, and
/// the entries carry their own [`PopupMenuItem::wired`] callbacks.
///
/// [`crate::popup::PopupMenuButton`] is the whole of upstream's: it owns the
/// menu, opens it into the overlay against its own measured position, and is
/// what a caller usually wants. This one stays for a caller assembling the
/// pieces themselves.
pub struct PopupMenuButton {
    id: u64,
    tooltip: Option<String>,
    enabled: bool,
    child: RefCell<Option<AnyWidget>>,
    handlers: PointerHandlers,
}

impl PopupMenuButton {
    pub fn new(id: u64) -> PopupMenuButton {
        PopupMenuButton {
            id,
            tooltip: None,
            enabled: true,
            child: RefCell::new(None),
            handlers: PointerHandlers::new(),
        }
    }

    /// Upstream's `child`. When not set, the button is the standard overflow
    /// icon -- upstream's `Icons.adaptive.more`, an icon-font glyph; there is
    /// no icon font in this crate, so the glyph is the "⋮" codepoint as text.
    pub fn with_child(self, child: AnyWidget) -> Self {
        *self.child.borrow_mut() = Some(child);
        self
    }

    /// Upstream's `tooltip`, kept as data for the caller to hand to a
    /// [`crate::controls::TooltipTrigger`] wrapping this button.
    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Upstream's `enabled`: a disabled button does not respond to presses.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Runs `open` when the button is tapped. Upstream's `onPressed:
    /// showButtonMenu`; the opening itself -- putting the menu in the stack --
    /// is what `open` should set up.
    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, open: fn(&mut S)) -> Self {
        if self.enabled {
            self.handlers = PointerHandlers::new().with_tap(move |_| {
                handle.set_state(move |state| open(state));
            });
        }
        self
    }

    /// The tap as a closure, for an opener that has to be carried.
    ///
    /// [`PopupMenuButton::wired`] takes a `fn`, which cannot capture -- and
    /// what a live menu needs captured is a
    /// [`crate::popup::PopupMenuOpener`] and the overlay to put it in. The same
    /// pair `Switch::wired` and `Switch::with_handlers` make, for the same
    /// reason.
    pub fn on_press(mut self, open: impl Fn() + 'static) -> Self {
        if self.enabled {
            self.handlers = PointerHandlers::new().with_tap(move |_| open());
        }
        self
    }
}

impl Component for PopupMenuButton {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let id = self.id;
        let handlers = self.handlers.clone();
        let enabled = self.enabled;
        let child = self.child.borrow().clone().unwrap_or_else(|| {
            let mut style = theme.body();
            if !enabled {
                style.color = theme.text.with_alpha(0x61);
            }
            let style = style.clone();
            leaf(move || Text::new("⋮").with_style(style.clone()))
        });

        crate::framework::single(child, move |inner| {
            // Upstream's `padding = const EdgeInsets.all(8.0)` around the icon.
            Pointer::new(
                id,
                Container::new()
                    .with_padding(EdgeInsets::symmetric(8.0, 8.0))
                    .with_child(inner),
            )
            .with_handlers(handlers.clone())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Theme;
    use crate::framework::{ElementTree, provide};
    use crate::render::RenderBox;

    fn lay_out(widget: AnyWidget, width: f32, height: f32) -> Size {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), widget));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::loose(width, height))
    }

    /// Lays out the way a menu's column does: bounded width, height whatever
    /// the content wants. A menu item's `Align` fills a bounded height (its
    /// `RenderPositionedBox` only shrink-wraps against an unbounded one), so
    /// measuring it under `loose` would measure the offer, not the item.
    fn lay_out_in_a_column(widget: AnyWidget, width: f32) -> Size {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), widget));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::new(0.0, width, 0.0, f32::INFINITY))
    }

    #[test]
    fn a_menu_item_is_at_least_the_minimum_interactive_dimension_tall() {
        // Upstream's default `height = kMinInteractiveDimension`.
        let size = lay_out_in_a_column(component(PopupMenuItem::new(1, "Copy", 1)), 300.0);
        assert_eq!(size.height, K_MIN_INTERACTIVE_DIMENSION);
    }

    #[test]
    fn a_divider_is_its_height_tall() {
        let size = lay_out(component(PopupMenuDivider::new()), 300.0, 600.0);
        assert_eq!(size.height, MENU_DIVIDER_HEIGHT);
        let taller = lay_out(
            component(PopupMenuDivider::new().with_height(32.0)),
            300.0,
            600.0,
        );
        assert_eq!(taller.height, 32.0);
    }

    #[test]
    fn a_menu_is_never_narrower_than_the_minimum_width() {
        let menu = PopupMenu::new().push(PopupMenuItem::new(1, "A", 1));
        let size = lay_out(component(menu), 800.0, 600.0);
        assert_eq!(size.width, MENU_MIN_WIDTH);
    }

    #[test]
    fn a_menu_is_never_wider_than_the_maximum_width() {
        let label = "A menu item label that goes on and on and on and on and on and on";
        let menu = PopupMenu::new().push(PopupMenuItem::new(1, label, 1));
        let size = lay_out(component(menu), 800.0, 600.0);
        assert!(size.width <= MENU_MAX_WIDTH, "{size:?}");
    }

    #[test]
    fn an_entry_represents_its_own_value_only() {
        let item = PopupMenuItem::new(1, "Copy", 7);
        assert!(item.represents(Some(&7)));
        assert!(!item.represents(Some(&8)));
        assert!(!item.represents(None));
        // Upstream's `PopupMenuDivider.represents` is always false.
        let divider = PopupMenuDivider::new();
        assert!(!PopupMenuEntry::represents(&divider, Some(&7)));
    }

    #[test]
    fn the_menu_grows_towards_the_edge_with_more_room() {
        let overlay = Size::new(800.0, 600.0);
        let menu = Size::new(200.0, 150.0);
        let no_inset = EdgeInsets::default();
        // An anchor on the left half: aligned to the anchor's left edge.
        let offset = popup_menu_offset(
            overlay,
            Rect::ltrb(40.0, 40.0, 90.0, 80.0),
            menu,
            no_inset,
            TextDirection::Ltr,
        );
        assert_eq!(offset, Offset::new(40.0, 40.0));
        // An anchor on the right half: aligned to the anchor's right edge.
        let offset = popup_menu_offset(
            overlay,
            Rect::ltrb(700.0, 40.0, 760.0, 80.0),
            menu,
            no_inset,
            TextDirection::Ltr,
        );
        assert_eq!(offset, Offset::new(760.0 - 200.0, 40.0));
    }

    #[test]
    fn a_centred_anchor_grows_in_the_reading_direction() {
        let overlay = Size::new(800.0, 600.0);
        let menu = Size::new(200.0, 150.0);
        // left == right inset: 300 from each side.
        let anchor = Rect::ltrb(300.0, 40.0, 500.0, 80.0);
        let ltr = popup_menu_offset(
            overlay,
            anchor,
            menu,
            EdgeInsets::default(),
            TextDirection::Ltr,
        );
        assert_eq!(ltr.dx, 300.0);
        let rtl = popup_menu_offset(
            overlay,
            anchor,
            menu,
            EdgeInsets::default(),
            TextDirection::Rtl,
        );
        assert_eq!(rtl.dx, 500.0 - 200.0);
    }

    #[test]
    fn the_menu_stays_off_the_screen_edges() {
        let overlay = Size::new(800.0, 600.0);
        let menu = Size::new(200.0, 150.0);
        // An anchor hard against the bottom-right corner.
        let offset = popup_menu_offset(
            overlay,
            Rect::ltrb(790.0, 580.0, 800.0, 600.0),
            menu,
            EdgeInsets::default(),
            TextDirection::Ltr,
        );
        assert_eq!(offset.dx, 800.0 - 200.0 - MENU_SCREEN_PADDING);
        assert_eq!(offset.dy, 600.0 - 150.0 - MENU_SCREEN_PADDING);
        // And one off the top-left corner, with an unsafe-area inset on top.
        let offset = popup_menu_offset(
            overlay,
            Rect::ltrb(0.0, 0.0, 4.0, 4.0),
            menu,
            EdgeInsets::only(0.0, 24.0, 0.0, 0.0),
            TextDirection::Ltr,
        );
        assert_eq!(offset.dx, MENU_SCREEN_PADDING);
        assert_eq!(offset.dy, MENU_SCREEN_PADDING + 24.0);
    }

    #[test]
    fn a_disabled_item_carries_no_tap_handler() {
        let item = PopupMenuItem::new(1, "Paste", 3).with_enabled(false);
        assert!(item.handlers.is_empty());
    }
}

// -- The two state classes ----------------------------------------------------------

/// What tapping a popup menu item does, in the order it does it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemTapStep {
    /// `Navigator.pop<T>(context, widget.value)`.
    PopTheMenu,
    /// `widget.onTap?.call()`.
    CallOnTap,
}

/// Upstream `PopupMenuItemState`.
///
/// A `State` subclass that upstream made public on purpose: its `buildChild`
/// and `handleTap` are `@protected` and documented as override points, so a
/// caller subclasses `PopupMenuItem` and replaces the pieces rather than
/// rebuilding the item.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PopupMenuItemState {
    pub enabled: bool,
    pub has_on_tap: bool,
}

impl PopupMenuItemState {
    pub fn new() -> PopupMenuItemState {
        PopupMenuItemState {
            enabled: true,
            has_on_tap: false,
        }
    }

    /// Upstream `handleTap`, whose two lines carry a comment explaining their
    /// order:
    ///
    /// ```dart
    /// // Need to pop the navigator first in case onTap may push new route onto navigator.
    /// Navigator.pop<T>(context, widget.value);
    /// widget.onTap?.call();
    /// ```
    ///
    /// **The menu closes itself before handing control over.** Not for
    /// tidiness: a callback that pushes a route would otherwise have its own
    /// route popped by the line meant to dismiss the menu. So the item takes
    /// itself off the stack while it still knows which entry is its own, and
    /// only then lets the caller do whatever it likes to the navigator.
    ///
    /// The same shape as the button elevation chain in tick 83 -- an ordering
    /// that is load-bearing, with a comment saying so.
    pub fn handle_tap(&self) -> Vec<ItemTapStep> {
        let mut steps = vec![ItemTapStep::PopTheMenu];
        if self.has_on_tap {
            steps.push(ItemTapStep::CallOnTap);
        }
        steps
    }

    /// Upstream wires the `InkWell` with `onTap: widget.enabled ? handleTap : null`,
    /// so a disabled item is not a tap that does nothing -- it has no handler at
    /// all, and the ink does not react either.
    pub fn tap_handler(&self) -> Option<Vec<ItemTapStep>> {
        self.enabled.then(|| self.handle_tap())
    }

    /// Upstream `buildChild`, documented as *"By default, this returns
    /// `PopupMenuItem.child`. Override this to put something else in the menu
    /// entry."*
    pub fn builds_widget_child_by_default() -> bool {
        true
    }
}

impl Default for PopupMenuItemState {
    fn default() -> Self {
        PopupMenuItemState::new()
    }
}

/// Upstream `PopupMenuButtonState`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PopupMenuButtonState {
    is_menu_expanded: bool,
    /// Upstream's `_cachedButtonRenderBox` and `_cachedOverlayRenderBox`, taken
    /// together. See [`PopupMenuButtonState::update_cached_objects`].
    has_cached_boxes: bool,
    mounted: bool,
}

impl PopupMenuButtonState {
    pub fn new() -> PopupMenuButtonState {
        PopupMenuButtonState {
            is_menu_expanded: false,
            has_cached_boxes: false,
            mounted: true,
        }
    }

    /// Upstream `_updateCachedObjects`, called from `didChangeDependencies`,
    /// with a comment and a linked issue:
    ///
    /// > Caches some objects relying on context used in `_positionBuilder()`
    /// > to avoid crashing when the popup menu is inactive.
    ///
    /// **A cache kept for lifetime rather than for speed.** The position builder
    /// runs while the menu route is animating, including on its way out, and by
    /// then the button's own render object may be gone. Reading the boxes early
    /// and holding them means the menu can finish positioning itself against a
    /// button that has already left.
    ///
    /// The `mounted` check is the other half: there is no point caching a
    /// context that is already dead.
    pub fn update_cached_objects(&mut self) {
        if self.mounted {
            self.has_cached_boxes = true;
        }
    }

    pub fn did_change_dependencies(&mut self) {
        self.update_cached_objects();
    }

    /// Whether the menu can still work out where to place itself.
    pub fn can_position_menu(&self) -> bool {
        self.has_cached_boxes
    }

    pub fn unmount(&mut self) {
        self.mounted = false;
    }

    /// Upstream `showButtonMenu`, which flips `_isMenuExpanded` and pushes the
    /// route; the flag is what the button reports to semantics.
    pub fn show_button_menu(&mut self) {
        self.is_menu_expanded = true;
    }

    pub fn menu_dismissed(&mut self) {
        self.is_menu_expanded = false;
    }

    pub fn is_menu_expanded(&self) -> bool {
        self.is_menu_expanded
    }
}

#[cfg(test)]
mod popup_state_tests {
    use super::*;

    #[test]
    fn the_menu_closes_itself_before_handing_control_over() {
        // A callback that pushes a route would otherwise lose its own route to
        // the pop meant for the menu.
        let mut item = PopupMenuItemState::new();
        item.has_on_tap = true;
        assert_eq!(
            item.handle_tap(),
            vec![ItemTapStep::PopTheMenu, ItemTapStep::CallOnTap]
        );
    }

    #[test]
    fn an_item_with_no_callback_still_pops() {
        // The pop is how the value gets back to whoever opened the menu; onTap
        // is extra.
        assert_eq!(
            PopupMenuItemState::new().handle_tap(),
            vec![ItemTapStep::PopTheMenu]
        );
    }

    #[test]
    fn a_disabled_item_has_no_handler_rather_than_an_empty_one() {
        let mut item = PopupMenuItemState::new();
        item.enabled = false;
        assert_eq!(item.tap_handler(), None, "and so the ink does not react");

        item.enabled = true;
        assert!(item.tap_handler().is_some());
    }

    // -- A cache kept for lifetime -------------------------------------------------

    #[test]
    fn the_render_boxes_are_taken_early_so_the_menu_can_outlive_its_button() {
        let mut button = PopupMenuButtonState::new();
        assert!(!button.can_position_menu(), "nothing cached yet");

        button.did_change_dependencies();
        button.show_button_menu();
        assert!(button.can_position_menu());

        // The button goes away while the menu is still animating out.
        button.unmount();
        assert!(
            button.can_position_menu(),
            "and the menu can still place itself"
        );
    }

    #[test]
    fn nothing_is_cached_from_a_context_that_is_already_gone() {
        let mut button = PopupMenuButtonState::new();
        button.unmount();
        button.did_change_dependencies();
        assert!(!button.can_position_menu());
    }

    #[test]
    fn the_expansion_flag_is_what_the_button_reports() {
        let mut button = PopupMenuButtonState::new();
        assert!(!button.is_menu_expanded());
        button.show_button_menu();
        assert!(button.is_menu_expanded());
        button.menu_dismissed();
        assert!(!button.is_menu_expanded());
    }
}

#[cfg(test)]
mod popup_menu_theme_tests {
    use super::*;
    use crate::component_themes::{PopupMenuTheme, PopupMenuThemeData, ResolvedPopupMenu};
    use crate::engine::{Color, TextStyle};
    use crate::framework::{Component, ElementTree, component, leaf, provide};
    use crate::render::EdgeInsets;
    use crate::theme::ThemeData;
    use crate::widget_state::{StateProperty, WidgetState};
    use crate::widgets::SizedBox;

    struct Reader {
        seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedPopupMenu>>>,
    }

    impl Component for Reader {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            *self.seen.borrow_mut() = Some(ResolvedPopupMenu::of(context));
            leaf(|| SizedBox::new(1.0, 1.0))
        }
    }

    fn resolve_under(theme: ThemeData, data: PopupMenuThemeData) -> ResolvedPopupMenu {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            theme,
            PopupMenuTheme::new(
                data,
                component(Reader {
                    seen: std::rc::Rc::clone(&seen),
                }),
            ),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    fn m3(data: PopupMenuThemeData) -> ResolvedPopupMenu {
        resolve_under(ThemeData::fallback(), data)
    }

    fn m2(data: PopupMenuThemeData) -> ResolvedPopupMenu {
        resolve_under(
            ThemeData {
                use_material3: false,
                ..ThemeData::fallback()
            },
            data,
        )
    }

    fn flat(color: Color) -> PopupMenuThemeData {
        let mut data = PopupMenuThemeData::new();
        data.text_style = Some(TextStyle {
            color,
            ..ThemeData::fallback().text_theme.title_medium.unwrap()
        });
        data
    }

    fn stateful(color: Color) -> PopupMenuThemeData {
        let base = ThemeData::fallback().text_theme.label_large.unwrap();
        let mut data = PopupMenuThemeData::new();
        data.label_text_style = Some(StateProperty::all(Some(TextStyle { color, ..base })));
        data
    }

    #[test]
    fn the_two_style_fields_never_meet() {
        // Not "one supersedes the other" -- `useMaterial3` picks which chain
        // runs, and the other field is not read at all.
        let mine = Color(0xFFAA0000);

        assert_eq!(
            m3(flat(mine)).entry_style(true).map(|style| style.color),
            ThemeData::fallback()
                .text_theme
                .label_large
                .map(|style| style.color),
            "a flat style under Material 3 is not consulted"
        );
        assert_eq!(
            m2(stateful(mine))
                .entry_style(true)
                .map(|style| style.color),
            ThemeData::fallback()
                .text_theme
                .title_medium
                .map(|style| style.color),
            "and a state property under Material 2 is not either"
        );

        // Each in its own branch does land, or the test above would prove
        // nothing more than that neither field works.
        assert_eq!(
            m2(flat(mine)).entry_style(true).map(|style| style.color),
            Some(mine)
        );
        assert_eq!(
            m3(stateful(mine))
                .entry_style(true)
                .map(|style| style.color),
            Some(mine)
        );
    }

    #[test]
    fn setting_both_does_not_make_them_compete_either() {
        // The reading the old doc invited: both set, and one wins. What
        // actually happens is that each branch reads its own and neither sees
        // the other.
        let flat_colour = Color(0xFFAA0000);
        let state_colour = Color(0xFF00AA00);
        let mut both = flat(flat_colour);
        both.label_text_style = stateful(state_colour).label_text_style;

        assert_eq!(
            m3(both.clone()).entry_style(true).map(|style| style.color),
            Some(state_colour)
        );
        assert_eq!(
            m2(both).entry_style(true).map(|style| style.color),
            Some(flat_colour)
        );
    }

    #[test]
    fn material_two_overwrites_a_disabled_items_colour_and_material_three_does_not() {
        // The visible consequence of where the disabled colour is applied. On
        // Material 2 it happens after the chain, so it lands on a caller's own
        // style; on Material 3 it happens inside the step the caller supplied.
        let mine = Color(0xFFAA0000);
        let disabled = ThemeData::fallback().disabled_color;

        assert_eq!(
            m2(flat(mine)).entry_style(false).map(|style| style.color),
            Some(disabled),
            "the caller's colour is gone"
        );
        assert_eq!(
            m3(stateful(mine))
                .entry_style(false)
                .map(|style| style.color),
            Some(mine),
            "the caller's resolver is the last word"
        );
    }

    #[test]
    fn material_threes_own_disabled_colour_comes_out_of_the_resolution() {
        // With no theme property, the default resolver fades it -- and it is a
        // fade of `onSurface`, not the theme's `disabledColor`.
        let scheme = ThemeData::fallback().color_scheme;
        let faded = crate::elevation_overlay::with_opacity(scheme.on_surface, 0.38);
        assert_eq!(
            m3(PopupMenuThemeData::new())
                .entry_style(false)
                .map(|style| style.color),
            Some(faded)
        );
        assert_ne!(faded, ThemeData::fallback().disabled_color);
        assert_eq!(
            m3(PopupMenuThemeData::new())
                .entry_style(true)
                .map(|style| style.color),
            Some(scheme.on_surface)
        );
    }

    #[test]
    fn a_state_property_that_answers_per_state_is_asked_per_state() {
        // The property is resolved with `{disabled}` or `{}`, so a resolver
        // that distinguishes them is distinguished.
        let enabled = Color(0xFF111111);
        let off = Color(0xFF222222);
        let base = ThemeData::fallback().text_theme.label_large.unwrap();
        let mut data = PopupMenuThemeData::new();
        data.label_text_style = Some(StateProperty::resolve_with(move |states| {
            Some(TextStyle {
                color: if states.contains(WidgetState::Disabled) {
                    off
                } else {
                    enabled
                },
                ..base.clone()
            })
        }));
        let resolved = m3(data);
        assert_eq!(resolved.entry_style(true).map(|s| s.color), Some(enabled));
        assert_eq!(resolved.entry_style(false).map(|s| s.color), Some(off));
    }

    #[test]
    fn the_menus_padding_and_the_items_are_perpendicular() {
        // They compose rather than fight: one pads top and bottom, the other
        // left and right, and neither has an opinion about the other's axis.
        let menu = m3(PopupMenuThemeData::new());
        assert_eq!(menu.menu_padding, EdgeInsets::symmetric(0.0, 8.0));
        assert_eq!(menu.item_padding, EdgeInsets::symmetric(12.0, 0.0));
        assert_eq!(menu.menu_padding.left, 0.0);
        assert_eq!(menu.item_padding.top, 0.0);
    }

    #[test]
    fn only_one_of_the_two_paddings_has_a_theme_step() {
        let mut data = PopupMenuThemeData::new();
        data.menu_padding = Some(crate::EdgeInsetsGeometry::Absolute(EdgeInsets::all(3.0)));
        let resolved = m3(data);
        assert_eq!(resolved.menu_padding, EdgeInsets::all(3.0));
        assert_eq!(
            resolved.item_padding,
            EdgeInsets::symmetric(12.0, 0.0),
            "the item's is a static on the defaults class, with no theme field \
             to move it"
        );
        assert_eq!(
            m2(PopupMenuThemeData::new()).item_padding,
            EdgeInsets::symmetric(16.0, 0.0),
            "and the material version is the only thing that does"
        );
    }

    #[test]
    fn the_surface_defaults_differ_by_material_version() {
        let scheme = ThemeData::fallback().color_scheme;
        let three = m3(PopupMenuThemeData::new());
        let two = m2(PopupMenuThemeData::new());
        assert_eq!(three.elevation, 3.0);
        assert_eq!(two.elevation, 8.0);
        assert!(three.shape.is_some());
        assert_eq!(two.shape, None, "Material 2 has no shape default");
        assert_eq!(three.shadow_color, Some(scheme.shadow()));
        assert_eq!(two.shadow_color, None);
        assert_eq!(three.surface_tint_color, Some(Color::TRANSPARENT));
        assert_eq!(two.surface_tint_color, None);
    }

    #[test]
    fn a_menu_opens_over_its_button_rather_than_under_it() {
        // Upstream's default `PopupMenuPosition.over`: the menu covers the
        // thing that opened it, so the item under the finger is where the
        // finger already is.
        assert_eq!(
            m3(PopupMenuThemeData::new()).position,
            crate::menu::PopupMenuPosition::Over
        );
        assert!(m3(PopupMenuThemeData::new()).enable_feedback);
    }
}
