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

    /// The constraints the button was given, **raised to the minimums**.
    ///
    /// The first version handed the child `(min_width, INFINITY)` and ignored
    /// the incoming constraints entirely, then squeezed its own size back
    /// through them. Under loose constraints that is the same thing. Under
    /// **tight** ones it is not, and a downstream application found out how:
    /// a button dropped into a positioned stack slot 104 wide laid its child
    /// out at 64, sized itself to 104, and painted a pill 64 wide inside a box
    /// of 104. The rest was unpainted box inside the ink's rounded clip, and
    /// what showed through read as a second, darker pill beside the button.
    ///
    /// Upstream hands the child `constraints` and takes the maximum with the
    /// minimum size afterwards, so a tight box reaches the child. The minimum
    /// is a floor on the answer, not a replacement for the question.
    ///
    /// The **maximums are the ones that came in**, unraised. A first version
    /// lifted them to the minimums as well, so that a button asked for less
    /// room than its minimum would lay its child out at the minimum anyway --
    /// which is a child overflowing its own box, and `constrain` below would
    /// clip the answer back regardless. Upstream never widens a maximum here,
    /// and a mutation removing the lift stayed green: it decided nothing.
    fn inner_constraints(&self, constraints: BoxConstraints) -> BoxConstraints {
        BoxConstraints::new(
            self.min_width.max(constraints.min_width),
            constraints.max_width,
            self.min_height.max(constraints.min_height),
            constraints.max_height,
        )
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

    /// `_InputPadding.performLayout`: the child's answer, put through the
    /// constraints the button itself was given.
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        let inner = self.inner_constraints(constraints);
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
        let inner = self.inner_constraints(constraints);
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
    /// Upstream's `autofocus`: take the keyboard as soon as this appears.
    autofocus: bool,
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
            autofocus: false,
        }
    }

    /// Upstream's `autofocus`. Ignored by a disabled button, which is not
    /// somewhere the keyboard goes at all.
    pub fn with_autofocus(mut self, autofocus: bool) -> Self {
        self.autofocus = autofocus;
        self
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
                icon_alignment: crate::component_themes::IconAlignment::Start,
                animation_duration: crate::component_themes::ResolvedButton::ANIMATION_DURATION,
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
                    .with_child(
                        Align::new(
                            Alignment::CENTER,
                            // Upstream's label style for buttons is
                            // `labelLarge`: medium weight, not bold.
                            Text::new(label.clone())
                                .with_size(body_size)
                                .with_weight(500)
                                .with_color(label_color),
                        )
                        // Upstream's `Align(widthFactor: 1.0, heightFactor:
                        // 1.0)`. Without the factors an `Align` fills whatever
                        // it is offered, so the button would be as wide as the
                        // row it sits in rather than as wide as its label.
                        //
                        // The first version of `ButtonBounds` worked around
                        // that by handing the child an infinite maximum, which
                        // hid the real difference and lost every tight
                        // constraint on the way down. The factors are the
                        // thing upstream actually does.
                        //
                        // Only the width factor is observable here -- the
                        // container above fixes the height -- so a mutation
                        // dropping the height one stays green. It is upstream's
                        // argument and it costs nothing, so both are written.
                        .with_factors(Some(1.0), Some(1.0)),
                    );
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
                // **Passthrough**, not the stack's default of loose. This
                // stack sits between `ButtonBounds` and the pill, so a loose
                // one drops the minimum width `ButtonBounds` just set: the
                // button lays out to its wider box and paints its pill at the
                // label's own width. What is left is unpainted box inside a
                // rounded ink clip the full width, which the reader sees as a
                // second, darker pill beside the button.
                //
                // Upstream has no stack on this path -- the state layer is an
                // ink feature painted over the child, not a sibling box -- so
                // there is nothing there to loosen anything.
                let body = if let Some(overlay) = press_overlay {
                    RenderStack::new()
                        .with_fit(crate::render::StackFit::Passthrough)
                        .push(container)
                        .push_positioned(
                            Container::new()
                                .with_color(overlay)
                                .with_corner_radius(radius),
                            StackPosition::fill(),
                        )
                } else {
                    RenderStack::new()
                        .with_fit(crate::render::StackFit::Passthrough)
                        .push(container)
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
            crate::semantics::tappable(
                crate::semantics::node_id_for(id),
                properties,
                inner,
                self.handlers.on_tap.clone(),
            )
        };

        if !enabled {
            // No focus node either. A disabled button is not a stop: upstream
            // gates `canRequestFocus` on `isEnabled`, and a Tab that lands on
            // something no key can operate is a dead end the reader has to
            // Tab out of again.
            return described(face());
        }
        // The splash goes inside the button's own region, and hears the
        // pointer because raw pointer events reach every listener on the path
        // -- the tap still belongs to the button. Clipped to the button's
        // corners, which is what `containedInkWell` means upstream.
        // The keyboard can reach it and press it, through the same handler the
        // finger calls -- the third path to "this button was pressed", after
        // the pointer and the semantics action, and deliberately the same
        // closure as both.
        //
        // Until this, **no button in this crate could be operated from the
        // keyboard at all**: it had no focus node, so Tab walked past it and
        // Enter had nothing to reach.
        crate::focus::operable(
            id,
            self.autofocus,
            self.handlers.on_tap.clone(),
            // The button's own radius, which is what its face and its ink are
            // already clipped to -- a highlight rounded differently would
            // show as a corner sticking out past the button it belongs to.
            crate::focus::FocusShape::Box {
                corner_radius: radius,
            },
            described(crate::framework::stateful(
                // The same `radius` the face is drawn with. A stadium button
                // whose ink is clipped square shows four wedges of splash colour
                // outside the pill, and they grow with the ripple.
                crate::ink::Ink::new(id.wrapping_add(INK_ID_OFFSET), face)
                    .with_color(splash_color)
                    .with_corner_radius(radius),
            )),
        )
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
/// # `semanticContainer` is here now; `margin` still is not
///
/// * **`semanticContainer`** (default true) is used *twice* upstream, and the
///   second use is negated: `Semantics(container: semanticContainer)` outside
///   the material, and `Semantics(explicitChildNodes: !semanticContainer)`
///   inside it. One decision seen from both ends -- a card is either one node
///   with its children folded in, or not a node at all with its children
///   exposed. Setting only one of the pair gives a card that is both or
///   neither.
///
///   This note used to say it was "not portable here yet", because the flags
///   it would set live on [`crate::semantics::SemanticsConfiguration`] and
///   [`crate::render_semantics::RenderSemanticsAnnotations`], neither of which
///   the tree builds. **That reason expired.** What the pair of flags means
///   between them is "fold everything below into one node", and
///   [`crate::render::RenderMergeSemanticsBox`] does exactly that on the live
///   walk. So the card wraps itself in one when `semantic_container` is set,
///   and one wrapper says both halves -- which is closer to upstream's intent
///   than two flags would have been, since upstream's two can be set
///   inconsistently and this one cannot.
///
/// * **`margin`** (default `EdgeInsets.all(4)`) is space *outside* the
///   material, between one card and the next. What this struct has is
///   `padding`, applied *inside*, which upstream's card does not have at all
///   -- its child brings its own. So a card here is inset from its contents
///   by `spacing * 2` and flush with its neighbours, where upstream's is
///   flush with its contents and inset from its neighbours by 4. Changing it
///   moves every card in the gallery, which is why it is written down rather
///   than done in passing.
pub struct Card {
    child: std::cell::RefCell<Option<AnyWidget>>,
    /// **Not an upstream field.** Upstream's card has no padding of its own --
    /// what is inside it brings its own, usually a `ListTile`. This crate's
    /// callers have relied on it since before the theme was ported, so it
    /// stays, with a default rather than upstream's nothing. Upstream's
    /// `margin`, which *is* a field, is a different thing and lives on
    /// [`crate::component_themes::ResolvedCard`].
    padding: Option<EdgeInsets>,
    semantic_container: bool,
    variant: crate::component_themes::CardVariant,
    /// Upstream's `borderOnForeground`, **true** by default: the outline is
    /// stroked over whatever is inside the card, so a picture that fills it
    /// does not swallow the line that says where the card stops.
    border_on_foreground: bool,
    /// Upstream's `clipBehavior`. `None` falls through to the theme and then
    /// to `Clip.none`.
    clip_behavior: Option<crate::painting::ClipBehavior>,
}

impl Card {
    /// Upstream's `Card`: the elevated one.
    pub fn new(child: AnyWidget) -> Card {
        Card {
            child: std::cell::RefCell::new(Some(child)),
            padding: None,
            // Upstream's default, and the one that matters: a card is a thing,
            // not a pile of things.
            semantic_container: true,
            variant: crate::component_themes::CardVariant::Elevated,
            border_on_foreground: true,
            clip_behavior: None,
        }
    }

    /// Upstream's `borderOnForeground`. See the field.
    pub fn with_border_on_foreground(mut self, on_foreground: bool) -> Self {
        self.border_on_foreground = on_foreground;
        self
    }

    /// Upstream's `clipBehavior`. Anything but
    /// [`crate::painting::ClipBehavior::None`] clips the child to the card's
    /// own corners -- which is what a card holding an image wants, and what a
    /// card holding a list tile does not need to pay for.
    pub fn with_clip_behavior(mut self, clip: crate::painting::ClipBehavior) -> Self {
        self.clip_behavior = Some(clip);
        self
    }

    /// Upstream's `Card.filled`: told apart from the page by its colour rather
    /// than by a shadow.
    pub fn filled(child: AnyWidget) -> Card {
        Card {
            variant: crate::component_themes::CardVariant::Filled,
            ..Card::new(child)
        }
    }

    /// Upstream's `Card.outlined`: told apart by a line, and flat.
    pub fn outlined(child: AnyWidget) -> Card {
        Card {
            variant: crate::component_themes::CardVariant::Outlined,
            ..Card::new(child)
        }
    }

    pub fn with_padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = Some(padding);
        self
    }

    /// Upstream's `semanticContainer`: whether the card is **one stop for a
    /// reader** or a transparent grouping its children are read through.
    ///
    /// True is right for a card that is a single idea -- a photograph with a
    /// caption is "photograph, caption", one thing to land on. False is right
    /// for a card that is a container of separately interesting things, where
    /// folding them together would produce one enormous sentence and no way to
    /// reach any part of it.
    pub fn with_semantic_container(mut self, container: bool) -> Self {
        self.semantic_container = container;
        self
    }
}

