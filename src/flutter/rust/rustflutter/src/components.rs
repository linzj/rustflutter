// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! A small component library.
//!
//! Upstream this tier is `material` (176,201 lines) and `cupertino` (48,253),
//! together 36% of the framework. Neither is ported here, and the reason is not
//! effort: they are implementations of two specific design languages, and
//! transliterating them into Rust produces something that is neither Flutter's
//! Material nor a Rust API. What is here instead is the set of components an
//! application actually reaches for, designed for this framework.
//!
//! # Theming
//!
//! Every component takes its colours and metrics from a [`Theme`], read through
//! [`crate::framework::BuildContext::inherited`]. Install one at the root:
//!
//! ```ignore
//! provide(Theme::dark(), component(MyPage))
//! ```
//!
//! A component with no theme above it falls back to [`Theme::default`] rather
//! than failing to build.
//!
//! # What is deliberately absent
//!
//! Text input lives one layer down, in [`crate::editable`]: a usable field
//! needs the platform's input method -- composition regions, candidate
//! windows, a cursor the OS can position -- and that belongs to the field
//! itself, not to a palette of shared widgets.

use std::rc::Rc;

use crate::engine::{Color, TextStyle};
use crate::framework::{AnyWidget, BuildContext, Component, StateHandle, component, leaf, many};
use crate::gestures::PointerHandlers;
use crate::render::{
    Alignment, BoxConstraints, BoxedRender, CrossAxisAlignment, EdgeInsets, EdgeInsetsDirectional,
    HitTestResult, MainAxisAlignment, MainAxisSize, Offset, PaintContext, RenderBox,
    RenderClipRect, RenderConstrainedBox, RenderFlex, RenderPadding, RenderRef, RenderStack, Size,
    StackFit, StackPosition, TextOverflow, UpdateEffect,
};
use crate::widgets::{
    Align, Center, Column, Container, Empty, K_MIDDLE_SPACING, Pointer, RenderNavigationToolbar,
    Row, SizedBox, Text,
};

// -- Theme --------------------------------------------------------------------

/// Colours and metrics shared by every component.
#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub background: Color,
    pub surface: Color,
    pub surface_variant: Color,
    pub outline: Color,
    pub primary: Color,
    pub on_primary: Color,
    pub danger: Color,
    pub text: Color,
    pub text_muted: Color,

    /// Corner radius for cards, buttons and fields.
    pub radius: f32,
    /// The spacing unit everything else is a multiple of.
    pub spacing: f32,
    pub body_size: f32,
    pub title_size: f32,
    /// The face everything is set in, or None for the system default.
    ///
    /// It lives on the theme rather than on each call site because a font is a
    /// property of the design, and a component that had to be told its family
    /// would be a component someone could forget to tell.
    pub font_family: Option<&'static str>,
}

impl Theme {
    pub fn dark() -> Theme {
        Theme {
            background: Color::rgb(0x0B, 0x11, 0x1E),
            surface: Color::rgb(0x16, 0x21, 0x33),
            surface_variant: Color::rgb(0x1E, 0x2B, 0x40),
            outline: Color::rgb(0x2A, 0x3B, 0x54),
            primary: Color::rgb(0x54, 0xC5, 0xF8),
            on_primary: Color::rgb(0x06, 0x1A, 0x24),
            danger: Color::rgb(0xE0, 0x7A, 0x9B),
            text: Color::rgb(0xE8, 0xEE, 0xF5),
            text_muted: Color::rgb(0x7F, 0x93, 0xAD),
            radius: 12.0,
            spacing: 8.0,
            body_size: 14.0,
            title_size: 20.0,
            font_family: None,
        }
    }

    pub fn light() -> Theme {
        Theme {
            background: Color::rgb(0xF7, 0xF9, 0xFC),
            surface: Color::WHITE,
            surface_variant: Color::rgb(0xED, 0xF2, 0xF9),
            outline: Color::rgb(0xD3, 0xDD, 0xEA),
            primary: Color::rgb(0x0B, 0x72, 0xB5),
            on_primary: Color::WHITE,
            danger: Color::rgb(0xC0, 0x39, 0x5F),
            text: Color::rgb(0x10, 0x1A, 0x27),
            text_muted: Color::rgb(0x5C, 0x6E, 0x85),
            radius: 12.0,
            spacing: 8.0,
            body_size: 14.0,
            title_size: 20.0,
            font_family: None,
        }
    }

    pub fn body(&self) -> TextStyle {
        TextStyle {
            font_size: self.body_size,
            color: self.text,
            font_family: self.font_family.map(str::to_string),
            ..TextStyle::default()
        }
    }

    pub fn muted(&self) -> TextStyle {
        TextStyle {
            font_size: self.body_size,
            color: self.text_muted,
            font_family: self.font_family.map(str::to_string),
            ..TextStyle::default()
        }
    }

    pub fn title(&self) -> TextStyle {
        TextStyle {
            font_size: self.title_size,
            color: self.text,
            font_weight: 700,
            font_family: self.font_family.map(str::to_string),
            ..TextStyle::default()
        }
    }
}

impl Default for Theme {
    fn default() -> Theme {
        Theme::dark()
    }
}

/// Reads the theme in scope, or the default.
pub fn theme_of(context: &BuildContext) -> Rc<Theme> {
    context.inherited_or_default::<Theme>()
}

// -- Ids ----------------------------------------------------------------------

/// Hit-test identities are `u64`, and a component that invents one must not
/// collide with another. Every component here takes its id from the caller,
/// and this is what a caller can use to get them without thinking about it.
///
/// Not thread safe by construction: the framework runs on the UI task runner.
pub struct IdSource {
    next: std::cell::Cell<u64>,
}

impl IdSource {
    pub fn new(first: u64) -> IdSource {
        IdSource {
            next: std::cell::Cell::new(first.max(1)),
        }
    }

    pub fn take(&self) -> u64 {
        let id = self.next.get();
        self.next.set(id + 1);
        id
    }
}

impl Default for IdSource {
    fn default() -> IdSource {
        IdSource::new(1)
    }
}

// -- Button -------------------------------------------------------------------

/// How a [`Button`] is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ButtonStyle {
    /// Filled with the primary colour. For the one action a screen is about.
    #[default]
    Filled,
    /// Outlined, transparent inside. For secondary actions.
    Outlined,
    /// Label only. For actions that should not compete.
    Text,
    /// Filled with the danger colour.
    Danger,
}

/// A button's height, and the least width one may be. Upstream's
/// `minimumSize` for `FilledButton`: `Size(64, 40)`.
const BUTTON_HEIGHT: f32 = 40.0;
const BUTTON_MIN_WIDTH: f32 = 64.0;

/// The horizontal padding a button's label gets, following the reader's text
/// size. Upstream `ButtonStyleButton.scaledPadding`: sixteen points at the
/// ordinary size, lerping down to eight by twice the size and to four by
/// three times it, where it stays -- the label keeps its room without the
/// button growing without bound.
fn button_padding(text_scale: f32) -> f32 {
    let lerp = |from: f32, to: f32, t: f32| from + (to - from) * t;
    if text_scale < 1.0 {
        16.0
    } else if text_scale < 2.0 {
        lerp(16.0, 8.0, text_scale - 1.0)
    } else if text_scale < 3.0 {
        lerp(8.0, 4.0, text_scale - 2.0)
    } else {
        4.0
    }
}

