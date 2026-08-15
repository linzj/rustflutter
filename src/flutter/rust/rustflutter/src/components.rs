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
//! Text input. A usable field needs the platform's input method -- composition
//! regions, candidate windows, a cursor the OS can position -- and that is a
//! platform-channel job, not a widget one. A `TextField` that only handled
//! ASCII keystrokes would look finished and be unusable in half the world's
//! languages, so there is not one.

use std::rc::Rc;

use crate::engine::{Color, TextStyle};
use crate::framework::{AnyWidget, BuildContext, Component, StateHandle, component, leaf, many};
use crate::gestures::PointerHandlers;
use crate::render::{
    Alignment, CrossAxisAlignment, EdgeInsets, MainAxisAlignment, MainAxisSize, RenderFlex,
};
use crate::widgets::{Align, Center, Column, Container, Empty, Pointer, Row, SizedBox, Text};

// -- Theme --------------------------------------------------------------------

/// Colours and metrics shared by every component.
#[derive(Clone, Debug)]
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
        IdSource { next: std::cell::Cell::new(first.max(1)) }
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

        let (mut fill, mut label_color, border) = match style {
            ButtonStyle::Filled => (Some(theme.primary), theme.on_primary, None),
            ButtonStyle::Danger => (Some(theme.danger), theme.on_primary, None),
            ButtonStyle::Outlined => (None, theme.primary, Some(theme.outline)),
            ButtonStyle::Text => (None, theme.primary, None),
        };
        if !enabled {
            // Disabled is one rule -- everything loses most of its alpha --
            // rather than a separate palette per style.
            fill = fill.map(|color| color.with_alpha(0x40));
            label_color = label_color.with_alpha(0x60);
        }
        let pressed_fill = match (pressed, fill) {
            (true, Some(color)) => Some(color.with_alpha(0xCC)),
            (true, None) => Some(theme.primary.with_alpha(0x22)),
            (false, fill) => fill,
        };
        let radius = theme.radius;
        let spacing = theme.spacing;
        let body_size = theme.body_size;

        leaf(move || {
            let mut container = Container::new()
                .with_height(44.0)
                .with_corner_radius(radius)
                .with_padding(EdgeInsets::symmetric(spacing * 2.0, 0.0))
                .with_child(Align::new(
                    Alignment::CENTER,
                    Text::new(label.clone())
                        .with_size(body_size)
                        .with_weight(700)
                        .with_color(label_color),
                ));
            if let Some(color) = pressed_fill {
                container = container.with_color(color);
            }
            if let Some(color) = border {
                container = container.with_border(1.5, if pressed { theme_border(color) } else { color });
            }
            if let Some(width) = min_width {
                container = container.with_width(width);
            }
            Pointer::new(id, container).with_handlers(handlers.clone())
        })
    }
}

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
        Card { child: std::cell::RefCell::new(Some(child)), padding: None }
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
        let child = self.child.borrow_mut().take().unwrap_or_else(|| leaf(|| Empty));
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
        Label { content: content.into(), style: LabelStyle::default() }
    }

    pub fn title(content: impl Into<String>) -> Label {
        Label { content: content.into(), style: LabelStyle::Title }
    }

    pub fn muted(content: impl Into<String>) -> Label {
        Label { content: content.into(), style: LabelStyle::Muted }
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
        leaf(move || Text::new(content.clone()).with_style(style.clone()))
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
        Switch { id, value, handlers: PointerHandlers::new() }
    }

    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, toggle: fn(&mut S)) -> Self {
        self.handlers = PointerHandlers::new().with_tap(move |_| {
            handle.set_state(move |state| toggle(state));
        });
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
        let knob = if value { theme.on_primary } else { theme.text_muted };

        leaf(move || {
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
                row.with_main_axis_alignment(MainAxisAlignment::End).push(knob_box)
            } else {
                row.with_main_axis_alignment(MainAxisAlignment::Start).push(knob_box)
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
        })
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
        ProgressBar { value: value.clamp(0.0, 1.0), width: 200.0 }
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
            Align::new(Alignment::CENTER_LEFT, Container::new()
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
                                .with_width((width * value).max(if value > 0.0 { 8.0 } else { 0.0 }))
                                .with_color(fill)
                                .with_corner_radius(4.0),
                        ),
                ))
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
        Slider { id, value: value.clamp(0.0, 1.0), width: 200.0, handlers: PointerHandlers::new() }
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
            Align::new(Alignment::CENTER_LEFT, Pointer::new(
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
            .with_handlers(handlers.clone()))
        })
    }
}

// -- Structure ----------------------------------------------------------------

/// A title bar.
pub struct AppBar {
    title: String,
    subtitle: Option<String>,
    trailing: std::cell::RefCell<Option<AnyWidget>>,
}