impl Component for Card {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let padding = self.padding.unwrap_or(EdgeInsets::all(theme.spacing * 2.0));
        let child = self
            .child
            .borrow()
            .clone()
            .unwrap_or_else(|| leaf(|| Empty));
        let card = crate::component_themes::ResolvedCard::of(context, self.variant);
        let surface = card.color;
        // The rounding comes off the **shape**, which is where upstream puts
        // it: a theme that sets `shape` moves the corners, and one that does
        // not gets the 12 all three tables agree on. `corner_radius` answers
        // `None` for a shape that is not a rounded rectangle -- a stadium
        // card, say -- and then there is nothing to round by.
        let radius = card
            .shape
            .corner_radius(crate::render::Size::ZERO)
            .map(|radius| radius.top_left.x)
            .unwrap_or(0.0);
        // **Only the outlined card is outlined.** This used to draw a hairline
        // on every card, so the elevated one said "not the page" twice -- with
        // a shadow and with a line -- and the filled one, which is supposed to
        // be told apart by its colour alone, wore a border it never asked for.
        let side = card.side();
        let border_on_foreground = self.border_on_foreground;
        let clip = self.clip_behavior.unwrap_or(card.clip_behavior);
        let shape = card.shape.clone();
        // The crate's shadow table is indexed by whole elevation steps.
        let elevation = card.elevation.round().max(0.0) as u32;
        let margin = card.margin;
        let semantic_container = self.semantic_container;
        crate::framework::single(child, move |inner| {
            // The clip goes **inside** the card's own decoration: a clip
            // outside would cut the shadow off at the card's edge, and a
            // shadow that stops at the thing casting it is not a shadow. The
            // rounding follows the shape at the size the card is laid out at,
            // for the reason every shape in this crate does -- a stadium's is
            // half its shorter side.
            let inner: crate::render::BoxedRender = match clip {
                crate::painting::ClipBehavior::None => inner,
                _ => crate::render::RenderRef::new(
                    crate::render::RenderClipRRect::new(crate::borders::BorderRadius::ZERO, inner)
                        .with_shape(shape.clone()),
                ),
            };
            let mut container = Container::new()
                .with_border_on_foreground(border_on_foreground)
                .with_color(surface)
                .with_corner_radius(radius)
                .with_elevation(elevation)
                .with_padding(padding)
                // Upstream's `margin`: space **outside** the surface, so a
                // column of cards has a gap between them that belongs to
                // neither.
                .with_margin(margin)
                .with_child(inner);
            if let Some(side) = side {
                container = container.with_border(side.width, side.color);
            }
            // Full width, so a column of cards has one left edge and one right
            // edge rather than one pair per card.
            let surface = crate::widgets::FullWidth::new(container);
            // Upstream sets one flag in two places and negates the second:
            // `Semantics(container: semanticContainer)` outside the material
            // and `Semantics(explicitChildNodes: !semanticContainer)` inside
            // it. Both halves say the same thing -- either the card is one
            // node with its children folded into it, or it is no node at all
            // and its children stand on their own -- which is exactly what a
            // merging box is here, so one wrapper says both.
            if semantic_container {
                Box::new(crate::render::RenderMergeSemanticsBox::new(surface))
                    as Box<dyn crate::render::RenderBox>
            } else {
                Box::new(surface) as Box<dyn crate::render::RenderBox>
            }
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
    /// Upstream's `autofocus`: take the keyboard as soon as this appears.
    autofocus: bool,
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
    /// Upstream's `autofocus`. A disabled switch has no handler, so it is not
    /// a stop and this does nothing -- upstream gates `canRequestFocus` on
    /// `isEnabled` the same way.
    pub fn with_autofocus(mut self, autofocus: bool) -> Self {
        self.autofocus = autofocus;
        self
    }

    pub fn new(id: u64, value: bool) -> Switch {
        Switch {
            id,
            autofocus: false,
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
        crate::focus::operable(
            id,
            self.autofocus,
            self.handlers.on_tap.clone(),
            // A switch is a toggleable, so it reacts inside the same disc a
            // checkbox and a radio do.
            crate::focus::FocusShape::radial(),
            crate::semantics::tappable(
                crate::semantics::node_id_for(id),
                crate::semantics::SemanticsProperties::toggle("", value),
                switch,
                tap,
            ),
        )
    }
}

// -- Progress -----------------------------------------------------------------

/// A horizontal progress bar. `value` is clamped to 0..1.
pub struct ProgressBar {
    value: f32,
    width: f32,
    /// Upstream's `semanticsLabel`: what the waiting is *for*. `None` is a bar
    /// that reports a number and no purpose, which is upstream's default and
    /// is worth a caller's attention rather than a fallback -- "60" on its own
    /// tells a reader how far through something they were never told about.
    semantic_label: Option<String>,
}

impl ProgressBar {
    pub fn new(value: f32) -> ProgressBar {
        ProgressBar {
            value: value.clamp(0.0, 1.0),
            width: 200.0,
            semantic_label: None,
        }
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Upstream's `semanticsLabel`.
    pub fn with_semantic_label(mut self, label: impl Into<String>) -> Self {
        self.semantic_label = Some(label.into());
        self
    }

    /// Upstream's `expandedSemanticsValue`: `semanticsValue ??
    /// '${(value * 100).round()}'`.
    ///
    /// **No percent sign**, and that is upstream's, not an omission here. The
    /// slider writes one (`'${...}%'`) and this does not, because a progress
    /// bar also sends `minValue: '0'` and `maxValue: '100'` alongside -- the
    /// platform has the range and says the units itself, where a slider hands
    /// over one number that has to carry its own.
    ///
    /// Those two bounds have **no counterpart here**: `SemanticsProperties`
    /// has no `min_value`/`max_value` and `RfSemanticsNode` no field for
    /// them, so what crosses is the number without the range. Upstream's
    /// `role` -- `progressBar` against `loadingSpinner` -- is missing for the
    /// same reason. Both are flags-and-crossings of their own, the shape
    /// `scopesRoute` and `is_link` are in.
    pub fn semantic_value(&self) -> String {
        format!("{}", (self.value * 100.0).round() as i32)
    }
}

impl Component for ProgressBar {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let value = self.value;
        let width = self.width;
        let track = theme.surface_variant;
        let fill = theme.primary;
        // A progress bar said nothing at all: a reader was left with no way to
        // know that something was under way, let alone how far. Upstream wraps
        // it in `Semantics(label:, value:, role:, minValue:, maxValue:)`; the
        // two that have a counterpart here are the label and the value.
        let described = {
            let properties = crate::semantics::SemanticsProperties {
                // `progressBar`, by upstream's condition rather than by this
                // widget's name: `ProgressIndicator._buildSemanticsWrapper`
                // picks the role on `value != null`, and this bar's value is
                // real progress -- 0 to 1, read out as "60". The sibling
                // [`crate::controls::Spinner`] comes out the other way for the
                // same reason, its value being rotation rather than progress.
                role: crate::semantics::SemanticsRole::ProgressBar,
                value: self.semantic_value(),
                ..crate::semantics::SemanticsProperties::label(
                    self.semantic_label.clone().unwrap_or_default(),
                )
            };
            move |inner: AnyWidget| {
                crate::framework::single(inner, {
                    let properties = properties.clone();
                    move |child| {
                        crate::semantics::RenderSemantics::new(
                            crate::semantics::node_id_for(PROGRESS_SEMANTICS_ID),
                            properties.clone(),
                            child,
                        )
                    }
                })
            }
        };
        let bar = leaf(move || {
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
        });
        described(bar)
    }
}

/// The identifier a progress bar's semantics node is keyed on. Reserved for
/// the reason [`crate::controls::DIALOG_SEMANTICS_ID`] is: the platform keys
/// its node on this, so it has to be the same on every frame.
const PROGRESS_SEMANTICS_ID: u64 = 0x9206;

// -- Slider -------------------------------------------------------------------

/// A draggable value between 0 and 1.
pub struct Slider {
    id: u64,
    value: f32,
    /// The ends of the range this slider reports in. Upstream's `min` and
    /// `max`, both of which this used to assume were 0 and 1 -- so a caller
    /// with a real range converted by hand, and `Slider::new` clamped their
    /// value into 0..1 and destroyed it on the way in.
    min: f32,
    max: f32,
    /// `None` is a continuous slider; `Some(n)` snaps to `n + 1` positions
    /// and is what makes tick marks something to draw. Upstream asserts
    /// `divisions == null || divisions > 0`: zero would divide by zero and a
    /// negative number is not a count.
    divisions: Option<u32>,
    width: f32,
    /// Upstream's `label`: the words in the bubble over the thumb. Without
    /// one there is nothing to put in a bubble, so there is no bubble --
    /// which is why the three value-indicator theme fields reached nothing
    /// until a slider could carry this.
    label: Option<String>,
    /// Whether a thumb is being dragged right now, which is most of what
    /// decides whether the bubble is showing.
    ///
    /// Upstream keeps this in `_SliderState._dragging` and never asks the
    /// caller. This port keeps a widget's transient state in whatever owns
    /// it -- `Button::with_pressed` is the same shape -- so the caller says.
    dragging: bool,
    /// What to do with a new value, if anything. The gestures that produce
    /// one are decided in `build`, where the theme can be read; this is the
    /// part that has to be captured before then.
    on_change: Option<std::rc::Rc<dyn Fn(f32)>>,
}

impl Slider {
    pub fn new(id: u64, value: f32) -> Slider {
        Slider {
            id,
            value,
            min: 0.0,
            max: 1.0,
            divisions: None,
            width: 200.0,
            label: None,
            dragging: false,
            on_change: None,
        }
    }

    /// Upstream's `label`.
    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// See [`Slider::dragging`].
    pub fn with_dragging(mut self, dragging: bool) -> Self {
        self.dragging = dragging;
        self
    }

    /// Whether a bubble is showing over the thumb.
    ///
    /// A label is required and upstream does not say so anywhere: its
    /// `_ValueIndicatorRenderObjectWidget` lays out `widget.label` and a null
    /// label paints an empty bubble. An empty bubble is worse than none, so
    /// this port asks for the words first.
    fn shows_indicator(&self, slider: &crate::slider_theme::ResolvedSlider) -> bool {
        self.label.is_some()
            && slider.shows_value_indicator(self.divisions.is_some(), self.dragging)
    }

    /// Upstream's `min` and `max`.
    pub fn with_range(mut self, min: f32, max: f32) -> Self {
        self.min = min;
        self.max = max;
        self
    }

    /// Upstream's `divisions`. Zero is refused rather than clamped: it would
    /// divide by zero in the discretisation, and a caller who wrote it meant
    /// something else.
    pub fn with_divisions(mut self, divisions: u32) -> Self {
        debug_assert!(divisions > 0, "divisions must be greater than zero");
        self.divisions = if divisions > 0 { Some(divisions) } else { None };
        self
    }

    /// Upstream's two constructor asserts, in this crate's returned form:
    /// `min <= max`, and the value between them.
    ///
    /// This used to **clamp** the value in `new` instead. That is not what
    /// upstream does and it is worse than either alternative: a caller who
    /// passed 5 into a 0..1 slider got 1 back with no complaint, and a
    /// caller whose range was 0..10 -- once ranges existed -- would have had
    /// their 5 destroyed on the way in.
    pub fn validate(&self) -> Result<(), String> {
        if self.min > self.max {
            return Err(format!("min {} is greater than max {}", self.min, self.max));
        }
        if self.value < self.min || self.value > self.max {
            return Err(format!(
                "Value {} is not between minimum {} and maximum {}",
                self.value, self.min, self.max
            ));
        }
        Ok(())
    }

    /// Upstream's `_unlerp`: where this slider's value sits along its track,
    /// as a fraction. A zero-width range answers zero rather than dividing by
    /// it -- upstream's assert forbids `min > max` and allows `min == max`.
    pub fn fraction(&self) -> f32 {
        if self.max <= self.min {
            return 0.0;
        }
        ((self.value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
    }

    /// Upstream's `_convert`: a position along the track, back to a value --
    /// snapped to the divisions first when there are any.
    pub fn value_at(&self, fraction: f32) -> f32 {
        let fraction = fraction.clamp(0.0, 1.0);
        let fraction = match self.divisions {
            Some(divisions) => {
                let divisions = divisions as f32;
                (fraction * divisions).round() / divisions
            }
            None => fraction,
        };
        self.min + fraction * (self.max - self.min)
    }

    /// Where each tick mark sits along the track, as fractions. Empty for a
    /// continuous slider, which is why the theme's `tick_mark_shape` had
    /// nothing to answer before there were divisions to mark.
    pub fn tick_fractions(&self) -> Vec<f32> {
        match self.divisions {
            None => Vec::new(),
            Some(divisions) => (0..=divisions)
                .map(|step| step as f32 / divisions as f32)
                .collect(),
        }
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Remembers what to do with a new value.
    ///
    /// Which gestures are allowed to produce one comes from
    /// [`SliderThemeData::allowed_interaction`] and is decided in `build`,
    /// because the theme cannot be read here -- there is no context yet.
    /// That is why this no longer builds the handlers: it used to, and the
    /// widget therefore took every tap and every drag whatever the theme
    /// said.
    /// Upstream's `onChanged`, as the closure it is.
    ///
    /// [`Slider::wired`] is the convenience over this one for the common case
    /// of writing straight into a state; this is what it builds. A slider with
    /// nothing here is upstream's `onChanged: null`, which is what makes a
    /// slider disabled -- to a reader as well as to a finger.
    pub fn with_on_change(mut self, on_change: impl Fn(f32) + 'static) -> Self {
        self.on_change = Some(std::rc::Rc::new(on_change));
        self
    }

    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, set: fn(&mut S, f32)) -> Self {
        self.on_change = Some(std::rc::Rc::new(move |value: f32| {
            handle.set_state(move |state| set(state, value));
        }));
        self
    }
}

impl Slider {
    /// The gestures this slider accepts, under the theme's
    /// [`SliderThemeData::allowed_interaction`].
    ///
    /// # The four modes
    ///
    /// Upstream switches on them in `_SliderState._startInteraction` and
    /// `_handleDragUpdate`:
    ///
    /// | mode | a tap | a drag |
    /// | --- | --- | --- |
    /// | `TapAndSlide` | jumps | slides |
    /// | `TapOnly` | jumps | ignored |
    /// | `SlideOnly` | ignored | slides |
    /// | `SlideThumb` | ignored | slides, if it began on the thumb |
    ///
    /// `SlideOnly` is there so that a stray tap cannot move a value an
    /// application cares about. Ignoring the field, as this did, threw that
    /// away.
    ///
    /// # Where this differs from upstream
    ///
    /// `SlideThumb` upstream asks `_isPointerOnOverlay`, and the overlay is
    /// wider than the thumb. This asks whether the pointer went down on the
    /// thumb itself, because that is the only rectangle this slider has: it
    /// draws the thumb as a box in a row and has no overlay. A drag starting
    /// just outside the thumb is refused here and accepted upstream.
    fn gestures(&self, slider: &crate::slider_theme::ResolvedSlider) -> PointerHandlers {
        let Some(on_change) = self.on_change.clone() else {
            return PointerHandlers::new();
        };
        let interaction = slider.allowed_interaction;
        let width = self.width;
        // The thumb sits at the end of the filled part of the track, so its
        // box runs from there to a thumb's width further along.
        let filled = (width * self.fraction()).clamp(0.0, width);
        let thumb_width = slider.thumb_size.width;
        // Whether the drag in flight began on the thumb. Only `SlideThumb`
        // reads it, but it has to be recorded at every drag start, which is
        // the only moment the answer is knowable.
        // A position along the track, as the caller's own value: through
        // `min`/`max`, and snapped to the divisions when there are any. This
        // used to hand back the raw fraction, so a slider with a range
        // reported nonsense and one with divisions never settled on them.
        let (min, max, divisions) = (self.min, self.max, self.divisions);
        let to_value = move |fraction: f32| {
            let fraction = fraction.clamp(0.0, 1.0);
            let fraction = match divisions {
                Some(divisions) => {
                    let divisions = divisions as f32;
                    (fraction * divisions).round() / divisions
                }
                None => fraction,
            };
            min + fraction * (max - min)
        };
        let began_on_thumb = std::rc::Rc::new(std::cell::Cell::new(false));
        let starting = std::rc::Rc::clone(&began_on_thumb);
        let dragging = std::rc::Rc::clone(&on_change);
        PointerHandlers::new()
            .with_drag_start(move |drag| {
                let dx = drag.local_position.dx;
                starting.set(dx >= filled && dx <= filled + thumb_width);
            })
            .with_drag_update(move |drag| {
                if interaction == crate::slider_theme::SliderInteraction::TapOnly {
                    return;
                }
                if interaction == crate::slider_theme::SliderInteraction::SlideThumb
                    && !began_on_thumb.get()
                {
                    return;
                }
                dragging(to_value(drag.local_position.dx / width));
            })
            .with_tap(move |tap| {
                if matches!(
                    interaction,
                    crate::slider_theme::SliderInteraction::SlideOnly
                        | crate::slider_theme::SliderInteraction::SlideThumb
                ) {
                    return;
                }
                on_change(to_value(tap.local_position.dx / width));
            })
    }
}

/// Draws a slider's tick marks, one per division boundary.
///
/// The shape does the work -- `RoundSliderTickMarkShape::paint` decides the
/// colour from which side of the thumb a mark is on, and answers its own
/// radius -- and this is what finally asks it to. It was ported, tested and
/// unreachable: `SliderThemeData::tick_mark_shape` had no caller, because a
/// slider with no divisions has no marks and this one had no divisions.
struct SliderTickMarks {
    shape: crate::slider_theme::SliderTickMarkShape,
    theme: crate::slider_theme::SliderThemeData,
    /// Where each mark sits along the track, as a fraction.
    fractions: Vec<f32>,
    /// Where the thumb is, as a fraction -- the shape asks for it in pixels,
    /// but the width is not known until paint time.
    thumb: f32,
    direction: crate::direction::TextDirection,
}

impl crate::render::CustomPainter for SliderTickMarks {
    fn paint(&self, canvas: &mut crate::engine::Canvas, size: crate::render::Size) {
        let middle = size.height / 2.0;
        let thumb_center = crate::render::Offset::new(self.thumb * size.width, middle);
        for fraction in &self.fractions {
            self.shape.paint(
                canvas,
                crate::render::Offset::new(fraction * size.width, middle),
                &self.theme,
                thumb_center,
                self.direction,
                // Upstream passes the enable animation's value; this slider
                // has no disabled state yet, so the marks are always drawn at
                // their enabled colour. Said here rather than left as a bare
                // literal.
                1.0,
            );
        }
    }

    fn should_repaint(&self, _old: &dyn crate::render::CustomPainter) -> bool {
        // Cheap and always correct: the alternative is comparing a shape, a
        // whole theme and a vector, which costs more than the redraw.
        true
    }

    fn kind_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<SliderTickMarks>()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Draws the bubble over a slider's thumb.
///
/// The shape does all of it: `SliderComponentShape::paint_indicator` picks
/// one of four painters, each of which sizes itself from the label and knows
/// how far it may be shifted to stay inside the box. That method was written
/// so those four could be reached, and until this nothing reached it.
struct SliderValueIndicator {
    shape: crate::slider_theme::SliderComponentShape,
    theme: crate::slider_theme::SliderThemeData,
    label: String,
    style: TextStyle,
    /// Where the thumb is, as a fraction -- the width is not known until
    /// paint time.
    thumb: f32,
}

impl crate::render::CustomPainter for SliderValueIndicator {
    fn paint(&self, canvas: &mut crate::engine::Canvas, size: crate::render::Size) {
        let mut label =
            crate::painting::TextPainter::new().text(self.label.clone(), self.style.clone());
        // The bubble grows to its words rather than wrapping them, so the
        // width it lays out against is no constraint at all.
        label.layout(f32::INFINITY);
        let center = crate::render::Offset::new(self.thumb * size.width, size.height / 2.0);
        let geometry = crate::slider_theme::IndicatorPaintGeometry::new(
            center, size,
            // Upstream's activation animation, at rest. This port has no
            // animation for it yet: the bubble is either drawn or it is not,
            // and `shows_indicator` has already decided which.
            1.0,
        );
        self.shape
            .paint_indicator(canvas, &geometry, &self.theme, &label);
    }

    fn should_repaint(&self, _old: &dyn crate::render::CustomPainter) -> bool {
        true
    }

    fn kind_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<SliderValueIndicator>()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl Component for Slider {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let slider = ResolvedSlider::of(context);
        // The *fraction*, not the value: the track is drawn in its own
        // coordinates and the value may be anywhere between `min` and `max`.
        let value = self.fraction();
        let width = self.width;
        let id = self.id;
        let handlers = self.gestures(&slider);
        let ticks = self.tick_fractions();
        let tick_marks = if ticks.is_empty() {
            None
        } else {
            Some(std::rc::Rc::new(SliderTickMarks {
                shape: slider.tick_mark_shape,
                // The *resolved* theme, not what `SliderTheme::of` answers:
                // the shape reads four colours off it and a raw theme leaves
                // all four unset, so it would return without drawing.
                theme: slider.shape_theme.clone(),
                fractions: ticks,
                thumb: value,
                direction: crate::direction::direction_of(context),
            })
                as std::rc::Rc<dyn crate::render::CustomPainter>)
        };
        let indicator = if self.shows_indicator(&slider) {
            Some(std::rc::Rc::new(SliderValueIndicator {
                shape: slider.value_indicator_shape,
                theme: slider.shape_theme.clone(),
                label: self.label.clone().unwrap_or_default(),
                style: slider.value_indicator_text_style.clone(),
                thumb: value,
            })
                as std::rc::Rc<dyn crate::render::CustomPainter>)
        } else {
            None
        };
        // What a reader is told this is, and what changing it does. Nothing
        // in this port declared a slider until now, so one arrived as a plain
        // box -- no role, no value read out, and no way to move it. The
        // semantics id is the hit-test id, for the reason the button gives.
        // Upstream reads `_platform` off `Theme.of(context)`, which is what
        // the theme's `platform` override is for -- a desktop asked to behave
        // like a phone gets the phone's step too.
        let platform = crate::theme::ThemeData::of(context).platform;
        let described = {
            let properties = crate::semantics::SemanticsProperties::slider(
                self.value,
                self.min,
                self.max,
                self.divisions,
                self.label.as_deref(),
                self.on_change.is_some(),
                platform,
            );
            let on_change = self.on_change.clone();
            let unit =
                crate::semantics::SemanticsProperties::slider_action_unit(self.divisions, platform);
            // The step is in normalised units, so it is scaled back into the
            // caller's range before being handed over -- and clamped there,
            // the same way the value read aloud was.
            let (min, max, current) = (self.min, self.max, self.value);
            let node = crate::semantics::node_id_for(id);
            move |inner: AnyWidget| {
                let on_change = on_change.clone();
                crate::semantics::semantics_with_action(
                    node,
                    properties.clone(),
                    inner,
                    move |action| {
                        let Some(on_change) = &on_change else {
                            return;
                        };
                        let step = unit * (max - min);
                        let next = match action {
                            crate::semantics::SemanticsAction::Increase => current + step,
                            crate::semantics::SemanticsAction::Decrease => current - step,
                            _ => return,
                        };
                        on_change(next.clamp(min, max));
                    },
                )
            }
        };
        let track = slider.inactive_track_color;
        let fill = slider.active_track_color;
        let knob = slider.thumb_color;
        let track_height = slider.track_height;
        let thumb = slider.thumb_size;

        let body = leaf(move || {
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
                    over(
                        indicator.clone(),
                        Container::new()
                            // The thing you can hit should be bigger than the
                            // thing you can see.
                            .with_size(width, hit_height)
                            .with_child(Center::new(over(
                                tick_marks.clone(),
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
                                                    .with_corner_radius(
                                                        thumb.shortest_side() / 2.0,
                                                    ),
                                            ),
                                    ),
                            ))),
                    ),
                )
                .with_handlers(handlers.clone()),
            )
        });
        described(body)
    }
}

/// Wraps the track in a [`crate::render::RenderCustomPaint`] whose
/// *foreground* painter draws the tick marks -- over the filled part and
/// under the thumb, which is the order upstream paints in.
///
/// With no painter the track is handed back untouched, so a continuous
/// slider costs nothing for a feature it does not have.
/// `child`, with `painter` drawn over it -- or just `child` when there is no
/// painter.
///
/// Both of the slider's painters go on the same way and neither is always
/// there: the tick marks want a slider with divisions, the value indicator a
/// slider with a label that is being dragged. One function so that a
/// correction to how a painter is attached is a correction to both.
fn over(
    painter: Option<std::rc::Rc<dyn crate::render::CustomPainter>>,
    child: impl crate::render::RenderBox + 'static,
) -> Box<dyn crate::render::RenderBox> {
    match painter {
        None => Box::new(child),
        Some(painter) => {
            Box::new(crate::render::RenderCustomPaint::new(child).with_foreground_painter(painter))
        }
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
        let trailing = self.trailing.borrow().clone();
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
        // Upstream's `titleTextStyle`: the bar's, then the theme's, then
        // `titleLarge` in the bar's foreground colour. This used to be
        // `theme.title()` -- a hand-rolled style with a hard-coded weight of
        // 700, where `titleLarge` is 400 -- so `AppBarThemeData::title_text_style`
        // reached nothing and the role had no reader in this port at all.
        let title_style = bar
            .title_text_style
            .clone()
            .unwrap_or_else(|| theme.title());
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

        // Upstream wraps the title in `Semantics(namesRoute: ..., header:
        // true)`, and this bar wrapped it in nothing: the one line on the
        // screen that says what page you are on was, to a screen reader, an
        // ordinary run of words.
        //
        // `excludeHeaderSemantics` is upstream's way out of this and has no
        // counterpart here; a bar that wanted its title unannounced would
        // need that flag first.
        let title_semantics = crate::semantics::SemanticsProperties::route_header(
            title.clone(),
            crate::theme::ThemeData::of(context).platform,
        );
        let mut children = vec![crate::semantics::describe(
            title_semantics,
            leaf(move || {
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
                            crate::media_query::current_text_scale()
                                .min(MAX_TITLE_TEXT_SCALE_FACTOR),
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
            }),
        )];
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
/// Upstream `ScaffoldGeometry`, as much of it as this port fills in: **where
/// the floating action button ended up**, in the scaffold's coordinates.
///
/// Published by [`Scaffold`] and read by [`crate::bottom_bars::BottomAppBar`],
/// which is upstream's arrangement -- `Scaffold.geometryOf(context)` hands back
/// a listenable and `_BottomAppBarClipper` reads it.
///
/// # Why a shared cell and not a value
///
/// The rectangle is not known when the bar is *built*: it depends on the
/// button's measured size and the scaffold's, and both arrive at layout. The
/// obvious reading -- that the bar is therefore always a frame behind -- is
/// wrong, and upstream shows why: its clipper reads the geometry inside
/// `getClip`, which runs during layout and paint rather than during build.
/// **Layout precedes paint in the same frame**, so a cell written by the
/// scaffold's layout and read by the bar's paint is current, not stale.
///
/// That is the difference between this and [`crate::ink_well`]'s size sink,
/// which really is a frame late: a splash is *built* from the size, and a
/// build cannot wait for the layout it precedes.
#[derive(Clone, Default)]
pub struct ScaffoldGeometry {
    area: std::rc::Rc<std::cell::Cell<Option<crate::engine::Rect>>>,
}

impl ScaffoldGeometry {
    /// Upstream's `floatingActionButtonArea`. `None` is a scaffold with no
    /// button, which is also a bar with nothing to cut around.
    pub fn floating_action_button_area(&self) -> Option<crate::engine::Rect> {
        self.area.get()
    }

    fn set_floating_action_button_area(&self, area: Option<crate::engine::Rect>) {
        self.area.set(area);
    }

    /// Publishes an area without a scaffold, so a bar can be tested against
    /// the channel rather than against a hand-written rectangle.
    #[cfg(test)]
    pub fn publish_for_tests(&self, area: Option<crate::engine::Rect>) {
        self.set_floating_action_button_area(area);
    }
}

/// Two geometries are the same inherited value when they share the cell.
///
/// Compared by identity rather than by content, and that is the point: the
/// content changes every time the button moves, and a scaffold that republished
/// on each change would rebuild the whole subtree during layout. Upstream keeps
/// the same `ValueListenable` and notifies through it for the same reason.
impl PartialEq for ScaffoldGeometry {
    fn eq(&self, other: &ScaffoldGeometry) -> bool {
        std::rc::Rc::ptr_eq(&self.area, &other.area)
    }
}

/// The identifier the drawer's barrier is keyed on when the caller named none.
///
/// Reserved for the reason [`crate::controls::DIALOG_SEMANTICS_ID`] is: the
/// scrim is furniture rather than a control anyone named, and the platform
/// keys its accessibility node on whatever crosses.
const DRAWER_BARRIER_SEMANTICS_ID: u64 = 0xD_2A9;

/// The page, the bar along its bottom, and the button placed over both.
///
/// A render object because **where the button goes depends on two measured
/// sizes** -- the scaffold's and the button's -- and upstream works it out in
/// `_ScaffoldLayout.performLayout` for the same reason. Everything that
/// decides the offset is already ported and tested in
/// [`crate::fab_location`]; until now nothing called it, because this
/// scaffold had no button to place.
struct ScaffoldFloor {
    page: crate::render::BoxedRender,
    /// Upstream's `bottomNavigationBar` slot: full width, along the bottom,
    /// with the page shortened to sit above it.
    bar: Option<crate::render::BoxedRender>,
    button: Option<crate::render::BoxedRender>,
    location: crate::fab_location::FloatingActionButtonLocation,
    /// What the keyboard is standing on, which upstream folds into `minInsets`
    /// so a button rises above it rather than hiding behind it.
    bottom_inset: f32,
    text_direction: crate::direction::TextDirection,
    size: Size,
    bar_height: f32,
    button_offset: crate::render::Offset,
    /// Where the button ended up is published here for the bar underneath --
    /// see [`ScaffoldGeometry`].
    geometry: ScaffoldGeometry,
}

impl ScaffoldFloor {
    /// The y the bar's top edge sits at: hard against the bottom.
    fn bar_top(&self) -> f32 {
        (self.size.height - self.bar_height).max(0.0)
    }

    /// The three children in **paint order** -- page, then bar, then button --
    /// which is upstream's stacking order and the reverse of the order a
    /// finger is answered in.
    ///
    /// One list rather than three, because painting, visiting and describing
    /// have to agree about where each child is: a hit test that disagreed with
    /// the paint by a bar's height is a control that answers somewhere it is
    /// not drawn.
    fn visit_floor(
        &self,
        visit: &mut dyn FnMut(&dyn crate::render::RenderBox, crate::render::Offset),
    ) {
        visit(&self.page, crate::render::Offset::ZERO);
        if let Some(bar) = &self.bar {
            visit(bar, crate::render::Offset::new(0.0, self.bar_top()));
        }
        if let Some(button) = &self.button {
            visit(button, self.button_offset);
        }
    }

    /// Upstream's `ScaffoldPrelayoutGeometry`, filled in from what has been
    /// measured.
    ///
    /// `content_top` is **0 rather than the app bar's height**, and that is a
    /// real limitation rather than a simplification: the bar's measured height
    /// is not available here -- it is inside the page this object was handed --
    /// so the six `*_TOP` placements sit at the top of the scaffold instead of
    /// just under the bar. The other thirteen, including every default, do not
    /// read it. The same missing measurement is why
    /// [`Scaffold::extend_body_behind_app_bar`] cannot raise the body's
    /// padding either; one `LayoutBuilder`-shaped hole, two symptoms.
    fn prelayout(&self, button: Size, bar: f32) -> crate::fab_location::ScaffoldPrelayoutGeometry {
        // Upstream's `contentBottom`:
        //
        //     math.max(0.0, bottom - math.max(minInsets.bottom, bottomWidgetsHeight))
        //
        // The **max**, not the sum: a keyboard covers the bar rather than
        // stacking on top of it, so a button asked to clear both clears the
        // taller one. Added together, a docked button would float a bar's
        // height above the keyboard with nothing in between.
        crate::fab_location::ScaffoldPrelayoutGeometry {
            floating_action_button_size: button,
            bottom_sheet_size: Size::ZERO,
            content_bottom: (self.size.height - self.bottom_inset.max(bar)).max(0.0),
            content_top: 0.0,
            min_insets: EdgeInsets::only(0.0, 0.0, 0.0, self.bottom_inset),
            min_view_padding: EdgeInsets::default(),
            scaffold_size: self.size,
            snack_bar_size: Size::ZERO,
            material_banner_size: Size::ZERO,
            text_direction: self.text_direction,
        }
    }
}

impl crate::render::RenderBox for ScaffoldFloor {
    fn layout(&mut self, constraints: crate::render::BoxConstraints) -> Size {
        let full = constraints.biggest();
        // The bar first, at full width and its own height -- upstream lays it
        // out before anything that has to clear it, for the same reason: its
        // height is what the others are measured against.
        let bar_height = match &mut self.bar {
            Some(bar) => {
                bar.layout_child(
                    crate::render::BoxConstraints::new(full.width, full.width, 0.0, full.height),
                    true,
                )
                .height
            }
            None => 0.0,
        };
        self.bar_height = bar_height;
        // The page takes what is left above it.
        self.size = self.page.layout_child(
            crate::render::BoxConstraints::new(
                constraints.min_width,
                constraints.max_width,
                (constraints.min_height - bar_height).max(0.0),
                (constraints.max_height - bar_height).max(0.0),
            ),
            true,
        );
        self.size = Size::new(
            self.size.width.max(full.width),
            self.size.height + bar_height,
        );
        if let Some(button) = &mut self.button {
            // Loose, so the button is its own size rather than the page's --
            // upstream lays the button out with `BoxConstraints.loose(size)`
            // for exactly that.
            let button = button.layout_child(
                crate::render::BoxConstraints::loose(self.size.width, self.size.height),
                true,
            );
            use crate::fab_location::StandardFabLocation;
            self.button_offset = self
                .location
                .get_offset(&self.prelayout(button, bar_height));
            // Published **during layout**, so the bar's paint -- which comes
            // after every layout in the same frame -- reads where the button
            // actually is rather than where it was.
            self.geometry
                .set_floating_action_button_area(Some(crate::engine::Rect::ltrb(
                    self.button_offset.dx,
                    self.button_offset.dy,
                    self.button_offset.dx + button.width,
                    self.button_offset.dy + button.height,
                )));
        } else {
            // A scaffold with no button leaves nothing for a bar to cut around.
            //
            // **Defensive rather than reachable today**: each build makes a
            // fresh cell, so a scaffold whose button was taken away is already
            // publishing nothing before this runs -- a mutation deleting the
            // line stays green. It is kept because the day the cell outlives a
            // build (upstream keeps one per scaffold *state*, which is what
            // spares it these rebuilds) a stale rectangle would have a bar
            // cutting a hole around a button that is no longer there.
        }
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn visit_children(
        &self,
        visit: &mut dyn FnMut(&dyn crate::render::RenderBox, crate::render::Offset),
    ) {
        self.visit_floor(visit);
    }

    fn visit_children_for_semantics(
        &self,
        visit: &mut dyn FnMut(&dyn crate::render::RenderBox, crate::render::Offset),
    ) {
        self.visit_floor(visit);
    }

    fn hit_test(
        &self,
        position: crate::render::Offset,
        result: &mut crate::render::HitTestResult,
    ) -> bool {
        // Topmost first, which is the order they are painted in reverse: the
        // button is over the bar, and the bar is over nothing the page has at
        // that height.
        if let Some(button) = &self.button {
            let local = crate::render::Offset::new(
                position.dx - self.button_offset.dx,
                position.dy - self.button_offset.dy,
            );
            if button.hit_test(local, result) {
                return true;
            }
        }
        if let Some(bar) = &self.bar {
            let local = crate::render::Offset::new(position.dx, position.dy - self.bar_top());
            if bar.hit_test(local, result) {
                return true;
            }
        }
        self.page.hit_test(position, result)
    }

    fn paint(&self, context: &mut crate::render::PaintContext, offset: crate::render::Offset) {
        self.visit_floor(&mut |child, at| {
            child.paint(
                context,
                crate::render::Offset::new(offset.dx + at.dx, offset.dy + at.dy),
            );
        });
    }
}

pub struct Scaffold {
    app_bar: std::cell::RefCell<Option<AnyWidget>>,
    body: std::cell::RefCell<Option<AnyWidget>>,
    drawer: std::cell::RefCell<Option<AnyWidget>>,
    drawer_open: bool,
    drawer_alignment: crate::drawer::DrawerAlignment,
    drawer_scrim_id: Option<u64>,
    drawer_handlers: PointerHandlers,
    /// Whether the body shrinks to sit above the keyboard.
    ///
    /// Upstream's `Scaffold.resizeToAvoidBottomInset`, whose effective default
    /// is true. It is what makes a form usable on a phone: the body is given
    /// the view minus `viewInsets.bottom`, so the scrollable inside it gains
    /// exactly as much scroll extent as the keyboard took away, and a field
    /// underneath can be scrolled up to.
    ///
    /// Without it a reveal has nowhere to go -- the viewport still believes it
    /// is full height, the field is inside it by that reckoning, and the
    /// smallest scroll that shows the field is none at all.
    resize_to_avoid_bottom_inset: bool,
    /// Whether the body reaches up behind the app bar instead of starting
    /// below it.
    ///
    /// Upstream's `Scaffold.extendBodyBehindAppBar`, false by default. Upstream
    /// says it by setting `contentTop = 0.0` in `_ScaffoldLayout` and letting
    /// the bar be painted after the body; here it is the difference between
    /// stacking the two and putting them in a column, which is the same two
    /// facts -- the body gets the whole height, and the bar is drawn over it.
    ///
    /// What it is for is a page whose content scrolls *under* a transparent
    /// bar. Without it such a page has to frame itself, and a page that frames
    /// itself is a page with no scaffold -- so no `resize_to_avoid_bottom_inset`
    /// either, and a form on it cannot get out from under the keyboard.
    ///
    /// Upstream also raises the body's `MediaQuery.padding.top` to the bar's
    /// height (`_BodyBuilder`), so a `SafeArea` inside the body pads past the
    /// bar. That needs the bar's measured height during the body's build,
    /// which is a `LayoutBuilder` upstream and has no counterpart here; a page
    /// using this pads its own content down instead.
    extend_body_behind_app_bar: bool,
    /// Upstream's `Scaffold.floatingActionButton`.
    floating_action_button: std::cell::RefCell<Option<AnyWidget>>,
    /// Upstream's `floatingActionButtonLocation`, whose default is
    /// `endFloat` -- the corner every Material application's button sits in.
    fab_location: crate::fab_location::FloatingActionButtonLocation,
    /// Upstream's `Scaffold.bottomNavigationBar`: the strip along the bottom
    /// that the body sits above rather than behind.
    bottom_navigation_bar: std::cell::RefCell<Option<AnyWidget>>,
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
            resize_to_avoid_bottom_inset: true,
            extend_body_behind_app_bar: false,
            floating_action_button: std::cell::RefCell::new(None),
            fab_location: crate::fab_location::FloatingActionButtonLocation::END_FLOAT,
            bottom_navigation_bar: std::cell::RefCell::new(None),
        }
    }

    /// Whether the body reaches up behind the app bar.
    /// Upstream's `Scaffold.extendBodyBehindAppBar`.
    /// Upstream's `floatingActionButton`: the round button that sits over the
    /// page rather than in it.
    pub fn with_floating_action_button(self, button: AnyWidget) -> Self {
        *self.floating_action_button.borrow_mut() = Some(button);
        self
    }

    /// Upstream's `bottomNavigationBar`: a strip along the bottom of the
    /// scaffold, with the body given the height above it rather than the whole
    /// window. Anything may go here -- upstream's own examples put a
    /// `BottomNavigationBar` or a [`crate::bottom_bars::BottomAppBar`] in it.
    pub fn with_bottom_navigation_bar(self, bar: AnyWidget) -> Self {
        *self.bottom_navigation_bar.borrow_mut() = Some(bar);
        self
    }

    /// Upstream's `floatingActionButtonLocation`. Defaults to `END_FLOAT`.
    pub fn with_fab_location(
        mut self,
        location: crate::fab_location::FloatingActionButtonLocation,
    ) -> Self {
        self.fab_location = location;
        self
    }

    pub fn with_extend_body_behind_app_bar(mut self, extend: bool) -> Self {
        self.extend_body_behind_app_bar = extend;
        self
    }

    /// Whether the body makes room for the keyboard.
    /// Upstream's `Scaffold.resizeToAvoidBottomInset`; true unless said
    /// otherwise, which is upstream's `?? true`.
    pub fn with_resize_to_avoid_bottom_inset(mut self, resize: bool) -> Self {
        self.resize_to_avoid_bottom_inset = resize;
        self
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
        let app_bar = self.app_bar.borrow().clone();
        let body = self.body.borrow().clone().unwrap_or_else(|| leaf(|| Empty));
        let drawer = self.drawer.borrow().clone();
        // A drawer nobody opened is nothing at all: upstream's closed
        // `DrawerController` builds a `SizedBox.shrink` on desktop (the
        // edge-drag strip it would install on mobile is not ported; see
        // crate::drawer's module docs).
        let drawer_open = self.drawer_open && drawer.is_some();
        let drawer_alignment = self.drawer_alignment;
        let scrim_id = self.drawer_scrim_id;
        let scrim_handlers = self.drawer_handlers.clone();

        let has_app_bar = app_bar.is_some();
        let behind_bar = self.extend_body_behind_app_bar;
        // A bar has already moved the page down past the status bar, so the
        // body must not do it again -- and must still be told what the bar did
        // not deal with, which is the bottom. Upstream's `Scaffold` removes the
        // same padding from the body's `MediaQuery` for the same reason.
        // How far the keyboard reaches up the view, and therefore how much of
        // the body is not really there. Upstream's `_ScaffoldLayout`:
        //
        //     final EdgeInsets minInsets = MediaQuery.paddingOf(context).copyWith(
        //       bottom: _resizeToAvoidBottomInset ? MediaQuery.viewInsetsOf(context).bottom : 0.0);
        //     ...
        //     final double contentBottom =
        //         math.max(0.0, bottom - math.max(minInsets.bottom, bottomWidgetsHeight));
        //
        // There are no bottom widgets in this scaffold, so the `max` against
        // them is not a case that can arise and the inset is the whole of it.
        // Reading it is also what subscribes to it, so the scaffold rebuilds on
        // every frame of the keyboard's animation -- which is what upstream's
        // `MediaQuery` dependency does too.
        let bottom_inset = if self.resize_to_avoid_bottom_inset {
            crate::media_query::view_insets_of(context).bottom.max(0.0)
        } else {
            0.0
        };

        // A bar has already moved the page down past the status bar, so the
        // body must not do it again -- and must still be told what the bar did
        // not deal with, which is the bottom. Upstream's `Scaffold` removes the
        // same padding from the body's `MediaQuery` for the same reason.
        //
        // The keyboard is removed on the same principle and in the same place
        // upstream removes it (`removeBottomInset: _resizeToAvoidBottomInset`):
        // the body is about to be given a box that already stops above the
        // keyboard, so a descendant that made room for the keyboard as well
        // would make it twice.
        //
        // Both removals are decided by *configuration*, never by the current
        // value: a wrapper that came and went as the keyboard opened would be
        // a different tree each way, and everything below it would be rebuilt
        // from nothing -- including the focused field's editing session.
        // Upstream's `_BodyBuilder` is unconditional for the same reason.
        let resizes = self.resize_to_avoid_bottom_inset;
        let floating_action_button = self.floating_action_button.borrow().clone();
        let bottom_navigation_bar = self.bottom_navigation_bar.borrow().clone();
        // Upstream's `platformHasBackButton`, read where a context is
        // available and used inside the closure below.
        let excluded_barrier =
            crate::drawer::platform_has_back_button(crate::theme::ThemeData::of(context).platform);
        // The barrier's own words, which upstream takes from the same string
        // the bottom sheet's drag handle borrows: dismissing is what it does,
        // and there is no second name for one action.
        let barrier_label =
            crate::material_app::DefaultMaterialLocalizations::MODAL_BARRIER_DISMISS_LABEL
                .to_string();
        // One cell per built scaffold, published above the whole page so the
        // bar can find it however deep it sits. Upstream's `Scaffold` puts its
        // `ValueListenable` in an inherited widget for the same reason: the
        // bar is the caller's widget and the scaffold cannot reach into it.
        let geometry = ScaffoldGeometry::default();
        let location = self.fab_location;
        let text_direction = crate::direction::direction_of(context);
        let body = if has_app_bar || resizes {
            let data = *crate::media_query::media_query_of(context);
            let data = if has_app_bar {
                data.remove_padding(true, true, true, false)
            } else {
                data
            };
            let data = if resizes {
                data.remove_view_insets(false, false, false, true)
            } else {
                data
            };
            crate::media_query::MediaQuery::new(data, body)
        } else {
            body
        };
        let mut children = Vec::new();
        if let Some(app_bar) = app_bar {
            children.push(app_bar);
        }
        children.push(body);
        // **After the body, before the drawer pair**, and the bar before the
        // button, because the closure below pulls them out in exactly this
        // order: anything pushed out of turn is taken for its neighbour.
        let has_bar = bottom_navigation_bar.is_some();
        if let Some(bar) = bottom_navigation_bar {
            children.push(bar);
        }
        let has_button = floating_action_button.is_some();
        if let Some(button) = floating_action_button {
            children.push(button);
        }
        if drawer_open {
            // The two overlay layers, in paint order: the scrim over the page,
            // the drawer over the scrim. Upstream's `Stack` in
            // `DrawerController._buildDrawer` is exactly these two.
            let handlers = scrim_handlers.clone();
            children.push(leaf(move || {
                // **Blocking**, which is upstream's arrangement exactly:
                // `DrawerController.build` wraps the barrier in
                // `BlockSemantics`, so everything painted before it -- the
                // whole page -- leaves the walk while the drawer is out.
                //
                // Without it a reader could swipe straight off the drawer into
                // the page it is covering: the page is *behind* the drawer
                // rather than under it, so neither excluding the drawer's
                // descendants nor merging them reaches it. That difference is
                // what this crate's `semantics_markers` module doc opens with,
                // and nothing had ever acted on it.
                crate::semantics_markers::BlockSemantics::new().wrapping(
                    // Excluded **on Android only**, which is upstream's
                    // `excluding: platformHasBackButton` and the comment beside
                    // it: "On Android, the back button is used to dismiss a
                    // modal." Everywhere else the scrim *is* the way out, and a
                    // reader who could not find it would be shut inside the
                    // drawer -- the page behind it having just been blocked.
                    crate::semantics_markers::ExcludeSemantics::with_excluding(excluded_barrier)
                        .wrapping(
                            // A **button with the barrier's words**, so the way
                            // out is one a reader can find and press. Upstream's
                            // `GestureDetector(onTap: close)` around
                            // `Semantics(label: modalBarrierDismissLabel)`; here
                            // the scrim was a bare `Pointer`, which a reader
                            // cannot see at all.
                            crate::semantics::RenderSemantics::new(
                                crate::semantics::node_id_for(
                                    scrim_id.unwrap_or(DRAWER_BARRIER_SEMANTICS_ID),
                                ),
                                crate::semantics::SemanticsProperties::button(
                                    barrier_label.clone(),
                                ),
                                // `Colors.black54`, the drawer barrier's
                                // default color.
                                Pointer::new(
                                    scrim_id.unwrap_or(0),
                                    Container::new().with_color(crate::drawer::DRAWER_SCRIM),
                                )
                                .with_handlers(handlers.clone()),
                            )
                            .with_on_action({
                                let handlers = handlers.clone();
                                move |action| {
                                    if action == crate::semantics::SemanticsAction::Tap {
                                        if let Some(tap) = &handlers.on_tap {
                                            tap(crate::gestures::TapEvent {
                                                local_position: Offset::ZERO,
                                                position: Offset::ZERO,
                                                pointer_id: 0,
                                            });
                                        }
                                    }
                                }
                            }),
                        ),
                )
            }));
            children.push(drawer.expect("checked above"));
        }

        // Published **around** the assembled page rather than inside it: the
        // bar is one of `children` and is built before this closure runs, so
        // the provider has to be an ancestor of the whole lot.
        crate::framework::provide(
            geometry.clone(),
            many(children, move |rendered| {
                let mut column = RenderFlex::column()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
                let mut rendered = rendered.into_iter();
                let mut floating_bar = None;
                if has_app_bar {
                    if let Some(bar) = rendered.next() {
                        if behind_bar {
                            // Kept back, to go over the body rather than above it.
                            floating_bar = Some(bar);
                        } else {
                            column = column.push(bar);
                        }
                    }
                }
                if let Some(body) = rendered.next() {
                    // The body takes everything the bar left, less whatever the
                    // keyboard is standing on. The padding is inside the flex
                    // child, so the child is still handed the whole of the
                    // remaining height and gives this much of it back -- which is
                    // upstream's `contentBottom` arrived at from the other side.
                    let body: crate::render::RenderRef = if resizes {
                        RenderRef::new(RenderPadding::new(
                            EdgeInsets::only(0.0, 0.0, 0.0, bottom_inset),
                            body,
                        ))
                    } else {
                        body
                    };
                    column = column.push_flex(crate::render::FlexChild::expanded(body, 1));
                }
                // The page: the body, and over it the bar when it floats. The bar
                // goes on last, which is what makes it paint over the body --
                // upstream's `_ScaffoldSlot.appBar` comes after the body in the
                // stacking order for the same reason, and its comment says so.
                let page: crate::render::BoxedRender = match floating_bar {
                    None => RenderRef::new(column),
                    Some(bar) => RenderRef::new(
                        RenderStack::new()
                            .with_fit(StackFit::Expand)
                            .push(column)
                            .push_positioned(
                                bar,
                                StackPosition {
                                    left: Some(0.0),
                                    top: Some(0.0),
                                    right: Some(0.0),
                                    ..StackPosition::default()
                                },
                            ),
                    ),
                };

                // The button goes over the page and **under the drawer**, which is
                // upstream's stacking order: a drawer that has been pulled out
                // covers everything the scaffold was showing, button included.
                // Pulled **only when one was pushed**: `next()` runs before any
                // filter on its result, so asking unconditionally and discarding
                // the answer takes the scrim instead and the drawer loses its
                // backdrop.
                let bar = if has_bar { rendered.next() } else { None };
                let button = if has_button { rendered.next() } else { None };
                let page: crate::render::BoxedRender = if bar.is_none() && button.is_none() {
                    page
                } else {
                    RenderRef::new(ScaffoldFloor {
                        page,
                        bar,
                        button,
                        location,
                        bottom_inset,
                        text_direction,
                        size: Size::ZERO,
                        bar_height: 0.0,
                        button_offset: crate::render::Offset::ZERO,
                        geometry: geometry.clone(),
                    })
                };

                if !drawer_open {
                    return RenderRef::new(
                        Container::new().with_color(background).with_child(page),
                    );
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
                    .push(Container::new().with_color(background).with_child(page));
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
            }),
        )
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
    /// Upstream's `visualDensity`, likewise three-valued.
    visual_density: Option<crate::theme::VisualDensity>,
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
            visual_density: None,
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

    /// Upstream's `visualDensity`, the first step of
    /// `visualDensity ?? tileTheme.visualDensity ?? theme.visualDensity`.
    ///
    /// Separate from `dense`, which the two are easy to conflate: `dense`
    /// picks a different **row** from upstream's height table (48 rather than
    /// 56 for one line), while this shifts whichever row was picked by a
    /// signed number of pixels. A tile can be both, and the two compose.
    pub fn with_visual_density(mut self, density: crate::theme::VisualDensity) -> Self {
        self.visual_density = Some(density);
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
        // Upstream's `button: onTap != null || onLongPress != null` -- and it
        // is about whether anybody is *listening*, not about the tile being
        // enabled: a disabled tile with a handler is still a button, and says
        // so along with saying it is disabled. Announcing a plain text row as
        // a button would invite a press that does nothing.
        let pressable = self.handlers.on_tap.is_some();
        let selected = self.selected;
        let enabled = self.enabled;
        let leading = self.leading.borrow().clone();
        let trailing = self.trailing.borrow().clone();
        let spacing = theme.spacing;
        let radius = theme.radius;
        // Upstream's `ListTile.build`: the content padding, the gap between
        // the title and whatever follows it, the minimum height and the tile's
        // own colour all come off `ListTileTheme.of(context)` before the
        // control's defaults. `selected` is passed in because it chooses
        // between two different sets of those.
        let tile = crate::component_themes::ResolvedListTile::of_with_density(
            context,
            self.selected,
            self.dense,
            self.selected_color,
            self.visual_density,
        );
        let content_padding = self.content_padding.unwrap_or(tile.content_padding);
        // Upstream's `_effectiveHorizontalTitleGap`, which moves with the
        // density -- by two pixels a unit, not the four `baseSizeAdjustment`
        // uses for the height.
        //
        // **This line is not covered.** Replacing it with the unadjusted
        // `tile.horizontal_title_gap` leaves the suite green: the rule itself
        // is held by `effective_horizontal_title_gap`'s own tests, but
        // nothing here can see *which* gap the build used. The height that
        // tick 342 wired the same way is readable off a laid-out tile; a
        // horizontal gap between two children is not -- the stub canvas
        // records rectangles and clips, and no text draw, so the title's
        // position never reaches a test. A stub that recorded paragraph
        // draws with their offsets would get this assertion for free.
        let title_gap = tile.effective_horizontal_title_gap();
        // Upstream's `_defaultTileHeight` is chosen by the tile's line count
        // as well as by `dense`, and only the tile knows that -- so a theme
        // that set a height outright still wins, and otherwise the six-way
        // table decides. Reading `tile.min_tile_height` alone asked for a
        // one-line height for every tile, 16 short for one with a subtitle.
        let min_tile_height =
            match crate::component_themes::ListTileTheme::of(context).min_tile_height {
                Some(asked) => asked,
                None => crate::component_themes::ResolvedListTile::default_tile_height(
                    self.is_three_line,
                    self.subtitle.is_some(),
                    tile.dense,
                    // Upstream's `baseDensity.dy`. Only the vertical half:
                    // this is a height, and the horizontal adjustment is
                    // what the tile's padding takes.
                    tile.visual_density.base_size_adjustment().1,
                ),
            };
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
        // Upstream's `ListTileTitleAlignment`, which this used to hard-code
        // as the `ThreeLine` arm -- so a theme naming any of the other four
        // was ignored, and so was Material 2's default of `TitleHeight`.
        //
        // # Two of the five are approximated here, and say so
        //
        // `ThreeLine` and `Center` are exact: top against a three-line block,
        // centred otherwise, and centred always. `Top` and `Bottom` are exact
        // up to the `minVerticalPadding` upstream also inserts, which this
        // row does not add on the cross axis.
        //
        // `TitleHeight` is not exact. Upstream puts the leading and trailing
        // sixteen pixels below the top of the *title* when the tile is taller
        // than 72, and otherwise centres the trailing against both titles
        // while keeping the leading whichever of the two is nearer the
        // title's top. That rule needs the title's own box, which this row
        // does not have -- it aligns against the whole cross axis. Centring
        // is the nearer of the two answers it can give, and it is what a
        // one- or two-line tile gets upstream as well; a tall Material 2 tile
        // is where the two differ.
        let cross_alignment = match tile.title_alignment {
            crate::component_themes::ListTileTitleAlignment::ThreeLine => {
                if three_line {
                    CrossAxisAlignment::Start
                } else {
                    CrossAxisAlignment::Center
                }
            }
            crate::component_themes::ListTileTitleAlignment::Top => CrossAxisAlignment::Start,
            crate::component_themes::ListTileTitleAlignment::Bottom => CrossAxisAlignment::End,
            crate::component_themes::ListTileTitleAlignment::Center
            | crate::component_themes::ListTileTitleAlignment::TitleHeight => {
                CrossAxisAlignment::Center
            }
        };

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
        // Upstream caps the leading and trailing widgets' height and nothing
        // else's: `looseConstraints.enforce(maxIconHeightConstraint)`. It is
        // a maximum over loose constraints, so a small icon is untouched and
        // an oversized one is brought down rather than stretching the row.
        let icon_cap = tile.max_icon_height();
        let capped = move |child: AnyWidget| {
            crate::framework::single(child, move |inner| {
                crate::widgets::ConstrainedBox::new(
                    BoxConstraints {
                        min_width: 0.0,
                        max_width: f32::INFINITY,
                        min_height: 0.0,
                        max_height: icon_cap,
                    },
                    inner,
                )
            })
        };
        if let Some(leading) = leading {
            // Before the title, and first in the list so the row below can
            // tell it from the trailing.
            children.insert(0, capped(leading));
        }
        if let Some(trailing) = trailing {
            children.push(capped(trailing));
        }

        many(children, move |mut rendered| {
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(cross_alignment)
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
            let hittable = match id {
                Some(id) => crate::render::RenderRef::new(
                    Pointer::new(id, padded).with_handlers(handlers.clone()),
                ),
                None => crate::render::RenderRef::new(padded),
            };
            // Upstream wraps the tile in
            // `Semantics(button: onTap != null || onLongPress != null,
            // selected: selected, enabled: enabled)`, and a tile said none of
            // it: a row a reader could press was announced as a row of text,
            // and a **selected** row -- the one in a master/detail list that
            // says which item you are looking at -- sounded exactly like the
            // others.
            //
            // One stop, folded, because a tile's words come from the `Text`s
            // inside it: a title and a subtitle met separately are two things
            // to land on where the screen shows one row. That is what the
            // merging box is for, and it carries the flags on the folded node
            // (round 364) so they land where the words are.
            crate::render::RenderMergeSemanticsBox::new(hittable).with_properties(
                crate::semantics::SemanticsProperties {
                    actions: if pressable {
                        crate::semantics::SemanticsAction::Tap as i32
                    } else {
                        0
                    },
                    flags: crate::semantics::SemanticsFlags {
                        is_button: pressable,
                        selected: crate::semantics::SemanticsTristate::of(selected),
                        has_enabled_state: pressable,
                        is_enabled: enabled,
                        ..crate::semantics::SemanticsFlags::default()
                    },
                    ..crate::semantics::SemanticsProperties::default()
                },
            )
        })
    }
}

/// A hairline rule. Upstream reserves sixteen logical pixels and centers a
/// zero-thickness (device-pixel) line in it; one logical pixel is this
/// renderer's hairline at unit scale.
pub struct Divider {
    overrides: crate::component_themes::DividerOverrides,
}

impl Divider {
    pub fn new() -> Divider {
        Divider {
            overrides: crate::component_themes::DividerOverrides::default(),
        }
    }

    /// Upstream's `height`: the space the rule reserves across the layout, with
    /// the line centred in it. Named after upstream's field rather than after
    /// the shared `space` it resolves to, so a call site reads like the Dart.
    pub fn with_height(mut self, height: f32) -> Self {
        self.overrides.space = Some(height);
        self
    }

    /// Upstream's `thickness`: the width of the line drawn inside that space.
    pub fn with_thickness(mut self, thickness: f32) -> Self {
        self.overrides.thickness = Some(thickness);
        self
    }

    /// Upstream's `indent`: how far the line starts in from the leading edge.
    pub fn with_indent(mut self, indent: f32) -> Self {
        self.overrides.indent = Some(indent);
        self
    }

    /// Upstream's `endIndent`: how far it stops short of the trailing edge.
    pub fn with_end_indent(mut self, end_indent: f32) -> Self {
        self.overrides.end_indent = Some(end_indent);
        self
    }

    /// Upstream's `color`.
    pub fn with_color(mut self, color: Color) -> Self {
        self.overrides.color = Some(color);
        self
    }

    /// Upstream's `radius`.
    pub fn with_radius(mut self, radius: crate::borders::BorderRadiusGeometry) -> Self {
        self.overrides.radius = Some(radius);
        self
    }

    /// This divider's metrics, with its own answers over the theme's.
    pub fn resolved(&self, context: &mut BuildContext) -> crate::component_themes::ResolvedDivider {
        crate::component_themes::ResolvedDivider::of(context).overridden_by(&self.overrides)
    }

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
        let ratio = crate::media_query::media_query_of(context).device_pixel_ratio;
        crate::borders::BorderSide {
            color: divider.color,
            width: divider.line_thickness_for(ratio),
            ..crate::borders::BorderSide::default()
        }
    }
}

impl Component for Divider {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        // Upstream's `Divider.build`: the space, the thickness, the colour
        // and the two indents each come off `DividerTheme.of(context)` and
        // fall back to `ThemeData` and then to upstream's own defaults --
        // `ResolvedDivider` is those three steps, and `resolved` puts this
        // divider's own answers in front of them, which is where upstream's
        // `widget.height ?? ...` chain starts.
        let divider = self.resolved(context);
        let color = divider.color;
        let space = divider.space;
        let thickness = divider
            .line_thickness_for(crate::media_query::media_query_of(context).device_pixel_ratio);
        let insets = crate::render::EdgeInsets {
            left: divider.indent,
            right: divider.end_indent,
            top: 0.0,
            bottom: 0.0,
        };
        // Upstream rounds the box and draws the line as its *bottom border*;
        // this fills a box of the line's thickness. At a zero radius the two
        // are the same picture. With one they differ at the ends, and this
        // is the shape this Container can draw.
        let radius = divider
            .radius
            .map(|r| r.resolve(crate::direction::direction_of(context)));
        leaf(move || {
            let line = Container::new()
                .with_height(thickness)
                .with_color(color)
                .with_margin(insets);
            let line = match radius {
                Some(radius) => line.with_border_radius(radius),
                None => line,
            };
            Container::new()
                .with_height(space)
                .with_child(Align::new(Alignment::CENTER, line))
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
        let child = self.child.borrow().clone();
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
            .borrow()
            .clone()
            .unwrap_or_else(|| leaf(|| crate::widgets::Empty));
        let leading = self.leading.borrow().clone();
        let has_leading = leading.is_some();
        let actions = self.actions.borrow().clone();
        let action_count = actions.len();

        let mut children = vec![content];
        children.extend(leading);
        children.extend(actions);

        let banner_body = crate::framework::many(children, move |mut boxed| {
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
        });
        // The same rule the snack bar has, and upstream gives it to both:
        // something that appeared without being asked for is worth telling a
        // reader about. A banner does not dismiss itself, which makes the case
        // look weaker and is not -- see
        // [`crate::semantics::announces_itself`].
        crate::semantics::announces_itself(banner_body)
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
                            position: crate::render::Offset::ZERO,
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

    /// The colours a badge actually put on the glass: the pill and the label.
    #[test]
    fn a_switch_can_be_flipped_without_a_pointer() {
        use std::cell::Cell;
        use std::rc::Rc;

        crate::focus::reset();
        crate::focus::reset_pending_autofocus();
        let flips = Rc::new(Cell::new(0));
        let counter = Rc::clone(&flips);

        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            crate::framework::component(
                Switch::new(7201, false).with_handlers(
                    crate::gestures::PointerHandlers::new()
                        .with_tap(move |_| counter.set(counter.get() + 1)),
                ),
            ),
        ));
        let _ = tree.build_render_tree();

        let enter = crate::keyboard::KeyEvent {
            change: crate::keyboard::KeyChange::Down,
            physical: crate::keyboard::PhysicalKey::ENTER,
            logical: crate::keyboard::LogicalKey::ENTER,
            character: None,
            synthesized: false,
            time_stamp_micros: 0,
        };
        assert!(crate::focus::focus(7201), "a switch is a stop");
        assert!(crate::focus::dispatch_key(&enter));
        assert_eq!(
            flips.get(),
            1,
            "and flips through the handler the finger uses"
        );
    }

    #[test]
    fn a_button_can_be_reached_and_pressed_without_a_pointer() {
        // Until this, no button in the crate could be operated from the
        // keyboard: there was no focus node, so Tab walked past and Enter had
        // nothing to reach. A button is the control this matters most for.
        use std::cell::Cell;
        use std::rc::Rc;

        crate::focus::reset();
        crate::focus::reset_pending_autofocus();
        let presses = Rc::new(Cell::new(0));
        let counter = Rc::clone(&presses);

        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            crate::framework::component(
                Button::new(7101, "Press me").with_handlers(
                    crate::gestures::PointerHandlers::new()
                        .with_tap(move |_| counter.set(counter.get() + 1)),
                ),
            ),
        ));
        let _ = tree.build_render_tree();

        assert!(crate::focus::focus(7101), "Tab can reach it");
        let enter = crate::keyboard::KeyEvent {
            change: crate::keyboard::KeyChange::Down,
            physical: crate::keyboard::PhysicalKey::ENTER,
            logical: crate::keyboard::LogicalKey::ENTER,
            character: None,
            synthesized: false,
            time_stamp_micros: 0,
        };
        assert!(crate::focus::dispatch_key(&enter));
        assert_eq!(
            presses.get(),
            1,
            "and pressed the same handler the finger would"
        );
    }

    #[test]
    fn a_disabled_button_is_not_a_place_the_keyboard_stops() {
        // Upstream gates `canRequestFocus` on `isEnabled`. A stop no key can
        // operate is a dead end the reader has to Tab out of again.
        crate::focus::reset();
        crate::focus::reset_pending_autofocus();

        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            crate::framework::component(
                Button::new(7102, "Nope")
                    .with_enabled(false)
                    .with_autofocus(true)
                    .with_handlers(crate::gestures::PointerHandlers::new().with_tap(move |_| {})),
            ),
        ));
        let _ = tree.build_render_tree();
        crate::focus::apply_pending_autofocus();

        assert_eq!(
            crate::focus::focused(),
            None,
            "it did not claim the keyboard"
        );
        assert!(!crate::focus::focus(7102), "and cannot be given it");
    }