/// A button's least size around its label, upstream's `_InputPadding`
/// (`button_style_button.dart`).
///
/// The one idea of it is that the label chain is measured with no ceiling on
/// it -- upstream calls `child.layout(const BoxConstraints())` -- so nothing
/// the space around the button offers reaches the label, and the label and
/// the padding around it alone decide how wide the button is. The least size
/// rides along as minimums on those constraints, which is where upstream
/// puts them too, one layer in: a label shorter than the minimum is centred
/// across it, not parked at the leading edge of a wider box.
///
/// A constrained box cannot stand in for this. One holding a least width
/// passes the width on offer down with it, the label centres into the offer,
/// and every button in a loose row comes out as wide as the row.
struct ButtonBounds {
    min_width: f32,
    min_height: f32,
    child: Option<BoxedRender>,
    size: Size,
}

impl ButtonBounds {
    fn new(min_width: f32, min_height: f32) -> ButtonBounds {
        ButtonBounds {
            min_width,
            min_height,
            child: None,
            size: Size::ZERO,
        }
    }

    fn with_child(mut self, child: impl RenderBox + 'static) -> Self {
        self.child = Some(RenderRef::new(child));
        self
    }
}

impl RenderBox for ButtonBounds {
    fn update_from(&mut self, fresh: &mut dyn RenderBox) -> Option<UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<ButtonBounds>()?;
        let same_child = |a: &Option<BoxedRender>, b: &Option<BoxedRender>| match (a, b) {
            (Some(a), Some(b)) => a.is(b),
            (None, None) => true,
            _ => false,
        };
        let effect = UpdateEffect::relayout_if(
            self.min_width != fresh.min_width
                || self.min_height != fresh.min_height
                || !same_child(&self.child, &fresh.child),
        );
        self.min_width = fresh.min_width;
        self.min_height = fresh.min_height;
        self.child = fresh.child.take();
        Some(effect)
    }

    /// `_InputPadding.performLayout`: the child's answer at nothing but the
    /// minimums, put through the constraints the button itself was given.
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let inner = BoxConstraints::new(
            self.min_width,
            f32::INFINITY,
            self.min_height,
            f32::INFINITY,
        );
        self.size = match &mut self.child {
            Some(child) => constraints.constrain(child.layout_child(inner, true)),
            None => constraints.constrain(Size::new(self.min_width, self.min_height)),
        };
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        let inner = BoxConstraints::new(
            self.min_width,
            f32::INFINITY,
            self.min_height,
            f32::INFINITY,
        );
        match &self.child {
            Some(child) => constraints.constrain(child.dry_layout(inner)),
            None => constraints.constrain(Size::new(self.min_width, self.min_height)),
        }
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        if let Some(child) = &self.child {
            context.paint_child(child, offset);
        }
    }

    fn hit_test_children(&self, position: Offset, result: &mut HitTestResult) -> bool {
        self.child
            .as_ref()
            .is_some_and(|child| child.hit_test(position, result))
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        if let Some(child) = &self.child {
            visit(child, Offset::ZERO);
        }
    }

    // `_InputPadding`'s intrinsics, each the child's answer held to the same
    // minimum the layout holds it to.
    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.child
            .as_ref()
            .map_or(0.0, |child| child.min_intrinsic_width(height))
            .max(self.min_width)
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.child
            .as_ref()
            .map_or(0.0, |child| child.max_intrinsic_width(height))
            .max(self.min_width)
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.child
            .as_ref()
            .map_or(0.0, |child| child.min_intrinsic_height(width))
            .max(self.min_height)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.child
            .as_ref()
            .map_or(0.0, |child| child.max_intrinsic_height(width))
            .max(self.min_height)
    }
}

/// A tappable button with a pressed state.
///
/// The pressed state is the caller's, not the button's: a button is rebuilt
/// every frame and cannot remember anything, so whoever owns the surrounding
/// state owns this too. [`ButtonGroupState`] is the usual place to put it.
pub struct Button {
    id: u64,
    label: String,
    style: ButtonStyle,
    pressed: bool,
    enabled: bool,
    handlers: PointerHandlers,
    min_width: Option<f32>,
}

impl Button {
    pub fn new(id: u64, label: impl Into<String>) -> Button {
        Button {
            id,
            label: label.into(),
            style: ButtonStyle::default(),
            pressed: false,
            enabled: true,
            handlers: PointerHandlers::new(),
            min_width: None,
        }
    }

    pub fn with_style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    pub fn with_pressed(mut self, pressed: bool) -> Self {
        self.pressed = pressed;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The least width this button may be, in place of the usual
    /// [`BUTTON_MIN_WIDTH`]. A longer label still widens the button; a short
    /// one stops at this.
    pub fn with_min_width(mut self, width: f32) -> Self {
        self.min_width = Some(width);
        self
    }

    pub fn with_handlers(mut self, handlers: PointerHandlers) -> Self {
        self.handlers = handlers;
        self
    }

    /// The usual wiring: a tap runs `action` and a press repaints.
    ///
    /// `pressed_field` says where in the state the currently held button id
    /// lives, which is what lets one field serve a whole screen of buttons.
    pub fn wired<S: 'static>(
        mut self,
        handle: StateHandle<S>,
        pressed_field: fn(&mut S) -> &mut Option<u64>,
        action: fn(&mut S),
    ) -> Self {
        if !self.enabled {
            return self;
        }
        let id = self.id;
        let tap_handle = handle.clone();
        let press_handle = handle;
        self.handlers = PointerHandlers::new()
            .with_tap(move |_| {
                tap_handle.set_state(move |state| action(state));
            })
            .with_press_change(move |down| {
                press_handle.set_state(move |state| {
                    *pressed_field(state) = if down { Some(id) } else { None };
                });
            });
        self
    }
}

impl Component for Button {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let style = self.style;
        let pressed = self.pressed && self.enabled;
        let enabled = self.enabled;
        let label = self.label.clone();
        let handlers = self.handlers.clone();
        let id = self.id;
        let min_width = self.min_width;

        let (mut fill, mut label_color, mut border) = match style {
            ButtonStyle::Filled => (Some(theme.primary), theme.on_primary, None),
            ButtonStyle::Danger => (Some(theme.danger), theme.on_primary, None),
            ButtonStyle::Outlined => (None, theme.primary, Some(theme.outline)),
            ButtonStyle::Text => (None, theme.primary, None),
        };
        if !enabled {
            // A disabled button is one rule rather than a palette per style,
            // and it is upstream M3's: the surface overlaid with 12% of the
            // on-surface colour where there was a fill or an outline, and the
            // label at 38% of it. Washing each colour out on its own reads as
            // a translucent button, which is not what a disabled one is.
            if fill.is_some() || border.is_some() {
                let wash = theme.text.with_alpha(0x1F);
                if fill.is_some() {
                    fill = Some(wash);
                }
                border = border.map(|_| wash);
            }
            label_color = theme.text.with_alpha(0x61);
        }
        // Pressed keeps the fill opaque and layers translucence over it, the
        // way the splash does: upstream's state layer, over the button rather
        // than thinning the button. An unfilled button tints in its own
        // colour, which is all a press has to say there.
        let press_overlay = pressed.then(|| match style {
            ButtonStyle::Filled | ButtonStyle::Danger => theme.on_primary.with_alpha(0x1A),
            _ => theme.primary.with_alpha(0x1A),
        });
        // As round as the button is tall: upstream's `StadiumBorder`.
        let radius = BUTTON_HEIGHT / 2.0;
        let padding = button_padding(crate::media_query::current_text_scale());
        let body_size = theme.body_size;
        // The splash is the button's own colour: a filled button splashes in
        // what it is written on, an outlined one in what it is outlined with.
        let splash_color = match style {
            ButtonStyle::Filled | ButtonStyle::Danger => theme.on_primary.with_alpha(0x30),
            _ => theme.primary.with_alpha(0x24),
        };

