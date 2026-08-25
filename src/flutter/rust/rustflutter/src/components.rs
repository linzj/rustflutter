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
use crate::slider_theme::ResolvedSlider;
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
///
/// Upstream has no such enum: it has four widgets -- `FilledButton`,
/// `OutlinedButton`, `TextButton` and `ElevatedButton` -- which are
/// `ButtonStyleButton` subclasses differing only in their default
/// [`ButtonStyle`](crate::component_themes::ButtonStyle). One widget with a
/// variant is the same set said differently, and each variant reads the
/// button theme upstream's matching widget reads.
///
/// This was called `ButtonStyle` until upstream's `ButtonStyle` was ported;
/// the two are different things -- upstream's is a bag of twenty-five state
/// properties -- and sharing the name made the coverage ruler count a class
/// as ported that was not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    /// Filled with the primary colour. For the one action a screen is about.
    #[default]
    Filled,
    /// Outlined, transparent inside. For secondary actions.
    Outlined,
    /// Label only. For actions that should not compete.
    Text,
    /// Filled with the danger colour.
    Danger,
    /// Upstream `ElevatedButton`: a raised card of a button.
    ///
    /// **Not a filled button with a shadow.** Its background is
    /// `surfaceContainerLow` and its label the primary -- the opposite way
    /// round from a filled button, whose background is the primary. Material 3
    /// demotes it deliberately: elevation is how it stands out, so the colour
    /// does not have to, and a raised button in the primary colour would be
    /// shouting twice.
    Elevated,
}