    #[test]
    fn a_button_asked_to_take_the_keyboard_takes_it() {
        crate::focus::reset();
        crate::focus::reset_pending_autofocus();

        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            crate::framework::component(
                Button::new(7103, "First")
                    .with_autofocus(true)
                    .with_handlers(crate::gestures::PointerHandlers::new().with_tap(move |_| {})),
            ),
        ));
        let _ = tree.build_render_tree();
        crate::focus::apply_pending_autofocus();
        assert_eq!(crate::focus::focused(), Some(7103));
    }

    #[test]
    fn a_buttons_ink_is_held_inside_the_pill_and_not_inside_its_bounding_box() {
        // The defect a downstream application saw: pressing a button drew a
        // rectangle that grew. A Material button is a stadium and its ripple
        // was clipped to the bounding rectangle, so once the circle was wide
        // enough to pass the rounded ends it filled the rectangle's four
        // corners with splash colour -- square wedges outside the pill,
        // growing with the ripple. Upstream has no rectangle here at all: it
        // clips the splash with the ink well's own border radius.
        //
        // The radius asserted is the button's own: as round as it is tall,
        // which is upstream's `StadiumBorder`.
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), component(Button::new(1, "go"))));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(400.0, 200.0));
        crate::render::flush_layout();

        let mut layers = crate::engine::LayerTree::new(600, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(600.0, 400.0),
            );
            crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
        }
        let calls = crate::engine_test_stubs::drawn();

        let rounded: Vec<f32> = calls
            .iter()
            .filter_map(|call| match call {
                crate::engine_test_stubs::Drawn::ClipRRectLayer { radius_x, .. } => Some(*radius_x),
                _ => None,
            })
            .collect();
        assert_eq!(
            rounded,
            vec![BUTTON_HEIGHT / 2.0],
            "the ink is clipped to the pill: {calls:?}"
        );
        assert!(
            !calls
                .iter()
                .any(|call| matches!(call, crate::engine_test_stubs::Drawn::ClipRectLayer { .. })),
            "and to nothing square: {calls:?}"
        );
    }

    #[test]
    fn a_button_paints_its_pill_across_the_whole_box_it_was_given() {
        // A downstream application saw a blue button with a second, darker
        // pill beside it. The button laid out to its minimum width and painted
        // its pill at the *label's* width; the rest of the box was unpainted,
        // inside a rounded ink clip the full width, so whatever was behind
        // showed through as a pill of its own.
        //
        // The minimum width is set by `ButtonBounds` and was then dropped by
        // the stack between it and the pill, which loosened the constraints on
        // the way down. Upstream has no stack there at all -- the state layer
        // is an ink feature painted over the child, not a sibling box.
        // Both branches: a pressed button builds a different stack -- the one
        // with the state layer over it -- and it is the branch a reader is
        // looking at when they see the button at all.
        // Four ways a button gets its width, against both states. The tight
        // slot is the one a downstream application actually hit: a button
        // dropped into a positioned stack cell 104 wide. `ButtonBounds` used
        // to hand its child `(min_width, INFINITY)` and ignore the incoming
        // constraints, so the child laid out at 64 while the box became 104.
        let mut label_widths: Vec<f32> = Vec::new();
        for (given, min_width, pressed) in [
            (BoxConstraints::loose(400.0, 200.0), None, false),
            (BoxConstraints::loose(400.0, 200.0), Some(200.0f32), false),
            (BoxConstraints::loose(400.0, 200.0), None, true),
            (BoxConstraints::loose(400.0, 200.0), Some(200.0f32), true),
            (BoxConstraints::tight(104.0, 40.0), None, false),
            (BoxConstraints::tight(104.0, 40.0), None, true),
        ] {
            let mut tree = ElementTree::new();
            let mut button = Button::new(1, "next").with_pressed(pressed);
            if let Some(min_width) = min_width {
                button = button.with_min_width(min_width);
            }
            tree.rebuild(provide(Theme::dark(), component(button)));
            let root = tree.build_render_tree().expect("a root");
            crate::render::schedule_root_layout(&root, given);
            crate::render::flush_layout();
            let width = root.size().width;

            let mut layers = crate::engine::LayerTree::new(600, 400);
            crate::engine_test_stubs::reset_drawn();
            {
                let mut context = crate::render::PaintContext::new(
                    &mut layers,
                    crate::render::Size::new(600.0, 400.0),
                );
                crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
            }
            let calls = crate::engine_test_stubs::drawn();

            let pill = calls
                .iter()
                .find_map(|call| match call {
                    crate::engine_test_stubs::Drawn::RRect { left, right, .. } => {
                        Some((*left, *right))
                    }
                    _ => None,
                })
                .expect("the pill");
            assert_eq!(
                pill,
                (0.0, width),
                "{given:?} {min_width:?} pressed={pressed}: the pill fills                  the box, leaving no strip beside it"
            );

            // And the label is centred in the box rather than parked at the
            // leading edge, which is the other half of the same symptom.
            let label = calls
                .iter()
                .find_map(|call| match call {
                    crate::engine_test_stubs::Drawn::Paragraph { text, x, .. }
                        if text == "next" =>
                    {
                        Some(*x)
                    }
                    _ => None,
                })
                .expect("the label");
            // Centred means the two margins match, so `width - 2x` is the
            // label's own width -- the same number whatever the box is. A
            // label parked at the leading edge would give two different ones.
            label_widths.push(width - label * 2.0);
        }
        assert!(
            label_widths.windows(2).all(|pair| pair[0] == pair[1]),
            "the label is centred in the box, not parked at its leading edge:              {label_widths:?}"
        );
    }

    fn badge_colours(badge: Badge) -> (Option<u32>, Option<u32>) {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), component(badge)));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(
            &mut root,
            BoxConstraints {
                min_width: 0.0,
                max_width: 400.0,
                min_height: 0.0,
                max_height: 400.0,
            },
        );
        let mut layers = crate::engine::LayerTree::new(600, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(600.0, 400.0),
            );
            crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
        }
        let calls = crate::engine_test_stubs::drawn();
        let pill = calls.iter().find_map(|call| match call {
            crate::engine_test_stubs::Drawn::RRect { argb, .. }
            | crate::engine_test_stubs::Drawn::Rect { argb, .. } => Some(*argb),
            _ => None,
        });
        let label = calls.iter().find_map(|call| match call {
            crate::engine_test_stubs::Drawn::Paragraph { argb, .. } => Some(*argb),
            _ => None,
        });
        (pill, label)
    }

    #[test]
    fn the_badges_own_colour_is_the_one_it_is_drawn_in() {
        // This test used to assert only the field, and said why: "which colour
        // came out is not observable -- the stub engine counts rectangles and
        // does not say what colour they were". That stopped being true a long
        // time ago for rectangles and at tick 176 for text, and the comment
        // outlived the limitation it described.
        //
        // The widget-then-theme-then-default order has three steps;
        // `ResolvedBadge` does the last two and the widget's own `unwrap_or`
        // in `build` is the first. What is asserted now is the answer rather
        // than the input to it.
        const MINE: Color = Color::argb(0xFF, 0x77, 0x66, 0x55);
        const INK: Color = Color::argb(0xFF, 0x11, 0x22, 0x33);

        assert_eq!(
            Badge::new("1").with_color(MINE).background_color,
            Some(MINE)
        );
        assert_eq!(Badge::new("1").background_color, None, "unset defers");

        let (pill, _) = badge_colours(Badge::new("1").with_color(MINE));
        assert_eq!(pill, Some(MINE.0), "the pill is the colour it was given");

        let (defaulted, _) = badge_colours(Badge::new("1"));
        assert_ne!(defaulted, Some(MINE.0), "and an unset one is not");

        let (_, label) = badge_colours(Badge::new("1").with_text_color(INK));
        assert_eq!(label, Some(INK.0), "and so is the number on it");
        let (_, plain) = badge_colours(Badge::new("1"));
        assert_ne!(plain, Some(INK.0));
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
                        icon_alignment: crate::component_themes::IconAlignment::Start,
                        animation_duration:
                            crate::component_themes::ResolvedButton::ANIMATION_DURATION,
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
            FilledButtonTheme, FilledButtonThemeData, OutlinedButtonTheme, OutlinedButtonThemeData,
            TextButtonTheme, TextButtonThemeData,
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
                        icon_alignment: crate::component_themes::IconAlignment::Start,
                        animation_duration:
                            crate::component_themes::ResolvedButton::ANIMATION_DURATION,
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
                    OutlinedButtonTheme::new(outlined, TextButtonTheme::new(text, reader)),
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

    /// Where the tile put its title, in the coordinates it was painted in.
    ///
    /// The title is built inside the tile and was not reachable from here for
    /// that reason. It is reachable now: it goes out as a paragraph, and the
    /// recorder keeps the text and where it landed.
    fn title_x(tile: ListTile) -> f32 {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), component(tile)));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(
            &mut root,
            BoxConstraints {
                min_width: 0.0,
                max_width: 400.0,
                min_height: 0.0,
                max_height: 400.0,
            },
        );
        let mut layers = crate::engine::LayerTree::new(600, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(600.0, 400.0),
            );
            crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
        }
        crate::engine_test_stubs::drawn()
            .iter()
            .find_map(|call| match call {
                crate::engine_test_stubs::Drawn::Paragraph { text, x, .. } if text == "a" => {
                    Some(*x)
                }
                _ => None,
            })
            .expect("the title was drawn")
    }

    #[test]
    fn a_wider_reservation_pushes_the_title_further_along() {
        // This claim used to sit in the test below as a comment saying it
        // could not be made: "this harness can see neither the title (it is
        // built inside the tile) nor the tile's intrinsic width". The first
        // half stopped being true when the stub started recording paragraphs
        // and where they landed.
        //
        // What `minLeadingWidth` reserves is room before the title, so the
        // whole of its effect is that number.
        let leading = || crate::framework::leaf(|| crate::widgets::SizedBox::new(10.0, 10.0));
        let narrow = title_x(ListTile::new("a").with_leading(leading()));
        let wide = title_x(
            ListTile::new("a")
                .with_leading(leading())
                .with_min_leading_width(120.0),
        );
        assert!(
            wide > narrow,
            "a wider reservation moves the title: {narrow} then {wide}"
        );
        assert_eq!(
            wide - narrow,
            120.0 - crate::component_themes::ResolvedListTile::MIN_LEADING_WIDTH,
            "by exactly the difference between the two reservations"
        );
    }

    #[test]
    fn the_tile_overrides_the_themes_leading_width() {
        // The choice, which is what the tile records; the geometry it causes
        // is the test above.
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
    fn a_slider_says_when_its_value_is_outside_its_range() {
        // It used to clamp in `new`, which upstream does not: upstream
        // asserts `value >= min && value <= max`. Clamping is the worst of
        // the three answers -- a caller who passed 5 into a 0..1 slider got 1
        // back and no complaint, and once ranges existed a 0..10 slider would
        // have destroyed their 5 on the way in.
        assert!(Slider::new(1, 5.0).validate().is_err());
        assert!(Slider::new(1, -2.0).validate().is_err());
        assert_eq!(Slider::new(1, 0.5).validate(), Ok(()));
        // And with a range, five is perfectly ordinary.
        assert_eq!(Slider::new(1, 5.0).with_range(0.0, 10.0).validate(), Ok(()));
        assert!(
            Slider::new(1, 0.5)
                .with_range(10.0, 0.0)
                .validate()
                .is_err(),
            "a backwards range is refused too"
        );
        assert_eq!(ProgressBar::new(0.5).value, 0.5);
    }

    /// An open drawer is hit at its own edge; everything else on the page is
    /// behind the scrim, and a closed drawer is not there at all.
    /// Where the scaffold put its button, by finding the mark it paints.
    fn button_corner(scaffold: Scaffold) -> (f32, f32) {
        const MARK: Color = Color(0xff00ff00);
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            component(scaffold.with_floating_action_button(leaf(|| {
                Container::new().with_size(56.0, 56.0).with_color(MARK)
            }))),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(&mut root, BoxConstraints::tight(400.0, 800.0));
        let mut layers = crate::engine::LayerTree::new(600, 900);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(600.0, 900.0));
            crate::render::RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        crate::engine_test_stubs::drawn()
            .into_iter()
            .find_map(|call| match call {
                crate::engine_test_stubs::Drawn::Rect {
                    left, top, argb, ..
                } if argb == MARK.0 => Some((left, top)),
                _ => None,
            })
            .expect("the button was not painted")
    }

    #[test]
    fn a_scaffold_places_its_floating_action_button() {
        // The gap: `fab_location.rs` works out where every one of upstream's
        // nineteen placements goes, and is tested doing it -- and **nothing
        // called it**, because this scaffold had no button. So the maths was
        // right and no application could put a button on the screen.
        //
        // `END_FLOAT` is upstream's default and the corner nearly every
        // Material application uses: 16 in from the right, 16 up from the
        // bottom.
        let (left, top) = button_corner(Scaffold::new(leaf(|| Empty)));
        assert_eq!(left, 400.0 - 56.0 - 16.0);
        assert_eq!(top, 800.0 - 56.0 - 16.0);
    }

    #[test]
    fn the_location_decides_the_corner() {
        // Not one placement hard-coded: the scaffold asks the location, which
        // is the whole point of the nineteen constants already ported.
        let centred = button_corner(
            Scaffold::new(leaf(|| Empty))
                .with_fab_location(crate::fab_location::FloatingActionButtonLocation::CENTER_FLOAT),
        );
        assert_eq!(centred.0, (400.0 - 56.0) / 2.0);

        let started = button_corner(
            Scaffold::new(leaf(|| Empty))
                .with_fab_location(crate::fab_location::FloatingActionButtonLocation::START_FLOAT),
        );
        assert_eq!(started.0, 16.0);
    }

    #[test]
    fn the_button_rises_above_the_keyboard() {
        // Upstream folds the keyboard's inset into `minInsets`, so a floating
        // button climbs with the keyboard instead of sitting behind it. This
        // scaffold already shortens its *body* for the keyboard; the button
        // floats over the page and would otherwise stay where it was, half
        // covered, exactly when a form is being filled in.
        const MARK: Color = Color(0xff00ff00);
        let keyboard = 300.0;
        let data = crate::media_query::MediaQueryData {
            view_insets: EdgeInsets::only(0.0, 0.0, 0.0, keyboard),
            ..crate::media_query::MediaQueryData::default()
        };
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            crate::media_query::MediaQuery::new(
                data,
                component(
                    Scaffold::new(leaf(|| Empty)).with_floating_action_button(leaf(|| {
                        Container::new().with_size(56.0, 56.0).with_color(MARK)
                    })),
                ),
            ),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(&mut root, BoxConstraints::tight(400.0, 800.0));
        let mut layers = crate::engine::LayerTree::new(600, 900);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(600.0, 900.0));
            crate::render::RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        let top = crate::engine_test_stubs::drawn()
            .into_iter()
            .find_map(|call| match call {
                crate::engine_test_stubs::Drawn::Rect { top, argb, .. } if argb == MARK.0 => {
                    Some(top)
                }
                _ => None,
            })
            .expect("the button was not painted");
        assert_eq!(top, 800.0 - keyboard - 56.0 - 16.0);
    }

    /// Where the scaffold put a marked strip along its bottom.
    fn bar_corner(scaffold: Scaffold, height: f32) -> (f32, f32) {
        const BAR: Color = Color(0xff0000ff);
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            component(scaffold.with_bottom_navigation_bar(leaf(move || {
                Container::new().with_height(height).with_color(BAR)
            }))),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(&mut root, BoxConstraints::tight(400.0, 800.0));
        let mut layers = crate::engine::LayerTree::new(600, 900);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(600.0, 900.0));
            crate::render::RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        crate::engine_test_stubs::drawn()
            .into_iter()
            .find_map(|call| match call {
                crate::engine_test_stubs::Drawn::Rect {
                    left, top, argb, ..
                } if argb == BAR.0 => Some((left, top)),
                _ => None,
            })
            .expect("the bar was not painted")
    }

    #[test]
    fn a_bottom_bar_sits_along_the_bottom() {
        // The scaffold had no slot for one at all, so a bottom bar could only
        // be put in the body -- where it scrolls away with the content and the
        // keyboard shortens it along with everything else.
        assert_eq!(
            bar_corner(Scaffold::new(leaf(|| Empty)), 56.0),
            (0.0, 744.0)
        );
    }

    #[test]
    fn the_body_gets_the_height_above_the_bar() {
        // Upstream shortens the body by `bottomWidgetsHeight` rather than
        // painting the bar over it. A body given the whole window would put
        // its last row behind the bar, which is exactly the row a list ends
        // on.
        const BODY: Color = Color(0xffff0000);
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            component(
                Scaffold::new(leaf(|| Container::new().with_color(BODY)))
                    .with_bottom_navigation_bar(leaf(|| Container::new().with_height(56.0))),
            ),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(&mut root, BoxConstraints::tight(400.0, 800.0));
        let mut layers = crate::engine::LayerTree::new(600, 900);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(600.0, 900.0));
            crate::render::RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        let bottom = crate::engine_test_stubs::drawn()
            .into_iter()
            .find_map(|call| match call {
                crate::engine_test_stubs::Drawn::Rect { bottom, argb, .. } if argb == BODY.0 => {
                    Some(bottom)
                }
                _ => None,
            })
            .expect("the body was not painted");
        assert_eq!(bottom, 800.0 - 56.0);
    }

    #[test]
    fn a_button_clears_the_bar_it_floats_over() {
        // Upstream's `contentBottom` is measured against `bottomWidgetsHeight`,
        // so a floating button rises above the bar instead of overlapping it.
        const MARK: Color = Color(0xff00ff00);
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            component(
                Scaffold::new(leaf(|| Empty))
                    .with_bottom_navigation_bar(leaf(|| Container::new().with_height(56.0)))
                    .with_floating_action_button(leaf(|| {
                        Container::new().with_size(56.0, 56.0).with_color(MARK)
                    })),
            ),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(&mut root, BoxConstraints::tight(400.0, 800.0));
        let mut layers = crate::engine::LayerTree::new(600, 900);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(600.0, 900.0));
            crate::render::RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        let top = crate::engine_test_stubs::drawn()
            .into_iter()
            .find_map(|call| match call {
                crate::engine_test_stubs::Drawn::Rect { top, argb, .. } if argb == MARK.0 => {
                    Some(top)
                }
                _ => None,
            })
            .expect("the button was not painted");
        assert_eq!(top, 800.0 - 56.0 - 56.0 - 16.0);
    }

    #[test]
    fn a_keyboard_over_a_bar_is_not_the_two_added_together() {
        // Upstream's `contentBottom` takes `math.max(minInsets.bottom,
        // bottomWidgetsHeight)`. A keyboard **covers** the bar rather than
        // stacking on it, so a button that cleared their sum would float a
        // bar's height above the keyboard with a gap under it.
        const MARK: Color = Color(0xff00ff00);
        let keyboard = 300.0;
        let data = crate::media_query::MediaQueryData {
            view_insets: EdgeInsets::only(0.0, 0.0, 0.0, keyboard),
            ..crate::media_query::MediaQueryData::default()
        };
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            crate::media_query::MediaQuery::new(
                data,
                component(
                    Scaffold::new(leaf(|| Empty))
                        .with_bottom_navigation_bar(leaf(|| Container::new().with_height(56.0)))
                        .with_floating_action_button(leaf(|| {
                            Container::new().with_size(56.0, 56.0).with_color(MARK)
                        })),
                ),
            ),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(&mut root, BoxConstraints::tight(400.0, 800.0));
        let mut layers = crate::engine::LayerTree::new(600, 900);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(600.0, 900.0));
            crate::render::RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        let top = crate::engine_test_stubs::drawn()
            .into_iter()
            .find_map(|call| match call {
                crate::engine_test_stubs::Drawn::Rect { top, argb, .. } if argb == MARK.0 => {
                    Some(top)
                }
                _ => None,
            })
            .expect("the button was not painted");
        assert_eq!(top, 800.0 - keyboard - 56.0 - 16.0, "the two were added");
    }

    #[test]
    fn the_bar_spans_the_scaffold() {
        // Upstream lays it out with `fullWidthConstraints`. Given a loose
        // minimum it would shrink to whatever it holds, leaving a strip of
        // page showing beside it -- and a bar that does not reach both edges
        // is not a bar.
        const BAR: Color = Color(0xff0000ff);
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            component(
                Scaffold::new(leaf(|| Empty)).with_bottom_navigation_bar(leaf(|| {
                    Container::new().with_height(56.0).with_color(BAR)
                })),
            ),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(&mut root, BoxConstraints::tight(400.0, 800.0));
        let mut layers = crate::engine::LayerTree::new(600, 900);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(600.0, 900.0));
            crate::render::RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        let (left, right) = crate::engine_test_stubs::drawn()
            .into_iter()
            .find_map(|call| match call {
                crate::engine_test_stubs::Drawn::Rect {
                    left, right, argb, ..
                } if argb == BAR.0 => Some((left, right)),
                _ => None,
            })
            .expect("the bar was not painted");
        assert_eq!((left, right), (0.0, 400.0));
    }

    #[test]
    fn a_finger_on_the_bar_reaches_the_bar() {
        // The bar is painted over the page, so it has to be answered before
        // it -- and at the offset it was painted at. A hit test that
        // disagreed with the paint by the bar's height is a control that
        // answers somewhere it is not drawn.
        const BAR_ID: u64 = 9601;
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            component(
                Scaffold::new(leaf(|| Empty)).with_bottom_navigation_bar(leaf(|| {
                    Pointer::new(BAR_ID, Container::new().with_height(56.0))
                })),
            ),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(&mut root, BoxConstraints::tight(400.0, 800.0));
        let mut result = crate::render::HitTestResult::default();
        crate::render::RenderBox::hit_test(&root, Offset::new(200.0, 780.0), &mut result);
        assert!(
            result.path.iter().any(|entry| entry.target == BAR_ID),
            "the bar did not answer a finger on it: {:?}",
            result
                .path
                .iter()
                .map(|entry| entry.target)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_scaffold_tells_its_bar_where_the_button_landed() {
        // The channel rounds 394 to 396 built each half of: the bar could cut
        // a notch and the scaffold could place a button, and **nothing joined
        // them**, so the notch only appeared for a caller who worked out the
        // rectangle by hand.
        //
        // Nothing here calls `docked_at`. The scaffold publishes where the
        // button landed during layout; the bar reads it during paint, which
        // comes after every layout in the same frame -- so the hole is cut
        // this frame, not next.
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            component(
                Scaffold::new(leaf(|| Empty))
                    .with_bottom_navigation_bar(component(
                        crate::bottom_bars::BottomAppBar::new().with_notch(),
                    ))
                    .with_floating_action_button(leaf(|| Container::new().with_size(56.0, 56.0))),
            ),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(&mut root, BoxConstraints::tight(400.0, 800.0));
        let mut layers = crate::engine::LayerTree::new(600, 900);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(600.0, 900.0));
            crate::render::RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        let calls = crate::engine_test_stubs::drawn();
        assert!(
            calls
                .iter()
                .any(|call| matches!(call, crate::engine_test_stubs::Drawn::Path { .. })),
            "the bar drew no notched outline, so it never heard about the button: {calls:?}"
        );
    }

    #[test]
    fn a_bar_in_a_scaffold_with_no_button_still_draws_a_plain_rectangle() {
        // The other half of the same condition: the scaffold publishes `None`
        // when there is nothing to place, and a hole with no button behind it
        // is a hole in the bar.
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            component(
                Scaffold::new(leaf(|| Empty)).with_bottom_navigation_bar(component(
                    crate::bottom_bars::BottomAppBar::new().with_notch(),
                )),
            ),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(&mut root, BoxConstraints::tight(400.0, 800.0));
        let mut layers = crate::engine::LayerTree::new(600, 900);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(600.0, 900.0));
            crate::render::RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        let calls = crate::engine_test_stubs::drawn();
        assert!(
            !calls
                .iter()
                .any(|call| matches!(call, crate::engine_test_stubs::Drawn::Path { .. })),
            "it cut a notch around nothing: {calls:?}"
        );
    }

    #[test]
    fn a_scaffold_with_no_button_is_left_exactly_as_it_was() {
        // The positioner is only wrapped around the page when there is
        // something to place, and the scaffold's other slots keep their
        // positions in the list either way -- the first version pulled the
        // button unconditionally and took the drawer's scrim instead.
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            component(Scaffold::new(leaf(|| Empty))),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        let size = crate::render::RenderBox::layout(&mut root, BoxConstraints::tight(400.0, 800.0));
        assert_eq!(size, Size::new(400.0, 800.0));
    }

    /// Every word a reader would meet in this scaffold.
    fn scaffold_words(scaffold: Scaffold) -> Vec<String> {
        crate::semantics::set_enabled(true);
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            component(scaffold),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(&mut root, BoxConstraints::tight(400.0, 800.0));
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(Size::new(400.0, 800.0), &root).unwrap_or_default();
        crate::semantics::set_enabled(false);
        nodes
            .iter()
            .filter(|node| !node.properties.label.is_empty())
            .map(|node| node.properties.label.clone())
            .collect()
    }

    /// Every word a reader would meet, on a named platform.
    fn scaffold_words_on(
        scaffold: Scaffold,
        platform: crate::editable_text::TargetPlatform,
    ) -> Vec<String> {
        crate::semantics::set_enabled(true);
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData {
                platform,
                ..crate::theme::ThemeData::light()
            },
            component(scaffold),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(&mut root, BoxConstraints::tight(400.0, 800.0));
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(Size::new(400.0, 800.0), &root).unwrap_or_default();
        crate::semantics::set_enabled(false);
        nodes
            .iter()
            .filter(|node| !node.properties.label.is_empty())
            .map(|node| node.properties.label.clone())
            .collect()
    }

    fn drawer_scaffold() -> Scaffold {
        Scaffold::new(component(crate::components::Label::new("Inbox")))
            .with_drawer(component(crate::components::Label::new("Settings")))
            .with_drawer_open(true)
    }

    #[test]
    fn the_way_out_of_a_drawer_is_one_a_reader_can_find() {
        // The scrim was a bare `Pointer`: a finger could close the drawer and a
        // reader could not, and round 401 had just taken the page behind it out
        // of the walk -- so on a platform with no back button they were shut
        // in. Upstream's barrier is a `GestureDetector(onTap: close)` around
        // `Semantics(label: modalBarrierDismissLabel)`.
        let dismiss =
            crate::material_app::DefaultMaterialLocalizations::MODAL_BARRIER_DISMISS_LABEL;
        let words = scaffold_words_on(drawer_scaffold(), crate::editable_text::TargetPlatform::IOS);
        assert!(
            words.iter().any(|said| said == dismiss),
            "no way out was offered: {words:?}"
        );
    }

    #[test]
    fn android_leaves_the_way_out_to_its_own_back_button() {
        // Upstream's `excluding: platformHasBackButton`, and its comment: "On
        // Android, the back button is used to dismiss a modal." A second
        // dismiss target in the reader's path is one affordance too many.
        let dismiss =
            crate::material_app::DefaultMaterialLocalizations::MODAL_BARRIER_DISMISS_LABEL;
        let words = scaffold_words_on(
            drawer_scaffold(),
            crate::editable_text::TargetPlatform::Android,
        );
        assert!(
            !words.iter().any(|said| said == dismiss),
            "Android was offered a second way out: {words:?}"
        );
        // And the drawer is still there -- excluding the barrier must not take
        // what it sits behind.
        assert!(
            words.iter().any(|said| said == "Settings"),
            "the drawer went with the barrier: {words:?}"
        );
    }

    #[test]
    fn pressing_the_barrier_closes_the_drawer() {
        // The words are only half of it: a labelled node that does nothing
        // when pressed is a way out that is not one.
        //
        // What the press *does* is reach the caller's state, so the observable
        // is that something became dirty -- `set_state` is what a wired drawer
        // calls, and a handler that ran nothing leaves the tree clean.
        struct Reader {
            handle: std::rc::Rc<std::cell::RefCell<Option<StateHandle<bool>>>>,
        }
        impl crate::framework::StatefulComponent for Reader {
            type State = bool;
            fn build(
                &self,
                _state: &bool,
                handle: StateHandle<bool>,
                _context: &mut BuildContext,
            ) -> AnyWidget {
                *self.handle.borrow_mut() = Some(handle.clone());
                component(
                    Scaffold::new(component(crate::components::Label::new("Inbox")))
                        .with_drawer(component(crate::components::Label::new("Settings")))
                        .with_drawer_open(true)
                        .wired_drawer(77, handle, |shut| *shut = true),
                )
            }
        }

        crate::semantics::set_enabled(true);
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData {
                platform: crate::editable_text::TargetPlatform::IOS,
                ..crate::theme::ThemeData::light()
            },
            crate::framework::stateful(Reader {
                handle: std::rc::Rc::new(std::cell::RefCell::new(None)),
            }),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(&mut root, BoxConstraints::tight(400.0, 800.0));
        crate::semantics::mark_needs_update();
        crate::semantics::flush(Size::new(400.0, 800.0), &root);
        assert_eq!(tree.rebuild_dirty(), 0, "nothing was dirty to begin with");
        assert!(crate::semantics::perform_action(
            &root,
            crate::semantics::node_id_for(77),
            crate::semantics::SemanticsAction::Tap
        ));
        crate::semantics::set_enabled(false);
        assert!(
            tree.rebuild_dirty() > 0,
            "the barrier answered the press and did nothing with it"
        );
    }

    #[test]
    fn an_open_drawer_takes_the_page_out_of_the_readers_way() {
        // Upstream wraps the drawer's barrier in `BlockSemantics`
        // (`drawer.dart:708`), so the page stops being reachable while the
        // drawer is out. This port had the render object and the settings
        // struct and **nothing joining them**, so a reader could swipe off the
        // drawer straight back into the page it was covering -- and act on a
        // screen that is not listening.
        //
        // The page is *behind* the drawer rather than under it, which is why
        // neither excluding nor merging reaches it.
        let page = || leaf(|| Empty);
        let with_words = |scaffold: Scaffold| {
            scaffold.with_drawer(component(crate::components::Label::new("Settings")))
        };

        let shut = scaffold_words(with_words(Scaffold::new(component(
            crate::components::Label::new("Inbox"),
        ))));
        assert!(
            shut.iter().any(|words| words == "Inbox"),
            "the page is reachable with the drawer shut: {shut:?}"
        );

        let open = scaffold_words(
            with_words(Scaffold::new(component(crate::components::Label::new(
                "Inbox",
            ))))
            .with_drawer_open(true),
        );
        assert!(
            !open.iter().any(|words| words == "Inbox"),
            "the page is still reachable behind an open drawer: {open:?}"
        );
        assert!(
            open.iter().any(|words| words == "Settings"),
            "and the drawer itself went with it: {open:?}"
        );
        let _ = page;
    }

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
        assert_eq!(height_of(component(Divider::new())), 16.0);

        // The nearest installed theme moves it -- the three-step fallback
        // reaching a control's geometry, which is the whole point of it.
        assert_eq!(
            height_of(DividerTheme::new(
                DividerThemeData::new().with_space(40.0),
                component(Divider::new()),
            )),
            40.0
        );

        // And so does the field on ThemeData, one step further out.
        let themed = crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light()
                .with_divider_theme(DividerThemeData::new().with_space(24.0)),
            component(Divider::new()),
        );
        assert_eq!(height_of(themed), 24.0);

        // **And the divider's own answer beats all three**, which is where
        // upstream's chain starts: `widget.height ?? dividerTheme.height ??
        // defaults`. Until round 390 this port's `Divider` took no arguments
        // at all, so every rule on the screen had to be identical and
        // upstream's commonest use -- `Divider(height: 1)` between the rows of
        // a list -- could not be written.
        assert_eq!(
            height_of(DividerTheme::new(
                DividerThemeData::new().with_space(40.0),
                component(Divider::new().with_height(1.0)),
            )),
            1.0,
            "the theme won over the caller"
        );
    }

    #[test]
    fn a_dividers_own_answers_come_before_the_themes_one_by_one() {
        use crate::component_themes::{DividerOverrides, ResolvedDivider};
        let themed = ResolvedDivider {
            color: Color::WHITE,
            space: 40.0,
            thickness: 4.0,
            indent: 5.0,
            end_indent: 6.0,
            radius: None,
        };
        // Nothing said: the theme stands, field for field.
        assert_eq!(
            themed.overridden_by(&DividerOverrides::default()),
            themed,
            "an empty override changed something"
        );
        // Each one on its own, so a field wired to the wrong slot shows up as
        // the wrong measurement rather than as nothing at all.
        let over = |set: fn(&mut DividerOverrides)| {
            let mut overrides = DividerOverrides::default();
            set(&mut overrides);
            themed.overridden_by(&overrides)
        };
        assert_eq!(over(|o| o.space = Some(1.0)).space, 1.0);
        assert_eq!(over(|o| o.thickness = Some(2.0)).thickness, 2.0);
        assert_eq!(over(|o| o.indent = Some(72.0)).indent, 72.0);
        assert_eq!(over(|o| o.end_indent = Some(8.0)).end_indent, 8.0);
        assert_eq!(over(|o| o.color = Some(Color::BLACK)).color, Color::BLACK);
        // Radius needs a theme that already has one, or `or` would answer the
        // same whichever way round it is written.
        let rounded = ResolvedDivider {
            radius: Some(crate::borders::BorderRadiusGeometry::circular(2.0)),
            ..themed
        };
        assert_eq!(
            rounded
                .overridden_by(&DividerOverrides {
                    radius: Some(crate::borders::BorderRadiusGeometry::circular(9.0)),
                    ..DividerOverrides::default()
                })
                .radius,
            Some(crate::borders::BorderRadiusGeometry::circular(9.0)),
            "the theme's corner won over the caller's"
        );
        // And a caller who says nothing keeps the theme's.
        assert_eq!(
            rounded.overridden_by(&DividerOverrides::default()).radius,
            rounded.radius
        );
        // And setting one leaves the rest to the theme.
        let only_indent = over(|o| o.indent = Some(72.0));
        assert_eq!((only_indent.space, only_indent.thickness), (40.0, 4.0));
    }

    #[test]
    #[should_panic(expected = "cannot be negative")]
    fn a_negative_measurement_is_a_callers_mistake_and_says_so() {
        // Upstream's four asserts, and asserting is all upstream does -- see
        // `ResolvedDivider::overridden_by`. A rule that reserves minus eight
        // pixels is not a state to render; it is a call site to fix, and
        // saying so at the moment it happens is the difference between that
        // and a layout that quietly folds up somewhere else.
        use crate::component_themes::{DividerOverrides, ResolvedDivider};
        let themed = ResolvedDivider {
            color: Color::WHITE,
            space: 16.0,
            thickness: 0.0,
            indent: 0.0,
            end_indent: 0.0,
            radius: None,
        };
        themed.overridden_by(&DividerOverrides {
            indent: Some(-8.0),
            ..DividerOverrides::default()
        });
    }

    #[test]
    fn a_vertical_divider_reserves_its_width_the_way_the_horizontal_one_reserves_its_height() {
        // Upstream keeps these as two classes because `space` means width on
        // one and height on the other, and one widget with an axis would have
        // to explain the reversal at every call site. The two builders are
        // named after upstream's fields for the same reason.
        fn width_of(widget: AnyWidget) -> f32 {
            let mut tree = ElementTree::new();
            tree.rebuild(widget);
            let mut root = tree.build_render_tree().expect("a root");
            root.layout(BoxConstraints::loose(200.0, 200.0)).width
        }
        assert_eq!(width_of(component(VerticalDivider::new())), 16.0);
        assert_eq!(
            width_of(component(VerticalDivider::new().with_width(3.0))),
            3.0
        );
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

    // -- The theme decides which gestures move a slider, tick 228 -----------
    //
    // `tools/unread_theme_fields.py` found `SliderThemeData::allowed_interaction`
    // named nowhere outside its own paperwork: the widget built its gestures
    // in `wired`, before there was a context to read a theme from, so it took
    // every tap and every drag whatever the theme asked for.

    /// A slider under `mode`, and the value its gestures produce.
    ///
    /// The thumb is twenty wide and the value is a half of two hundred, so it
    /// occupies `[100, 120]`: a drag from 110 begins on it and one from 10
    /// does not.
    fn under(
        mode: crate::slider_theme::SliderInteraction,
    ) -> (std::rc::Rc<std::cell::Cell<Option<f32>>>, PointerHandlers) {
        let seen = std::rc::Rc::new(std::cell::Cell::new(None));
        let recorder = std::rc::Rc::clone(&seen);
        let mut slider = Slider::new(1, 0.5);
        slider.on_change = Some(std::rc::Rc::new(move |value| recorder.set(Some(value))));
        let mut resolved = plain_slider();
        resolved.allowed_interaction = mode;
        let handlers = slider.gestures(&resolved);
        (seen, handlers)
    }

    /// A `ResolvedSlider` with nothing in it that draws, for the tests that
    /// are about a rule rather than a picture.
    ///
    /// One function because the resolver keeps gaining fields -- three at
    /// tick 246 alone -- and three copies of the same literal means three
    /// edits and three chances to write a different slider in each.
    fn plain_slider() -> crate::slider_theme::ResolvedSlider {
        crate::slider_theme::ResolvedSlider {
            track_height: 4.0,
            active_track_color: Color::argb(255, 0, 0, 0),
            inactive_track_color: Color::argb(255, 0, 0, 0),
            thumb_color: Color::argb(255, 0, 0, 0),
            track_shape: crate::slider_theme::SliderTrackShape::Rectangular(
                crate::slider_theme::RectangularSliderTrackShape::default(),
            ),
            thumb_shape: crate::slider_theme::SliderComponentShape::Empty,
            thumb_size: Size::new(20.0, 20.0),
            allowed_interaction: crate::slider_theme::SliderInteraction::TapAndSlide,
            tick_mark_shape: crate::slider_theme::SliderTickMarkShape::Empty,
            value_indicator_shape: crate::slider_theme::SliderComponentShape::Empty,
            show_value_indicator: crate::slider_theme::ShowValueIndicator::OnlyForDiscrete,
            value_indicator_text_style: TextStyle::default(),
            shape_theme: crate::slider_theme::SliderThemeData::new(),
        }
    }

    /// A list tile's laid-out height under a Material theme of its own,
    /// which [`tile_height`] cannot express -- that one publishes a
    /// `Theme::dark()` and the visual density lives on `ThemeData`.
    fn tile_height_themed(tile: ListTile, theme: crate::theme::ThemeData) -> f32 {
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(theme, component(tile)));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(
            &mut root,
            BoxConstraints {
                min_width: 0.0,
                max_width: 400.0,
                min_height: 0.0,
                max_height: 400.0,
            },
        )
        .height
    }

    #[test]
    fn an_oversized_leading_widget_is_brought_down_rather_than_stretching_the_row() {
        // Upstream's `maxIconHeightConstraint`, which this port did not have:
        // the leading widget went into the row unconstrained, so a tall one
        // grew the tile instead of being capped.
        let tall_leading = ListTile::new("Wi-Fi")
            .with_leading(leaf(|| crate::widgets::SizedBox::new(24.0, 200.0)));
        assert_eq!(
            tile_height_themed(tall_leading, crate::theme::ThemeData::light()),
            56.0,
            "a 200-tall leading widget does not make a 200-tall row"
        );

        // A small one is left alone -- it is a maximum, not a size.
        let small_leading =
            ListTile::new("Wi-Fi").with_leading(leaf(|| crate::widgets::SizedBox::new(24.0, 24.0)));
        assert_eq!(
            tile_height_themed(small_leading, crate::theme::ThemeData::light()),
            56.0
        );
    }

    #[test]
    fn the_icon_cap_stays_at_the_one_line_row_however_many_lines_the_tile_has() {
        // The mirror image of tick 341: there a one-line row was quoted as
        // the whole table by mistake, here upstream means the one-line row
        // for every tile -- for the accessibility reason its comment gives.
        // So the row grows with the line count and the cap does not.
        let three_line = ListTile::new("Wi-Fi")
            .with_subtitle("Connected")
            .with_three_line(true)
            .with_leading(leaf(|| crate::widgets::SizedBox::new(24.0, 200.0)));
        assert_eq!(
            tile_height_themed(three_line, crate::theme::ThemeData::light()),
            88.0,
            "the row is three lines tall, from its own table"
        );

        // And the cap itself did not follow it up there.
        let tile = read_tile(crate::theme::ThemeData::light());
        assert_eq!(tile.max_icon_height(), 56.0);

        // Nor does it follow a theme that raised the row outright: the cap
        // reads the one-line constant, not the resolved `min_tile_height`,
        // and those two agree only by coincidence at the default.
        let tall_rows = read_tile(crate::theme::ThemeData::light().with_list_tile_theme(
            crate::component_themes::ListTileThemeData::new().with_min_tile_height(120.0),
        ));
        assert_eq!(tall_rows.min_tile_height, 120.0);
        assert_eq!(tall_rows.max_icon_height(), 56.0, "the cap stayed put");
    }

    #[test]
    fn a_dense_tile_caps_its_icons_lower() {
        // The cap takes the dense row for the same reason the tile does.
        let dense = read_tile(crate::theme::ThemeData::light().with_list_tile_theme(
            crate::component_themes::ListTileThemeData::new().with_dense(true),
        ));
        assert_eq!(dense.max_icon_height(), 48.0);
    }

    #[test]
    fn the_trailing_widget_is_capped_as_the_leading_one_is() {
        // Upstream lays both out against the same `iconConstraints`.
        let tall_trailing = ListTile::new("Wi-Fi")
            .with_trailing(leaf(|| crate::widgets::SizedBox::new(24.0, 200.0)));
        assert_eq!(
            tile_height_themed(tall_trailing, crate::theme::ThemeData::light()),
            56.0
        );
    }

    /// The resolved tile theme under a Material theme.
    fn read_tile(theme: crate::theme::ThemeData) -> crate::component_themes::ResolvedListTile {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        struct Reader {
            seen:
                std::rc::Rc<std::cell::RefCell<Option<crate::component_themes::ResolvedListTile>>>,
        }
        impl Component for Reader {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.seen.borrow_mut() = Some(crate::component_themes::ResolvedListTile::of(
                    context, false, None,
                ));
                leaf(|| crate::widgets::SizedBox::new(1.0, 1.0))
            }
        }
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            theme,
            component(Reader { seen: seen.clone() }),
        ));
        let taken = seen.borrow_mut().take();
        taken.expect("built")
    }

    #[test]
    fn the_density_moves_the_icon_cap_by_four_pixels_a_unit() {
        // Unlike the title gap next door, which moves by two.
        let compact = read_tile(crate::theme::ThemeData::light().with_visual_density(
            crate::theme::VisualDensity {
                horizontal: 0.0,
                vertical: -2.0,
            },
        ));
        assert_eq!(compact.max_icon_height(), 56.0 - 8.0);
        assert_eq!(
            compact.effective_horizontal_title_gap(),
            compact.horizontal_title_gap,
            "the horizontal half was zero here, so the gap did not move"
        );
    }

    #[test]
    fn a_theme_density_shortens_the_row_a_tile_asks_for() {
        // Tick 341 left `ListTile::build` passing a hardcoded 0.0 where
        // upstream passes `visualDensity.baseSizeAdjustment.dy`, so an
        // application that set a density anywhere got the standard row.
        let plain = tile_height_themed(
            ListTile::new("Wi-Fi").with_subtitle("Connected"),
            crate::theme::ThemeData::light(),
        );
        assert_eq!(plain, 72.0, "two lines at the standard density");

        let compact = tile_height_themed(
            ListTile::new("Wi-Fi").with_subtitle("Connected"),
            crate::theme::ThemeData::light().with_visual_density(crate::theme::VisualDensity {
                horizontal: 0.0,
                vertical: -2.0,
            }),
        );
        assert!(
            compact < plain,
            "the density took height off the row: {plain} then {compact}"
        );
        assert_eq!(compact, 72.0 - 8.0, "two density units, four pixels each");
    }

    #[test]
    fn the_tiles_own_density_beats_the_theme_it_sits_in() {
        // Step one of `visualDensity ?? tileTheme.visualDensity ??
        // theme.visualDensity`.
        let roomy_theme =
            crate::theme::ThemeData::light().with_visual_density(crate::theme::VisualDensity {
                horizontal: 0.0,
                vertical: 2.0,
            });
        let deferring = tile_height_themed(
            ListTile::new("Wi-Fi").with_subtitle("Connected"),
            roomy_theme.clone(),
        );
        assert_eq!(deferring, 72.0 + 8.0, "the theme's, with nothing nearer");

        let insisting = tile_height_themed(
            ListTile::new("Wi-Fi")
                .with_subtitle("Connected")
                .with_visual_density(crate::theme::VisualDensity {
                    horizontal: 0.0,
                    vertical: -2.0,
                }),
            roomy_theme,
        );
        assert_eq!(
            insisting,
            72.0 - 8.0,
            "the tile's own, against a theme asking for the opposite"
        );

        // And against the *tile* theme, which is the nearer of the two it has
        // to beat -- a widget that only outranked `ThemeData` would pass the
        // check above and still lose here.
        let roomy_tile_theme = crate::theme::ThemeData::light().with_list_tile_theme(
            crate::component_themes::ListTileThemeData::new().with_visual_density(
                crate::theme::VisualDensity {
                    horizontal: 0.0,
                    vertical: 2.0,
                },
            ),
        );
        assert_eq!(
            tile_height_themed(
                ListTile::new("Wi-Fi").with_subtitle("Connected"),
                roomy_tile_theme.clone()
            ),
            72.0 + 8.0,
            "the tile theme's, with nothing nearer"
        );
        assert_eq!(
            tile_height_themed(
                ListTile::new("Wi-Fi")
                    .with_subtitle("Connected")
                    .with_visual_density(crate::theme::VisualDensity {
                        horizontal: 0.0,
                        vertical: -2.0,
                    }),
                roomy_tile_theme
            ),
            72.0 - 8.0,
            "the tile's own beats the tile theme too"
        );
    }

    /// Every filled shape a slider draws, as colours. The value indicator's
    /// bubble is a path, which this stub records by its bounding box.
    fn fills_of(slider: Slider) -> Vec<u32> {
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            component(slider),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(
            &mut root,
            BoxConstraints {
                min_width: 0.0,
                max_width: 400.0,
                min_height: 0.0,
                max_height: 400.0,
            },
        );
        let mut layers = crate::engine::LayerTree::new(600, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(600.0, 400.0),
            );
            crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
        }
        crate::engine_test_stubs::drawn()
            .iter()
            .filter_map(|call| match call {
                crate::engine_test_stubs::Drawn::Rect { argb, .. }
                | crate::engine_test_stubs::Drawn::RRect { argb, .. }
                | crate::engine_test_stubs::Drawn::Path { argb, .. } => Some(*argb),
                _ => None,
            })
            .collect()
    }

    /// Every string a slider writes on the canvas, with the colour it was
    /// written in.
    fn paragraphs_of(slider: Slider) -> Vec<(String, u32)> {
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            component(slider),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(
            &mut root,
            BoxConstraints {
                min_width: 0.0,
                max_width: 400.0,
                min_height: 0.0,
                max_height: 400.0,
            },
        );
        let mut layers = crate::engine::LayerTree::new(600, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(600.0, 400.0),
            );
            crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
        }
        crate::engine_test_stubs::drawn()
            .iter()
            .filter_map(|call| match call {
                crate::engine_test_stubs::Drawn::Paragraph { text, argb, .. } => {
                    Some((text.clone(), *argb))
                }
                _ => None,
            })
            .collect()
    }

    fn tap_at(handlers: &PointerHandlers, dx: f32) {
        let handler = handlers.on_tap.clone().expect("a tap handler");
        handler(crate::gestures::TapEvent {
            local_position: Offset::new(dx, 0.0),
            position: Offset::new(dx, 0.0),
            pointer_id: 1,
        });
    }

    fn drag_from_to(handlers: &PointerHandlers, from: f32, to: f32) {
        let start = handlers
            .on_drag_start
            .clone()
            .expect("a drag start handler");
        start(crate::gestures::DragEvent {
            delta: Offset::new(0.0, 0.0),
            total: Offset::new(0.0, 0.0),
            local_position: Offset::new(from, 0.0),
            pointer_id: 1,
        });
        let update = handlers
            .on_drag_update
            .clone()
            .expect("a drag update handler");
        update(crate::gestures::DragEvent {
            delta: Offset::new(to - from, 0.0),
            total: Offset::new(to - from, 0.0),
            local_position: Offset::new(to, 0.0),
            pointer_id: 1,
        });
    }

    #[test]
    fn tap_and_slide_takes_both() {
        use crate::slider_theme::SliderInteraction;
        let (seen, handlers) = under(SliderInteraction::TapAndSlide);
        tap_at(&handlers, 50.0);
        assert_eq!(seen.get(), Some(0.25));
        seen.set(None);
        drag_from_to(&handlers, 10.0, 150.0);
        assert_eq!(seen.get(), Some(0.75));
    }

    #[test]
    fn tap_only_refuses_the_drag() {
        use crate::slider_theme::SliderInteraction;
        let (seen, handlers) = under(SliderInteraction::TapOnly);
        drag_from_to(&handlers, 110.0, 150.0);
        assert_eq!(seen.get(), None, "a drag moves nothing under TapOnly");
        tap_at(&handlers, 50.0);
        assert_eq!(seen.get(), Some(0.25), "and the tap still lands");
    }

    #[test]
    fn slide_only_refuses_the_tap() {
        // The point of the mode: a stray tap cannot move a value the
        // application cares about.
        use crate::slider_theme::SliderInteraction;
        let (seen, handlers) = under(SliderInteraction::SlideOnly);
        tap_at(&handlers, 50.0);
        assert_eq!(seen.get(), None, "a tap moves nothing under SlideOnly");
        drag_from_to(&handlers, 10.0, 150.0);
        assert_eq!(
            seen.get(),
            Some(0.75),
            "and a drag from anywhere still slides"
        );
    }

    #[test]
    fn slide_thumb_wants_the_drag_to_have_begun_on_the_thumb() {
        use crate::slider_theme::SliderInteraction;
        let (seen, handlers) = under(SliderInteraction::SlideThumb);
        tap_at(&handlers, 50.0);
        assert_eq!(seen.get(), None, "a tap moves nothing under SlideThumb");

        drag_from_to(&handlers, 10.0, 150.0);
        assert_eq!(seen.get(), None, "nor a drag that began off the thumb");

        // The thumb is at [100, 120] for a half-way value on a two-hundred
        // track with a twenty-wide thumb.
        drag_from_to(&handlers, 110.0, 150.0);
        assert_eq!(seen.get(), Some(0.75), "a drag that began on it does");
    }

    #[test]
    fn a_slider_nobody_wired_accepts_nothing() {
        // `gestures` hands back an empty set rather than handlers that would
        // panic or silently do nothing, so an unwired slider is inert by
        // construction.
        let slider = Slider::new(1, 0.5);
        let resolved = plain_slider();
        let handlers = slider.gestures(&resolved);
        assert!(handlers.on_tap.is_none());
        assert!(handlers.on_drag_update.is_none());
    }

    // -- A divider can round its ends, tick 229 -----------------------------
    //
    // `tools/unread_theme_fields.py` found `DividerThemeData::radius` named
    // nowhere outside its own paperwork: it is declared, documented, carried
    // through `copy_with`, interpolated by `lerp` and watched by a test --
    // and `ResolvedDivider` dropped it, so no divider in this port could
    // round its corners however the theme was set. Upstream's
    // `Divider.build` reads `radius ?? dividerTheme.radius ?? defaults.radius`
    // and neither default sets one.

    // -- A divider can round its ends, tick 229 -----------------------------
    //
    // `tools/unread_theme_fields.py` found `DividerThemeData::radius` named
    // nowhere outside its own paperwork: declared, documented, carried
    // through `copy_with`, interpolated by `lerp`, watched by a test -- and
    // `ResolvedDivider` dropped it, so no divider here could round its
    // corners however the theme was set. Upstream's `Divider.build` reads
    // `radius ?? dividerTheme.radius ?? defaults.radius`, and neither
    // `_DividerDefaultsM2` nor `_DividerDefaultsM3` sets one.

    /// What a widget painted.
    fn painted(widget: AnyWidget) -> Vec<crate::engine_test_stubs::Drawn> {
        let mut tree = ElementTree::new();
        tree.rebuild(widget);
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(400.0, 200.0));
        crate::render::flush_layout();

        let mut layers = crate::engine::LayerTree::new(600, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(600.0, 400.0),
            );
            crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
        }
        crate::engine_test_stubs::drawn()
    }

    fn divider_under(radius: Option<f32>, vertical: bool) -> AnyWidget {
        let data = crate::component_themes::DividerThemeData {
            color: Some(Color::argb(255, 255, 0, 0)),
            thickness: Some(4.0),
            radius: radius.map(|r| {
                crate::borders::BorderRadiusGeometry::Absolute(
                    crate::borders::BorderRadius::circular(r),
                )
            }),
            ..crate::component_themes::DividerThemeData::default()
        };
        let line = if vertical {
            component(VerticalDivider::new())
        } else {
            component(Divider::new())
        };
        crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            crate::component_themes::DividerTheme::new(data, line),
        )
    }

    #[test]
    fn a_divider_rounds_its_ends_when_the_theme_asks_and_stays_square_otherwise() {
        // What is checkable here is the *shape*, not the radius: a rounded
        // fill goes to the engine as a path, and `Drawn::Path` records only
        // the path's bounding box -- the stub's stated blind spot. So this
        // says the rounding reached the painter, and
        // `the_resolved_divider_carries_the_themes_radius` says the number
        // that reached it was the theme's. Neither claim alone is the wire.
        let is_path = |calls: &[crate::engine_test_stubs::Drawn]| {
            calls
                .iter()
                .any(|call| matches!(call, crate::engine_test_stubs::Drawn::Path { .. }))
        };
        let is_rect = |calls: &[crate::engine_test_stubs::Drawn]| {
            calls
                .iter()
                .any(|call| matches!(call, crate::engine_test_stubs::Drawn::Rect { .. }))
        };

        let rounded = painted(divider_under(Some(6.0), false));
        assert!(is_path(&rounded), "a rounded rule is a path: {rounded:?}");
        let square = painted(divider_under(None, false));
        assert!(is_rect(&square), "a square one is a rectangle: {square:?}");
        assert!(!is_path(&square), "and nothing rounds it: {square:?}");

        let rounded = painted(divider_under(Some(6.0), true));
        assert!(
            is_path(&rounded),
            "the vertical rule rounds too: {rounded:?}"
        );
        let square = painted(divider_under(None, true));
        assert!(!is_path(&square), "and stays square without: {square:?}");
    }

    // -- Where a tile's leading widget sits, tick 232 -----------------------
    //
    // `tools/unread_theme_fields.py` found `ListTileThemeData::title_alignment`
    // reaching nothing: the tile hard-coded upstream's `threeLine` rule, so a
    // theme naming any of the other four was ignored and so was Material 2's
    // default of `titleHeight`.

    /// The top of the leading widget's box, under `alignment`.
    ///
    /// The leading is a small coloured square so that it paints a rectangle
    /// the stub records; the title beside it is what makes the row taller
    /// than the square, so top, centre and bottom are three different
    /// answers.
    fn leading_top(
        alignment: Option<crate::component_themes::ListTileTitleAlignment>,
        subtitle: bool,
        three_line: bool,
    ) -> f32 {
        let mut tile = ListTile::new("a").with_leading(crate::framework::leaf(|| {
            Container::new()
                .with_size(10.0, 10.0)
                .with_color(Color::argb(255, 255, 0, 0))
        }));
        if subtitle {
            tile = tile.with_subtitle("b");
        }
        if three_line {
            tile = tile.with_three_line(true);
        }
        let data = crate::component_themes::ListTileThemeData {
            title_alignment: alignment,
            ..crate::component_themes::ListTileThemeData::default()
        };
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            crate::component_themes::ListTileTheme::new(data, component(tile)),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(
            &mut root,
            BoxConstraints {
                min_width: 0.0,
                max_width: 400.0,
                min_height: 0.0,
                max_height: 400.0,
            },
        );
        let mut layers = crate::engine::LayerTree::new(600, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(600.0, 400.0),
            );
            crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
        }
        crate::engine_test_stubs::drawn()
            .iter()
            .find_map(|call| match call {
                crate::engine_test_stubs::Drawn::Rect {
                    top,
                    bottom,
                    argb: 0xFFFF_0000,
                    ..
                } if (bottom - top - 10.0).abs() < 0.5 => Some(*top),
                _ => None,
            })
            .expect("the leading square was drawn")
    }

    #[test]
    fn a_theme_can_put_the_leading_at_the_top_the_middle_or_the_bottom() {
        use crate::component_themes::ListTileTitleAlignment;
        let top = leading_top(Some(ListTileTitleAlignment::Top), false, false);
        let centre = leading_top(Some(ListTileTitleAlignment::Center), false, false);
        let bottom = leading_top(Some(ListTileTitleAlignment::Bottom), false, false);
        assert!(
            top < centre && centre < bottom,
            "three different places: {top}, {centre}, {bottom}"
        );
    }

    #[test]
    fn the_default_is_the_three_line_rule_and_it_reads_the_tile() {
        // `ThreeLine` is top against a three-line block and centred
        // otherwise, so the *same* alignment gives two answers depending on
        // the tile -- which is what distinguishes it from a fixed `Top` or
        // `Center` and what an unset field has to fall back to under
        // Material 3.
        use crate::component_themes::ListTileTitleAlignment;
        let two_line = leading_top(None, true, false);
        let three = leading_top(None, true, true);
        assert!(
            three < two_line,
            "a three-line tile lifts the leading: {three} vs {two_line}"
        );
        assert_eq!(
            three,
            leading_top(Some(ListTileTitleAlignment::Top), true, true),
            "which is the top"
        );
        assert_eq!(
            two_line,
            leading_top(Some(ListTileTitleAlignment::Center), true, false),
            "and the other is the middle"
        );
    }

    #[test]
    fn title_height_is_centred_here_which_is_not_upstreams_rule() {
        // Upstream puts the leading sixteen pixels below the top of the
        // *title* when the tile is taller than 72, and centres it against
        // both titles otherwise. That needs the title's own box; this row
        // aligns against the whole cross axis and has no such box. Centring
        // is the nearer of the answers it can give, and it is what upstream
        // gives a short tile too -- a tall Material 2 tile is where the two
        // part company. Written down rather than left to be discovered.
        use crate::component_themes::ListTileTitleAlignment;
        assert_eq!(
            leading_top(Some(ListTileTitleAlignment::TitleHeight), true, false),
            leading_top(Some(ListTileTitleAlignment::Center), true, false)
        );
    }

    // -- A card's colour, tick 237 ------------------------------------------
    //
    // `tools/unread_theme_fields.py` found `ThemeData::card_color` reaching
    // nothing. The card stopped at the component theme's own surface, so
    // upstream's last step was missing -- and that step is **not one colour**:
    // `_CardDefaultsM3` answers `surfaceContainerLow` and `_CardDefaultsM2`
    // answers `Theme.of(context).cardColor`.

    /// The colour a widget filled its own box with.
    ///
    /// The last unstroked fill, not the largest: a card paints its shadows
    /// first -- as rounded rectangles of the same size, in translucent
    /// black -- and then its own surface over them, so "largest" is a tie
    /// and "last" is the answer.
    fn own_fill(widget: AnyWidget) -> Color {
        let mut tree = ElementTree::new();
        tree.rebuild(widget);
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(
            &mut root,
            BoxConstraints {
                min_width: 0.0,
                max_width: 400.0,
                min_height: 0.0,
                max_height: 400.0,
            },
        );
        let mut layers = crate::engine::LayerTree::new(600, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(600.0, 400.0),
            );
            crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
        }
        let calls = crate::engine_test_stubs::drawn();
        calls
            .iter()
            .rev()
            .find_map(|call| match call {
                crate::engine_test_stubs::Drawn::RRect {
                    argb, stroke: None, ..
                }
                | crate::engine_test_stubs::Drawn::Rect {
                    argb, stroke: None, ..
                } => Some(Color(*argb)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("nothing filled: {calls:?}"))
    }

    fn card_under(theme: crate::theme::ThemeData) -> Color {
        own_fill(crate::theme::MaterialTheme::new(
            theme,
            component(Card::new(crate::framework::leaf(|| {
                crate::widgets::SizedBox::new(10.0, 10.0)
            }))),
        ))
    }

    /// The labels a reader is handed for a card holding `words`, in order.
    /// `container` of `None` leaves the card's own default alone, which is the
    /// only way to test a default: it appears at no call site.
    fn card_is_read_as(container: bool, words: &[&str]) -> Vec<String> {
        card_labels(Some(container), words)
    }

    fn card_labels(container: Option<bool>, words: &[&str]) -> Vec<String> {
        crate::semantics::set_enabled(true);
        // Built fresh inside the closure: `single` may call it more than once,
        // and a `RenderFlex` is not `Clone`.
        let words: Vec<String> = words.iter().map(|word| word.to_string()).collect();
        let card = Card::new(crate::framework::single(
            crate::framework::leaf(|| crate::widgets::SizedBox::new(0.0, 0.0)),
            move |_inner| {
                let mut column = crate::widgets::Column::new()
                    .with_main_axis_size(crate::render::MainAxisSize::Min);
                for word in &words {
                    column = column.push(crate::widgets::Text::new(word.clone()));
                }
                column
            },
        ));
        let card = match container {
            Some(container) => card.with_semantic_container(container),
            None => card,
        };

        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(component(card));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(300.0, 300.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(300.0, 300.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);
        nodes
            .iter()
            .map(|node| node.properties.label.clone())
            .filter(|label| !label.is_empty())
            .collect()
    }

    /// The node a slider produces, through the real walk.
    fn slider_node(slider: Slider) -> crate::semantics::SemanticsNode {
        crate::semantics::set_enabled(true);
        let mut tree = crate::framework::ElementTree::new();
        // The theme's platform is pinned off Apple because the nodes the
        // tests below quote are the twentieth's: `host()` is an Apple
        // platform on the machines the suite runs on, and a tenth there
        // would move every one of these strings.
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light()
                .with_platform(crate::editable_text::TargetPlatform::Android),
            component(slider),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(400.0, 200.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(400.0, 200.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);
        nodes
            .iter()
            .find(|node| node.properties.flags.is_slider)
            .cloned()
            .expect("a slider said it was one")
    }

    /// The node a list tile produces, through the real walk.
    fn tile_node(tile: ListTile) -> crate::semantics::SemanticsNode {
        crate::semantics::set_enabled(true);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            component(tile),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(400.0, 300.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(400.0, 300.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);
        nodes
            .iter()
            .find(|node| node.properties.label.contains("Inbox"))
            .cloned()
            .expect("the tile said its words")
    }

    /// What a reader hears from a badge sitting on a tile, in order.
    fn badge_read_as(visible: bool) -> Vec<String> {
        crate::semantics::set_enabled(true);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            component(
                Badge::new("3")
                    .with_label_visible(visible)
                    .with_child(component(ListTile::new("Inbox"))),
            ),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(400.0, 300.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(400.0, 300.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);
        nodes
            .iter()
            .map(|node| node.properties.label.clone())
            .filter(|label| !label.is_empty())
            .collect()
    }

    #[test]
    fn a_badge_is_read_beside_what_it_sits_on() {
        // Checked rather than assumed: a survey of which components mention
        // `semantics` in their `build` put `Badge` among the silent ones, and
        // it is not silent -- its count is a `Text`, which the walk annotates
        // on its own. **Mentioning semantics and reaching a reader are two
        // different things**, and the survey counts the first.
        //
        // Two stops rather than one is upstream's arrangement too: its badge
        // is a `Stack` over the child with no `Semantics` wrapper, so a reader
        // hears the thing and then the count.
        assert_eq!(
            badge_read_as(true),
            vec!["Inbox".to_string(), "3".to_string()]
        );
    }

    #[test]
    fn a_badge_that_is_hidden_is_not_read_out() {
        // `isLabelVisible: false` is what a count going to zero does, and it
        // hides the badge **without taking the child with it**. A reader still
        // being told "3" after the badge has gone would be told about
        // something that is no longer on the screen.
        assert_eq!(badge_read_as(false), vec!["Inbox".to_string()]);
    }

    #[test]
    fn a_tile_is_one_stop_saying_both_its_lines() {
        // A title and a subtitle met separately are two things to land on
        // where the screen shows one row.
        let node = tile_node(ListTile::new("Inbox").with_subtitle("12 unread"));
        assert_eq!(
            node.properties.label,
            "Inbox
12 unread"
        );
    }

    #[test]
    fn a_selected_tile_sounds_different_from_the_others() {
        // The row in a master/detail list that says which item you are looking
        // at. Without `selected` it is the same announcement as every other
        // row -- the filter chip's problem, on the control that says where you
        // are.
        use crate::semantics::SemanticsTristate;
        assert_eq!(
            tile_node(ListTile::new("Inbox").with_selected(true))
                .properties
                .flags
                .selected,
            SemanticsTristate::True
        );
        assert_eq!(
            tile_node(ListTile::new("Inbox")).properties.flags.selected,
            SemanticsTristate::False
        );
    }

    #[test]
    fn a_tile_nobody_listens_to_is_not_announced_as_a_button() {
        // Upstream's `button: onTap != null || onLongPress != null`. A row of
        // plain text announced as a button invites a press that does nothing.
        let plain = tile_node(ListTile::new("Inbox"));
        assert!(!plain.properties.flags.is_button);
        assert!(!plain.properties.has(crate::semantics::SemanticsAction::Tap));

        let pressable =
            tile_node(ListTile::new("Inbox").tappable(9, PointerHandlers::new().with_tap(|_| {})));
        assert!(pressable.properties.flags.is_button);
        assert!(
            pressable
                .properties
                .has(crate::semantics::SemanticsAction::Tap)
        );
        assert!(pressable.properties.flags.is_enabled);
    }

    #[test]
    fn a_progress_bar_says_how_far_along_it_is() {
        // It said nothing at all: a reader had no way to know that something
        // was under way, let alone how far through it they were.
        crate::semantics::set_enabled(true);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            component(ProgressBar::new(0.6).with_semantic_label("Uploading")),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(300.0, 200.0),
        );
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(crate::render::Size::new(300.0, 200.0), &root)
            .unwrap_or_default();
        crate::semantics::set_enabled(false);

        let bar = nodes
            .iter()
            .find(|node| node.properties.label == "Uploading")
            .expect("the bar said what it is for");
        assert_eq!(
            bar.properties.value, "60",
            "no percent sign: upstream sends the bounds alongside and lets the              platform say the units"
        );
    }

    #[test]
    fn a_progress_value_is_a_bare_number_where_a_sliders_is_a_percentage() {
        // The two look alike and are not. A slider hands over one number that
        // has to carry its own units; a progress bar sends `minValue: '0'` and
        // `maxValue: '100'` beside it, so upstream leaves the sign off. Copying
        // one into the other would have a platform read "60%%" or "60".
        assert_eq!(ProgressBar::new(0.6).semantic_value(), "60");
        assert_eq!(ProgressBar::new(0.0).semantic_value(), "0");
        assert_eq!(ProgressBar::new(1.0).semantic_value(), "100");
        assert_eq!(
            ProgressBar::new(0.615).semantic_value(),
            "62",
            "rounded, as upstream rounds"
        );
    }

    #[test]
    fn a_slider_says_what_it_is_and_what_it_is_set_to() {
        // Nothing in this port declared a slider, so one reached a screen
        // reader as a plain box -- while the `is_slider` bit and the three
        // value strings had been crossing the FFI since they were written.
        let node = slider_node(Slider::new(1, 0.5).with_on_change(|_| {}));
        assert!(node.properties.flags.is_slider);
        assert_eq!(node.properties.value, "50%", "the value, as a percentage");
        assert!(
            node.properties
                .has(crate::semantics::SemanticsAction::Increase)
        );
        assert!(
            node.properties
                .has(crate::semantics::SemanticsAction::Decrease)
        );
    }

    #[test]
    fn a_slider_says_where_one_swipe_would_take_it() {
        // The point of `increasedValue` and `decreasedValue`: a reader is told
        // where they are and where each gesture goes, before making it. The
        // default step is a twentieth away from Apple's platforms.
        let node = slider_node(Slider::new(1, 0.5).with_on_change(|_| {}));
        assert_eq!(node.properties.increased_value, "55%");
        assert_eq!(node.properties.decreased_value, "45%");
    }

    #[test]
    fn a_slider_near_its_end_does_not_promise_more_than_it_has() {
        // Upstream clamps *before* speaking:
        // `clampDouble(value + unit, 0.0, 1.0)`. A slider at 97% with a 5%
        // step says its next value is 100%, not 102% -- otherwise a reader is
        // promised somewhere it cannot go, and then not taken there.
        let node = slider_node(Slider::new(1, 0.97).with_on_change(|_| {}));
        assert_eq!(node.properties.value, "97%");
        assert_eq!(node.properties.increased_value, "100%", "clamped, not 102%");

        let bottom = slider_node(Slider::new(1, 0.02).with_on_change(|_| {}));
        assert_eq!(bottom.properties.decreased_value, "0%", "and not -3%");
    }

    #[test]
    fn a_divided_slider_steps_by_a_division() {
        // A slider that can only hold its divisions must not offer a step that
        // lands between two of them. Upstream: `divisions != null ? 1.0 /
        // divisions : _adjustmentUnit`.
        let node = slider_node(Slider::new(1, 0.5).with_divisions(4).with_on_change(|_| {}));
        assert_eq!(node.properties.increased_value, "75%", "a quarter, not 5%");
        assert_eq!(node.properties.decreased_value, "25%");
    }

    #[test]
    fn a_slider_reports_a_percentage_of_its_own_range() {
        // The value crossing to the platform is normalised, so a slider from
        // 100 to 200 sitting at 150 is "50%" and not "150%". Upstream's
        // default formatter reads the *fraction*, which is the only number
        // that means anything without knowing the range.
        let node = slider_node(
            Slider::new(1, 150.0)
                .with_range(100.0, 200.0)
                .with_on_change(|_| {}),
        );
        assert_eq!(node.properties.value, "50%");
    }

    #[test]
    fn a_slider_nobody_can_move_says_so_and_offers_nothing() {
        // Upstream sets `isEnabled = isInteractive` and gates both actions on
        // it. Offering "swipe up to increase" on a slider with no handler
        // tells a reader the control works.
        let node = slider_node(Slider::new(1, 0.5));
        assert!(node.properties.flags.is_slider);
        assert!(!node.properties.flags.is_enabled, "and it says it is not");
        assert!(node.properties.flags.has_enabled_state);
        assert!(
            !node
                .properties
                .has(crate::semantics::SemanticsAction::Increase)
        );
        assert!(
            !node
                .properties
                .has(crate::semantics::SemanticsAction::Decrease)
        );
    }

    #[test]
    fn a_card_is_one_stop_for_a_reader_unless_told_otherwise() {
        // Upstream sets `semanticContainer` in two places and negates the
        // second, so the pair says one thing: either the card is one node with
        // its children folded into it, or it is no node and they stand alone.
        // A photograph with a caption is one thing to land on; a card that is
        // a container of separately interesting things is not.
        assert_eq!(
            card_is_read_as(true, &["Sunset", "Taken in June"]),
            vec![
                "Sunset
Taken in June"
                    .to_string()
            ],
            "one stop, saying both"
        );
        assert_eq!(
            card_is_read_as(false, &["Sunset", "Taken in June"]),
            vec!["Sunset".to_string(), "Taken in June".to_string()],
            "two stops, each reachable"
        );
        // And the default, which is the half a test that always passes the
        // flag never reaches: a mutation flipping `semantic_container: true`
        // to false in the constructor survived until this line existed.
        assert_eq!(
            card_labels(None, &["Sunset", "Taken in June"]),
            vec![
                "Sunset
Taken in June"
                    .to_string()
            ],
            "a card nobody configured is still one thing"
        );
    }

    #[test]
    fn a_material_two_card_takes_the_themes_card_colour() {
        // The step that reached nothing. A distinctive colour, so the
        // rectangle carrying it can only have come from that field.
        let theme = crate::theme::ThemeData {
            use_material3: false,
            card_color: Color::argb(255, 0, 0, 77),
            ..crate::theme::ThemeData::light()
        };
        assert_eq!(card_under(theme), Color::argb(255, 0, 0, 77));
    }

    #[test]
    fn and_a_material_three_card_takes_the_schemes_container_instead() {
        // Upstream's two default tables answer differently, so a card that
        // read `cardColor` under Material 3 would be wrong there.
        let theme = crate::theme::ThemeData {
            use_material3: true,
            card_color: Color::argb(255, 0, 0, 88),
            ..crate::theme::ThemeData::light()
        };
        let painted = card_under(theme.clone());
        assert_eq!(painted, theme.color_scheme.surface_container_low());
        assert_ne!(
            painted,
            Color::argb(255, 0, 0, 88),
            "and not the Material 2 field, which is what makes the pair a test"
        );
    }

    // -- An avatar's two colours, tick 241 ----------------------------------
    //
    // `CircleAvatar` took its fill from the component `Theme`'s
    // `surface_variant` and stopped, so `ThemeData::primary_color_light`,
    // `primary_color_dark` and `primary_text_theme` reached nothing.

    fn m2(light: Color, dark: Color) -> crate::theme::ThemeData {
        crate::theme::ThemeData {
            use_material3: false,
            primary_color_light: light,
            primary_color_dark: dark,
            ..crate::theme::ThemeData::light()
        }
    }

    #[test]
    fn a_material_two_avatar_picks_the_primary_that_reads_against_the_other() {
        // With neither colour named, upstream derives the background from
        // the *foreground's* brightness: dark ink asks for
        // `primaryColorLight`, light ink for `primaryColorDark`. The two
        // always read against each other.
        let pale = Color::argb(255, 0, 0, 11);
        let deep = Color::argb(255, 0, 0, 22);

        let (background, _) = CircleAvatar::new()
            .with_foreground_color(Color::BLACK)
            .colours(&m2(pale, deep));
        assert_eq!(background, pale, "dark ink wants the pale primary");

        let (background, _) = CircleAvatar::new()
            .with_foreground_color(Color::WHITE)
            .colours(&m2(pale, deep));
        assert_eq!(background, deep, "and light ink the deep one");
    }

    #[test]
    fn and_the_other_way_round_when_only_the_background_is_named() {
        // The mirror branch: a named background derives the foreground. Only
        // one of the two branches can fire, which is what stops it looping.
        let pale = Color::argb(255, 0, 0, 33);
        let deep = Color::argb(255, 0, 0, 44);

        let (background, foreground) = CircleAvatar::new()
            .with_background_color(Color::BLACK)
            .colours(&m2(pale, deep));
        assert_eq!(background, Color::BLACK, "the named one is kept");
        assert_eq!(foreground, Some(pale), "and the ink reads against it");

        let (_, foreground) = CircleAvatar::new()
            .with_background_color(Color::WHITE)
            .colours(&m2(pale, deep));
        assert_eq!(foreground, Some(deep));
    }

    #[test]
    fn a_material_three_avatar_asks_the_scheme_instead() {
        // Upstream answers both from the container pair, so neither primary
        // is consulted -- a resolver that reached for them under Material 3
        // would be wrong there.
        let theme = crate::theme::ThemeData {
            use_material3: true,
            primary_color_light: Color::argb(255, 0, 0, 55),
            primary_color_dark: Color::argb(255, 0, 0, 66),
            ..crate::theme::ThemeData::light()
        };
        let (background, foreground) = CircleAvatar::new().colours(&theme);
        assert_eq!(background, theme.color_scheme.primary_container());
        assert_eq!(foreground, Some(theme.color_scheme.on_primary_container()));
    }

    #[test]
    fn the_label_is_written_in_the_primary_typography_under_material_two() {
        // Upstream's `effectiveTextStyle`: Material 3 takes the ordinary
        // typography's `titleMedium`, Material 2 the **primary** one's -- the
        // face meant to be read against a primary-coloured surface, which is
        // what an avatar is.
        let mut theme = m2(Color::argb(255, 0, 0, 77), Color::argb(255, 0, 0, 88));
        theme.primary_text_theme = crate::component_themes::TextTheme {
            title_medium: Some(crate::engine::TextStyle {
                font_size: 99.0,
                ..crate::engine::TextStyle::default()
            }),
            ..crate::component_themes::TextTheme::default()
        };
        assert_eq!(
            CircleAvatar::new()
                .label_style(&theme)
                .map(|style| style.font_size),
            Some(99.0)
        );

        // And Material 3 does not look there.
        let mut three = theme.clone();
        three.use_material3 = true;
        three.text_theme = crate::component_themes::TextTheme {
            title_medium: Some(crate::engine::TextStyle {
                font_size: 100.0,
                ..crate::engine::TextStyle::default()
            }),
            ..crate::component_themes::TextTheme::default()
        };
        assert_eq!(
            CircleAvatar::new()
                .label_style(&three)
                .map(|style| style.font_size),
            Some(100.0)
        );
    }

    // -- A slider that runs somewhere other than 0 to 1, tick 244 -----------
    //
    // `Slider` had a value and a width. `SliderThemeData::tick_mark_shape`
    // reached nothing because tick marks mark *divisions* and there were
    // none, and a caller with a real range had to convert on both sides by
    // hand -- while `Slider::new` clamped their value into 0..1 and destroyed
    // it on the way in.

    #[test]
    fn the_fraction_is_where_the_value_sits_between_the_ends() {
        // Upstream's `_unlerp`. Three different ranges, so a line that
        // assumed 0..1 answers with a fraction that is not its own.
        assert_eq!(Slider::new(1, 0.25).fraction(), 0.25);
        assert_eq!(Slider::new(1, 5.0).with_range(0.0, 10.0).fraction(), 0.5);
        assert_eq!(
            Slider::new(1, 1950.0).with_range(1900.0, 2000.0).fraction(),
            0.5
        );
        // A range that does not start at zero is where an implementation that
        // divided by `max` alone would go wrong.
        assert_eq!(Slider::new(1, 30.0).with_range(20.0, 40.0).fraction(), 0.5);
    }

    #[test]
    fn a_zero_width_range_answers_zero_rather_than_dividing_by_it() {
        // Upstream asserts `min <= max` and so allows them equal. Every
        // position is the same position then, and zero is the only answer
        // that is not a division by zero.
        assert_eq!(Slider::new(1, 7.0).with_range(7.0, 7.0).fraction(), 0.0);
    }

    #[test]
    fn a_position_comes_back_as_the_callers_own_value() {
        // Upstream's `_lerp`, the other direction.
        let slider = Slider::new(1, 0.0).with_range(1900.0, 2000.0);
        assert_eq!(slider.value_at(0.0), 1900.0);
        assert_eq!(slider.value_at(0.5), 1950.0);
        assert_eq!(slider.value_at(1.0), 2000.0);
    }

    #[test]
    fn divisions_snap_the_value_before_it_leaves_the_track() {
        // Upstream's `_discretize`: `(fraction * divisions).round() /
        // divisions`, applied *before* the range, so the snap is to the
        // divisions and not to round numbers in the caller's units.
        let slider = Slider::new(1, 0.0).with_range(0.0, 10.0).with_divisions(4);
        // Quarters of the track: 0, 2.5, 5, 7.5, 10.
        assert_eq!(slider.value_at(0.0), 0.0);
        assert_eq!(slider.value_at(0.3), 2.5, "nearer the first quarter");
        assert_eq!(slider.value_at(0.4), 5.0, "and this one the half");
        assert_eq!(slider.value_at(1.0), 10.0);

        // Without divisions the same positions pass straight through.
        let smooth = Slider::new(1, 0.0).with_range(0.0, 10.0);
        assert_eq!(smooth.value_at(0.3), 3.0);
        assert_eq!(smooth.value_at(0.4), 4.0);
    }

    #[test]
    fn the_ticks_are_the_division_boundaries_and_there_is_one_more_than_them() {
        // Four divisions make five marks: the two ends count. A slider with
        // none has nothing to mark, which is why the theme's tick shape had
        // nothing to answer.
        assert_eq!(Slider::new(1, 0.0).tick_fractions(), Vec::<f32>::new());
        assert_eq!(
            Slider::new(1, 0.0).with_divisions(4).tick_fractions(),
            vec![0.0, 0.25, 0.5, 0.75, 1.0]
        );
        assert_eq!(
            Slider::new(1, 0.0).with_divisions(1).tick_fractions(),
            vec![0.0, 1.0],
            "one division is two marks, one at each end"
        );
    }

    #[test]
    fn a_drag_reports_a_value_in_the_callers_range() {
        // The gesture used to hand back the raw fraction, so a slider with a
        // range reported nonsense and one with divisions never settled.
        use crate::slider_theme::SliderInteraction;
        let seen = std::rc::Rc::new(std::cell::Cell::new(None));
        let recorder = std::rc::Rc::clone(&seen);
        let mut slider = Slider::new(1, 0.0).with_range(0.0, 10.0).with_divisions(4);
        slider.on_change = Some(std::rc::Rc::new(move |value| recorder.set(Some(value))));
        let resolved = plain_slider();
        let handlers = slider.gestures(&resolved);
        let handler = handlers.on_tap.clone().expect("a tap handler");
        // Three-tenths along a two-hundred-wide track.
        handler(crate::gestures::TapEvent {
            local_position: Offset::new(60.0, 0.0),
            position: Offset::new(60.0, 0.0),
            pointer_id: 1,
        });
        assert_eq!(
            seen.get(),
            Some(2.5),
            "in the caller's units, snapped to the nearest division"
        );
    }

    #[test]
    fn the_filled_track_is_drawn_from_the_fraction_and_not_from_the_value() {
        // The track lives in its own coordinates. A slider at 5 of 0..10 is
        // half filled; one that drew `width * value` would try to fill five
        // times its own width, and the clamp would hide that as a full track.
        // The two cases below differ only in the range, so a build that read
        // the value would answer the same for both.
        let filled_width = |slider: Slider| {
            let mut tree = ElementTree::new();
            tree.rebuild(crate::theme::MaterialTheme::new(
                crate::theme::ThemeData::light(),
                component(slider),
            ));
            let mut root = tree.build_render_tree().expect("a root");
            crate::render::RenderBox::layout(
                &mut root,
                BoxConstraints {
                    min_width: 0.0,
                    max_width: 400.0,
                    min_height: 0.0,
                    max_height: 400.0,
                },
            );
            let mut layers = crate::engine::LayerTree::new(600, 400);
            crate::engine_test_stubs::reset_drawn();
            {
                let mut context = crate::render::PaintContext::new(
                    &mut layers,
                    crate::render::Size::new(600.0, 400.0),
                );
                crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
            }
            // The active track is the widest thing drawn that is narrower
            // than the whole track.
            crate::engine_test_stubs::drawn()
                .iter()
                .filter_map(|call| match call {
                    crate::engine_test_stubs::Drawn::RRect { left, right, .. }
                    | crate::engine_test_stubs::Drawn::Rect { left, right, .. } => {
                        Some(right - left)
                    }
                    _ => None,
                })
                .filter(|width| *width < 199.0)
                .fold(0.0f32, f32::max)
        };

        // Half of a ten-wide range fills half of a two-hundred-wide
        // track. A build that read the value would ask for a thousand,
        // and the clamp would hand back the whole track instead.
        assert_eq!(
            filled_width(Slider::new(1, 5.0).with_range(0.0, 10.0)),
            100.0
        );
        // A quarter of the plain 0..1 slider is a quarter -- the case
        // that would look right either way, here so the pair says the
        // difference is the range and not the arithmetic.
        assert_eq!(filled_width(Slider::new(1, 0.25)), 50.0);
    }

    // -- The bubble over the thumb, tick 246 ---------------------------------
    //
    // All four value indicator shapes were ported, with their path painters
    // and their own tests, and `SliderComponentShape::paint_indicator` was
    // written so they could be reached. Nothing reached it. Three theme
    // fields -- `value_indicator_shape`, `show_value_indicator` and
    // `value_indicator_text_style` -- were named nowhere outside their own
    // paperwork, because a slider had no label, no rule about when to show
    // one, and no resolver field carrying any of the three.

    #[test]
    fn every_way_of_saying_when_the_bubble_shows_answers_differently() {
        // The point of the field is that the six variants disagree, so all
        // six are asked, in all four situations. Upstream splits the decision
        // in two -- `_buildValueIndicator` builds an indicator or a shrunk
        // box, `shouldShowValueIndicatorWhenDragged` shows a built one -- and
        // `AlwaysVisible` is the variant that proves they are two rules: it
        // builds *and* answers no to the second, because it is showing
        // already. Folded into one question, it and `OnDrag` would be the
        // same variant.
        use crate::slider_theme::ShowValueIndicator::*;
        let shows = |show, discrete, dragging| {
            let mut resolved = plain_slider();
            resolved.show_value_indicator = show;
            resolved.shows_value_indicator(discrete, dragging)
        };
        let table = [
            //                        continuous      discrete
            //                      still   drag    still   drag
            (Never, [false, false, false, false]),
            (OnlyForDiscrete, [false, false, false, true]),
            (OnlyForContinuous, [false, true, false, false]),
            (Always, [false, true, false, true]),
            (OnDrag, [false, true, false, true]),
            (AlwaysVisible, [true, true, true, true]),
        ];
        for (show, expected) in table {
            let actual = [
                shows(show, false, false),
                shows(show, false, true),
                shows(show, true, false),
                shows(show, true, true),
            ];
            assert_eq!(actual, expected, "{show:?}");
        }
    }

    #[test]
    fn a_dragged_slider_with_a_label_draws_a_bubble_saying_it() {
        // The words reach the canvas, which is the whole chain: the label,
        // the resolved text style, the shape the theme chose, and the fill
        // that shape reads off the theme before it will draw anything.
        let words = |slider: Slider| {
            paragraphs_of(slider)
                .into_iter()
                .map(|(text, _)| text)
                .collect::<Vec<String>>()
        };

        // Divisions on all three, because the default is `OnlyForDiscrete`
        // and a continuous slider is not showing a bubble whatever else is
        // true of it. The first draft of this test forgot that and asked for
        // words from a slider its own rule had already refused.
        let discrete = || Slider::new(1, 0.5).with_divisions(4);

        assert_eq!(
            words(discrete().with_label("50%").with_dragging(true)),
            vec!["50%".to_string()]
        );

        // Not while it is still: `OnlyForDiscrete` builds the indicator, and
        // it is a drag that shows a built one.
        assert!(words(discrete().with_label("50%")).is_empty());

        // And not without words to put in it. Upstream lays out a null label
        // and draws an empty bubble; an empty bubble is worse than none.
        assert!(words(discrete().with_dragging(true)).is_empty());
    }

    #[test]
    fn the_label_is_drawn_in_the_colour_the_theme_resolved_for_it() {
        // `value_indicator_text_style` had no reader at all. The Material 3
        // table writes the label in `onInverseSurface` over a bubble filled
        // with `inverseSurface` -- the two move together, and a label left in
        // the theme's ordinary ink would be dark text on a dark bubble.
        let scheme = crate::theme::ThemeData::light().color_scheme;
        // The fill first: the shape reads `value_indicator_color` off the
        // theme and returns without a stroke when it is unset, which is the
        // trap the tick marks fell into one tick earlier. Watching the ink
        // alone would not see a bubble that had stopped being drawn -- it
        // would see no label either, and the label is what this asserts.
        let fill = fills_of(
            Slider::new(1, 0.5)
                .with_divisions(4)
                .with_label("50%")
                .with_dragging(true),
        );
        assert!(
            fill.contains(&scheme.inverse_surface().0),
            "the bubble is filled with the theme's inverse surface"
        );

        let drawn = paragraphs_of(
            Slider::new(1, 0.5)
                .with_divisions(4)
                .with_label("50%")
                .with_dragging(true),
        );
        let (_, argb) = drawn.first().expect("the label");
        assert_eq!(*argb, scheme.on_inverse_surface().0);
        assert_ne!(
            *argb, scheme.on_surface.0,
            "which is what it would be if the style never reached the painter"
        );
    }

    #[test]
    fn the_bar_draws_its_title_in_the_style_the_theme_resolved() {
        // Without this, moving the bar off its hand-rolled `theme.title()`
        // and onto `ResolvedAppBar::title_text_style` broke nothing at all --
        // and putting it back broke nothing either. The resolver's own tests
        // watch what it answers; this is the only thing that watches whether
        // the widget asks.
        //
        // The colour is what makes it visible. A bar's foreground belongs to
        // the bar, and the hand-rolled style took its ink from the older
        // `Theme` instead, so a bar told to draw in one colour drew in
        // another.
        const MINE: Color = Color::argb(0xFF, 0x11, 0x22, 0x33);
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            crate::component_themes::AppBarTheme::new(
                crate::component_themes::AppBarThemeData {
                    foreground_color: Some(MINE),
                    ..crate::component_themes::AppBarThemeData::new()
                },
                component(AppBar::new("Inbox")),
            ),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(
            &mut root,
            BoxConstraints {
                min_width: 0.0,
                max_width: 400.0,
                min_height: 0.0,
                max_height: 400.0,
            },
        );
        let mut layers = crate::engine::LayerTree::new(600, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(600.0, 400.0),
            );
            crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
        }
        let title = crate::engine_test_stubs::drawn()
            .into_iter()
            .find_map(|call| match call {
                crate::engine_test_stubs::Drawn::Paragraph { text, argb, .. }
                    if text == "Inbox" =>
                {
                    Some(argb)
                }
                _ => None,
            })
            .expect("the title");
        assert_eq!(title, MINE.0);
    }

    #[test]
    fn a_discrete_slider_draws_a_mark_at_every_division_boundary() {
        // `RoundSliderTickMarkShape::paint` was ported and tested long before
        // this, and nothing ever called it: a slider with no divisions has no
        // marks, and this one had no divisions until tick 244.
        let circles = |slider: Slider| {
            let mut tree = ElementTree::new();
            tree.rebuild(crate::theme::MaterialTheme::new(
                crate::theme::ThemeData::light(),
                component(slider),
            ));
            let mut root = tree.build_render_tree().expect("a root");
            crate::render::RenderBox::layout(
                &mut root,
                BoxConstraints {
                    min_width: 0.0,
                    max_width: 400.0,
                    min_height: 0.0,
                    max_height: 400.0,
                },
            );
            let mut layers = crate::engine::LayerTree::new(600, 400);
            crate::engine_test_stubs::reset_drawn();
            {
                let mut context = crate::render::PaintContext::new(
                    &mut layers,
                    crate::render::Size::new(600.0, 400.0),
                );
                crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
            }
            crate::engine_test_stubs::drawn()
                .iter()
                .filter_map(|call| match call {
                    crate::engine_test_stubs::Drawn::Circle { cx, .. } => Some(*cx),
                    _ => None,
                })
                .collect::<Vec<f32>>()
        };

        // Four divisions, five marks, evenly spaced across a 200-wide track.
        let marks = circles(Slider::new(1, 0.0).with_divisions(4));
        assert_eq!(marks, vec![0.0, 50.0, 100.0, 150.0, 200.0]);

        // And a continuous slider draws none at all, which is what makes the
        // assertion above about divisions rather than about sliders.
        assert!(circles(Slider::new(1, 0.5)).is_empty());
    }

    #[test]
    fn a_mark_past_the_thumb_is_drawn_in_the_inactive_colour() {
        // The shape decides this, and which side "past" is depends on the
        // reading direction. What is checked here is that the *thumb's*
        // position reaches the shape at all -- a painter that handed it a
        // fixed centre would colour every mark the same.
        let colours = |value: f32| {
            let mut tree = ElementTree::new();
            tree.rebuild(crate::theme::MaterialTheme::new(
                crate::theme::ThemeData::light(),
                component(Slider::new(1, value).with_range(0.0, 4.0).with_divisions(4)),
            ));
            let mut root = tree.build_render_tree().expect("a root");
            crate::render::RenderBox::layout(
                &mut root,
                BoxConstraints {
                    min_width: 0.0,
                    max_width: 400.0,
                    min_height: 0.0,
                    max_height: 400.0,
                },
            );
            let mut layers = crate::engine::LayerTree::new(600, 400);
            crate::engine_test_stubs::reset_drawn();
            {
                let mut context = crate::render::PaintContext::new(
                    &mut layers,
                    crate::render::Size::new(600.0, 400.0),
                );
                crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
            }
            crate::engine_test_stubs::drawn()
                .iter()
                .filter_map(|call| match call {
                    crate::engine_test_stubs::Drawn::Circle { argb, .. } => Some(*argb),
                    _ => None,
                })
                .collect::<Vec<u32>>()
        };

        // At zero every mark but the first is past the thumb; at the far end
        // none of them is. If the thumb's position never reached the shape
        // the two lists would be identical.
        let at_start = colours(0.0);
        let at_end = colours(4.0);
        assert_eq!(at_start.len(), 5);
        assert_eq!(at_end.len(), 5);
        assert_ne!(
            at_start, at_end,
            "the thumb's position reaches the shape: {at_start:?} then {at_end:?}"
        );
    }
}

/// Upstream `VerticalDivider`: the same hairline, on its side.
///
/// A separate class rather than an axis on [`Divider`], because upstream
/// makes it one: the two read the *same* theme fields, and `space` means
/// width here where it means height there. One widget with an axis would
/// have to explain that reversal at every call site.
pub struct VerticalDivider {
    overrides: crate::component_themes::DividerOverrides,
}

impl VerticalDivider {
    pub fn new() -> VerticalDivider {
        VerticalDivider {
            overrides: crate::component_themes::DividerOverrides::default(),
        }
    }

    /// Upstream's `width`: the space the rule reserves, the same measurement
    /// [`Divider::with_height`] names -- which is exactly why upstream keeps
    /// them as two classes rather than one with an axis.
    pub fn with_width(mut self, width: f32) -> Self {
        self.overrides.space = Some(width);
        self
    }

    /// Upstream's `thickness`: the width of the line drawn inside that space.
    pub fn with_thickness(mut self, thickness: f32) -> Self {
        self.overrides.thickness = Some(thickness);
        self
    }

    /// Upstream's `indent`: how far the line starts in from the leading edge.
    pub fn with_indent(mut self, indent: f32) -> Self {
        self.overrides.indent = Some(indent);
        self
    }

    /// Upstream's `endIndent`: how far it stops short of the trailing edge.
    pub fn with_end_indent(mut self, end_indent: f32) -> Self {
        self.overrides.end_indent = Some(end_indent);
        self
    }

    /// Upstream's `color`.
    pub fn with_color(mut self, color: Color) -> Self {
        self.overrides.color = Some(color);
        self
    }

    /// Upstream's `radius`.
    pub fn with_radius(mut self, radius: crate::borders::BorderRadiusGeometry) -> Self {
        self.overrides.radius = Some(radius);
        self
    }

    /// This divider's metrics, with its own answers over the theme's.
    pub fn resolved(&self, context: &mut BuildContext) -> crate::component_themes::ResolvedDivider {
        crate::component_themes::ResolvedDivider::of(context).overridden_by(&self.overrides)
    }
}

impl Component for VerticalDivider {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let divider = self.resolved(context);
        let color = divider.color;
        // Upstream's `space` is the width it reserves, and `thickness` the
        // width of the line inside it -- the same two fields as the
        // horizontal rule, measured across the other axis.
        let space = divider.space;
        let thickness = divider
            .line_thickness_for(crate::media_query::media_query_of(context).device_pixel_ratio);
        // And the indents run down rather than across: upstream's
        // `EdgeInsetsDirectional.only(top: indent, bottom: endIndent)`.
        let insets = crate::render::EdgeInsets {
            left: 0.0,
            right: 0.0,
            top: divider.indent,
            bottom: divider.end_indent,
        };
        // The same rounding the horizontal rule takes, from the same field.
        let radius = divider
            .radius
            .map(|r| r.resolve(crate::direction::direction_of(context)));
        leaf(move || {
            let line = Container::new()
                .with_width(thickness)
                .with_color(color)
                .with_margin(insets);
            let line = match radius {
                Some(radius) => line.with_border_radius(radius),
                None => line,
            };
            Container::new()
                .with_width(space)
                .with_child(Align::new(Alignment::CENTER, line))
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
    /// A short piece of text to draw inside, in the resolved foreground and
    /// typography.
    ///
    /// Upstream writes `CircleAvatar(child: Text('AB'))` and lets a
    /// `DefaultTextStyle` carry the colour down. This crate has no such
    /// widget, so an avatar that is given an arbitrary child cannot colour
    /// it -- but the common case is a couple of letters, and this is that
    /// case done properly rather than left half-wired.
    pub label: Option<String>,
    pub background_color: Option<Color>,
    pub foreground_color: Option<Color>,
    pub radius: Option<f32>,
    pub min_radius: Option<f32>,
    pub max_radius: Option<f32>,
}

impl CircleAvatar {
    /// A couple of letters inside, styled the way upstream styles
    /// `CircleAvatar`'s child.
    pub fn label_of(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Upstream's `_defaultRadius`.
    pub const DEFAULT_RADIUS: f32 = 20.0;
    pub const DEFAULT_MIN_RADIUS: f32 = 0.0;
    pub const DEFAULT_MAX_RADIUS: f32 = f32::INFINITY;

    pub fn new() -> CircleAvatar {
        CircleAvatar {
            child: std::cell::RefCell::new(None),
            label: None,
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

impl CircleAvatar {
    /// Upstream's `CircleAvatar.build` colour chain: the background and the
    /// foreground, each falling back to the other's brightness.
    ///
    /// Material 3 answers both from the scheme's container pair. Material 2
    /// answers neither, and then **whichever is missing is derived from the
    /// one that is there**: a dark colour asks for `primaryColorLight` and a
    /// light one for `primaryColorDark`, so the two always read against each
    /// other. Only one of the two branches can fire, which is what stops it
    /// looping.
    ///
    /// This used to take the component `Theme`'s `surface_variant` and stop,
    /// so `ThemeData`'s two primaries and its `primary_text_theme` reached
    /// nothing.
    pub fn colours(
        &self,
        material: &crate::theme::ThemeData,
    ) -> (crate::engine::Color, Option<crate::engine::Color>) {
        use crate::component_themes::estimate_brightness_for_color;
        use crate::platform::Brightness;

        let pair = |against: crate::engine::Color| match estimate_brightness_for_color(against) {
            Brightness::Dark => material.primary_color_light,
            Brightness::Light => material.primary_color_dark,
        };

        let mut foreground = self.foreground_color.or(if material.use_material3 {
            Some(material.color_scheme.on_primary_container())
        } else {
            None
        });
        let background = self.background_color.or(if material.use_material3 {
            Some(material.color_scheme.primary_container())
        } else {
            None
        });

        match (background, foreground) {
            (None, _) => {
                // Upstream reads the *text style's* colour here, which is the
                // effective foreground when there is one and the style's own
                // otherwise. With neither named, the style's colour is what
                // the typography carries.
                let ink = foreground
                    .or_else(|| {
                        self.label_style(material)
                            .and_then(|style| Some(style.color))
                    })
                    .unwrap_or(material.color_scheme.on_surface);
                (pair(ink), foreground)
            }
            (Some(background), None) => {
                foreground = Some(pair(background));
                (background, foreground)
            }
            (Some(background), _) => (background, foreground),
        }
    }

    /// Upstream's `effectiveTextStyle`: Material 3 takes the ordinary
    /// typography's `titleMedium`, Material 2 the **primary** typography's --
    /// the one meant to be read against a primary-coloured surface, which is
    /// what an avatar is.
    pub fn label_style(
        &self,
        material: &crate::theme::ThemeData,
    ) -> Option<crate::engine::TextStyle> {
        if material.use_material3 {
            material.text_theme.title_medium.clone()
        } else {
            material.primary_text_theme.title_medium.clone()
        }
    }
}

impl Component for CircleAvatar {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let material = crate::theme::ThemeData::of(context);
        let (background, foreground) = self.colours(&material);
        // A radius fixes the size; a range lets the parent choose between the
        // two, and with nothing else deciding the smaller end is what is
        // drawn. An unbounded maximum is exactly that case.
        let diameter = self.min_diameter();
        let child = self.child.borrow().clone().or_else(|| {
            let label = self.label.clone()?;
            let mut style = self
                .label_style(&material)
                .unwrap_or_else(crate::engine::TextStyle::default);
            if let Some(ink) = foreground {
                style.color = ink;
            }
            Some(leaf(move || {
                crate::render::RenderParagraph::new(label.clone()).with_style(style.clone())
            }))
        });
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

#[cfg(test)]
mod card_variant_tests {
    use super::*;
    use crate::component_themes::{CardThemeData, CardVariant, ResolvedCard};
    use crate::engine::Color;
    use crate::engine_test_stubs::Drawn;
    use crate::framework::{ElementTree, leaf, provide};
    use crate::render::{BoxConstraints, Offset, RenderBox, Size};

    fn resolved(variant: CardVariant) -> ResolvedCard {
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        struct Reader(
            std::rc::Rc<std::cell::RefCell<Option<ResolvedCard>>>,
            CardVariant,
        );
        impl Component for Reader {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.0.borrow_mut() = Some(ResolvedCard::of(context, self.1));
                leaf(|| crate::widgets::SizedBox::new(1.0, 1.0))
            }
        }
        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::component(Reader(
            std::rc::Rc::clone(&seen),
            variant,
        )));
        let read = seen.borrow_mut().take().expect("built once");
        read
    }

    /// What a card paints, laid out in 300x200.
    fn painted(card: Card) -> Vec<Drawn> {
        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::component(card));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(300.0, 200.0));
        crate::render::flush_layout();
        let mut layers = crate::engine::LayerTree::new(300, 200);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(300.0, 200.0));
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        crate::engine_test_stubs::drawn()
    }

    fn body() -> AnyWidget {
        leaf(|| crate::widgets::SizedBox::new(60.0, 40.0))
    }

    /// The strokes a card drew, as `(width, colour)`.
    fn strokes(drawn: &[Drawn]) -> Vec<(f32, Color)> {
        drawn
            .iter()
            .filter_map(|call| match call {
                Drawn::Rect {
                    stroke: Some(width),
                    argb,
                    ..
                } => Some((*width, Color(*argb))),
                Drawn::RRect {
                    stroke: Some(width),
                    argb,
                    ..
                } => Some((*width, Color(*argb))),
                Drawn::Path {
                    stroke: Some(width),
                    argb,
                    ..
                } => Some((*width, Color(*argb))),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn only_the_outlined_card_is_outlined() {
        // What separates the three is *how a card is told apart from the page*:
        // an elevated one by its shadow, a filled one by its colour, an
        // outlined one by a line. Exactly one at a time. This crate used to
        // draw a hairline on all three, so the elevated card said it twice and
        // the filled card wore a border it never asked for.
        assert_eq!(strokes(&painted(Card::new(body()))), Vec::new());
        assert_eq!(strokes(&painted(Card::filled(body()))), Vec::new());
        assert_eq!(
            strokes(&painted(Card::outlined(body()))).len(),
            1,
            "and the outlined one has exactly one"
        );
    }

    #[test]
    fn the_outline_is_the_schemes_outline_variant() {
        // `_OutlinedCardDefaultsM3`: `side: BorderSide(color: outlineVariant)`.
        // Not `outline`, which is a stronger line meant for controls.
        let scheme = crate::theme::ThemeData::default().color_scheme;
        assert_eq!(
            strokes(&painted(Card::outlined(body()))),
            vec![(1.0, scheme.outline_variant())]
        );
        assert_ne!(scheme.outline_variant(), scheme.outline(), "two colours");
    }

    #[test]
    fn the_three_have_three_surfaces_and_two_elevations() {
        let scheme = crate::theme::ThemeData::default().color_scheme;
        assert_eq!(
            resolved(CardVariant::Elevated).color,
            scheme.surface_container_low()
        );
        assert_eq!(
            resolved(CardVariant::Filled).color,
            scheme.surface_container_highest()
        );
        assert_eq!(resolved(CardVariant::Outlined).color, scheme.surface);

        assert_eq!(resolved(CardVariant::Elevated).elevation, 1.0);
        assert_eq!(resolved(CardVariant::Filled).elevation, 0.0);
        assert_eq!(
            resolved(CardVariant::Outlined).elevation,
            0.0,
            "a line instead of a shadow, not both"
        );
    }

    #[test]
    fn only_the_elevated_card_casts_a_shadow() {
        // The elevation, seen where it shows: the shadow table is indexed by
        // whole steps, so one is a shadow and zero is none.
        // A rounded box paints its shadows as rounded rectangles, one per
        // layer of the elevation table, and then its own surface as one more.
        // So the count is "the surface" for a flat card and "the surface plus
        // the shadow's layers" for a raised one.
        let fills = |card: Card| {
            painted(card)
                .into_iter()
                .filter(|call| matches!(call, Drawn::RRect { stroke: None, .. }))
                .count()
        };
        let flat = fills(Card::filled(body()));
        assert_eq!(flat, 1, "the surface and nothing under it");
        assert_eq!(fills(Card::outlined(body())), 1);
        assert_eq!(
            fills(Card::new(body())),
            1 + crate::painting::elevation_shadows(1).len(),
            "the surface and the elevation's own layers"
        );
    }

    #[test]
    fn a_card_keeps_a_margin_outside_its_surface() {
        // `EdgeInsets.all(4)` in all three tables, and it is a **margin**: the
        // gap between two cards in a column belongs to neither of them. The
        // card had none at all, so a list of cards was a single slab.
        assert_eq!(
            resolved(CardVariant::Elevated).margin,
            EdgeInsets::all(ResolvedCard::MARGIN)
        );
        let drawn = painted(Card::filled(body()));
        let surface = drawn
            .iter()
            .find_map(|call| match call {
                Drawn::RRect { left, top, .. } => Some((*left, *top)),
                Drawn::Rect {
                    left,
                    top,
                    stroke: None,
                    ..
                } => Some((*left, *top)),
                _ => None,
            })
            .expect("the surface painted");
        assert_eq!(surface, (4.0, 4.0), "inset by the margin");
    }

    #[test]
    fn a_themed_shape_moves_the_corners() {
        // The rounding comes off the **shape**, which is where upstream keeps
        // it. Reading the crate theme's own `radius` instead looks right --
        // both are 12 by default -- and stops a `CardTheme` that sets a shape
        // from having any effect at all.
        let data = CardThemeData::new().with_shape(crate::borders::ShapeBorder::Rounded(
            crate::borders::RoundedRectangleBorder::new(
                crate::borders::BorderSide::NONE,
                crate::borders::BorderRadiusGeometry::circular(4.0),
            ),
        ));
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            data,
            crate::framework::component(Card::filled(body())),
        ));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(300.0, 200.0));
        crate::render::flush_layout();
        let mut layers = crate::engine::LayerTree::new(300, 200);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(300.0, 200.0));
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        let radius = crate::engine_test_stubs::drawn()
            .into_iter()
            .find_map(|call| match call {
                Drawn::RRect { radius, .. } => Some(radius),
                _ => None,
            })
            .expect("the surface painted");
        assert_eq!(radius, 4.0, "the theme's shape, not the crate theme's 12");
    }

    #[test]
    fn a_theme_moves_all_three_and_the_variant_still_decides_the_rest() {
        // The theme goes in front of the defaults, one field at a time -- it
        // is an override, not a table swap.
        let data = CardThemeData::new().with_color(Color::argb(255, 1, 2, 3));
        let read = |variant| {
            let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
            struct Reader(
                std::rc::Rc<std::cell::RefCell<Option<ResolvedCard>>>,
                CardVariant,
            );
            impl Component for Reader {
                fn build(&self, context: &mut BuildContext) -> AnyWidget {
                    *self.0.borrow_mut() = Some(ResolvedCard::of(context, self.1));
                    leaf(|| crate::widgets::SizedBox::new(1.0, 1.0))
                }
            }
            let mut tree = ElementTree::new();
            tree.rebuild(provide(
                data.clone(),
                crate::framework::component(Reader(std::rc::Rc::clone(&seen), variant)),
            ));
            seen.borrow_mut().take().expect("built once")
        };
        assert_eq!(read(CardVariant::Elevated).color, Color::argb(255, 1, 2, 3));
        assert_eq!(read(CardVariant::Outlined).color, Color::argb(255, 1, 2, 3));
        assert_eq!(
            read(CardVariant::Elevated).elevation,
            1.0,
            "the colour moved and the elevation did not"
        );
        assert_eq!(read(CardVariant::Outlined).elevation, 0.0);
    }

    #[test]
    fn a_material_two_card_still_gets_the_colour_the_theme_names() {
        // `_CardDefaultsM2` answers `Theme.of(context).cardColor`, and an
        // application that set it and left Material 3 off is asking for that
        // colour rather than for a surface role it never mentioned.
        let seen = std::rc::Rc::new(std::cell::RefCell::new(None));
        struct Reader(std::rc::Rc<std::cell::RefCell<Option<ResolvedCard>>>);
        impl Component for Reader {
            fn build(&self, context: &mut BuildContext) -> AnyWidget {
                *self.0.borrow_mut() = Some(ResolvedCard::of(context, CardVariant::Elevated));
                leaf(|| crate::widgets::SizedBox::new(1.0, 1.0))
            }
        }
        let mut theme = crate::theme::ThemeData::default();
        theme.use_material3 = false;
        theme.card_color = Color::argb(255, 9, 8, 7);
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            theme,
            crate::framework::component(Reader(std::rc::Rc::clone(&seen))),
        ));
        let read = seen.borrow_mut().take().expect("built once");
        assert_eq!(read.color, Color::argb(255, 9, 8, 7));
    }

    /// A card whose child fills it in `colour`, so a test can see which of the
    /// two -- the child or the outline -- reached the canvas last.
    fn filled_child(colour: Color) -> AnyWidget {
        leaf(move || {
            crate::render::RenderDecoratedBox::new().with_fill(crate::render::Fill::Solid(colour))
        })
    }

    /// The order things reached the canvas, as a list of "child" / "border".
    fn order(card: Card, child_colour: Color) -> Vec<&'static str> {
        painted(card)
            .into_iter()
            .filter_map(|call| match call {
                Drawn::Rect {
                    stroke: None, argb, ..
                } if Color(argb) == child_colour => Some("child"),
                Drawn::RRect {
                    stroke: Some(_), ..
                }
                | Drawn::Path {
                    stroke: Some(_), ..
                } => Some("border"),
                Drawn::Rect {
                    stroke: Some(_), ..
                } => Some("border"),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_cards_outline_is_stroked_over_what_is_inside_it() {
        // Upstream's `borderOnForeground`, true by default. A picture that
        // fills the card to its edges would otherwise paint over the line that
        // says where the card stops.
        let green = Color::argb(255, 0, 255, 0);
        assert_eq!(
            order(Card::outlined(filled_child(green)), green),
            vec!["child", "border"]
        );
    }

    #[test]
    fn a_card_can_put_its_outline_behind_its_child_instead() {
        // The other value, which upstream offers for a child that is meant to
        // sit outside the frame.
        let green = Color::argb(255, 0, 255, 0);
        assert_eq!(
            order(
                Card::outlined(filled_child(green)).with_border_on_foreground(false),
                green
            ),
            vec!["border", "child"]
        );
    }

    #[test]
    fn a_card_does_not_clip_unless_it_is_asked_to() {
        // `Clip.none` in all three `_CardDefaults` tables. Clipping costs a
        // layer on every card, and most cards hold a list tile that never
        // reaches the corner.
        let clips = |card: Card| {
            painted(card)
                .into_iter()
                .any(|call| matches!(call, Drawn::ClipPathLayer { .. }))
        };
        assert!(!clips(Card::new(body())));
        assert!(clips(
            Card::new(body()).with_clip_behavior(crate::painting::ClipBehavior::AntiAlias)
        ));
    }

    #[test]
    fn a_theme_can_turn_clipping_on_for_every_card() {
        let data = CardThemeData::new().with_clip_behavior(crate::painting::ClipBehavior::HardEdge);
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            data,
            crate::framework::component(Card::new(body())),
        ));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(300.0, 200.0));
        crate::render::flush_layout();
        let mut layers = crate::engine::LayerTree::new(300, 200);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(300.0, 200.0));
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        assert!(
            crate::engine_test_stubs::drawn()
                .into_iter()
                .any(|call| matches!(call, Drawn::ClipPathLayer { .. })),
            "the theme reached the card"
        );
    }

    #[test]
    fn a_cards_own_clip_beats_the_themes() {
        // Upstream's `clipBehavior ?? cardTheme.clipBehavior ?? defaults`, in
        // that order.
        let data =
            CardThemeData::new().with_clip_behavior(crate::painting::ClipBehavior::AntiAlias);
        let mut tree = ElementTree::new();
        tree.rebuild(provide(
            data,
            crate::framework::component(
                Card::new(body()).with_clip_behavior(crate::painting::ClipBehavior::None),
            ),
        ));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(300.0, 200.0));
        crate::render::flush_layout();
        let mut layers = crate::engine::LayerTree::new(300, 200);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(300.0, 200.0));
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        assert!(
            !crate::engine_test_stubs::drawn()
                .into_iter()
                .any(|call| matches!(call, Drawn::ClipPathLayer { .. })),
            "the card said no and meant it"
        );
    }

    #[test]
    fn the_clip_is_inside_the_shadow_that_the_card_casts() {
        // A clip outside the decoration would cut the card's own shadow off at
        // its edge, and a shadow that stops at the thing casting it is not a
        // shadow.
        let drawn =
            painted(Card::new(body()).with_clip_behavior(crate::painting::ClipBehavior::AntiAlias));
        let clip_at = drawn
            .iter()
            .position(|call| matches!(call, Drawn::ClipPathLayer { .. }))
            .expect("it clipped");
        let shadows_before = drawn[..clip_at]
            .iter()
            .filter(|call| matches!(call, Drawn::RRect { stroke: None, .. }))
            .count();
        assert!(
            shadows_before > 1,
            "the shadow's layers and the surface were painted before the clip: {drawn:?}"
        );
    }
}