        // A closure, because the ink below rebuilds its child on every splash
        // frame: a stateful component is built from the same widget instance
        // each time, so a child handed over once is gone the second time.
        let face = move || {
            let label = label.clone();
            let handlers = handlers.clone();
            leaf(move || {
                let mut container = Container::new()
                    .with_height(BUTTON_HEIGHT)
                    .with_corner_radius(radius)
                    .with_padding(EdgeInsets::symmetric(padding, 0.0))
                    .with_child(Align::new(
                        Alignment::CENTER,
                        // Upstream's label style for buttons is `labelLarge`:
                        // medium weight, not bold.
                        Text::new(label.clone())
                            .with_size(body_size)
                            .with_weight(500)
                            .with_color(label_color),
                    ));
                if let Some(color) = fill {
                    container = container.with_color(color);
                }
                if let Some(color) = border {
                    container = container
                        .with_border(1.5, if pressed { theme_border(color) } else { color });
                }
                // A press tints the whole button over its opaque fill, the way
                // the splash does, rather than thinning the fill.
                let body = if let Some(overlay) = press_overlay {
                    RenderStack::new().push(container).push_positioned(
                        Container::new()
                            .with_color(overlay)
                            .with_corner_radius(radius),
                        StackPosition::fill(),
                    )
                } else {
                    RenderStack::new().push(container)
                };
                // The least size a button may be, upstream's `minimumSize`, held
                // by the bounds box above: a longer label still widens it, a
                // short one stops here.
                Pointer::new(
                    id,
                    ButtonBounds::new(min_width.unwrap_or(BUTTON_MIN_WIDTH), BUTTON_HEIGHT)
                        .with_child(body),
                )
                .with_handlers(handlers.clone())
            })
        };

        // What a reader is told, and what activating it does. The
        // semantics id is the hit-test id, so the two answers to "which
        // control is this" cannot drift apart; the action calls the same
        // closure the finger would, which is upstream's rule for
        // `Semantics.onTap` -- the two paths must not be able to disagree
        // about what pressing this does.
        let described = |inner: AnyWidget| {
            let properties = if enabled {
                crate::semantics::SemanticsProperties::button(&self.label)
            } else {
                crate::semantics::SemanticsProperties::disabled_button(&self.label)
            };
            let tap = self.handlers.on_tap.clone();
            crate::semantics::semantics_with_action(
                crate::semantics::node_id_for(id),
                properties,
                inner,
                move |action| {
                    if action == crate::semantics::SemanticsAction::Tap {
                        if let Some(tap) = &tap {
                            tap(crate::gestures::TapEvent {
                                local_position: crate::render::Offset::ZERO,
                                pointer_id: 0,
                            });
                        }
                    }
                },
            )
        };

        if !enabled {
            return described(face());
        }
        // The splash goes inside the button's own region, and hears the
        // pointer because raw pointer events reach every listener on the path
        // -- the tap still belongs to the button. Clipped to the button's
        // corners, which is what `containedInkWell` means upstream.
        described(crate::framework::stateful(
            crate::ink::Ink::new(id.wrapping_add(INK_ID_OFFSET), face).with_color(splash_color),
        ))
    }
}

/// How far a button's splash id is from the button's own.
///
/// Ids are the caller's, and a component that invents one has to be sure it
/// does not collide with a caller's. A large fixed offset is the same trick
/// the examples use for their own id blocks.
const INK_ID_OFFSET: u64 = 1 << 40;

/// Slightly brighter than the resting outline, so a pressed outlined button
/// reads as pressed without changing size.
fn theme_border(color: Color) -> Color {
    Color::argb(
        0xFF,
        color.red().saturating_add(0x30),
        color.green().saturating_add(0x30),
        color.blue().saturating_add(0x30),
    )
}

/// The state a screen full of [`Button`]s needs: which one is held.
#[derive(Default)]
pub struct ButtonGroupState {
    pub pressed: Option<u64>,
}

// -- Card ---------------------------------------------------------------------

/// A surface with a border and padding, for grouping.
pub struct Card {
    child: std::cell::RefCell<Option<AnyWidget>>,
    padding: Option<EdgeInsets>,
}

impl Card {
    pub fn new(child: AnyWidget) -> Card {
        Card {
            child: std::cell::RefCell::new(Some(child)),
            padding: None,
        }
    }

    pub fn with_padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = Some(padding);
        self
    }
}

impl Component for Card {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let padding = self.padding.unwrap_or(EdgeInsets::all(theme.spacing * 2.0));
        let child = self
            .child
            .borrow_mut()
            .take()
            .unwrap_or_else(|| leaf(|| Empty));
        let surface = theme.surface;
        let outline = theme.outline;
        let radius = theme.radius;
        crate::framework::single(child, move |inner| {
            // Full width, so a column of cards has one left edge and one right
            // edge rather than one pair per card.
            Box::new(crate::widgets::FullWidth::new(
                Container::new()
                    .with_color(surface)
                    .with_corner_radius(radius)
                    // Material 3's elevated card sits one step off the page.
                    .with_elevation(1)
                    .with_border(1.0, outline)
                    .with_padding(padding)
                    .with_child(inner),
            ))
        })
    }
}

// -- Text ---------------------------------------------------------------------

/// A themed run of text.
pub struct Label {
    content: String,
    style: LabelStyle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LabelStyle {
    #[default]
    Body,
    Muted,
    Title,
}

impl Label {
    pub fn new(content: impl Into<String>) -> Label {
        Label {
            content: content.into(),
            style: LabelStyle::default(),
        }
    }

    pub fn title(content: impl Into<String>) -> Label {
        Label {
            content: content.into(),
            style: LabelStyle::Title,
        }
    }

    pub fn muted(content: impl Into<String>) -> Label {
        Label {
            content: content.into(),
            style: LabelStyle::Muted,
        }
    }
}

impl Component for Label {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let style = match self.style {
            LabelStyle::Body => theme.body(),
            LabelStyle::Muted => theme.muted(),
            LabelStyle::Title => theme.title(),
        };
        let content = self.content.clone();
        let text = leaf(move || Text::new(content.clone()).with_style(style.clone()));

        // Text is the one thing that needs no arranging to be accessible: what
        // it says is what it is. A title is also a heading, which is what lets
        // a screen reader jump between sections instead of reading everything.
        // Upstream `Text` does the same, in its own `build`.
        let properties = match self.style {
            LabelStyle::Title => crate::semantics::SemanticsProperties::header(&self.content),
            _ => crate::semantics::SemanticsProperties::label(&self.content),
        };
        crate::semantics::describe(properties, text)
    }
}

// -- Switch -------------------------------------------------------------------