impl AppBar {
    pub fn new(title: impl Into<String>) -> AppBar {
        AppBar { title: title.into(), subtitle: None, trailing: std::cell::RefCell::new(None) }
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
        let spacing = theme.spacing;

        let mut children = vec![leaf(move || {
            let mut stack = Column::new()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(2.0)
                .push(Text::new(title.clone()).with_style(title_style.clone()));
            if let Some(subtitle) = &subtitle {
                stack = stack.push(Text::new(subtitle.clone()).with_style(muted.clone()));
            }
            stack
        })];
        if let Some(trailing) = trailing {
            children.push(leaf(|| Empty));
            children.push(trailing);
        }
        let has_trailing = children.len() > 1;

        many(children, move |mut rendered| {
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            if has_trailing {
                let trailing = rendered.pop();
                let spacer = rendered.pop();
                let leading = rendered.pop();
                if let Some(leading) = leading {
                    row = row.push(leading);
                }
                if let Some(spacer) = spacer {
                    row = row.push_flex(crate::render::FlexChild::expanded(spacer, 1));
                }
                if let Some(trailing) = trailing {
                    row = row.push(trailing);
                }
            } else {
                for child in rendered {
                    row = row.push(child);
                }
            }
            Box::new(
                Container::new()
                    .with_color(surface)
                    .with_border(1.0, outline)
                    .with_padding(EdgeInsets::only(
                        spacing * 2.5 + system.left,
                        spacing * 1.75 + system.top,
                        spacing * 2.5 + system.right,
                        spacing * 1.75,
                    ))
                    .with_child(row),
            )
        })
    }
}

/// A title bar over a body, on the theme's background.
pub struct Scaffold {
    app_bar: std::cell::RefCell<Option<AnyWidget>>,
    body: std::cell::RefCell<Option<AnyWidget>>,
}

impl Scaffold {
    pub fn new(body: AnyWidget) -> Scaffold {
        Scaffold {
            app_bar: std::cell::RefCell::new(None),
            body: std::cell::RefCell::new(Some(body)),
        }
    }

    pub fn with_app_bar(self, app_bar: AnyWidget) -> Self {
        *self.app_bar.borrow_mut() = Some(app_bar);
        self
    }
}

impl Component for Scaffold {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let background = theme.background;
        let app_bar = self.app_bar.borrow_mut().take();
        let body = self.body.borrow_mut().take().unwrap_or_else(|| leaf(|| Empty));

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
            Box::new(
                Container::new()
                    .with_color(background)
                    .with_child(column),
            )
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
            let mut column = Column::new()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(3.0)
                .push(
                    Text::new(title.clone())
                        .with_style(TextStyle { font_weight: 700, ..body.clone() }),
                );
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
            children.push(leaf(|| Empty));
            children.push(trailing);
        }

        many(children, move |mut rendered| {
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            if has_trailing {
                let trailing = rendered.pop();
                let spacer = rendered.pop();
                let leading = rendered.pop();
                if let Some(leading) = leading {
                    row = row.push(leading);
                }
                if let Some(spacer) = spacer {
                    row = row.push_flex(crate::render::FlexChild::expanded(spacer, 1));
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
                Some(id) => Box::new(Pointer::new(id, padded).with_handlers(handlers.clone())),
                None => Box::new(padded),
            }
        })
    }
}

/// A hairline rule.
pub struct Divider;

impl Component for Divider {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let color = theme.outline;
        leaf(move || Container::new().with_height(1.0).with_color(color))
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
        Badge { label: label.into(), color: None }
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
                component(ListTile::new(title.to_string()).with_trailing(crate::framework::leaf(
                    || {
                        // A fixed box rather than a Label: the engine stubs
                        // these tests link report zero-sized text, so a
                        // paragraph would measure nothing.
                        crate::widgets::Pointer::new(
                            TRAILING,
                            Container::new().with_size(TRAILING_WIDTH, 12.0),
                        )
                    },
                ))),
            ));
            let mut root = tree.build_render_tree().expect("a root");
            let size = root.layout(BoxConstraints::tight(WIDTH, 60.0));
            let mut result = crate::render::HitTestResult::new();
            root.hit_test(crate::render::Offset::new(x, size.height / 2.0), &mut result);
            result.path.iter().any(|entry| entry.target == TRAILING)
        }

        // The tile pads itself, so the trailing's right edge is inset by that
        // much rather than flush with the tile's own edge.
        let inset = Theme::dark().spacing * 1.5;
        let inside = WIDTH - inset - TRAILING_WIDTH / 2.0;

        for title in ["Rent", "RedPay Credit", "A considerably longer bill name"] {
            assert!(hits(title, inside), "trailing should be at the right for {title:?}");
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
    fn a_slider_clamps_its_value() {
        assert_eq!(Slider::new(1, 5.0).value, 1.0);
        assert_eq!(Slider::new(1, -2.0).value, 0.0);
        assert_eq!(ProgressBar::new(0.5).value, 0.5);
    }
}