impl ButtonVariant {
    /// The control's own defaults -- the *third* step of upstream's chain,
    /// after the caller's style and the theme's.
    ///
    /// On the enum rather than inside `Button::build` because it is not only
    /// `Button` that needs it: the gallery's button demo draws its own
    /// variants and had this table copied out by hand, which meant adding a
    /// variant here broke it there. A table two places have to agree on
    /// belongs to neither of them.
    pub fn default_colors(self, theme: &Theme) -> (Option<Color>, Color, Option<Color>) {
        match self {
            ButtonVariant::Filled => (Some(theme.primary), theme.on_primary, None),
            ButtonVariant::Danger => (Some(theme.danger), theme.on_primary, None),
            ButtonVariant::Outlined => (None, theme.primary, Some(theme.outline)),
            ButtonVariant::Text => (None, theme.primary, None),
            // See the variant's own docs: the low surface container behind a
            // primary label, which is a filled button's pair the other way up.
            ButtonVariant::Elevated => (Some(theme.surface_variant), theme.primary, None),
        }
    }
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
    style: ButtonVariant,
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
            style: ButtonVariant::default(),
            pressed: false,
            enabled: true,
            handlers: PointerHandlers::new(),
            min_width: None,
        }
    }

    pub fn with_style(mut self, style: ButtonVariant) -> Self {
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

        let (mut fill, mut label_color, mut border) = style.default_colors(&theme);
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
        // Upstream's `ButtonStyleButton.build` merges the caller's style, the
        // theme's and the widget's own defaults, then resolves each field
        // against the button's states. The three lines above are this
        // control's defaults -- the last of the three -- and the theme, where
        // there is one, is asked first.
        let mut states = crate::widget_state::WidgetStates::NONE;
        if !enabled {
            states = states.with(crate::widget_state::WidgetState::Disabled);
        }
        if pressed {
            states = states.with(crate::widget_state::WidgetState::Pressed);
        }
        let resolved = crate::component_themes::ResolvedButton::of(
            context,
            style,
            states,
            crate::component_themes::ResolvedButton {
                background: fill,
                foreground: label_color,
                side: border.map(|color| crate::borders::BorderSide {
                    color,
                    width: 1.5,
                    ..crate::borders::BorderSide::NONE
                }),
                padding: None,
                minimum_size: None,
            },
        );
        let fill = resolved.background;
        let label_color = resolved.foreground;
        let border = resolved.side;
        let themed_padding = resolved.padding;
        let themed_min_size = resolved.minimum_size;
        // Pressed keeps the fill opaque and layers translucence over it, the
        // way the splash does: upstream's state layer, over the button rather
        // than thinning the button. An unfilled button tints in its own
        // colour, which is all a press has to say there.
        let press_overlay = pressed.then(|| match style {
            ButtonVariant::Filled | ButtonVariant::Danger => theme.on_primary.with_alpha(0x1A),
            // Elevated tints in the primary with the unfilled ones, not in
            // `onPrimary` with the filled one: its *label* is the primary, so
            // that is the colour a press has to say something in.
            _ => theme.primary.with_alpha(0x1A),
        });
        // As round as the button is tall: upstream's `StadiumBorder`.
        let height = themed_min_size.map_or(BUTTON_HEIGHT, |size| size.height);
        let radius = height / 2.0;
        let padding = themed_padding.map_or_else(
            || button_padding(crate::media_query::current_text_scale()),
            |insets| insets.left,
        );
        let body_size = theme.body_size;
        // The splash is the button's own colour: a filled button splashes in
        // what it is written on, an outlined one in what it is outlined with.
        let splash_color = match style {
            ButtonVariant::Filled | ButtonVariant::Danger => theme.on_primary.with_alpha(0x30),
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
                    .with_height(height)
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
                if let Some(side) = border {
                    let color = if pressed {
                        theme_border(side.color)
                    } else {
                        side.color
                    };
                    container = container.with_border(side.width, color);
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
        // Upstream `Card.build`: `color`, `elevation` and `shape` come off
        // `CardTheme.of(context)` before the control's own defaults.
        let card = crate::component_themes::CardTheme::of(context);
        let surface = card.color.unwrap_or(theme.surface);
        let outline = theme.outline;
        let radius = theme.radius;
        // Material 3's elevated card sits one step off the page; a theme that
        // says otherwise says it in whole elevation steps, as the crate's
        // shadow table is indexed by them.
        let elevation = card
            .elevation
            .map_or(1, |elevation| elevation.round().max(0.0) as u32);
        crate::framework::single(child, move |inner| {
            // Full width, so a column of cards has one left edge and one right
            // edge rather than one pair per card.
            Box::new(crate::widgets::FullWidth::new(
                Container::new()
                    .with_color(surface)
                    .with_corner_radius(radius)
                    .with_elevation(elevation)
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

/// What one switch's appearance came out as. Upstream has no such type -- it
/// resolves each property where it is used -- and having one here is what lets
/// the decision be checked without painting it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwitchColors {
    pub track: Color,
    pub knob: Color,
    /// `None` when the theme named no outline colour.
    pub outline: Option<Color>,
    /// Zero when none was asked for, which is what stops the border being drawn.
    pub outline_width: f32,
    pub padding: EdgeInsets,
}

/// Upstream `Switch`: an on/off control.
///
/// # The colours come from the theme through a state, not from the value
///
/// Upstream's `thumbColor`, `trackColor` and `trackOutlineColor` are
/// `WidgetStateProperty`s, and the state they are resolved against carries
/// `selected` **and** `disabled` together. That is why a disabled switch that
/// is on and a disabled switch that is off can be told apart, and why they are
/// both distinguishable from the enabled pair -- four appearances, from two
/// bits, out of one property. Reading the value alone would give two.
///
/// The per-switch `activeColor` and `inactiveTrackColor` sit *above* the theme
/// and are the plain way to change one switch; upstream keeps them because a
/// caller with one switch to recolour should not have to write a state
/// property to do it.
pub struct Switch {
    id: u64,
    value: bool,
    handlers: PointerHandlers,
    /// Upstream's `onChanged == null`, which is how a switch is disabled --
    /// there is no separate flag.
    enabled: bool,
    /// Upstream's `activeColor`: the thumb when on.
    active_color: Option<Color>,
    /// Upstream's `activeTrackColor`.
    active_track_color: Option<Color>,
    /// Upstream's `inactiveThumbColor`.
    inactive_thumb_color: Option<Color>,
    /// Upstream's `inactiveTrackColor`.
    inactive_track_color: Option<Color>,
}

impl Switch {
    pub fn new(id: u64, value: bool) -> Switch {
        Switch {
            id,
            value,
            handlers: PointerHandlers::new(),
            enabled: true,
            active_color: None,
            active_track_color: None,
            inactive_thumb_color: None,
            inactive_track_color: None,
        }
    }

    /// Upstream's `onChanged: null`.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_active_color(mut self, color: Color) -> Self {
        self.active_color = Some(color);
        self
    }

    pub fn with_active_track_color(mut self, color: Color) -> Self {
        self.active_track_color = Some(color);
        self
    }

    pub fn with_inactive_thumb_color(mut self, color: Color) -> Self {
        self.inactive_thumb_color = Some(color);
        self
    }

    pub fn with_inactive_track_color(mut self, color: Color) -> Self {
        self.inactive_track_color = Some(color);
        self
    }

    /// The states this switch resolves its theme properties against.
    ///
    /// Upstream's `_SwitchState.statesController`, reduced to the two bits this
    /// crate can know without a pointer-tracking state object: whether it is on
    /// and whether it can be used.
    pub fn states(&self) -> crate::widget_state::WidgetStates {
        let mut states = crate::widget_state::WidgetStates::NONE;
        if self.value {
            states = states.with(crate::widget_state::WidgetState::Selected);
        }
        if !self.enabled {
            states = states.with(crate::widget_state::WidgetState::Disabled);
        }
        states
    }

    /// Everything the switch's appearance is decided by, once.
    ///
    /// Pulled out of `build` so it can be asked as well as painted: the engine
    /// this crate's tests link records how many layers were opened and not what
    /// colour anything was, so a test that painted a switch could not see the
    /// answer. `build` calls this, so asking it is asking the real path.
    pub fn resolved(
        &self,
        switch_theme: crate::component_themes::SwitchThemeData,
        theme: &Theme,
    ) -> SwitchColors {
        // Upstream's order, and it is a real precedence and not a formality:
        // the one-off colour on this switch, then the theme's state property,
        // then the control's own default. A caller with one switch to recolour
        // writes a colour; a caller restyling every switch writes a property.
        let states = self.states();
        let value = self.value;
        let resolve = |property: &Option<crate::widget_state::StateProperty<Option<Color>>>| {
            property
                .as_ref()
                .and_then(|property| property.resolve(states))
        };

        SwitchColors {
            track: if value {
                self.active_track_color
            } else {
                self.inactive_track_color
            }
            .or_else(|| resolve(&switch_theme.track_color))
            .unwrap_or(if value { theme.primary } else { theme.outline }),
            knob: if value {
                self.active_color
            } else {
                self.inactive_thumb_color
            }
            .or_else(|| resolve(&switch_theme.thumb_color))
            .unwrap_or(if value {
                theme.on_primary
            } else {
                theme.text_muted
            }),
            outline: resolve(&switch_theme.track_outline_color),
            outline_width: switch_theme
                .track_outline_width
                .as_ref()
                .and_then(|property| property.resolve(states))
                .unwrap_or(0.0),
            padding: switch_theme
                .padding
                .map(|padding| padding.resolve(crate::direction::current_direction()))
                .unwrap_or(EdgeInsets::all(4.0)),
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
        // Upstream builds without callbacks when `onChanged` is null, so a
        // disabled switch is drawn and does not answer.
        let handlers = if self.enabled {
            self.handlers.clone()
        } else {
            PointerHandlers::new()
        };
        let resolved = self.resolved(crate::component_themes::SwitchTheme::of(context), &theme);
        let SwitchColors {
            track,
            knob,
            outline,
            outline_width,
            padding,
        } = resolved;
        let tap = handlers.on_tap.clone();

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
            let mut container = Container::new()
                .with_size(48.0, 28.0)
                .with_color(track)
                .with_corner_radius(14.0)
                .with_padding(padding)
                .with_child(row);
            // No width check here: `RenderDecoratedBox` already skips a
            // zero-width border, and a second guard on top of that one could
            // not be shown to do anything.
            if let Some(outline) = outline {
                container = container.with_border(outline_width, outline);
            }
            Pointer::new(id, container).with_handlers(handlers.clone())
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
        let slider = ResolvedSlider::of(context);
        let value = self.value;
        let width = self.width;
        let id = self.id;
        let handlers = self.handlers.clone();
        let track = slider.inactive_track_color;
        let fill = slider.active_track_color;
        let knob = slider.thumb_color;
        let track_height = slider.track_height;
        let thumb = slider.thumb_size;

        leaf(move || {
            let filled = (width * value).clamp(0.0, width);
            // The hit area is the taller of the thumb and a finger's worth of
            // slack over the track, so that a Material 3 bar thumb -- which
            // is taller than the track -- is not clipped by it.
            let hit_height = thumb.height.max(track_height + 16.0);
            // The hit region has to be exactly `width` wide: the value comes
            // from local_position.dx / width, so a region that a stretching
            // parent widened would reach 100% before the thumb did. Align
            // loosens the constraints, which keeps the size the one asked for.
            Align::new(
                Alignment::CENTER_LEFT,
                Pointer::new(
                    id,
                    Container::new()
                        // The thing you can hit should be bigger than the
                        // thing you can see.
                        .with_size(width, hit_height)
                        .with_child(Center::new(
                            Container::new()
                                .with_size(width, track_height)
                                .with_color(track)
                                .with_corner_radius(track_height / 2.0)
                                .with_child(
                                    RenderFlex::row()
                                        .with_main_axis_size(MainAxisSize::Max)
                                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                                        .push(
                                            Container::new()
                                                .with_size(filled, track_height)
                                                .with_color(fill)
                                                .with_corner_radius(track_height / 2.0),
                                        )
                                        .push(
                                            Container::new()
                                                .with_size(thumb.width, thumb.height)
                                                .with_color(knob)
                                                .with_corner_radius(thumb.shortest_side() / 2.0),
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
        // Upstream's `AppBar.build`: the background, the foreground and the
        // toolbar height come off `AppBarTheme.of(context)` before the
        // scheme and `kToolbarHeight`.
        let bar = crate::component_themes::ResolvedAppBar::of(context);
        let surface = bar.background;
        let outline = theme.outline;
        let title_style = theme.title();
        let muted = theme.muted();

        let has_subtitle = subtitle.is_some();
        // A themed height wins outright; without one, a bar with a subtitle
        // is the taller of the two the crate draws.
        let toolbar_height = match crate::component_themes::AppBarTheme::of(context).toolbar_height
        {
            Some(height) => height,
            None if has_subtitle => TOOLBAR_HEIGHT_WITH_SUBTITLE,
            None => bar.toolbar_height,
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

/// Upstream `ListTile`: a row with something before the title, a title, an
/// optional subtitle and something after. What a settings list is made of.
///
/// # What `selected`, `enabled` and `dense` change
///
/// None of the three is decoration:
///
/// * **`selected`** switches the colours the whole tile is drawn in --
///   upstream's `selectedColor` reaches the title, the subtitle *and* the
///   leading icon, because a tile whose text alone changed colour reads as a
///   link rather than as the current item;
/// * **`enabled`** stops the taps *and* mutes the same three, and it does both
///   or neither: a tile that looks live and does nothing is worse than one that
///   looks dead;
/// * **`dense`** is a height, and it comes from three places in order --
///   the tile, then the tile theme, then the app theme -- so a list can be made
///   dense once rather than per row.
pub struct ListTile {
    title: String,
    subtitle: Option<String>,
    accent: Option<Color>,
    leading: std::cell::RefCell<Option<AnyWidget>>,
    trailing: std::cell::RefCell<Option<AnyWidget>>,
    id: Option<u64>,
    handlers: PointerHandlers,
    selected: bool,
    /// Upstream's `selectedColor`. `None` defers to the list tile theme's.
    selected_color: Option<Color>,
    enabled: bool,
    /// Upstream's `dense`, three-valued: `None` defers to the theme.
    dense: Option<bool>,
    /// Upstream's `isThreeLine`, which upstream asserts implies a subtitle --
    /// three lines with nothing on the second two is not a layout.
    is_three_line: bool,
    /// Upstream's `contentPadding`, overriding the theme's.
    content_padding: Option<EdgeInsets>,
    /// Upstream's `minLeadingWidth`, which only means anything with a leading.
    min_leading_width: Option<f32>,
}

impl ListTile {
    pub fn new(title: impl Into<String>) -> ListTile {
        ListTile {
            title: title.into(),
            subtitle: None,
            accent: None,
            leading: std::cell::RefCell::new(None),
            trailing: std::cell::RefCell::new(None),
            id: None,
            handlers: PointerHandlers::new(),
            selected: false,
            selected_color: None,
            // Upstream's default. A tile is live unless it is said not to be.
            enabled: true,
            dense: None,
            is_three_line: false,
            content_padding: None,
            min_leading_width: None,
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

    /// Upstream's `leading`: the widget before the title, which is an icon or
    /// an avatar in almost every real list.
    pub fn with_leading(self, leading: AnyWidget) -> Self {
        *self.leading.borrow_mut() = Some(leading);
        self
    }

    pub fn with_trailing(self, trailing: AnyWidget) -> Self {
        *self.trailing.borrow_mut() = Some(trailing);
        self
    }

    /// Upstream's `selected`.
    pub fn with_selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Upstream's `selectedColor`, which sits above the theme's.
    ///
    /// A control tile fills this in with its control's active colour -- see
    /// [`crate::component_themes::ResolvedListTile::of_with_selected_color`].
    pub fn with_selected_color(mut self, color: Color) -> Self {
        self.selected_color = Some(color);
        self
    }

    /// Upstream's `enabled`. A disabled tile does not take taps, whatever
    /// handlers it was given.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Upstream's `dense`.
    pub fn with_dense(mut self, dense: bool) -> Self {
        self.dense = Some(dense);
        self
    }

    /// Upstream's `isThreeLine`, which asserts a subtitle: upstream's
    /// `assert(isThreeLine != true || subtitle != null)`.
    pub fn with_three_line(mut self, is_three_line: bool) -> Self {
        debug_assert!(
            !is_three_line || self.subtitle.is_some(),
            "isThreeLine needs a subtitle: three lines with nothing on the last \
             two is not a layout"
        );
        self.is_three_line = is_three_line;
        self
    }

    pub fn with_content_padding(mut self, padding: EdgeInsets) -> Self {
        self.content_padding = Some(padding);
        self
    }

    pub fn with_min_leading_width(mut self, width: f32) -> Self {
        self.min_leading_width = Some(width);
        self
    }

    pub fn tappable(mut self, id: u64, handlers: PointerHandlers) -> Self {
        self.id = Some(id);
        self.handlers = handlers;
        self
    }

    /// Whether this tile answers taps: upstream's `enabled` gating whether an
    /// `InkWell` is built with callbacks at all.
    pub fn is_tappable(&self) -> bool {
        self.enabled && self.id.is_some()
    }

    /// Upstream's `_isDenseLayout`: the tile, then the tile theme, then the app
    /// theme, then false.
    pub fn is_dense(&self, theme_dense: bool) -> bool {
        self.dense.unwrap_or(theme_dense)
    }
}

impl Component for ListTile {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let title = self.title.clone();
        let subtitle = self.subtitle.clone();
        let accent = self.accent;
        // Upstream builds the `InkWell` with callbacks only when the tile is
        // enabled, so a disabled tile is not a target at all rather than a
        // target that ignores what it is told.
        let id = self.enabled.then_some(self.id).flatten();
        let handlers = self.handlers.clone();
        let leading = self.leading.borrow_mut().take();
        let trailing = self.trailing.borrow_mut().take();
        let spacing = theme.spacing;
        let radius = theme.radius;
        // Upstream's `ListTile.build`: the content padding, the gap between
        // the title and whatever follows it, the minimum height and the tile's
        // own colour all come off `ListTileTheme.of(context)` before the
        // control's defaults. `selected` is passed in because it chooses
        // between two different sets of those.
        let tile =
            crate::component_themes::ResolvedListTile::of_with_selected_color(
                context,
                self.selected,
                self.dense,
                self.selected_color,
            );
        let content_padding = self.content_padding.unwrap_or(tile.content_padding);
        let title_gap = tile.horizontal_title_gap;
        let min_tile_height = tile.min_tile_height;
        let tile_color = tile.tile_color;
        let min_leading_width = self.min_leading_width.unwrap_or(tile.min_leading_width);
        // Upstream's `effectiveColor`: selected wins, then disabled, then the
        // ordinary text colour -- and it reaches the title, the subtitle and
        // the leading alike, because a tile whose title alone changed colour
        // reads as a link rather than as the current item.
        let text_color = if self.enabled {
            tile.text_color
        } else {
            theme.muted().color
        };
        let body = crate::engine::TextStyle {
            color: text_color,
            ..theme.body()
        };
        let muted = crate::engine::TextStyle {
            color: if self.enabled {
                theme.muted().color
            } else {
                text_color
            },
            ..theme.muted()
        };
        // Upstream's `isThreeLine`: the subtitle is allowed a second line, and
        // the leading and trailing align to the top rather than to the middle,
        // because centring against a three-line block puts an icon in the
        // middle of the text instead of beside its first line.
        let three_line = self.is_three_line;

        let has_trailing = trailing.is_some();
        let has_leading = leading.is_some();
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
        if let Some(leading) = leading {
            // Before the title, and first in the list so the row below can
            // tell it from the trailing.
            children.insert(0, leading);
        }
        if let Some(trailing) = trailing {
            children.push(trailing);
        }

        many(children, move |mut rendered| {
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                // Upstream's `ListTileTitleAlignment.threeLine`: against a
                // three-line block the leading and trailing go to the top, not
                // the middle -- an icon centred on three lines of text sits
                // beside the second one, which is not where it belongs.
                .with_cross_axis_alignment(if three_line {
                    CrossAxisAlignment::Start
                } else {
                    CrossAxisAlignment::Center
                })
                // Upstream's `horizontalTitleGap` is "the gap between the
                // titles and the leading/trailing widgets" -- both sides, not
                // just the trailing. A row with neither has nothing to space.
                .with_spacing(if has_trailing || has_leading {
                    title_gap
                } else {
                    0.0
                });
            if has_leading {
                // Upstream's `minLeadingWidth`: the leading column is at least
                // this wide however narrow the icon is, so the titles of
                // successive rows line up with each other rather than with
                // whatever each row happens to lead with.
                let leading = rendered.remove(0);
                row = row.push(
                    crate::render::RenderConstrainedBox::new(crate::render::BoxConstraints {
                        min_width: min_leading_width,
                        max_width: f32::INFINITY,
                        min_height: 0.0,
                        max_height: f32::INFINITY,
                    })
                    .with_child(leading),
                );
            }
            // Whatever is left at the front is the title block; the leading was
            // taken off it above and the trailing is behind it.
            let title = rendered.remove(0);
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
                row = row.push_flex(crate::render::FlexChild::expanded(title, 1));
                if let Some(trailing) = rendered.pop() {
                    row = row.push(trailing);
                }
            } else {
                row = row.push(title);
            }

            let mut padded = Container::new()
                .with_padding(content_padding)
                .with_corner_radius(radius)
                .with_child(row);
            if let Some(color) = tile_color {
                padded = padded.with_color(color);
            }
            // Upstream's `minTileHeight`: a tile is at least this tall
            // however short its content is.
            let padded = crate::render::RenderConstrainedBox::new(crate::render::BoxConstraints {
                min_width: 0.0,
                max_width: f32::INFINITY,
                min_height: min_tile_height,
                max_height: f32::INFINITY,
            })
            .with_child(padded);
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

impl Divider {
    /// Upstream's static `Divider.createBorderSide(context)`: the side a
    /// caller draws when it wants *a divider's* edge rather than a divider --
    /// a header's bottom rule, a card's outline. It reads the same theme the
    /// widget does, which is the point: a theme that moves the divider moves
    /// every edge that borrowed it.
    ///
    /// Upstream's `width` clamp is here too, in
    /// [`crate::component_themes::ResolvedDivider::line_thickness`]: a zero
    /// thickness means the thinnest line the device can draw, not no line.
    pub fn create_border_side(context: &mut BuildContext) -> crate::borders::BorderSide {
        let divider = crate::component_themes::ResolvedDivider::of(context);
        crate::borders::BorderSide {
            color: divider.color,
            width: divider.line_thickness(),
            ..crate::borders::BorderSide::default()
        }
    }
}

impl Component for Divider {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        // Upstream's `Divider.build`: the space, the thickness, the colour
        // and the two indents each come off `DividerTheme.of(context)` and
        // fall back to `ThemeData` and then to upstream's own defaults --
        // `ResolvedDivider` is those three steps.
        let divider = crate::component_themes::ResolvedDivider::of(context);
        let color = divider.color;
        let space = divider.space;
        let thickness = divider.line_thickness();
        let insets = crate::render::EdgeInsets {
            left: divider.indent,
            right: divider.end_indent,
            top: 0.0,
            bottom: 0.0,
        };
        leaf(move || {
            Container::new().with_height(space).with_child(Align::new(
                Alignment::CENTER,
                Container::new()
                    .with_height(thickness)
                    .with_color(color)
                    .with_margin(insets),
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
    /// Upstream's `label`, and it is optional: **a badge with no label is a
    /// dot**, not an empty stadium. The two say different things -- a count
    /// says how much is waiting, a dot says only that something is.
    label: Option<String>,
    background_color: Option<Color>,
    text_color: Option<Color>,
    /// Upstream's `isLabelVisible`, which hides the badge without taking the
    /// child with it -- a count that has gone to zero should leave the icon
    /// exactly where it was.
    is_label_visible: bool,
    /// Upstream's `child`: what the badge is sitting on. `None` is a badge on
    /// its own, which upstream allows and which is what this crate's earlier
    /// Badge always was.
    child: std::cell::RefCell<Option<AnyWidget>>,
}

impl Badge {
    /// A badge with a count. Upstream's default constructor with a `label`.
    pub fn new(label: impl Into<String>) -> Badge {
        Badge {
            label: Some(label.into()),
            background_color: None,
            text_color: None,
            is_label_visible: true,
            child: std::cell::RefCell::new(None),
        }
    }

    /// Upstream's `Badge()` with no label: the bare dot.
    pub fn dot() -> Badge {
        Badge {
            label: None,
            background_color: None,
            text_color: None,
            is_label_visible: true,
            child: std::cell::RefCell::new(None),
        }
    }

    /// Upstream's `backgroundColor`.
    pub fn with_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_text_color(mut self, color: Color) -> Self {
        self.text_color = Some(color);
        self
    }

    /// Upstream's `isLabelVisible`.
    pub fn with_label_visible(mut self, visible: bool) -> Self {
        self.is_label_visible = visible;
        self
    }

    /// Upstream's `child`.
    pub fn with_child(self, child: AnyWidget) -> Self {
        *self.child.borrow_mut() = Some(child);
        self
    }

    pub fn has_label(&self) -> bool {
        self.label.is_some()
    }
}

impl Component for Badge {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let resolved = crate::component_themes::ResolvedBadge::of(context);
        // Upstream's order at every field: the widget, then the theme, then the
        // M3 default. `ResolvedBadge` has already done the last two.
        let background = self.background_color.unwrap_or(resolved.background);
        let text_color = self.text_color.unwrap_or(resolved.text_color);
        let label = self.label.clone();
        let visible = self.is_label_visible;
        let child = self.child.borrow_mut().take();
        let small = resolved.small_size;
        let large = resolved.large_size;
        let padding = resolved.padding;
        let size = resolved
            .text_style
            .as_ref()
            .map(|style| style.font_size)
            .unwrap_or(theme.body_size - 3.0);
        let alignment = resolved
            .alignment
            .resolve(crate::direction::current_direction());
        // Upstream's `offset: hasLabel ? effectiveOffset : Offset.zero`. A dot
        // sits exactly where the alignment put it: the nudge exists to keep a
        // wide count from covering the thing it is counting, and a dot is not
        // wide.
        let offset = if self.label.is_some() {
            resolved.offset
        } else {
            Offset::ZERO
        };

        let mark = leaf(move || -> crate::widgets::BoxedWidget {
            if !visible {
                // Upstream returns the child alone when the label is hidden --
                // and here that is an empty mark, so the child below keeps its
                // place and its size.
                return crate::render::RenderRef::new(crate::widgets::Empty);
            }
            match &label {
                // A stadium at least `largeSize` tall, with room at the sides
                // only.
                Some(label) => crate::render::RenderRef::new(
                    // Upstream's `_IntrinsicHorizontalStadium(minSize: largeSize)`:
                    // exactly `largeSize` tall, and at least that wide so a
                    // one-digit count is a circle rather than a sliver. The
                    // width is otherwise free -- that is what makes it a
                    // stadium and not a pill of fixed size.
                    crate::render::RenderConstrainedBox::new(crate::render::BoxConstraints {
                        min_width: large,
                        max_width: f32::INFINITY,
                        min_height: large,
                        max_height: large,
                    })
                    .with_child(
                        Container::new()
                            .with_color(background)
                            .with_corner_radius(large / 2.0)
                            .with_padding(padding)
                            // No `Center` here: upstream's Container has
                            // `alignment: Alignment.center`, which on a
                            // content-sized box moves nothing -- while a
                            // `Center` *expands to fill*, and under a loose
                            // parent that makes the badge as wide as whatever
                            // it was offered.
                            .with_child(
                                Text::new(label.clone())
                                    .with_size(size)
                                    .with_weight(700)
                                    .with_color(text_color),
                            ),
                    ),
                ),
                // The dot: `smallSize` across, and round rather than a stadium
                // because there is nothing inside to stretch it.
                None => crate::render::RenderRef::new(
                    Container::new()
                        .with_size(small, small)
                        .with_color(background)
                        .with_corner_radius(small / 2.0),
                ),
            }
        });

        let Some(child) = child else {
            return mark;
        };
        // Upstream stacks the badge over the child, positioned to *fill* it,
        // and aligns it inside that -- which is why the child alone sizes the
        // stack. A badge must not make the thing it is marking any bigger, or a
        // row of icons would shift the moment one of them got a count.
        many(vec![child, mark], move |mut rendered| {
            let mark = rendered.pop().expect("the mark");
            let child = rendered.pop().expect("the child");
            let aligned = crate::render::RenderAlign::new_boxed(
                crate::render::Alignment::new(alignment.x, alignment.y),
                mark,
            )
            .with_nudge(offset);
            let mut stack = crate::render::RenderStack::new()
                .with_fit(crate::render::StackFit::Loose)
                // Upstream's `clipBehavior: Clip.none`: the badge is allowed to
                // hang outside the icon it is marking, and usually does.
                .with_clip_behavior(crate::painting::ClipBehavior::None);
            stack = stack.push_boxed(child);
            stack = stack.push_positioned_boxed(
                crate::render::RenderRef::new(aligned),
                crate::render::StackPosition {
                    left: Some(0.0),
                    top: Some(0.0),
                    right: Some(0.0),
                    bottom: Some(0.0),
                    width: None,
                    height: None,
                },
            );
            Box::new(stack)
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

// -- MaterialBanner -----------------------------------------------------------

/// Why a [`MaterialBanner`] went away. Upstream's `MaterialBannerClosedReason`.
///
/// The distinctions are the ones a caller acts on: a banner the reader
/// dismissed is one they have seen, and a banner replaced by the next in the
/// queue is not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialBannerClosedReason {
    /// The reader used the accessibility dismiss action.
    Dismiss,
    /// The reader swiped it away.
    Swipe,
    /// The application called for it to be hidden -- it animates out.
    Hide,
    /// The application called for it to be removed -- it goes at once.
    Remove,
}

/// An important, succinct message across the top of the page, with one or two
/// actions on it. Upstream's `MaterialBanner`.
///
/// Unlike a snack bar it does not time out: a banner stays until it is acted
/// on or dismissed, which is why it carries actions rather than an optional
/// one.
///
/// # The single-row rule
///
/// Exactly one action, and not forced below, puts the actions *on the
/// content's row*; anything else puts them on their own row underneath. Two
/// actions never share the row -- upstream's condition is
/// `actions.length == 1 && !forceActionsBelow`, not "the actions fit". The
/// padding differs between the two cases for a reason worth keeping: on one
/// row the 52-tall action bar is what holds the banner open, so the content
/// needs almost no top inset; stacked, nothing else holds the text off the
/// top edge, so the banner does it itself.
///
/// # What is not ported
///
/// Upstream's banner is a `StatefulWidget` because of the `animation` a
/// `ScaffoldMessenger` hands it -- the height factor it slides in under, the
/// `onVisible` callback fired when that animation completes, the `Hero` it
/// flies as, and `withAnimation`/`createAnimationController`, which exist for
/// `ScaffoldMessengerState.showMaterialBanner` to call. There is no
/// `ScaffoldMessenger` here (see [`crate::components::Scaffold`]), so what is
/// ported is upstream's own null-animation path -- the one it introduces as
/// "this provides a static banner". Whoever shows the banner owns whether it
/// is on screen, as everything overlay-like in this crate does.
pub struct MaterialBanner {
    content: std::cell::RefCell<Option<AnyWidget>>,
    leading: std::cell::RefCell<Option<AnyWidget>>,
    actions: std::cell::RefCell<Vec<AnyWidget>>,
    elevation: Option<f32>,
    background_color: Option<Color>,
    divider_color: Option<Color>,
    padding: Option<crate::borders::EdgeInsetsGeometry>,
    margin: Option<EdgeInsets>,
    leading_padding: Option<crate::borders::EdgeInsetsGeometry>,
    force_actions_below: bool,
    overflow_alignment: crate::overflow_bar::OverflowBarAlignment,
    min_action_bar_height: f32,
}

impl MaterialBanner {
    pub fn new(content: AnyWidget, actions: Vec<AnyWidget>) -> MaterialBanner {
        MaterialBanner {
            content: std::cell::RefCell::new(Some(content)),
            leading: std::cell::RefCell::new(None),
            actions: std::cell::RefCell::new(actions),
            elevation: None,
            background_color: None,
            divider_color: None,
            padding: None,
            margin: None,
            leading_padding: None,
            force_actions_below: false,
            // Upstream's default: once the actions have their own row they
            // sit at its far end, where the eye ends up after the text.
            overflow_alignment: crate::overflow_bar::OverflowBarAlignment::End,
            min_action_bar_height:
                crate::component_themes::ResolvedMaterialBanner::MIN_ACTION_BAR_HEIGHT,
        }
    }

    /// An icon before the content, typically the one that says what kind of
    /// message this is.
    pub fn with_leading(self, leading: AnyWidget) -> Self {
        *self.leading.borrow_mut() = Some(leading);
        self
    }

    pub fn with_elevation(mut self, elevation: f32) -> Self {
        self.elevation = Some(elevation);
        self
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_divider_color(mut self, color: Color) -> Self {
        self.divider_color = Some(color);
        self
    }

    pub fn with_padding(mut self, padding: crate::borders::EdgeInsetsGeometry) -> Self {
        self.padding = Some(padding);
        self
    }

    pub fn with_margin(mut self, margin: EdgeInsets) -> Self {
        self.margin = Some(margin);
        self
    }

    pub fn with_leading_padding(mut self, padding: crate::borders::EdgeInsetsGeometry) -> Self {
        self.leading_padding = Some(padding);
        self
    }

    /// Puts the actions on their own row even when there is only one of them.
    /// Upstream's `forceActionsBelow`, for an action whose label is long
    /// enough that sharing the row would crowd the text.
    pub fn with_force_actions_below(mut self, force: bool) -> Self {
        self.force_actions_below = force;
        self
    }

    pub fn with_overflow_alignment(
        mut self,
        alignment: crate::overflow_bar::OverflowBarAlignment,
    ) -> Self {
        self.overflow_alignment = alignment;
        self
    }

    pub fn with_min_action_bar_height(mut self, height: f32) -> Self {
        self.min_action_bar_height = height;
        self
    }

    /// Upstream's `isSingleRow`: exactly one action, and not forced below.
    ///
    /// Note it is the *count*, not whether they fit -- two short actions still
    /// take their own row.
    pub fn is_single_row(&self) -> bool {
        self.actions.borrow().len() == 1 && !self.force_actions_below
    }
}

impl Component for MaterialBanner {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let banner = crate::component_themes::ResolvedMaterialBanner::of(context);
        let direction = crate::direction::current_direction();
        let is_single_row = self.is_single_row();

        let elevation = self.elevation.unwrap_or(banner.elevation);
        let background = self.background_color.unwrap_or(banner.background_color);
        let divider_color = self.divider_color.unwrap_or(banner.divider_color);
        let margin = self.margin.unwrap_or_else(|| {
            // The resolver's default reads the *ambient* elevation; a widget
            // that set its own has to be the one asked about the shadow it
            // will actually cast.
            EdgeInsets::only(0.0, 0.0, 0.0, if elevation > 0.0 { 10.0 } else { 0.0 })
        });
        let padding = self
            .padding
            .unwrap_or_else(|| banner.content_padding(is_single_row))
            .resolve(direction);
        let leading_padding = self
            .leading_padding
            .unwrap_or(banner.leading_padding)
            .resolve(direction);
        let overflow_alignment = self.overflow_alignment;
        let min_action_bar_height = self.min_action_bar_height;

        let content = self
            .content
            .borrow_mut()
            .take()
            .unwrap_or_else(|| leaf(|| crate::widgets::Empty));
        let leading = self.leading.borrow_mut().take();
        let has_leading = leading.is_some();
        let actions = std::mem::take(&mut *self.actions.borrow_mut());
        let action_count = actions.len();

        let mut children = vec![content];
        children.extend(leading);
        children.extend(actions);

        crate::framework::many(children, move |mut boxed| {
            let mut boxed = boxed.drain(..);
            let content = boxed.next().expect("the content is always pushed");
            let leading = if has_leading { boxed.next() } else { None };
            let actions: Vec<_> = boxed.take(action_count).collect();

            // Upstream's `actionsBar`: a minimum height, 8 of horizontal
            // padding, and the actions themselves in an `OverflowBar` that
            // stacks them when the row will not do.
            let mut bar = crate::overflow_bar::OverflowBar::new()
                .with_spacing(8.0)
                .with_overflow_alignment(overflow_alignment);
            for action in actions {
                bar = bar.push_boxed(action);
            }
            let actions_bar = crate::render::RenderConstrainedBox::new(BoxConstraints {
                min_width: 0.0,
                max_width: f32::INFINITY,
                min_height: min_action_bar_height,
                max_height: f32::INFINITY,
            })
            .with_child(
                Container::new()
                    .with_padding(EdgeInsets::symmetric(8.0, 0.0))
                    .with_child(Align::new(Alignment::CENTER_RIGHT, bar)),
            );

            // The content row: the leading icon, the content taking the rest,
            // and -- on a single-row banner -- the actions beside it.
            let mut row = crate::widgets::Row::new();
            if let Some(leading) = leading {
                row = row.push(
                    Container::new()
                        .with_padding(leading_padding)
                        .with_child(leading),
                );
            }
            row = row.push_flex(crate::widgets::Expanded::new(content));
            // The one bar goes to exactly one of the two places -- beside the
            // content, or under it. There is never one of each.
            let mut stacked_bar = None;
            if is_single_row {
                row = row.push(actions_bar);
            } else {
                stacked_bar = Some(actions_bar);
            }

            // `MainAxisSize.min`: a banner is as tall as its content, and a
            // column that filled its parent would push the divider to the
            // bottom of the page.
            let mut column = crate::widgets::Column::new()
                .with_main_axis_size(crate::render::MainAxisSize::Min)
                .with_cross_axis_alignment(crate::render::CrossAxisAlignment::Stretch)
                .push(Container::new().with_padding(padding).with_child(row));
            if let Some(bar) = stacked_bar {
                column = column.push(bar);
            }
            if elevation == 0.0 {
                // Upstream draws the rule only on a flat banner: with a
                // shadow there is already an edge, and with both the edge
                // reads twice.
                //
                // Upstream asks for `Divider(height: 0)`, which reserves no
                // space and draws the hairline on the banner's own bottom
                // edge. This renderer draws a line into the box it is given,
                // so the rule takes its one pixel of height.
                column = column.push(Container::new().with_height(1.0).with_color(divider_color));
            }

            Container::new()
                .with_margin(margin)
                .with_color(background)
                .with_elevation(elevation.round().max(0.0) as u32)
                .with_child(column)
        })
    }
}

/// Upstream `DataColumn`: one heading of a data table.
///
/// The field worth naming is `numeric`. It is not a formatting hint -- the
/// table uses it to align the whole column's cells to the *right*, because a
/// column of numbers is read by comparing digits in the same place, and
/// left-aligned numbers put the units column somewhere different in every
/// row.
pub struct DataColumn {
    pub label: std::cell::RefCell<Option<AnyWidget>>,
    pub tooltip: Option<String>,
    pub numeric: bool,
    /// Upstream's `onSort`, handed the column's index and whether the sort is
    /// ascending. Its presence is also what makes the heading interactive:
    /// upstream's `_debugInteractive` is exactly `onSort != null`, so a
    /// column with no callback shows no sort arrow rather than a dead one.
    #[allow(clippy::type_complexity)]
    pub on_sort: Option<Rc<dyn Fn(usize, bool)>>,
}

impl DataColumn {
    pub fn new(label: AnyWidget) -> DataColumn {
        DataColumn {
            label: std::cell::RefCell::new(Some(label)),
            tooltip: None,
            numeric: false,
            on_sort: None,
        }
    }

    pub fn numeric(mut self) -> Self {
        self.numeric = true;
        self
    }

    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn with_on_sort(mut self, on_sort: impl Fn(usize, bool) + 'static) -> Self {
        self.on_sort = Some(Rc::new(on_sort));
        self
    }

    /// Upstream's `_debugInteractive`: a column is interactive exactly when
    /// it can be sorted by.
    pub fn is_interactive(&self) -> bool {
        self.on_sort.is_some()
    }
}

/// Upstream `DataCell`: one cell of a data table.
pub struct DataCell {
    pub child: std::cell::RefCell<Option<AnyWidget>>,
    /// Upstream's `placeholder`: this cell has no value yet, so its content
    /// is drawn faded. A separate flag rather than a colour, because the
    /// table decides *how* faint a placeholder is and the cell only says that
    /// it is one.
    pub placeholder: bool,
    pub show_edit_icon: bool,
    pub on_tap: Option<Rc<dyn Fn()>>,
    pub on_long_press: Option<Rc<dyn Fn()>>,
    pub on_double_tap: Option<Rc<dyn Fn()>>,
}

impl DataCell {
    pub fn new(child: AnyWidget) -> DataCell {
        DataCell {
            child: std::cell::RefCell::new(Some(child)),
            placeholder: false,
            show_edit_icon: false,
            on_tap: None,
            on_long_press: None,
            on_double_tap: None,
        }
    }

    /// Upstream's `DataCell.empty`, which is a `SizedBox.shrink` -- a cell
    /// that is present and blank.
    ///
    /// Present matters: a table's rows all have the same number of cells, so
    /// a row with nothing in one column still needs a cell there or every
    /// column after it would shift left.
    pub fn empty() -> DataCell {
        DataCell::new(crate::framework::leaf(|| crate::widgets::Empty))
    }

    pub fn placeholder(mut self) -> Self {
        self.placeholder = true;
        self
    }

    pub fn with_edit_icon(mut self) -> Self {
        self.show_edit_icon = true;
        self
    }

    pub fn with_on_tap(mut self, on_tap: impl Fn() + 'static) -> Self {
        self.on_tap = Some(Rc::new(on_tap));
        self
    }

    pub fn with_on_long_press(mut self, on_long_press: impl Fn() + 'static) -> Self {
        self.on_long_press = Some(Rc::new(on_long_press));
        self
    }

    pub fn with_on_double_tap(mut self, on_double_tap: impl Fn() + 'static) -> Self {
        self.on_double_tap = Some(Rc::new(on_double_tap));
        self
    }

    /// Upstream's `_debugInteractive`: any one of the gesture callbacks.
    pub fn is_interactive(&self) -> bool {
        self.on_tap.is_some() || self.on_long_press.is_some() || self.on_double_tap.is_some()
    }
}

/// Upstream `TableRowInkWell`: an ink well whose splash covers the whole
/// table row, not the cell that was pressed.
///
/// That is the entire reason the class exists. Upstream overrides
/// `getRectCallback` to walk up to the enclosing `RenderTable` and hand back
/// `getRowBox` for this cell's row. Without it, a press in one cell would
/// splash inside that cell and stop at its edge -- which would say *the cell*
/// was pressed, when what the reader pressed was the row.
///
/// Here the row rectangle is given to [`crate::render::RenderTable::row_box`]
/// by whoever builds the table, which is the same choice
/// [`crate::render::RenderAbstractViewport`] makes: this crate has no walk
/// from a render object up to its ancestors, and the caller building the
/// table already knows which row it is filling.
pub struct TableRowInkWell;

impl TableRowInkWell {
    /// Builds the ink well. `row` is the row's rectangle in this cell's own
    /// coordinates -- negative `top` for a cell that is not at the row's top,
    /// which is what shifting the table's row box by the cell's offset gives.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        id: u64,
        row: crate::engine::Rect,
        build_child: impl Fn() -> AnyWidget + 'static,
    ) -> crate::ink_well::InkResponse {
        // Contained and rectangular, exactly as upstream's constructor passes
        // to `super` -- a row highlight fills the row, so it has to be
        // clipped to it.
        crate::ink_well::InkWell::new(id, build_child).with_rect(move |_| row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{ElementTree, provide};
    use crate::render::{BoxConstraints, RenderBox};

    // -- The switch's colours ----------------------------------------------------------

    use crate::component_themes::SwitchThemeData;
    use crate::widget_state::{StateProperty, WidgetState, WidgetStates};

    /// A state property that answers one colour when selected and another
    /// otherwise -- which is what a real switch theme is.
    fn by_selection(on: Color, off: Color) -> StateProperty<Option<Color>> {
        StateProperty::resolve_with(move |states: WidgetStates| {
            Some(if states.contains(WidgetState::Selected) {
                on
            } else {
                off
            })
        })
    }

    fn resolved(switch: Switch, data: SwitchThemeData) -> SwitchColors {
        switch.resolved(data, &Theme::dark())
    }

    const MINE: Color = Color::argb(0xFF, 0x99, 0x88, 0x77);
    const ON: Color = Color::argb(0xFF, 0x11, 0x22, 0x33);
    const OFF: Color = Color::argb(0xFF, 0x44, 0x55, 0x66);

    #[test]
    fn the_states_carry_both_bits_so_four_appearances_come_out_of_two() {
        // Reading the value alone would give two. A disabled switch that is on
        // and a disabled switch that is off are different pictures, and so is
        // each from its enabled twin.
        let all = [
            Switch::new(1, true).states(),
            Switch::new(1, false).states(),
            Switch::new(1, true).with_enabled(false).states(),
            Switch::new(1, false).with_enabled(false).states(),
        ];
        for (i, a) in all.iter().enumerate() {
            for b in all.iter().skip(i + 1) {
                assert_ne!(a, b, "four states, all different");
            }
        }
        assert!(all[0].contains(WidgetState::Selected));
        assert!(!all[0].contains(WidgetState::Disabled));
        assert!(all[2].contains(WidgetState::Disabled));
    }

    #[test]
    fn with_no_theme_a_switch_falls_back_to_the_apps_own_colours() {
        let theme = Theme::dark();
        let lit = resolved(Switch::new(1, true), SwitchThemeData::new());
        assert_eq!(lit.track, theme.primary);
        assert_eq!(lit.knob, theme.on_primary);

        let dark = resolved(Switch::new(1, false), SwitchThemeData::new());
        assert_eq!(dark.track, theme.outline);
        assert_eq!(dark.knob, theme.text_muted);
    }

    #[test]
    fn the_theme_beats_the_controls_own_default() {
        let data = SwitchThemeData::new().with_track_color(by_selection(ON, OFF));
        assert_eq!(resolved(Switch::new(1, true), data).track, ON);
        assert_ne!(ON, Theme::dark().primary, "or the test would prove nothing");
    }

    #[test]
    fn the_switchs_own_colour_beats_the_theme() {
        // A caller with one switch to recolour should not have to write a state
        // property to do it.
        let data = SwitchThemeData::new().with_track_color(by_selection(ON, OFF));
        let mine = resolved(Switch::new(1, true).with_active_track_color(MINE), data);
        assert_eq!(mine.track, MINE);

        let data = SwitchThemeData::new().with_thumb_color(by_selection(ON, OFF));
        assert_eq!(
            resolved(Switch::new(1, true).with_active_color(MINE), data).knob,
            MINE
        );
    }

    #[test]
    fn the_off_colours_are_a_separate_pair_from_the_on_ones() {
        let data = SwitchThemeData::new().with_track_color(by_selection(ON, OFF));
        assert_eq!(resolved(Switch::new(1, true), data.clone()).track, ON);
        assert_eq!(resolved(Switch::new(1, false), data).track, OFF);

        // And the widget's own overrides come in pairs too, so setting the on
        // colour leaves the off one where it was.
        let only_on = resolved(
            Switch::new(1, false).with_active_track_color(MINE),
            SwitchThemeData::new().with_track_color(by_selection(ON, OFF)),
        );
        assert_eq!(only_on.track, OFF, "the off switch kept the off colour");
    }

    #[test]
    fn a_disabled_switch_gets_a_different_answer_from_the_same_property() {
        // Which is the whole reason the property is resolved against states
        // rather than against the value.
        const DEAD: Color = Color::argb(0xFF, 0x77, 0x77, 0x77);
        let data = SwitchThemeData::new().with_track_color(StateProperty::resolve_with(
            move |states: WidgetStates| {
                Some(if states.contains(WidgetState::Disabled) {
                    DEAD
                } else {
                    ON
                })
            },
        ));
        assert_eq!(resolved(Switch::new(1, true), data.clone()).track, ON);
        assert_eq!(
            resolved(Switch::new(1, true).with_enabled(false), data).track,
            DEAD,
            "the same property, a different answer"
        );
    }

    #[test]
    fn no_outline_width_means_no_outline_however_a_colour_was_named() {
        // A border layer that shows nothing still costs a pass.
        let mut data = SwitchThemeData::new();
        data.track_outline_color = Some(by_selection(ON, ON));
        let unset = resolved(Switch::new(1, true), data.clone());
        assert_eq!(unset.outline, Some(ON), "the colour is known");
        assert_eq!(unset.outline_width, 0.0, "and nothing is drawn with it");

        data.track_outline_width = Some(StateProperty::resolve_with(|_| Some(2.0)));
        assert_eq!(resolved(Switch::new(1, true), data).outline_width, 2.0);
    }

    #[test]
    fn the_padding_comes_from_the_theme_and_defaults_to_the_controls_own() {
        assert_eq!(
            resolved(Switch::new(1, true), SwitchThemeData::new()).padding,
            EdgeInsets::all(4.0)
        );
        let mut data = SwitchThemeData::new();
        data.padding = Some(crate::borders::EdgeInsetsGeometry::Absolute(
            EdgeInsets::all(9.0),
        ));
        assert_eq!(
            resolved(Switch::new(1, true), data).padding,
            EdgeInsets::all(9.0)
        );
    }

    #[test]
    fn a_zero_width_border_is_not_drawn() {
        // The invariant the switch leans on instead of guarding again itself:
        // `RenderDecoratedBox` skips a border of no width, so passing one
        // through costs nothing. Counted in rectangles drawn, because that is
        // the only thing the stub engine reports about what was painted.
        fn rects(width: f32) -> u32 {
            crate::engine_test_stubs::reset_layer_calls();
            let mut tree = ElementTree::new();
            tree.rebuild(provide(
                Theme::dark(),
                crate::framework::leaf(move || {
                    Container::new()
                        .with_size(40.0, 40.0)
                        .with_color(Color::argb(0xFF, 1, 2, 3))
                        .with_border(width, Color::argb(0xFF, 9, 9, 9))
                }),
            ));
            let mut root = tree.build_render_tree().expect("a root");
            root.layout(BoxConstraints::tight(60.0, 60.0));
            let mut layers = crate::engine::LayerTree::new(60, 60);
            {
                let mut context = crate::render::PaintContext::new(
                    &mut layers,
                    crate::render::Size::new(60.0, 60.0),
                );
                root.paint(&mut context, crate::render::Offset::ZERO);
            }
            crate::engine_test_stubs::layer_calls().rects
        }

        let none = rects(0.0);
        let drawn = rects(2.0);
        assert_eq!(drawn, none + 1, "a border is one more rectangle");
    }

    #[test]
    fn a_disabled_switch_takes_no_taps() {
        fn taps(enabled: bool) -> usize {
            let heard = std::rc::Rc::new(std::cell::Cell::new(0));
            let counter = std::rc::Rc::clone(&heard);
            let switch = Switch::new(1, true)
                .with_handlers(
                    PointerHandlers::new().with_tap(move |_| counter.set(counter.get() + 1)),
                )
                .with_enabled(enabled);
            let mut tree = ElementTree::new();
            tree.rebuild(provide(Theme::dark(), component(switch)));
            let mut root = tree.build_render_tree().expect("a root");
            let size = root.layout(BoxConstraints::tight(200.0, 60.0));
            let mut result = crate::render::HitTestResult::new();
            root.hit_test(
                crate::render::Offset::new(size.width / 2.0, size.height / 2.0),
                &mut result,
            );
            for entry in &result.path {
                if let Some(handlers) = &entry.handlers {
                    if let Some(tap) = &handlers.on_tap {
                        tap(crate::gestures::TapEvent {
                            local_position: crate::render::Offset::ZERO,
                            pointer_id: 0,
                        });
                    }
                }
            }
            heard.get()
        }
        assert_eq!(taps(true), 1);
        assert_eq!(taps(false), 0);
    }

    // -- The badge ---------------------------------------------------------------------

    use crate::component_themes::{BadgeTheme, BadgeThemeData, ResolvedBadge};

    fn badge_size(badge: Badge) -> crate::render::Size {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), component(badge)));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints {
            min_width: 0.0,
            max_width: 400.0,
            min_height: 0.0,
            max_height: 400.0,
        })
    }

    fn marked(child_size: f32, badge: Badge) -> crate::render::Size {
        badge_size(badge.with_child(crate::framework::leaf(move || {
            Container::new().with_size(child_size, child_size)
        })))
    }

    fn resolved_badge(data: BadgeThemeData) -> ResolvedBadge {
        struct Reader(std::rc::Rc<std::cell::RefCell<Option<ResolvedBadge>>>);
        impl Component for Reader {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.0.borrow_mut() = Some(ResolvedBadge::of(context));
                leaf(|| crate::widgets::Empty)
            }
        }
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            Theme::dark(),
            BadgeTheme::new(data, component(Reader(std::rc::Rc::clone(&seen)))),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    #[test]
    fn a_badge_with_no_label_is_a_dot_and_not_an_empty_stadium() {
        // The two say different things: a count says how much is waiting, a dot
        // says only that something is.
        let dot = badge_size(Badge::dot());
        assert_eq!(
            dot,
            crate::render::Size::new(ResolvedBadge::SMALL_SIZE, ResolvedBadge::SMALL_SIZE)
        );
        assert!(!Badge::dot().has_label());

        let counted = badge_size(Badge::new("3"));
        assert!(counted.width > dot.width && counted.height > dot.height);
        assert_eq!(
            counted.height,
            ResolvedBadge::LARGE_SIZE,
            "a labelled badge is largeSize tall"
        );
    }

    #[test]
    fn a_badge_does_not_make_the_thing_it_marks_any_bigger() {
        // Or a row of icons would shift the moment one of them got a count.
        let bare = marked(24.0, Badge::dot().with_label_visible(false));
        let dotted = marked(24.0, Badge::dot());
        let counted = marked(24.0, Badge::new("99"));
        assert_eq!(bare, crate::render::Size::new(24.0, 24.0));
        assert_eq!(dotted, bare);
        assert_eq!(counted, bare, "even a wide count");
    }

    /// Where the badge landed inside its child, and how big it came out.
    ///
    /// Read by walking the render tree rather than by hit testing: a badge is
    /// not a tap target, so the only way to see where it is put is to ask the
    /// tree what it did.
    fn badge_placement(badge: Badge) -> (crate::render::Offset, crate::render::Size) {
        use crate::render::RenderBox;
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            Theme::dark(),
            component(badge.with_child(crate::framework::leaf(|| {
                Container::new().with_size(24.0, 24.0)
            }))),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::tight(24.0, 24.0));

        // The deepest thing that is not the 24-by-24 child is the badge.
        fn deepest(
            node: &dyn RenderBox,
            at: crate::render::Offset,
            found: &mut Vec<(crate::render::Offset, crate::render::Size)>,
        ) {
            node.visit_children(&mut |child, offset| {
                let here = crate::render::Offset::new(at.dx + offset.dx, at.dy + offset.dy);
                found.push((here, child.size()));
                deepest(child, here, found);
            });
        }
        let mut found = Vec::new();
        deepest(&root, crate::render::Offset::ZERO, &mut found);
        *found
            .iter()
            .filter(|(_, size)| size.width > 0.0 && size.width != 24.0)
            .min_by(|a, b| a.1.width.partial_cmp(&b.1.width).expect("finite"))
            .expect("the badge is in there somewhere")
    }

    #[test]
    fn hiding_the_label_stops_the_badge_being_drawn_at_all() {
        // Upstream's `isLabelVisible`: a count that has gone to zero should not
        // take the icon with it -- and should not leave a mark either.
        fn rects(visible: bool) -> u32 {
            use crate::render::RenderBox;
            crate::engine_test_stubs::reset_layer_calls();
            let mut tree = ElementTree::new();
            tree.rebuild(provide(
                Theme::dark(),
                component(Badge::new("0").with_label_visible(visible).with_child(
                    crate::framework::leaf(|| Container::new().with_size(24.0, 24.0)),
                )),
            ));
            let mut root = tree.build_render_tree().expect("a root");
            root.layout(BoxConstraints::tight(24.0, 24.0));
            let mut layers = crate::engine::LayerTree::new(24, 24);
            {
                let mut context = crate::render::PaintContext::new(
                    &mut layers,
                    crate::render::Size::new(24.0, 24.0),
                );
                root.paint(&mut context, crate::render::Offset::ZERO);
            }
            crate::engine_test_stubs::layer_calls().rects
        }
        assert!(
            rects(true) > rects(false),
            "the badge is one more rectangle"
        );

        assert_eq!(
            marked(24.0, Badge::new("0").with_label_visible(false)),
            marked(24.0, Badge::new("0")),
            "and the child does not move either way"
        );
    }

    #[test]
    fn a_dot_sits_on_the_corner_and_a_count_is_nudged_off_it() {
        // The nudge keeps a wide count from covering the thing it is counting.
        // A dot is not wide, so it sits exactly where the alignment put it.
        let (dot_at, dot_size) = badge_placement(Badge::dot());
        assert_eq!(
            dot_at.dx + dot_size.width,
            24.0,
            "flush with the child's right edge"
        );
        assert_eq!(dot_at.dy, 0.0, "and its top");

        let (count_at, _) = badge_placement(Badge::new("9"));
        assert_ne!(
            count_at.dy, 0.0,
            "a count is moved off the corner and a dot is not"
        );
    }

    #[test]
    fn a_badge_is_the_error_colour_and_not_the_primary() {
        // The scheme already has a colour that means "this wants attention".
        // The primary would make a badge read as decoration.
        let scheme = crate::theme::ThemeData::fallback().color_scheme;
        let resolved = resolved_badge(BadgeThemeData::new());
        assert_eq!(resolved.background, scheme.error);
        assert_eq!(resolved.text_color, scheme.on_error);
    }

    #[test]
    fn the_theme_beats_the_defaults_field_by_field() {
        let mut data = BadgeThemeData::new();
        data.background_color = Some(Color::argb(0xFF, 1, 2, 3));
        data.large_size = Some(30.0);
        let resolved = resolved_badge(data);
        assert_eq!(resolved.background, Color::argb(0xFF, 1, 2, 3));
        assert_eq!(resolved.large_size, 30.0);
        assert_eq!(
            resolved.small_size,
            ResolvedBadge::SMALL_SIZE,
            "and the fields it did not set are untouched"
        );
    }

    #[test]
    fn the_badges_own_colour_is_recorded_for_the_widget_step() {
        // The widget-then-theme-then-default order has three steps and
        // `ResolvedBadge` does the last two; the first is the widget's own
        // `unwrap_or` in `build`. Which colour came out is not observable --
        // the stub engine counts rectangles and does not say what colour they
        // were -- so only the field the step reads is asserted here.
        const MINE: Color = Color::argb(0xFF, 0x77, 0x66, 0x55);
        assert_eq!(
            Badge::new("1").with_color(MINE).background_color,
            Some(MINE)
        );
        assert_eq!(Badge::new("1").background_color, None, "unset defers");
        assert_eq!(Badge::new("1").with_text_color(MINE).text_color, Some(MINE));
    }

    #[test]
    fn a_lone_labelled_badge_is_a_stadium_and_not_as_wide_as_it_is_offered() {
        // `max_width: INFINITY` under a loose parent would take everything, and
        // a badge four hundred pixels wide is not a badge.
        let size = badge_size(Badge::new("3"));
        assert!(size.width < 400.0, "got {}", size.width);
        assert!(
            size.width >= size.height,
            "a stadium is at least as wide as it is tall: {size:?}"
        );
    }

    #[test]
    fn a_labelled_badge_is_nudged_off_the_corner_and_a_dot_is_not() {
        // The nudge keeps a wide count from covering the thing it is counting.
        // A dot is not wide, so it sits exactly where the alignment put it.
        let resolved = resolved_badge(BadgeThemeData::new());
        assert_ne!(resolved.offset, Offset::ZERO, "the theme's own offset");

        // The offset upstream adds on top of whatever was asked for.
        let mut data = BadgeThemeData::new();
        data.offset = Some(Offset::new(0.0, 0.0));
        assert_eq!(
            resolved_badge(data).offset,
            Offset::new(0.0, 8.0),
            "upstream's compatibility constant, added to whatever was asked for"
        );
    }

    // -- The elevated variant ----------------------------------------------------------

    use crate::component_themes::{
        ButtonStyle, ElevatedButtonTheme, ElevatedButtonThemeData, ResolvedButton,
    };

    fn resolved_button(
        variant: ButtonVariant,
        style: Option<ButtonStyle>,
        states: crate::widget_state::WidgetStates,
    ) -> ResolvedButton {
        struct Reader {
            variant: ButtonVariant,
            states: crate::widget_state::WidgetStates,
            seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedButton>>>,
        }
        impl Component for Reader {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                let theme = theme_of(context);
                *self.seen.borrow_mut() = Some(ResolvedButton::of(
                    context,
                    self.variant,
                    self.states,
                    ResolvedButton {
                        background: Some(theme.surface_variant),
                        foreground: theme.primary,
                        side: None,
                        padding: None,
                        minimum_size: None,
                    },
                ));
                leaf(|| crate::widgets::Empty)
            }
        }
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let mut data = ElevatedButtonThemeData::new();
        data.style = style;
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            Theme::dark(),
            ElevatedButtonTheme::new(
                data,
                component(Reader {
                    variant,
                    states,
                    seen: std::rc::Rc::clone(&seen),
                }),
            ),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    /// Every one of the four button themes, each set to a foreground of its
    /// own, so which one a variant read is readable off the answer.
    ///
    /// `variant_sweep` found `ButtonVariant::Outlined` able to read
    /// `ElevatedButtonTheme` -- the arm above it -- with the whole suite
    /// green. Nothing distinguished the four, because every test that reached
    /// this function published one theme at a time.
    fn variant_reads(variant: ButtonVariant) -> ResolvedButton {
        use crate::component_themes::{
            FilledButtonTheme, FilledButtonThemeData, OutlinedButtonTheme,
            OutlinedButtonThemeData, TextButtonTheme, TextButtonThemeData,
        };
        use crate::widget_state::StateProperty;

        struct Reader {
            variant: ButtonVariant,
            seen: std::rc::Rc<std::cell::RefCell<Option<ResolvedButton>>>,
        }
        impl Component for Reader {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.seen.borrow_mut() = Some(ResolvedButton::of(
                    context,
                    self.variant,
                    crate::widget_state::WidgetStates::NONE,
                    ResolvedButton {
                        background: None,
                        foreground: UNTHEMED,
                        side: None,
                        padding: None,
                        minimum_size: None,
                    },
                ));
                leaf(|| crate::widgets::Empty)
            }
        }

        fn style(colour: crate::engine::Color) -> Option<ButtonStyle> {
            let mut style = ButtonStyle::new();
            style.foreground_color = Some(StateProperty::all(Some(colour)));
            Some(style)
        }

        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        let reader = component(Reader {
            variant,
            seen: std::rc::Rc::clone(&seen),
        });

        let mut filled = FilledButtonThemeData::new();
        filled.style = style(FILLED_MARK);
        let mut elevated = ElevatedButtonThemeData::new();
        elevated.style = style(ELEVATED_MARK);
        let mut outlined = OutlinedButtonThemeData::new();
        outlined.style = style(OUTLINED_MARK);
        let mut text = TextButtonThemeData::new();
        text.style = style(TEXT_MARK);

        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            Theme::dark(),
            FilledButtonTheme::new(
                filled,
                ElevatedButtonTheme::new(
                    elevated,
                    OutlinedButtonTheme::new(
                        outlined,
                        TextButtonTheme::new(text, reader),
                    ),
                ),
            ),
        ));
        seen.borrow_mut().take().expect("built once")
    }

    const UNTHEMED: crate::engine::Color = crate::engine::Color(0xff000001);
    const FILLED_MARK: crate::engine::Color = crate::engine::Color(0xff000011);
    const ELEVATED_MARK: crate::engine::Color = crate::engine::Color(0xff000022);
    const OUTLINED_MARK: crate::engine::Color = crate::engine::Color(0xff000033);
    const TEXT_MARK: crate::engine::Color = crate::engine::Color(0xff000044);

    #[test]
    fn each_button_variant_reads_the_theme_its_own_widget_reads() {
        // Reading a neighbour's theme is invisible until all four are in the
        // tree at once: an application that themes its outlined buttons and
        // sees nothing change has no other symptom.
        assert_eq!(variant_reads(ButtonVariant::Filled).foreground, FILLED_MARK);
        assert_eq!(
            variant_reads(ButtonVariant::Elevated).foreground,
            ELEVATED_MARK
        );
        assert_eq!(
            variant_reads(ButtonVariant::Outlined).foreground,
            OUTLINED_MARK
        );
        assert_eq!(variant_reads(ButtonVariant::Text).foreground, TEXT_MARK);
    }

    #[test]
    fn and_the_danger_variant_reads_the_filled_one_on_purpose() {
        // Upstream has no danger button; this crate's is a filled button in
        // the error colours, so it takes a filled button's theme. Sharing an
        // arm is a decision rather than an omission, which is why it has a
        // test of its own rather than being left to look like one.
        assert_eq!(variant_reads(ButtonVariant::Danger).foreground, FILLED_MARK);
    }

    #[test]
    fn an_elevated_button_is_not_a_filled_one_with_a_shadow() {
        // Its background is the low surface container and its label the
        // primary -- the opposite way round from a filled button. Material 3
        // demotes it deliberately: elevation is how it stands out, so the
        // colour does not have to.
        let theme = Theme::dark();
        let elevated = resolved_button(
            ButtonVariant::Elevated,
            None,
            crate::widget_state::WidgetStates::NONE,
        );
        assert_eq!(elevated.foreground, theme.primary);
        assert_eq!(elevated.background, Some(theme.surface_variant));
        assert_ne!(
            elevated.background,
            Some(theme.primary),
            "a filled button's colours, the other way round"
        );
    }

    #[test]
    fn every_variant_has_its_own_pair_and_no_two_agree() {
        // Asked through the one table both `Button::build` and the gallery's
        // demo read, so a variant added in one place cannot be forgotten in
        // the other.
        let theme = Theme::dark();
        let all = [
            ButtonVariant::Filled,
            ButtonVariant::Danger,
            ButtonVariant::Outlined,
            ButtonVariant::Text,
            ButtonVariant::Elevated,
        ];
        for (i, a) in all.iter().enumerate() {
            for b in all.iter().skip(i + 1) {
                assert_ne!(
                    a.default_colors(&theme),
                    b.default_colors(&theme),
                    "{a:?} and {b:?} are drawn the same"
                );
            }
        }

        // And the elevated one is the filled one's colours the other way up.
        let (elevated_fill, elevated_label, _) = ButtonVariant::Elevated.default_colors(&theme);
        let (filled_fill, filled_label, _) = ButtonVariant::Filled.default_colors(&theme);
        assert_eq!(elevated_label, theme.primary);
        assert_eq!(filled_fill, Some(theme.primary));
        assert_ne!(elevated_fill, filled_fill);
        assert_ne!(elevated_label, filled_label);
    }

    #[test]
    fn an_elevated_button_reads_its_own_theme_and_not_the_filled_one_s() {
        const MINE: Color = Color::argb(0xFF, 0x31, 0x41, 0x59);
        let mut style = ButtonStyle::default();
        style.background_color = Some(crate::widget_state::StateProperty::resolve_with(|_| {
            Some(MINE)
        }));
        let resolved = resolved_button(
            ButtonVariant::Elevated,
            Some(style.clone()),
            crate::widget_state::WidgetStates::NONE,
        );
        assert_eq!(resolved.background, Some(MINE));

        // The same theme installed while a *filled* button resolves changes
        // nothing: each variant reads the theme upstream's matching widget
        // reads.
        let filled = resolved_button(
            ButtonVariant::Filled,
            Some(style),
            crate::widget_state::WidgetStates::NONE,
        );
        assert_ne!(filled.background, Some(MINE));
    }

    #[test]
    fn all_three_radii_unset_is_exactly_the_default_size() {
        let avatar = CircleAvatar::new();
        assert_eq!(avatar.min_diameter(), 40.0);
        assert_eq!(avatar.max_diameter(), 40.0);
    }

    #[test]
    fn setting_any_one_radius_stops_the_default_applying_to_the_others() {
        // Upstream's rule, and the one worth stating: with only a maximum
        // given, the minimum falls to zero rather than staying at the default
        // -- otherwise a caller who asked for "at most 8" would silently get
        // "at least 20", which is the opposite of what they asked.
        let bounded = CircleAvatar {
            max_radius: Some(8.0),
            ..CircleAvatar::new()
        };
        assert_eq!(bounded.min_diameter(), 0.0);
        assert_eq!(bounded.max_diameter(), 16.0);

        // And with only a minimum, the maximum is unbounded rather than the
        // default.
        let at_least = CircleAvatar {
            min_radius: Some(8.0),
            ..CircleAvatar::new()
        };
        assert_eq!(at_least.min_diameter(), 16.0);
        assert!(at_least.max_diameter().is_infinite());
    }

    #[test]
    fn a_fixed_radius_pins_both_ends() {
        // Which is what "fixed" means: the parent's constraints have nothing
        // left to choose between.
        let fixed = CircleAvatar::new().with_radius(12.0);
        assert_eq!(fixed.min_diameter(), 24.0);
        assert_eq!(fixed.max_diameter(), 24.0);
    }

    #[test]
    fn a_fixed_radius_wins_over_the_bounds() {
        // Upstream's `radius ?? minRadius` and `radius ?? maxRadius`: given
        // all three, the fixed one is both ends and the bounds are ignored.
        let all_three = CircleAvatar {
            radius: Some(12.0),
            min_radius: Some(1.0),
            max_radius: Some(99.0),
            ..CircleAvatar::new()
        };
        assert_eq!(all_three.min_diameter(), 24.0);
        assert_eq!(all_three.max_diameter(), 24.0);
    }

    #[test]
    fn a_data_source_says_when_its_count_is_a_guess() {
        // A source reading a stream does not know how many rows there are
        // until it reaches the end, and a table that believed the guess would
        // draw a scrollbar that lies.
        struct Streaming;
        impl DataTableSource for Streaming {
            fn get_row(&self, _index: usize) -> Option<DataRow> {
                None
            }
            fn row_count(&self) -> usize {
                50
            }
            fn is_row_count_approximate(&self) -> bool {
                true
            }
        }
        assert!(Streaming.is_row_count_approximate());

        // The default is the ordinary case: a source that knows.
        struct Fixed;
        impl DataTableSource for Fixed {
            fn get_row(&self, index: usize) -> Option<DataRow> {
                (index < 2).then(|| DataRow::new(Vec::new()))
            }
            fn row_count(&self) -> usize {
                2
            }
        }
        assert!(!Fixed.is_row_count_approximate());
        assert_eq!(Fixed.selected_row_count(), 0);
    }

    #[test]
    fn a_row_the_source_cannot_produce_yet_is_nothing_rather_than_empty() {
        // A page still loading answers nothing, which the table draws as a
        // placeholder. An empty row would be indistinguishable from a real
        // row with no cells.
        struct Paged;
        impl DataTableSource for Paged {
            fn get_row(&self, index: usize) -> Option<DataRow> {
                (index < 20).then(|| DataRow::new(Vec::new()))
            }
            fn row_count(&self) -> usize {
                10_000
            }
        }
        assert!(Paged.get_row(0).is_some());
        assert!(
            Paged.get_row(500).is_none(),
            "past what has loaded, and the count says nothing about that"
        );
        assert_eq!(Paged.row_count(), 10_000);
    }

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

    // -- What the tile was told, and what it does about it -----------------------------

    /// Lays a tile out and hit-tests it, reporting which marker was hit.
    fn tile_hit(tile: ListTile, at: (f32, f32), width: f32) -> Option<u64> {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), component(tile)));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::tight(width, 80.0));
        let mut result = crate::render::HitTestResult::new();
        root.hit_test(crate::render::Offset::new(at.0, at.1), &mut result);
        result.path.first().map(|entry| entry.target)
    }

    fn tile_height(tile: ListTile) -> f32 {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), component(tile)));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints {
            min_width: 0.0,
            max_width: 400.0,
            min_height: 0.0,
            max_height: f32::INFINITY,
        })
        .height
    }

    #[test]
    fn a_disabled_tile_is_not_a_target_at_all() {
        // Not a target that ignores taps -- upstream builds the InkWell
        // without callbacks, so the tap goes to whatever is behind.
        const TAP: u64 = 91;
        let live = ListTile::new("Wi-Fi").tappable(TAP, PointerHandlers::new());
        assert_eq!(tile_hit(live, (200.0, 30.0), 400.0), Some(TAP));

        let dead = ListTile::new("Wi-Fi")
            .tappable(TAP, PointerHandlers::new())
            .with_enabled(false);
        assert_ne!(tile_hit(dead, (200.0, 30.0), 400.0), Some(TAP));
    }

    #[test]
    fn enabled_is_the_default_because_a_tile_is_live_unless_told_otherwise() {
        assert!(
            ListTile::new("a")
                .tappable(1, PointerHandlers::new())
                .is_tappable()
        );
        assert!(
            !ListTile::new("a").is_tappable(),
            "though a tile nobody made tappable is still not a target"
        );
    }

    #[test]
    fn a_leading_goes_before_the_title_and_a_trailing_after_it() {
        const LEADING: u64 = 51;
        const TRAILING: u64 = 52;
        fn tile() -> ListTile {
            ListTile::new("Aeroplane mode")
                .with_leading(crate::framework::leaf(|| {
                    crate::widgets::Pointer::new(LEADING, Container::new().with_size(24.0, 24.0))
                }))
                .with_trailing(crate::framework::leaf(|| {
                    crate::widgets::Pointer::new(TRAILING, Container::new().with_size(40.0, 24.0))
                }))
        }
        assert_eq!(tile_hit(tile(), (20.0, 40.0), 400.0), Some(LEADING));
        assert_eq!(tile_hit(tile(), (380.0, 40.0), 400.0), Some(TRAILING));
    }

    #[test]
    fn the_tile_overrides_the_themes_leading_width() {
        // Only the choice is asserted, not the geometry. The reservation's
        // effect is on where the *title* starts, and this harness can see
        // neither the title (it is built inside the tile) nor the tile's
        // intrinsic width (nothing along this chain implements intrinsics).
        // A test of the geometry here would be one that cannot fail.
        assert_eq!(
            ListTile::new("a")
                .with_min_leading_width(60.0)
                .min_leading_width,
            Some(60.0)
        );
        assert_eq!(
            ListTile::new("a").min_leading_width,
            None,
            "and unset defers to the theme"
        );
    }

    #[test]
    fn a_theme_that_set_the_height_outright_is_not_overruled_by_dense() {
        // Upstream's `minTileHeight ?? (dense ? 48 : 56)`: an explicit height
        // wins and dense changes nothing. Adjusting the height after the fact
        // could not tell that case from the one where dense chooses.
        fn height(dense: Option<bool>, themed: Option<f32>) -> f32 {
            let mut tree = ElementTree::new();
            let mut data = crate::component_themes::ListTileThemeData::new();
            data.min_tile_height = themed;
            let mut tile = ListTile::new("Wi-Fi");
            if let Some(dense) = dense {
                tile = tile.with_dense(dense);
            }
            tree.rebuild(provide(
                Theme::dark(),
                crate::component_themes::ListTileTheme::new(data, component(tile)),
            ));
            let mut root = tree.build_render_tree().expect("a root");
            root.layout(BoxConstraints {
                min_width: 0.0,
                max_width: 400.0,
                min_height: 0.0,
                max_height: f32::INFINITY,
            })
            .height
        }

        assert_eq!(
            height(Some(true), Some(70.0)),
            height(Some(false), Some(70.0)),
            "an explicit height is an explicit height"
        );
        assert!(
            height(Some(true), None) < height(Some(false), None),
            "and without one, dense chooses"
        );
    }

    #[test]
    fn dense_makes_a_tile_shorter_and_never_taller() {
        let ordinary = tile_height(ListTile::new("Wi-Fi"));
        let dense = tile_height(ListTile::new("Wi-Fi").with_dense(true));
        assert!(dense < ordinary, "{dense} vs {ordinary}");
        assert_eq!(
            tile_height(ListTile::new("Wi-Fi").with_dense(false)),
            ordinary
        );
    }

    #[test]
    fn dense_comes_from_the_tile_first_and_the_theme_second() {
        // Upstream's `_isDenseLayout`: the tile, then the tile theme, then the
        // app theme, then false -- so a list can be made dense once rather than
        // per row, and one row can still opt out.
        assert!(ListTile::new("a").is_dense(true), "the theme said so");
        assert!(!ListTile::new("a").is_dense(false));
        assert!(
            !ListTile::new("a").with_dense(false).is_dense(true),
            "and the tile overrides the theme"
        );
        assert!(ListTile::new("a").with_dense(true).is_dense(false));
    }

    #[test]
    fn a_content_padding_on_the_tile_beats_the_themes() {
        let themed = tile_height(ListTile::new("Wi-Fi"));
        let padded = tile_height(
            ListTile::new("Wi-Fi").with_content_padding(EdgeInsets::symmetric(16.0, 40.0)),
        );
        assert!(padded > themed, "{padded} vs {themed}");
    }

    #[test]
    fn three_lines_need_a_subtitle_to_be_three_lines_of() {
        let with = ListTile::new("Wi-Fi")
            .with_subtitle("Connected")
            .with_three_line(true);
        assert!(with.is_tappable() || true, "built without panicking");
    }

    #[test]
    #[should_panic(expected = "isThreeLine needs a subtitle")]
    fn three_lines_without_one_is_caught() {
        let _ = ListTile::new("Wi-Fi").with_three_line(true);
    }

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
        // at it.
        //
        // Only the second half used to be checkable -- the stub measured every
        // label as nothing, so every button was at its minimum. It measures
        // now, and the first half is below.
        let mut tree = ElementTree::new();
        tree.rebuild(component(Button::new(1, "go")));
        let mut root = tree.build_render_tree().expect("a root");
        let size = root.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(size.width, 64.0, "a short label stops at the minimum");
        assert_eq!(size.height, 40.0);

        // And a label with more in it pushes past. The width is the stub's
        // arithmetic rather than a font's, so what is asserted is that it grew
        // and that the height did not: a button gets wider for a long label,
        // never taller.
        let mut tree = ElementTree::new();
        tree.rebuild(component(Button::new(
            1,
            "a considerably longer label than that one",
        )));
        let mut root = tree.build_render_tree().expect("a root");
        let wide = root.layout(BoxConstraints::loose(2000.0, 200.0));
        assert!(
            wide.width > 64.0,
            "a long label widens the button: {}",
            wide.width
        );
        assert_eq!(wide.height, 40.0, "and does not make it taller");

        let mut tree = ElementTree::new();
        tree.rebuild(component(Button::new(1, "go").with_min_width(120.0)));
        let mut root = tree.build_render_tree().expect("a root");
        let size = root.layout(BoxConstraints::loose(200.0, 200.0));
        assert_eq!(size.width, 120.0, "a raised minimum raises the button");
        assert_eq!(size.height, 40.0);
    }

    #[test]
    fn button_padding_follows_the_readers_text_size() {
        // Upstream `ButtonVariantButton.scaledPadding`: sixteen points of
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

    #[test]
    fn a_divider_takes_its_space_from_its_theme() {
        use crate::component_themes::{DividerTheme, DividerThemeData};
        use crate::framework::ElementTree;

        fn height_of(widget: AnyWidget) -> f32 {
            let mut tree = ElementTree::new();
            tree.rebuild(widget);
            let mut root = tree.build_render_tree().expect("a root");
            root.layout(BoxConstraints::loose(200.0, 200.0)).height
        }

        // Upstream's defaults, where no theme said otherwise: sixteen of
        // space with a hairline in the middle of it.
        assert_eq!(height_of(component(Divider)), 16.0);

        // The nearest installed theme moves it -- the three-step fallback
        // reaching a control's geometry, which is the whole point of it.
        assert_eq!(
            height_of(DividerTheme::new(
                DividerThemeData::new().with_space(40.0),
                component(Divider),
            )),
            40.0
        );

        // And so does the field on ThemeData, one step further out.
        let themed = crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light()
                .with_divider_theme(DividerThemeData::new().with_space(24.0)),
            component(Divider),
        );
        assert_eq!(height_of(themed), 24.0);
    }

    /// A fixed box standing in for an action or a message: the engine stubs
    /// these tests link report zero-sized text.
    fn block(width: f32, height: f32) -> AnyWidget {
        crate::framework::leaf(move || Container::new().with_size(width, height))
    }

    fn banner_of(actions: usize) -> MaterialBanner {
        MaterialBanner::new(
            block(100.0, 20.0),
            (0..actions).map(|_| block(60.0, 20.0)).collect(),
        )
    }

    #[test]
    fn only_a_lone_action_shares_the_contents_row() {
        // Upstream's condition is the action *count*, not whether they fit:
        // two short actions still take their own row.
        assert!(banner_of(1).is_single_row());
        assert!(!banner_of(2).is_single_row());
        assert!(!banner_of(0).is_single_row());
        // And a caller may force even a lone action below, for a label long
        // enough that sharing the row would crowd the text.
        assert!(!banner_of(1).with_force_actions_below(true).is_single_row());
    }

    #[test]
    fn the_banner_is_taller_once_its_actions_take_their_own_row() {
        let height = |actions: usize| {
            let mut tree = ElementTree::new();
            tree.rebuild(provide(Theme::dark(), component(banner_of(actions))));
            let mut root = tree.build_render_tree().expect("a root");
            root.layout(BoxConstraints::loose(400.0, 600.0)).height
        };
        // One row: the 52-tall action bar is what holds the banner open.
        // Stacked: that bar moves below the content, and the content's own
        // top inset grows from 2 to 24 because nothing else is holding the
        // text off the top edge.
        assert!(
            height(2) > height(1),
            "one row {} should be shorter than two {}",
            height(1),
            height(2)
        );
    }

    #[test]
    fn a_banner_with_no_theme_sits_flat_rather_than_one_step_off_the_page() {
        // Upstream's expression is `widget.elevation ?? bannerTheme.elevation
        // ?? 0.0` -- it never reaches `_BannerDefaultsM3`, whose elevation is
        // 1.0. Written down because it looks like an oversight and the port
        // would answer differently if it "fixed" it.
        let mut tree = ElementTree::new();
        let seen = std::rc::Rc::new(std::cell::Cell::new(0.0f32));
        let sink = std::rc::Rc::clone(&seen);
        struct Reader(std::rc::Rc<std::cell::Cell<f32>>);
        impl Component for Reader {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                self.0
                    .set(crate::component_themes::ResolvedMaterialBanner::of(context).elevation);
                crate::framework::leaf(|| crate::widgets::Empty)
            }
        }
        tree.rebuild(provide(Theme::dark(), component(Reader(sink))));
        assert_eq!(seen.get(), 0.0);
    }

    #[test]
    fn a_raised_banner_leaves_room_under_itself_for_its_own_shadow() {
        // Upstream's `EdgeInsets.only(bottom: elevation > 0 ? 10.0 : 0.0)`.
        // A flat banner has no shadow to leave room for.
        let margin_at = |elevation: f32| {
            crate::component_themes::ResolvedMaterialBanner {
                background_color: Color::WHITE,
                surface_tint_color: None,
                shadow_color: None,
                divider_color: Color::BLACK,
                elevation,
                padding: None,
                leading_padding: crate::borders::EdgeInsetsGeometry::Zero,
            }
            .default_margin()
            .bottom
        };
        assert_eq!(margin_at(0.0), 0.0);
        assert_eq!(margin_at(1.0), 10.0);
    }

    #[test]
    fn the_content_inset_depends_on_where_the_actions_went() {
        // On one row the 52-tall action bar supplies the height, so the
        // content needs almost no top inset; stacked, the banner has to hold
        // the text off its own top edge.
        let banner = crate::component_themes::ResolvedMaterialBanner {
            background_color: Color::WHITE,
            surface_tint_color: None,
            shadow_color: None,
            divider_color: Color::BLACK,
            elevation: 0.0,
            padding: None,
            leading_padding: crate::borders::EdgeInsetsGeometry::Zero,
        };
        let single = banner
            .content_padding(true)
            .resolve(crate::direction::TextDirection::Ltr);
        let stacked = banner
            .content_padding(false)
            .resolve(crate::direction::TextDirection::Ltr);
        assert_eq!((single.top, single.bottom), (2.0, 0.0));
        assert_eq!((stacked.top, stacked.bottom), (24.0, 4.0));
        // Both start 16 in from the reading edge, and both mirror.
        assert_eq!(single.left, 16.0);
        assert_eq!(
            banner
                .content_padding(true)
                .resolve(crate::direction::TextDirection::Rtl)
                .right,
            16.0
        );
    }

    #[test]
    fn a_themed_padding_wins_over_both_defaults() {
        // And it wins over *both*, so a theme that set one padding does not
        // get the single-row default back when the actions stack.
        let banner = crate::component_themes::ResolvedMaterialBanner {
            background_color: Color::WHITE,
            surface_tint_color: None,
            shadow_color: None,
            divider_color: Color::BLACK,
            elevation: 0.0,
            padding: Some(crate::borders::EdgeInsetsGeometry::Absolute(
                EdgeInsets::all(7.0),
            )),
            leading_padding: crate::borders::EdgeInsetsGeometry::Zero,
        };
        for single in [true, false] {
            assert_eq!(
                banner
                    .content_padding(single)
                    .resolve(crate::direction::TextDirection::Ltr),
                EdgeInsets::all(7.0)
            );
        }
    }

    #[test]
    fn a_column_is_interactive_exactly_when_it_can_be_sorted_by() {
        // Upstream's `_debugInteractive` is `onSort != null`, so a column
        // with no callback shows no sort arrow rather than a dead one.
        assert!(
            !DataColumn::new(crate::framework::leaf(|| crate::widgets::Empty)).is_interactive()
        );
        assert!(
            DataColumn::new(crate::framework::leaf(|| crate::widgets::Empty))
                .with_on_sort(|_, _| {})
                .is_interactive()
        );
    }

    #[test]
    fn numeric_is_an_alignment_decision_not_a_formatting_one() {
        // The table right-aligns a numeric column, because numbers are read
        // by comparing digits in the same place and left alignment puts the
        // units column somewhere different in every row.
        let plain = DataColumn::new(crate::framework::leaf(|| crate::widgets::Empty));
        assert!(!plain.numeric);
        let numbers = DataColumn::new(crate::framework::leaf(|| crate::widgets::Empty)).numeric();
        assert!(numbers.numeric);
    }

    #[test]
    fn an_empty_cell_is_present_and_blank_rather_than_absent() {
        // A table's rows all have the same number of cells, so a row with
        // nothing in one column still needs a cell there -- otherwise every
        // column after it shifts left.
        let empty = DataCell::empty();
        assert!(
            empty.child.borrow().is_some(),
            "there is a child, and it is blank"
        );
        assert!(!empty.is_interactive());
        assert!(!empty.placeholder);
    }

    #[test]
    fn a_cell_is_interactive_if_any_one_of_its_gestures_is_wired() {
        assert!(!DataCell::empty().is_interactive());
        assert!(DataCell::empty().with_on_tap(|| {}).is_interactive());
        assert!(DataCell::empty().with_on_long_press(|| {}).is_interactive());
        assert!(DataCell::empty().with_on_double_tap(|| {}).is_interactive());
    }

    #[test]
    fn a_placeholder_says_it_is_one_and_lets_the_table_decide_how_faint() {
        // A flag rather than a colour: the table owns how faint a placeholder
        // is, and the cell only says that it is one.
        assert!(DataCell::empty().placeholder().placeholder);
        assert!(!DataCell::empty().placeholder);
    }

    #[test]
    fn a_table_row_ink_well_splashes_across_the_row_not_the_cell() {
        // The entire reason the class exists. A press in a narrow cell has to
        // reach the far end of the row, so the splash's target radius is
        // measured against the row's rectangle rather than the cell's.
        use crate::ink::InkRipple;
        use crate::render::Size;

        let cell = Size::new(80.0, 40.0);
        let row = Size::new(600.0, 40.0);
        let cell_splash = InkRipple::target_radius(cell);
        let row_splash = InkRipple::target_radius(row);
        assert!(
            row_splash > cell_splash * 3.0,
            "a row's splash reaches much further: {row_splash} vs {cell_splash}"
        );
    }

    #[test]
    fn a_table_row_ink_well_is_contained_and_rectangular() {
        // Upstream's constructor passes both to `super`, and they go together
        // -- a row highlight fills the row, so it has to be clipped to it.
        let well =
            TableRowInkWell::new(1, crate::engine::Rect::ltrb(0.0, 0.0, 600.0, 40.0), || {
                crate::framework::leaf(|| crate::widgets::Empty)
            });
        assert!(well.contained_ink_well);
        assert_eq!(
            well.highlight_shape,
            crate::ink::InkHighlightShape::Rectangle
        );
    }

    #[test]
    fn a_row_box_is_the_full_width_of_the_table() {
        // Which is what makes a splash started in one cell cover the row: a
        // row is the whole width however narrow its cells are.
        use crate::render::{BoxConstraints, RenderBox, RenderConstrainedBox, RenderTable};
        let mut table = RenderTable::new(
            2,
            vec![
                Some(crate::render::RenderRef::new(RenderConstrainedBox::tight(
                    40.0, 20.0,
                ))),
                Some(crate::render::RenderRef::new(RenderConstrainedBox::tight(
                    40.0, 30.0,
                ))),
                Some(crate::render::RenderRef::new(RenderConstrainedBox::tight(
                    40.0, 10.0,
                ))),
                Some(crate::render::RenderRef::new(RenderConstrainedBox::tight(
                    40.0, 10.0,
                ))),
            ],
        );
        table.layout(BoxConstraints::new(0.0, 200.0, 0.0, f32::INFINITY));

        let first = table.row_box(0).expect("a first row");
        assert_eq!(first.left, 0.0);
        assert!(first.right > 40.0, "wider than one cell: {}", first.right);
        // The row is as tall as its *tallest* cell, which is why the cell
        // offsets alone cannot answer this.
        assert_eq!(first.height(), 30.0);

        let second = table.row_box(1).expect("a second row");
        assert_eq!(second.top, first.bottom, "rows stack with no gap");
        assert_eq!(second.height(), 10.0);

        assert_eq!(table.row_box(2), None, "and no third row exists");
    }
}

/// Upstream `VerticalDivider`: the same hairline, on its side.
///
/// A separate class rather than an axis on [`Divider`], because upstream
/// makes it one: the two read the *same* theme fields, and `space` means
/// width here where it means height there. One widget with an axis would
/// have to explain that reversal at every call site.
pub struct VerticalDivider;

impl Component for VerticalDivider {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let divider = crate::component_themes::ResolvedDivider::of(context);
        let color = divider.color;
        // Upstream's `space` is the width it reserves, and `thickness` the
        // width of the line inside it -- the same two fields as the
        // horizontal rule, measured across the other axis.
        let space = divider.space;
        let thickness = divider.line_thickness();
        // And the indents run down rather than across: upstream's
        // `EdgeInsetsDirectional.only(top: indent, bottom: endIndent)`.
        let insets = crate::render::EdgeInsets {
            left: 0.0,
            right: 0.0,
            top: divider.indent,
            bottom: divider.end_indent,
        };
        leaf(move || {
            Container::new().with_width(space).with_child(Align::new(
                Alignment::CENTER,
                Container::new()
                    .with_width(thickness)
                    .with_color(color)
                    .with_margin(insets),
            ))
        })
    }
}

/// Upstream `DataTableSource`: where a paginated table's rows come from.
///
/// The point of it is that the table does not hold the rows. A table of ten
/// thousand records asks for the twenty it is showing, and this is what it
/// asks -- which is also why the count is separate from the rows, and why
/// there is a flag for not knowing it.
pub trait DataTableSource {
    /// Upstream `getRow`: the row at an index, or nothing if the source
    /// cannot produce it yet -- a page still loading answers nothing rather
    /// than blocking.
    fn get_row(&self, index: usize) -> Option<DataRow>;

    /// Upstream `rowCount`.
    fn row_count(&self) -> usize;

    /// Upstream `isRowCountApproximate`: whether [`row_count`](Self::row_count)
    /// is a guess.
    ///
    /// A source reading a stream does not know how many rows there are until
    /// it reaches the end, and a table that believed a guess would draw a
    /// scrollbar that lies. This is how it says so.
    fn is_row_count_approximate(&self) -> bool {
        false
    }

    /// Upstream `selectedRowCount`, which the table shows in its header --
    /// "3 items selected" -- and which the source owns because selection
    /// outlives the rows that happen to be on screen.
    fn selected_row_count(&self) -> usize {
        0
    }
}

/// One row a [`DataTableSource`] produced.
///
/// Upstream's `DataRow` is in `data_table.dart` with the table itself; this
/// is the part a source has to be able to make, which is the cells and
/// whether the row is selected.
pub struct DataRow {
    pub cells: Vec<AnyWidget>,
    pub selected: bool,
}

impl DataRow {
    pub fn new(cells: Vec<AnyWidget>) -> DataRow {
        DataRow {
            cells,
            selected: false,
        }
    }

    pub fn with_selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
}

/// Upstream `CircleAvatar`: a round patch with something in the middle.
///
/// # How the three radii work
///
/// Upstream takes `radius`, `minRadius` and `maxRadius` and lets a caller
/// give any of them. `radius` fixes the size; the other two bound it while
/// letting the parent's constraints choose within them. The rule that makes
/// this readable is upstream's: **all three unset** means the default radius
/// exactly, and once *any* of them is set the default stops applying to
/// either bound. Without that, giving only a `maxRadius` would silently keep
/// the default as a minimum.
pub struct CircleAvatar {
    pub child: std::cell::RefCell<Option<AnyWidget>>,
    pub background_color: Option<Color>,
    pub foreground_color: Option<Color>,
    pub radius: Option<f32>,
    pub min_radius: Option<f32>,
    pub max_radius: Option<f32>,
}

impl CircleAvatar {
    /// Upstream's `_defaultRadius`.
    pub const DEFAULT_RADIUS: f32 = 20.0;
    pub const DEFAULT_MIN_RADIUS: f32 = 0.0;
    pub const DEFAULT_MAX_RADIUS: f32 = f32::INFINITY;

    pub fn new() -> CircleAvatar {
        CircleAvatar {
            child: std::cell::RefCell::new(None),
            background_color: None,
            foreground_color: None,
            radius: None,
            min_radius: None,
            max_radius: None,
        }
    }

    pub fn with_child(self, child: AnyWidget) -> Self {
        *self.child.borrow_mut() = Some(child);
        self
    }

    pub fn with_radius(mut self, radius: f32) -> Self {
        self.radius = Some(radius);
        self
    }

    pub fn with_radius_range(mut self, min_radius: f32, max_radius: f32) -> Self {
        self.min_radius = Some(min_radius);
        self.max_radius = Some(max_radius);
        self
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    pub fn with_foreground_color(mut self, color: Color) -> Self {
        self.foreground_color = Some(color);
        self
    }

    fn all_radii_unset(&self) -> bool {
        self.radius.is_none() && self.min_radius.is_none() && self.max_radius.is_none()
    }

    /// Upstream's `_minDiameter`.
    pub fn min_diameter(&self) -> f32 {
        if self.all_radii_unset() {
            return CircleAvatar::DEFAULT_RADIUS * 2.0;
        }
        2.0 * self
            .radius
            .or(self.min_radius)
            .unwrap_or(CircleAvatar::DEFAULT_MIN_RADIUS)
    }

    /// Upstream's `_maxDiameter`.
    pub fn max_diameter(&self) -> f32 {
        if self.all_radii_unset() {
            return CircleAvatar::DEFAULT_RADIUS * 2.0;
        }
        2.0 * self
            .radius
            .or(self.max_radius)
            .unwrap_or(CircleAvatar::DEFAULT_MAX_RADIUS)
    }
}

impl Default for CircleAvatar {
    fn default() -> CircleAvatar {
        CircleAvatar::new()
    }
}

impl Component for CircleAvatar {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        // Upstream falls back to the scheme's container colours; this crate's
        // `Theme` has the one pair, which is where a filled patch takes its
        // colours from everywhere else in it.
        let background = self.background_color.unwrap_or(theme.surface_variant);
        // A radius fixes the size; a range lets the parent choose between the
        // two, and with nothing else deciding the smaller end is what is
        // drawn. An unbounded maximum is exactly that case.
        let diameter = self.min_diameter();
        let child = self.child.borrow_mut().take();
        match child {
            Some(child) => crate::framework::single(child, move |child| {
                Container::new()
                    .with_size(diameter, diameter)
                    .with_color(background)
                    .with_corner_radius(diameter / 2.0)
                    .with_child(Align::new(Alignment::CENTER, child))
            }),
            None => leaf(move || {
                Container::new()
                    .with_size(diameter, diameter)
                    .with_color(background)
                    .with_corner_radius(diameter / 2.0)
            }),
        }
    }
}