/// An on/off control.
pub struct Switch {
    id: u64,
    value: bool,
    handlers: PointerHandlers,
}

impl Switch {
    pub fn new(id: u64, value: bool) -> Switch {
        Switch {
            id,
            value,
            handlers: PointerHandlers::new(),
        }
    }

    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, toggle: fn(&mut S)) -> Self {
        self.handlers = PointerHandlers::new().with_tap(move |_| {
            handle.set_state(move |state| toggle(state));
        });
        self
    }

    /// Handlers built by the caller, for a switch whose action needs to carry
    /// something with it.
    ///
    /// [`Switch::wired`] takes a `fn` and not a closure, which is what makes it
    /// short: there is nothing to capture and nothing to allocate. That is the
    /// right trade for a switch that toggles one named field, and the wrong one
    /// for a row of switches built from a list, where the action has to know
    /// *which* row it is. [`Button::with_handlers`] exists for the same reason.
    pub fn with_handlers(mut self, handlers: PointerHandlers) -> Self {
        self.handlers = handlers;
        self
    }
}

impl Component for Switch {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let value = self.value;
        let id = self.id;
        let handlers = self.handlers.clone();
        let track = if value { theme.primary } else { theme.outline };
        let knob = if value {
            theme.on_primary
        } else {
            theme.text_muted
        };
        let tap = self.handlers.on_tap.clone();

        let switch = leaf(move || {
            // The knob is a positioned child of a row rather than a Stack, so
            // that the track's own padding does the insetting.
            let knob_box = Container::new()
                .with_size(20.0, 20.0)
                .with_color(knob)
                .with_corner_radius(10.0);
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            row = if value {
                row.with_main_axis_alignment(MainAxisAlignment::End)
                    .push(knob_box)
            } else {
                row.with_main_axis_alignment(MainAxisAlignment::Start)
                    .push(knob_box)
            };
            Pointer::new(
                id,
                Container::new()
                    .with_size(48.0, 28.0)
                    .with_color(track)
                    .with_corner_radius(14.0)
                    .with_padding(EdgeInsets::all(4.0))
                    .with_child(row),
            )
            .with_handlers(handlers.clone())
        });

        // A switch says which way it is, and that is the whole point of
        // `has_checked_state`: without it a reader is told there is a switch
        // and not whether it is on. The label is left to whatever is beside
        // it -- upstream's `Switch` has none of its own either, because a
        // switch with no context is not a thing a reader can act on.
        crate::semantics::semantics_with_action(
            crate::semantics::node_id_for(id),
            crate::semantics::SemanticsProperties::toggle("", value),
            switch,
            move |action| {
                if action == crate::semantics::SemanticsAction::Tap {
                    if let Some(tap) = &tap {
                        tap(crate::gestures::TapEvent {
                            local_position: crate::render::Offset::ZERO,
                            pointer_id: 0,
                        });
                    }
                }
            },
        )
    }
}

// -- Progress -----------------------------------------------------------------

/// A horizontal progress bar. `value` is clamped to 0..1.
pub struct ProgressBar {
    value: f32,
    width: f32,
}

impl ProgressBar {
    pub fn new(value: f32) -> ProgressBar {
        ProgressBar {
            value: value.clamp(0.0, 1.0),
            width: 200.0,
        }
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }
}

impl Component for ProgressBar {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let value = self.value;
        let width = self.width;
        let track = theme.surface_variant;
        let fill = theme.primary;
        leaf(move || {
            // Align hands its child loose constraints, which is what stops a
            // stretching parent from widening the bar past `width`.
            Align::new(
                Alignment::CENTER_LEFT,
                Container::new()
                    .with_size(width, 8.0)
                    .with_color(track)
                    .with_corner_radius(4.0)
                    .with_child(
                        RenderFlex::row()
                            .with_main_axis_size(MainAxisSize::Max)
                            .with_main_axis_alignment(MainAxisAlignment::Start)
                            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                            // A zero-width child would be invisible either way, but
                            // an explicit floor keeps the rounded cap from
                            // collapsing into a sliver at very low values.
                            .push(
                                Container::new()
                                    .with_width((width * value).max(if value > 0.0 {
                                        8.0
                                    } else {
                                        0.0
                                    }))
                                    .with_color(fill)
                                    .with_corner_radius(4.0),
                            ),
                    ),
            )
        })
    }
}

// -- Slider -------------------------------------------------------------------

/// A draggable value between 0 and 1.
pub struct Slider {
    id: u64,
    value: f32,
    width: f32,
    handlers: PointerHandlers,
}

impl Slider {
    pub fn new(id: u64, value: f32) -> Slider {
        Slider {
            id,
            value: value.clamp(0.0, 1.0),
            width: 200.0,
            handlers: PointerHandlers::new(),
        }
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Drags and taps both set the value from the pointer's position along the
    /// track, which is the behaviour that needs no separate "thumb grabbed"
    /// state.
    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, set: fn(&mut S, f32)) -> Self {
        let width = self.width;
        let drag_handle = handle.clone();
        let tap_handle = handle;
        self.handlers = PointerHandlers::new()
            .with_drag_update(move |drag| {
                let value = (drag.local_position.dx / width).clamp(0.0, 1.0);
                drag_handle.set_state(move |state| set(state, value));
            })
            .with_tap(move |tap| {
                let value = (tap.local_position.dx / width).clamp(0.0, 1.0);
                tap_handle.set_state(move |state| set(state, value));
            });
        self
    }
}

impl Component for Slider {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let value = self.value;
        let width = self.width;
        let id = self.id;
        let handlers = self.handlers.clone();
        let track = theme.surface_variant;
        let fill = theme.primary;
        let knob = theme.text;

        leaf(move || {
            let filled = (width * value).clamp(0.0, width);
            // The hit region has to be exactly `width` wide: the value comes
            // from local_position.dx / width, so a region that a stretching
            // parent widened would reach 100% before the thumb did. Align
            // loosens the constraints, which keeps the size the one asked for.
            Align::new(
                Alignment::CENTER_LEFT,
                Pointer::new(
                    id,
                    Container::new()
                        // A 32px tall hit area over an 8px track: the thing you can
                        // hit should be bigger than the thing you can see.
                        .with_size(width, 32.0)
                        .with_child(Center::new(
                            Container::new()
                                .with_size(width, 8.0)
                                .with_color(track)
                                .with_corner_radius(4.0)
                                .with_child(
                                    RenderFlex::row()
                                        .with_main_axis_size(MainAxisSize::Max)
                                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                                        .push(
                                            Container::new()
                                                .with_size(filled, 8.0)
                                                .with_color(fill)
                                                .with_corner_radius(4.0),
                                        )
                                        .push(
                                            Container::new()
                                                .with_size(18.0, 18.0)
                                                .with_color(knob)
                                                .with_corner_radius(9.0),
                                        ),
                                ),
                        )),
                )
                .with_handlers(handlers.clone()),
            )
        })
    }
}

// -- Structure ----------------------------------------------------------------

/// How tall a title bar is. Upstream's `kToolbarHeight`
/// (`material/constants.dart`).
pub const K_TOOLBAR_HEIGHT: f32 = 56.0;

/// How tall a title bar carrying a subtitle is.
///
/// Upstream's `AppBar` has no subtitle, so it has no value for this and no
/// need of one. `toolbarHeight` is an `AppBar` parameter upstream
/// (`toolbarHeight ?? appBarTheme.toolbarHeight ?? kToolbarHeight`); this is
/// this bar's choice of it for the two-line case, which is [`K_TOOLBAR_HEIGHT`]
/// plus room for a `body_size` line and the 2px between them.
pub const TOOLBAR_HEIGHT_WITH_SUBTITLE: f32 = 76.0;

/// The gap between a list tile's text and whatever sits at its edges.
/// Upstream's `ListTile.horizontalTitleGap`, which defaults to 16
/// (`list_tile.dart`, where the theme falls back to `?? 16`).
const HORIZONTAL_TITLE_GAP: f32 = 16.0;

/// The most a title's text is allowed to grow with the reader's text size.
/// Upstream's `_kMaxTitleTextScaleFactor` (`material/app_bar.dart`), which
/// exists because the bar's height does not grow with it.
const MAX_TITLE_TEXT_SCALE_FACTOR: f32 = 1.34;

/// A title bar.
pub struct AppBar {
    title: String,
    subtitle: Option<String>,
    trailing: std::cell::RefCell<Option<AnyWidget>>,
}

impl AppBar {
    pub fn new(title: impl Into<String>) -> AppBar {
        AppBar {
            title: title.into(),
            subtitle: None,
            trailing: std::cell::RefCell::new(None),
        }
    }

    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn with_trailing(self, trailing: AnyWidget) -> Self {
        *self.trailing.borrow_mut() = Some(trailing);
        self
    }
}

impl Component for AppBar {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let title = self.title.clone();
        let subtitle = self.subtitle.clone();
        let trailing = self.trailing.borrow_mut().take();
        // The bar's own background is what should extend under the status bar,
        // not the page behind it, so this is padding *inside* the surface
        // rather than a `SafeArea` wrapped around it. Upstream reaches the same
        // shape from the other side: `AppBar.build` ends with
        // `SafeArea(bottom: false, child: appBar)` where `appBar` is already
        // the coloured `Material`.
        let system = crate::media_query::media_query_of(context).padding;
        let surface = theme.surface;
        let outline = theme.outline;
        let title_style = theme.title();
        let muted = theme.muted();

        let has_subtitle = subtitle.is_some();
        let toolbar_height = if has_subtitle {
            TOOLBAR_HEIGHT_WITH_SUBTITLE
        } else {
            K_TOOLBAR_HEIGHT
        };

        let mut children = vec![leaf(move || {
            // One line each, cut with an ellipsis. Upstream wraps the title in
            // `DefaultTextStyle(softWrap: false, overflow: TextOverflow.ellipsis)`
            // for exactly this: a bar is a fixed height, so a title that wrapped
            // would only be clipped, and a clipped word reads worse than an
            // elided one.
            let one_line = |text: &str, style: &TextStyle| {
                Text::new(text.to_string())
                    .with_style(style.clone())
                    .with_soft_wrap(false)
                    .with_overflow(TextOverflow::Ellipsis)
                    .with_max_lines(1)
                    // Upstream clamps the title's text scale to
                    // `_kMaxTitleTextScaleFactor` -- "to keep the visual
                    // hierarchy the same even with larger font sizes" -- which
                    // is also what keeps a reader's larger text inside a bar
                    // whose height does not grow with it.
                    .with_text_scale(
                        crate::media_query::current_text_scale().min(MAX_TITLE_TEXT_SCALE_FACTOR),
                    )
            };
            // `MainAxisSize.min` because `_ToolbarLayout` centres a
            // content-sized middle (`_getMiddleOffset`); a max-sized column
            // would fill the bar's height and pin the title to its top.
            let mut stack = Column::new()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(2.0)
                .push(one_line(&title, &title_style));
            if let Some(subtitle) = &subtitle {
                stack = stack.push(one_line(subtitle, &muted));
            }
            stack
        })];
        if let Some(trailing) = trailing {
            children.push(trailing);
        }
        let has_trailing = children.len() > 1;

        many(children, move |mut rendered| {
            let trailing = if has_trailing { rendered.pop() } else { None };
            let middle = rendered.pop();

            // Upstream `AppBar.build` assembles exactly this: a
            // `NavigationToolbar` of leading / middle / trailing, wrapped in a
            // `ClipRect` around a `CustomSingleChildLayout` that pins the
            // toolbar to `toolbarHeight`. There is no leading here -- this bar
            // puts its one action on the trailing side.
            let mut toolbar = RenderNavigationToolbar::new()
                // `_getEffectiveCenterTitle` answers false on Android, Linux,
                // Windows and Fuchsia, and only iOS and macOS centre.
                .with_center_middle(false)
                .with_middle_spacing(K_MIDDLE_SPACING);
            if let Some(middle) = middle {
                toolbar = toolbar.with_middle(middle);
            }
            if let Some(trailing) = trailing {
                // Upstream: `actions = Padding(padding: actionsPadding, child:
                // Row(mainAxisSize: MainAxisSize.min, crossAxisAlignment:
                // center, children: actions))`.
                //
                // The min-size row is not decoration. `_ToolbarLayout` hands
                // the trailing `BoxConstraints.loose(size)` -- a *bounded*
                // width -- and anything that fills what it is offered would
                // take the whole bar and leave the title nothing. An `Align`
                // does exactly that (upstream's `RenderPositionedBox` shrink
                // wraps only against an unbounded constraint), and this
                // framework's `Button` centres its label with one. A row that
                // asks for `MainAxisSize.min` is what makes the actions report
                // the width they actually want.
                //
                // The padding is upstream's `actionsPadding`. Upstream's own
                // actions need less of it than this one does -- an
                // `IconButton` is 48 wide around a 24pt icon and carries its
                // own margin, where a `Button` here is a pill with a border
                // that would otherwise sit flush against the edge of the bar.
                //
                // It is also directional, upstream's `actionsPadding` being an
                // `EdgeInsetsDirectional`: the actions sit at the *trailing*
                // end of the bar, which is the right in an LTR subtree and the
                // left in an RTL one, so the inset resolves against the
                // ambient direction the moment this is built -- the same
                // `Directionality.of` consumption every directional widget in
                // `basic.dart` makes.
                toolbar = toolbar.with_trailing(RenderPadding::new(
                    EdgeInsetsDirectional::only(K_MIDDLE_SPACING, 0.0, 0.0, 0.0)
                        .resolve(crate::direction::current_direction()),
                    RenderFlex::row()
                        .with_main_axis_size(MainAxisSize::Min)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .push(trailing),
                ));
            }

            // `_ToolbarContainerLayout`: the toolbar is tightened to
            // `toolbarHeight` and the bar is exactly that tall, so a title too
            // big for it is clipped rather than allowed to grow the bar.
            //
            // Upstream's delegate also bottom-justifies the toolbar inside a
            // container that has been given *less* than `toolbarHeight`, which
            // is how a `SliverAppBar` scrolls its toolbar away. There is no
            // sliver protocol here, so that case cannot arise and the
            // justification would be dead code; a tight height says the rest.
            let bar = RenderClipRect::new(
                RenderConstrainedBox::new(BoxConstraints::new(
                    0.0,
                    f32::INFINITY,
                    toolbar_height,
                    toolbar_height,
                ))
                .with_child(toolbar),
            );

            Box::new(
                Container::new()
                    .with_color(surface)
                    .with_border(1.0, outline)
                    // Only the safe area, now: the horizontal inset that used
                    // to be here is the toolbar's `middleSpacing` and the
                    // trailing's own padding, which is where upstream keeps it.
                    .with_padding(EdgeInsets::only(system.left, system.top, system.right, 0.0))
                    .with_child(bar),
            )
        })
    }
}

/// A title bar over a body, on the theme's background.
pub struct Scaffold {
    app_bar: std::cell::RefCell<Option<AnyWidget>>,
    body: std::cell::RefCell<Option<AnyWidget>>,
    drawer: std::cell::RefCell<Option<AnyWidget>>,
    drawer_open: bool,
    drawer_alignment: crate::drawer::DrawerAlignment,
    drawer_scrim_id: Option<u64>,
    drawer_handlers: PointerHandlers,
}

impl Scaffold {
    pub fn new(body: AnyWidget) -> Scaffold {
        Scaffold {
            app_bar: std::cell::RefCell::new(None),
            body: std::cell::RefCell::new(Some(body)),
            drawer: std::cell::RefCell::new(None),
            drawer_open: false,
            drawer_alignment: crate::drawer::DrawerAlignment::default(),
            drawer_scrim_id: None,
            drawer_handlers: PointerHandlers::new(),
        }
    }

    pub fn with_app_bar(self, app_bar: AnyWidget) -> Self {
        *self.app_bar.borrow_mut() = Some(app_bar);
        self
    }

    /// Upstream's `Scaffold.drawer` (`material/scaffold.dart`): the panel the
    /// scaffold shows over the body, behind a scrim, while it is open.
    ///
    /// Upstream the scaffold owns the drawer's opening -- `ScaffoldState.
    /// openDrawer`, the edge swipe, the back button -- through a
    /// `DrawerController` with an animation controller. None of that machinery
    /// is portable (see [`crate::drawer`]'s module docs), so whether the
    /// drawer is open is the application's state, handed over with
    /// [`Scaffold::with_drawer_open`].
    pub fn with_drawer(self, drawer: AnyWidget) -> Self {
        *self.drawer.borrow_mut() = Some(drawer);
        self
    }

    /// Whether the drawer is currently shown. Upstream this is the
    /// `DrawerController`'s animation being at either end; here it is simply
    /// the application's state.
    pub fn with_drawer_open(mut self, open: bool) -> Self {
        self.drawer_open = open;
        self
    }

    /// Which edge the drawer is pinned to. Upstream's drawers are `start` by
    /// default, with an `endDrawer` slot for the other side; one slot with an
    /// alignment says the same thing. Upstream's `DrawerAlignment`.
    pub fn with_drawer_alignment(mut self, alignment: crate::drawer::DrawerAlignment) -> Self {
        self.drawer_alignment = alignment;
        self
    }

    /// Runs `close` when the scrim behind the open drawer is tapped.
    ///
    /// Upstream this is the barrier of `DrawerController._buildDrawer`, whose
    /// `onTap` is `close` because `drawerBarrierDismissible` defaults to true.
    pub fn wired_drawer<S: 'static>(
        mut self,
        id: u64,
        handle: StateHandle<S>,
        close: fn(&mut S),
    ) -> Self {
        self.drawer_scrim_id = Some(id);
        self.drawer_handlers = PointerHandlers::new().with_tap(move |_| {
            handle.set_state(move |state| close(state));
        });
        self
    }
}

impl Component for Scaffold {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let background = theme.background;
        let app_bar = self.app_bar.borrow_mut().take();
        let body = self
            .body
            .borrow_mut()
            .take()
            .unwrap_or_else(|| leaf(|| Empty));
        let drawer = self.drawer.borrow_mut().take();
        // A drawer nobody opened is nothing at all: upstream's closed
        // `DrawerController` builds a `SizedBox.shrink` on desktop (the
        // edge-drag strip it would install on mobile is not ported; see
        // crate::drawer's module docs).
        let drawer_open = self.drawer_open && drawer.is_some();
        let drawer_alignment = self.drawer_alignment;
        let scrim_id = self.drawer_scrim_id;
        let scrim_handlers = self.drawer_handlers.clone();

        let has_app_bar = app_bar.is_some();
        // A bar has already moved the page down past the status bar, so the
        // body must not do it again -- and must still be told what the bar did
        // not deal with, which is the bottom. Upstream's `Scaffold` removes the
        // same padding from the body's `MediaQuery` for the same reason.
        let body = if has_app_bar {
            let data = crate::media_query::media_query_of(context);
            crate::media_query::MediaQuery::new(data.remove_padding(true, true, true, false), body)
        } else {
            body
        };
        let mut children = Vec::new();
        if let Some(app_bar) = app_bar {
            children.push(app_bar);
        }
        children.push(body);
        if drawer_open {
            // The two overlay layers, in paint order: the scrim over the page,
            // the drawer over the scrim. Upstream's `Stack` in
            // `DrawerController._buildDrawer` is exactly these two.
            let handlers = scrim_handlers.clone();
            children.push(leaf(move || {
                // `Colors.black54`, the drawer barrier's default color.
                Pointer::new(
                    scrim_id.unwrap_or(0),
                    Container::new().with_color(crate::drawer::DRAWER_SCRIM),
                )
                .with_handlers(handlers.clone())
            }));
            children.push(drawer.expect("checked above"));
        }

        many(children, move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            let mut rendered = rendered.into_iter();
            if has_app_bar {
                if let Some(bar) = rendered.next() {
                    column = column.push(bar);
                }
            }
            if let Some(body) = rendered.next() {
                // The body takes everything the bar left.
                column = column.push_flex(crate::render::FlexChild::expanded(body, 1));
            }
            if !drawer_open {
                return RenderRef::new(Container::new().with_color(background).with_child(column));
            }
            let scrim = rendered.next();
            let drawer = rendered.next();

            // The page is the stack's unpositioned child and fills it; the
            // scrim fills it by position; the drawer is pinned to its edge and
            // stretched top to bottom, which is the `Align` of
            // `_drawerOuterAlignment` plus the `widthFactor: 1.0` of a fully
            // open drawer.
            let mut stack = RenderStack::new()
                .with_fit(StackFit::Expand)
                .push(Container::new().with_color(background).with_child(column));
            if let Some(scrim) = scrim {
                stack = stack.push_positioned(scrim, StackPosition::fill());
            }
            if let Some(drawer) = drawer {
                // Which physical edge `start` is depends on the reading
                // direction, resolved now -- the same moment upstream's build
                // reads `Directionality.of(context)`.
                let on_left = crate::drawer::drawer_on_left(
                    drawer_alignment,
                    crate::direction::current_direction(),
                );
                let position = StackPosition {
                    left: on_left.then_some(0.0),
                    right: (!on_left).then_some(0.0),
                    top: Some(0.0),
                    bottom: Some(0.0),
                    ..Default::default()
                };
                stack = stack.push_positioned(drawer, position);
            }
            RenderRef::new(stack)
        })
    }
}

/// A row with a leading marker, a title, an optional subtitle and an optional
/// trailing widget. What a settings list is made of.
pub struct ListTile {
    title: String,
    subtitle: Option<String>,
    accent: Option<Color>,
    trailing: std::cell::RefCell<Option<AnyWidget>>,
    id: Option<u64>,
    handlers: PointerHandlers,
}

impl ListTile {
    pub fn new(title: impl Into<String>) -> ListTile {
        ListTile {
            title: title.into(),
            subtitle: None,
            accent: None,
            trailing: std::cell::RefCell::new(None),
            id: None,
            handlers: PointerHandlers::new(),
        }
    }

    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn with_accent(mut self, color: Color) -> Self {
        self.accent = Some(color);
        self
    }

    pub fn with_trailing(self, trailing: AnyWidget) -> Self {
        *self.trailing.borrow_mut() = Some(trailing);
        self
    }

    pub fn tappable(mut self, id: u64, handlers: PointerHandlers) -> Self {
        self.id = Some(id);
        self.handlers = handlers;
        self
    }
}

impl Component for ListTile {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let title = self.title.clone();
        let subtitle = self.subtitle.clone();
        let accent = self.accent;
        let id = self.id;
        let handlers = self.handlers.clone();
        let trailing = self.trailing.borrow_mut().take();
        let body = theme.body();
        let muted = theme.muted();
        let spacing = theme.spacing;
        let radius = theme.radius;

        let has_trailing = trailing.is_some();
        let mut children = vec![leaf(move || {
            // `MainAxisSize.min`: upstream `_RenderListTile` centres the
            // title block vertically rather than stretching it to the tile,
            // and the row below centres this column the same way.
            let mut column = Column::new()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(3.0)
                .push(Text::new(title.clone()).with_style(TextStyle {
                    font_weight: 700,
                    ..body.clone()
                }));
            if let Some(subtitle) = &subtitle {
                column = column.push(Text::new(subtitle.clone()).with_style(muted.clone()));
            }
            let mut row = Row::new().with_spacing(spacing * 1.5);
            if let Some(accent) = accent {
                row = row.push(
                    Container::new()
                        .with_size(10.0, 10.0)
                        .with_color(accent)
                        .with_corner_radius(5.0),
                );
            }
            row.push(column)
        })];
        if let Some(trailing) = trailing {
            children.push(trailing);
        }

        many(children, move |mut rendered| {
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(if has_trailing {
                    HORIZONTAL_TITLE_GAP
                } else {
                    0.0
                });
            if has_trailing {
                // The trailing is inflexible, so pass one gives it its width
                // and the title takes what is left rather than shouldering the
                // trailing off the end. Upstream's `ListTile` reaches the same
                // place through `_RenderListTile._computeSizes`, which sizes
                // the leading and the trailing first, positions the trailing at
                // `tileWidth - trailingSize.width`, and gives the text
                // `looseConstraints.tighten(width: tileWidth - titleStart -
                // adjustedTrailingWidth)` -- *tightened*, which is why the
                // title here is `expanded` and not `flexible`.
                //
                // One line of upstream's is not expressed: its reserve is
                // `math.max(trailingSize.width + gap, 32.0)`, where a flex
                // reserves exactly `trailing + spacing`. The two differ only
                // for a trailing narrower than the gap itself, and every
                // trailing here -- a switch, a price, a button -- is wider
                // than 16.

                let trailing = rendered.pop();
                let title = rendered.pop();
                if let Some(title) = title {
                    row = row.push_flex(crate::render::FlexChild::expanded(title, 1));
                }
                if let Some(trailing) = trailing {
                    row = row.push(trailing);
                }
            } else if let Some(leading) = rendered.pop() {
                row = row.push(leading);
            }

            let padded = Container::new()
                .with_padding(EdgeInsets::symmetric(spacing * 1.5, spacing * 1.5))
                .with_corner_radius(radius)
                .with_child(row);
            match id {
                Some(id) => crate::render::RenderRef::new(
                    Pointer::new(id, padded).with_handlers(handlers.clone()),
                ),
                None => crate::render::RenderRef::new(padded),
            }
        })
    }
}

/// A hairline rule. Upstream reserves sixteen logical pixels and centers a
/// zero-thickness (device-pixel) line in it; one logical pixel is this
/// renderer's hairline at unit scale.
pub struct Divider;

impl Component for Divider {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let color = theme.outline;
        leaf(move || {
            Container::new().with_height(16.0).with_child(Align::new(
                Alignment::CENTER,
                Container::new().with_height(1.0).with_color(color),
            ))
        })
    }
}

/// Empty space of a fixed size, in theme spacing units.
pub struct Gap(pub f32);

impl Component for Gap {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let size = self.0 * theme.spacing;
        leaf(move || SizedBox::new(size, size))
    }
}

/// Convenience: a vertical gap of `units` theme spacings.
pub fn gap(units: f32) -> AnyWidget {
    component(Gap(units))
}

/// A badge: a small pill of text.
pub struct Badge {
    label: String,
    color: Option<Color>,
}

impl Badge {
    pub fn new(label: impl Into<String>) -> Badge {
        Badge {
            label: label.into(),
            color: None,
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

impl Component for Badge {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let color = self.color.unwrap_or(theme.primary);
        let label = self.label.clone();
        let size = theme.body_size - 3.0;
        leaf(move || {
            Container::new()
                .with_color(color.with_alpha(0x22))
                .with_corner_radius(9.0)
                .with_padding(EdgeInsets::symmetric(9.0, 5.0))
                .with_child(
                    Text::new(label.clone())
                        .with_size(size)
                        .with_weight(700)
                        .with_color(color),
                )
        })
    }
}

/// A column of children with the theme's spacing between them.
pub fn stack_column(children: Vec<AnyWidget>, spacing: f32) -> AnyWidget {
    many(children, move |rendered| {
        let mut column = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);
        for child in rendered {
            column = column.push(child);
        }
        Box::new(column)
    })
}

/// A row of children with the theme's spacing between them.
pub fn stack_row(children: Vec<AnyWidget>, spacing: f32) -> AnyWidget {
    many(children, move |rendered| {
        let mut row = RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing);
        for child in rendered {
            row = row.push(child);
        }
        Box::new(row)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{ElementTree, provide};
    use crate::render::{BoxConstraints, RenderBox};

    #[test]
    fn a_component_without_a_theme_still_builds() {
        let mut tree = ElementTree::new();
        tree.rebuild(component(Label::new("no theme installed")));
        let mut root = tree.build_render_tree().expect("a root");
        let size = root.layout(BoxConstraints::loose(200.0, 200.0));
        assert!(size.width >= 0.0);
    }

    /// A tile's trailing sits against the right edge, whatever its title is.
    ///
    /// Measured by hit test rather than by reading geometry: a render tree
    /// reports where it was touched, not where it put things, and where it was
    /// touched is the thing that has to be right anyway.
    #[test]
    fn a_list_tile_pushes_its_trailing_to_the_right_edge() {
        const TRAILING: u64 = 77;
        const WIDTH: f32 = 400.0;
        const TRAILING_WIDTH: f32 = 40.0;

        fn hits(title: &str, x: f32) -> bool {
            let mut tree = ElementTree::new();
            tree.rebuild(provide(
                Theme::dark(),
                component(
                    ListTile::new(title.to_string()).with_trailing(crate::framework::leaf(|| {
                        // A fixed box rather than a Label: the engine stubs
                        // these tests link report zero-sized text, so a
                        // paragraph would measure nothing.
                        crate::widgets::Pointer::new(
                            TRAILING,
                            Container::new().with_size(TRAILING_WIDTH, 12.0),
                        )
                    })),
                ),
            ));
            let mut root = tree.build_render_tree().expect("a root");
            let size = root.layout(BoxConstraints::tight(WIDTH, 60.0));
            let mut result = crate::render::HitTestResult::new();
            root.hit_test(
                crate::render::Offset::new(x, size.height / 2.0),
                &mut result,
            );
            result.path.iter().any(|entry| entry.target == TRAILING)
        }

        // The tile pads itself, so the trailing's right edge is inset by that
        // much rather than flush with the tile's own edge.
        let inset = Theme::dark().spacing * 1.5;
        let inside = WIDTH - inset - TRAILING_WIDTH / 2.0;

        for title in ["Rent", "RedPay Credit", "A considerably longer bill name"] {
            assert!(
                hits(title, inside),
                "trailing should be at the right for {title:?}"
            );
            assert!(
                !hits(title, WIDTH / 2.0),
                "trailing should not stretch back to the middle for {title:?}"
            );
        }
    }

    #[test]
    fn a_provided_theme_reaches_a_descendant() {
        struct Probe;

        impl Component for Probe {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                let theme = theme_of(context);
                // The light theme's background is nothing like the dark one's,
                // so reading it back proves the lookup found the right one.
                assert_eq!(theme.background, Theme::light().background);
                leaf(|| Empty)
            }
        }

        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::light(), component(Probe)));
        assert!(tree.build_render_tree().is_some());
    }

    #[test]
    fn the_nearest_provider_wins() {
        struct Inner;

        impl Component for Inner {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                let theme = theme_of(context);
                assert_eq!(theme.background, Theme::dark().background);
                leaf(|| Empty)
            }
        }

        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            Theme::light(),
            provide(Theme::dark(), component(Inner)),
        ));
        assert!(tree.build_render_tree().is_some());
    }

    #[test]
    fn a_provider_adds_nothing_to_layout() {
        let mut bare = ElementTree::new();
        bare.rebuild(leaf(|| SizedBox::new(40.0, 20.0)));
        let mut bare_root = bare.build_render_tree().unwrap();
        let bare_size = bare_root.layout(BoxConstraints::loose(200.0, 200.0));

        let mut wrapped = ElementTree::new();
        wrapped.rebuild(provide(Theme::dark(), leaf(|| SizedBox::new(40.0, 20.0))));
        let mut wrapped_root = wrapped.build_render_tree().unwrap();
        let wrapped_size = wrapped_root.layout(BoxConstraints::loose(200.0, 200.0));

        assert_eq!(bare_size, wrapped_size);
    }

    #[test]
    fn an_id_source_hands_out_distinct_ids() {
        let ids = IdSource::new(1);
        let first = ids.take();
        let second = ids.take();
        assert_ne!(first, second);
        // Zero means "no identity" to the hit tester, so it must never be
        // handed out.
        assert_ne!(first, 0);
    }

    #[test]
    fn a_disabled_button_has_no_handlers() {
        let button = Button::new(1, "go").with_enabled(false).wired(
            StateHandle::<ButtonGroupState>::detached(),
            |s| &mut s.pressed,
            |_| {},
        );
        assert!(button.handlers.is_empty());
    }

    #[test]
    fn a_button_is_forty_tall_and_no_narrower_than_sixty_four() {
        // Upstream `FilledButton`'s defaults: a `minimumSize` of
        // `Size(64, 40)` with a `StadiumBorder`, as round as it is tall. A
        // longer label widens the button past the minimum; a short one stops
        // at it. The stubbed engine measures the label as nothing, so this is
        // exactly the minimum-size case.
        let mut tree = ElementTree::new();
        tree.rebuild(component(Button::new(1, "go")));
        let mut root = tree.build_render_tree().expect("a root");
        let size = root.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(size.width, 64.0, "a short label stops at the minimum");
        assert_eq!(size.height, 40.0);

        let mut tree = ElementTree::new();
        tree.rebuild(component(Button::new(1, "go").with_min_width(120.0)));
        let mut root = tree.build_render_tree().expect("a root");
        let size = root.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(size.width, 120.0, "a raised minimum raises the button");
        assert_eq!(size.height, 40.0);
    }

    #[test]
    fn button_padding_follows_the_readers_text_size() {
        // Upstream `ButtonStyleButton.scaledPadding`: sixteen points of
        // horizontal padding at the ordinary text size, eight at twice it and
        // four at three times, lerped in between and held at four after.
        assert_eq!(button_padding(0.5), 16.0);
        assert_eq!(button_padding(1.0), 16.0);
        assert_eq!(button_padding(1.5), 12.0);
        assert_eq!(button_padding(2.0), 8.0);
        assert_eq!(button_padding(2.5), 6.0);
        assert_eq!(button_padding(3.0), 4.0);
        assert_eq!(button_padding(4.0), 4.0);
    }

    #[test]
    fn a_slider_clamps_its_value() {
        assert_eq!(Slider::new(1, 5.0).value, 1.0);
        assert_eq!(Slider::new(1, -2.0).value, 0.0);
        assert_eq!(ProgressBar::new(0.5).value, 0.5);
    }

    /// An open drawer is hit at its own edge; everything else on the page is
    /// behind the scrim, and a closed drawer is not there at all.
    #[test]
    fn an_open_drawer_covers_the_page_behind_a_scrim() {
        const SCRIM: u64 = 41;
        const DRAWER_ITEM: u64 = 42;

        fn close(_: &mut ()) {}

        fn hits_at(open: bool, position: crate::render::Offset) -> Vec<u64> {
            let drawer = component(crate::drawer::Drawer::new(crate::framework::leaf(|| {
                crate::widgets::Pointer::new(DRAWER_ITEM, Container::new().with_color(Color::WHITE))
            })));
            let scaffold = Scaffold::new(crate::framework::leaf(|| Empty))
                .with_drawer(drawer)
                .with_drawer_open(open)
                .wired_drawer(SCRIM, StateHandle::<()>::detached(), close);
            let mut tree = ElementTree::new();
            tree.rebuild(provide(Theme::dark(), component(scaffold)));
            let mut root = tree.build_render_tree().expect("a root");
            root.layout(BoxConstraints::tight(800.0, 600.0));
            let mut result = crate::render::HitTestResult::new();
            root.hit_test(position, &mut result);
            result.path.iter().map(|entry| entry.target).collect()
        }

        // Closed: neither the drawer nor its scrim is in the tree.
        let hits = hits_at(false, crate::render::Offset::new(20.0, 20.0));
        assert!(!hits.contains(&DRAWER_ITEM), "{hits:?}");
        assert!(!hits.contains(&SCRIM), "{hits:?}");

        // Open: the drawer's own edge hits the drawer...
        let hits = hits_at(true, crate::render::Offset::new(20.0, 20.0));
        assert!(hits.contains(&DRAWER_ITEM), "{hits:?}");
        // ...and a point past the drawer hits the scrim instead of the page.
        let hits = hits_at(true, crate::render::Offset::new(500.0, 300.0));
        assert!(hits.contains(&SCRIM), "{hits:?}");
        assert!(!hits.contains(&DRAWER_ITEM), "{hits:?}");
    }
}
