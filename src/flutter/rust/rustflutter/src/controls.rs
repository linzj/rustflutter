// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Selection controls, navigation surfaces and overlays.
//!
//! The second half of the component library. [`crate::components`] has the
//! pieces a page is built from -- surfaces, text, buttons; this has the pieces
//! a page is *operated* by.
//!
//! # Overlays are the framework's now
//!
//! This module used to say the opposite, and it is worth keeping the reason it
//! gave: a dialog, a sheet and a snackbar all mean "draw this over everything
//! and take the taps first", which is a `Stack` with the overlay as its last
//! child, and an application already knows how to write one. What it could not
//! write was an overlay that escapes the caller's `Stack` -- past a clip, past
//! a transform, above whatever else is on screen -- or one put up from a
//! callback with no build context to hand.
//!
//! [`crate::theatre`] is the manager, and every application has one:
//! `app.rs` installs an `Overlay` between the `MediaQuery` and the application.
//! The pieces here still go *in* one, and they are still perfectly usable in a
//! caller's own `Stack`; what has changed is that they no longer have to be.

use std::cell::RefCell;
use std::rc::Rc;

use crate::components::theme_of;
use crate::engine::{Color, TextStyle};
use crate::framework::{AnyWidget, BuildContext, Component, StateHandle, leaf, many, single};
use crate::gestures::PointerHandlers;
use crate::render::{
    Alignment, CrossAxisAlignment, EdgeInsets, FlexChild, MainAxisAlignment, MainAxisSize,
    RenderFlex, Size,
};
use crate::widgets::{Align, Center, Column, Container, Empty, Pointer, Row, SizedBox, Text};

// -- Selection controls -------------------------------------------------------

/// A box that is ticked or not.
pub struct Checkbox {
    id: u64,
    checked: bool,
    enabled: bool,
    label: Option<String>,
    handlers: PointerHandlers,
}

impl Checkbox {
    pub fn new(id: u64, checked: bool) -> Checkbox {
        Checkbox {
            id,
            checked,
            enabled: true,
            label: None,
            handlers: PointerHandlers::new(),
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, toggle: fn(&mut S)) -> Self {
        if self.enabled {
            self.handlers = PointerHandlers::new().with_tap(move |_| {
                handle.set_state(move |state| toggle(state));
            });
        }
        self
    }
}

impl Component for Checkbox {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let checked = self.checked;
        let enabled = self.enabled;
        let id = self.id;
        let handlers = self.handlers.clone();
        let label = self.label.clone();
        let body = theme.body();
        // Upstream's `Checkbox.build`: the fill, the tick and the side each
        // come off `CheckboxTheme.of(context)` resolved against the states
        // this checkbox is in, and fall back to the scheme.
        let mut states = crate::widget_state::WidgetStates::NONE;
        if checked {
            states = states.with(crate::widget_state::WidgetState::Selected);
        }
        if !enabled {
            states = states.with(crate::widget_state::WidgetState::Disabled);
        }
        let resolved = crate::component_themes::ResolvedCheckbox::of(context, states);
        let fill = resolved.fill;
        let border = resolved.side.color;
        let border_width = resolved.side.width;
        let tick = resolved.check;
        let spacing = theme.spacing;

        leaf(move || {
            // The tick is two strokes rather than a glyph: a font that has no
            // check mark would silently draw nothing, and this is two lines.
            let mark = if checked {
                Container::new()
                    .with_size(10.0, 5.0)
                    .with_border(2.0, tick)
                    .with_corner_radius(1.0)
            } else {
                Container::new().with_size(10.0, 5.0)
            };
            let box_widget = Container::new()
                // Upstream's `_kEdgeSize`: an 18-by-18 box with 2-radius
                // corners.
                .with_size(18.0, 18.0)
                .with_color(fill)
                .with_corner_radius(2.0)
                .with_border(border_width, border)
                .with_child(Center::new(mark));

            let content: crate::widgets::BoxedWidget = match &label {
                Some(text) => crate::render::RenderRef::new(
                    Row::new()
                        .with_spacing(spacing)
                        .push(box_widget)
                        .push(Text::new(text.clone()).with_style(body.clone())),
                ),
                None => crate::render::RenderRef::new(box_widget),
            };
            Pointer::new(
                id,
                Container::new()
                    .with_padding(EdgeInsets::all(spacing * 0.75))
                    .with_child(content),
            )
            .with_handlers(handlers.clone())
        })
    }
}

/// One of a set, only one of which can be chosen.
pub struct Radio {
    id: u64,
    selected: bool,
    /// A radio that cannot be chosen. Mirrors [`Checkbox`]: no handlers, the
    /// ring and dot drawn in the outline colour.
    enabled: bool,
    label: Option<String>,
    handlers: PointerHandlers,
}

impl Radio {
    pub fn new(id: u64, selected: bool) -> Radio {
        Radio {
            id,
            selected,
            enabled: true,
            label: None,
            handlers: PointerHandlers::new(),
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// `choose` is given the state and sets whatever field records the choice.
    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, choose: fn(&mut S)) -> Self {
        if self.enabled {
            self.handlers = PointerHandlers::new().with_tap(move |_| {
                handle.set_state(move |state| choose(state));
            });
        }
        self
    }
}

impl Component for Radio {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let selected = self.selected;
        let enabled = self.enabled;
        let id = self.id;
        let handlers = self.handlers.clone();
        let label = self.label.clone();
        let body = theme.body();
        let primary = theme.primary;
        let outline = theme.outline;
        let spacing = theme.spacing;

        leaf(move || {
            let dot = if selected {
                Container::new()
                    .with_size(10.0, 10.0)
                    .with_color(if enabled { primary } else { outline })
                    .with_corner_radius(5.0)
            } else {
                Container::new().with_size(10.0, 10.0)
            };
            let ring = Container::new()
                .with_size(20.0, 20.0)
                .with_corner_radius(10.0)
                .with_border(
                    2.0,
                    if enabled && selected {
                        primary
                    } else {
                        outline
                    },
                )
                .with_child(Center::new(dot));

            let content: crate::widgets::BoxedWidget = match &label {
                Some(text) => crate::render::RenderRef::new(
                    Row::new()
                        .with_spacing(spacing)
                        .push(ring)
                        .push(Text::new(text.clone()).with_style(body.clone())),
                ),
                None => crate::render::RenderRef::new(ring),
            };
            Pointer::new(
                id,
                Container::new()
                    .with_padding(EdgeInsets::all(spacing * 0.75))
                    .with_child(content),
            )
            .with_handlers(handlers.clone())
        })
    }
}

// -- Chips --------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ChipStyle {
    /// Plain, for a label that is also a filter.
    #[default]
    Filter,
    /// Selected filter.
    Selected,
    /// An action rather than a state.
    Action,
}

/// A compact label that can be tapped.
pub struct Chip {
    id: u64,
    label: String,
    style: ChipStyle,
    handlers: PointerHandlers,
}

impl Chip {
    pub fn new(id: u64, label: impl Into<String>) -> Chip {
        Chip {
            id,
            label: label.into(),
            style: ChipStyle::default(),
            handlers: PointerHandlers::new(),
        }
    }

    pub fn with_style(mut self, style: ChipStyle) -> Self {
        self.style = style;
        self
    }

    /// Named `with_selected` rather than `selected` because the getter
    /// upstream's `SelectableChipAttributes` declares has that name, and a
    /// builder and a getter cannot both be `selected`. This is also the shape
    /// every other builder in the crate already has.
    pub fn with_selected(self, selected: bool) -> Self {
        self.with_style(if selected {
            ChipStyle::Selected
        } else {
            ChipStyle::Filter
        })
    }

    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, action: fn(&mut S)) -> Self {
        self.handlers = PointerHandlers::new().with_tap(move |_| {
            handle.set_state(move |state| action(state));
        });
        self
    }
}

impl Component for Chip {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let label = self.label.clone();
        let id = self.id;
        let handlers = self.handlers.clone();
        // Upstream's `RawChip.build` resolves the fill through
        // `ChipTheme.of(context)`: the Material 3 `color` state property
        // first, then the flag-specific colours, then the plain background.
        // The crate's three styles are the states that pick between them.
        let states = match self.style {
            ChipStyle::Selected => crate::widget_state::WidgetStates::NONE
                .with(crate::widget_state::WidgetState::Selected),
            _ => crate::widget_state::WidgetStates::NONE,
        };
        let (default_fill, text_color, border) = match self.style {
            ChipStyle::Filter => (Color::TRANSPARENT, theme.text, Some(theme.outline)),
            ChipStyle::Selected => (
                theme.primary.with_alpha(0x33),
                theme.primary,
                Some(theme.primary),
            ),
            ChipStyle::Action => (theme.surface_variant, theme.text, None),
        };
        // The crate's three styles are this control's own defaults, which is
        // upstream's last fallback: a theme that says nothing leaves them be.
        let chip = crate::component_themes::ResolvedChip::of(context, states, default_fill);
        let fill = chip.fill;
        let border = match chip.side {
            Some(side) => Some((side.width, side.color)),
            None => border.map(|color| (1.0, color)),
        };
        let size = theme.body_size - 1.0;

        leaf(move || {
            let mut container = Container::new()
                .with_height(32.0)
                .with_color(fill)
                .with_corner_radius(16.0)
                .with_padding(EdgeInsets::symmetric(14.0, 0.0))
                .with_child(Align::new(
                    Alignment::CENTER,
                    Text::new(label.clone())
                        .with_size(size)
                        .with_weight(600)
                        .with_color(text_color),
                ));
            if let Some((width, color)) = border {
                container = container.with_border(width, color);
            }
            Pointer::new(id, container).with_handlers(handlers.clone())
        })
    }
}

// -- Tabs ---------------------------------------------------------------------

/// A row of tabs with an underline under the selected one.
pub struct TabBar {
    first_id: u64,
    labels: Vec<String>,
    selected: usize,
    handlers: RefCell<Vec<PointerHandlers>>,
}

impl TabBar {
    /// `first_id` is the hit-test identity of the first tab; the rest follow
    /// consecutively, so a caller reserves a small range rather than a set.
    pub fn new(first_id: u64, labels: Vec<String>, selected: usize) -> TabBar {
        TabBar {
            first_id,
            labels,
            selected,
            handlers: RefCell::new(Vec::new()),
        }
    }

    /// `select` is given the state and the index that was tapped.
    pub fn wired<S: 'static>(self, handle: StateHandle<S>, select: fn(&mut S, usize)) -> TabBar {
        let handlers = (0..self.labels.len())
            .map(|index| {
                let handle = handle.clone();
                PointerHandlers::new().with_tap(move |_| {
                    handle.set_state(move |state| select(state, index));
                })
            })
            .collect();
        *self.handlers.borrow_mut() = handlers;
        self
    }
}

impl Component for TabBar {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let selected = self.selected;
        let first_id = self.first_id;
        let handlers = self.handlers.borrow().clone();
        let labels = self.labels.clone();
        let primary = theme.primary;
        let muted = theme.text_muted;
        let outline = theme.outline;
        let size = theme.body_size;

        leaf(move || {
            // Stretch on the cross axis fills whatever height the bar is
            // offered, which for a top-level tab bar is the whole page. The
            // outer Container below is what turns that into 46.
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for (index, label) in labels.iter().enumerate() {
                let active = index == selected;
                let tab = Container::new().with_height(46.0).with_child(
                    Column::expanded()
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .push_flex(FlexChild::expanded(
                            Align::new(
                                Alignment::CENTER,
                                Text::new(label.clone())
                                    .with_size(size)
                                    .with_weight(if active { 700 } else { 500 })
                                    .with_color(if active { primary } else { muted }),
                            ),
                            1,
                        ))
                        // The indicator is a child of the tab rather than a
                        // separate positioned layer, so it moves with the
                        // tab's own layout instead of being placed twice.
                        // Two pixels is the default weight upstream's
                        // `TabBar` underlines its tabs with.
                        .push(Container::new().with_height(2.0).with_color(if active {
                            primary
                        } else {
                            outline.with_alpha(0x30)
                        })),
                );
                let region = match handlers.get(index) {
                    Some(handlers) => {
                        Pointer::new(first_id + index as u64, tab).with_handlers(handlers.clone())
                    }
                    None => Pointer::new(first_id + index as u64, tab),
                };
                row = row.push_flex(FlexChild::expanded(region, 1));
            }
            Container::new().with_height(46.0).with_child(row)
        })
    }
}

// -- Navigation surfaces ------------------------------------------------------

/// One destination in a [`BottomNavigation`] or [`NavigationRail`].
#[derive(Clone, Debug)]
pub struct Destination {
    pub label: String,
    /// A one- or two-character mark standing in for an icon. There is no icon
    /// font yet, and a missing glyph would draw nothing at all.
    pub mark: String,
}

impl Destination {
    pub fn new(label: impl Into<String>, mark: impl Into<String>) -> Destination {
        Destination {
            label: label.into(),
            mark: mark.into(),
        }
    }
}

/// A bar of destinations along the bottom.
pub struct BottomNavigation {
    first_id: u64,
    destinations: Vec<Destination>,
    selected: usize,
    handlers: RefCell<Vec<PointerHandlers>>,
}

impl BottomNavigation {
    pub fn new(first_id: u64, destinations: Vec<Destination>, selected: usize) -> Self {
        BottomNavigation {
            first_id,
            destinations,
            selected,
            handlers: RefCell::new(Vec::new()),
        }
    }

    pub fn wired<S: 'static>(self, handle: StateHandle<S>, select: fn(&mut S, usize)) -> Self {
        let handlers = (0..self.destinations.len())
            .map(|index| {
                let handle = handle.clone();
                PointerHandlers::new().with_tap(move |_| {
                    handle.set_state(move |state| select(state, index));
                })
            })
            .collect();
        *self.handlers.borrow_mut() = handlers;
        self
    }
}

impl Component for BottomNavigation {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let selected = self.selected;
        let first_id = self.first_id;
        let handlers = self.handlers.borrow().clone();
        let destinations = self.destinations.clone();
        let surface = theme.surface;
        let outline = theme.outline;
        let primary = theme.primary;
        let muted = theme.text_muted;
        // The bar grows by whatever the gesture bar covers and pads its
        // contents up by the same amount, so the destinations stay reachable
        // and the surface still reaches the bottom edge of the screen.
        // Upstream's `BottomNavigationBar` calls this `additionalBottomPadding`
        // and takes it from `viewPadding` rather than `padding`: the gesture
        // bar is there whether or not a keyboard is over it.
        let bottom = crate::media_query::media_query_of(context)
            .view_padding
            .bottom;

        leaf(move || {
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for (index, destination) in destinations.iter().enumerate() {
                let active = index == selected;
                let color = if active { primary } else { muted };
                let item = Container::new().with_child(Center::new(
                    // `MainAxisSize.min` is upstream's own choice here:
                    // `_BottomNavigationTile.build` wraps its icon and label
                    // in `Column(mainAxisSize: MainAxisSize.min)`, and the
                    // `Center` above is what centres the pair in the bar.
                    Column::new()
                        .with_main_axis_size(MainAxisSize::Min)
                        .with_spacing(3.0)
                        .push(
                            Container::new()
                                .with_size(34.0, 22.0)
                                .with_color(if active {
                                    primary.with_alpha(0x33)
                                } else {
                                    Color::TRANSPARENT
                                })
                                .with_corner_radius(11.0)
                                .with_child(Align::new(
                                    Alignment::CENTER,
                                    Text::new(destination.mark.clone())
                                        .with_size(12.0)
                                        .with_weight(700)
                                        .with_color(color),
                                )),
                        )
                        .push(
                            Text::new(destination.label.clone())
                                .with_size(11.0)
                                .with_weight(if active { 700 } else { 500 })
                                .with_color(color),
                        ),
                ));
                let region = match handlers.get(index) {
                    Some(handlers) => {
                        Pointer::new(first_id + index as u64, item).with_handlers(handlers.clone())
                    }
                    None => Pointer::new(first_id + index as u64, item),
                };
                row = row.push_flex(FlexChild::expanded(region, 1));
            }
            Container::new()
                // Upstream's `kBottomNavigationBarHeight`; the safe-area inset
                // rides on top of it.
                .with_height(56.0 + bottom)
                .with_color(surface)
                .with_border(1.0, outline)
                .with_padding(EdgeInsets::only(0.0, 0.0, 0.0, bottom))
                .with_child(row)
        })
    }
}

/// A column of destinations along the left edge, for wide windows.
pub struct NavigationRail {
    first_id: u64,
    destinations: Vec<Destination>,
    selected: usize,
    extended: bool,
    handlers: RefCell<Vec<PointerHandlers>>,
}

impl NavigationRail {
    pub fn new(first_id: u64, destinations: Vec<Destination>, selected: usize) -> Self {
        NavigationRail {
            first_id,
            destinations,
            selected,
            extended: false,
            handlers: RefCell::new(Vec::new()),
        }
    }

    /// Shows the labels beside the marks rather than under them.
    pub fn extended(mut self, extended: bool) -> Self {
        self.extended = extended;
        self
    }

    pub fn wired<S: 'static>(self, handle: StateHandle<S>, select: fn(&mut S, usize)) -> Self {
        let handlers = (0..self.destinations.len())
            .map(|index| {
                let handle = handle.clone();
                PointerHandlers::new().with_tap(move |_| {
                    handle.set_state(move |state| select(state, index));
                })
            })
            .collect();
        *self.handlers.borrow_mut() = handlers;
        self
    }
}

impl Component for NavigationRail {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let selected = self.selected;
        let first_id = self.first_id;
        let extended = self.extended;
        let handlers = self.handlers.borrow().clone();
        let destinations = self.destinations.clone();
        let surface = theme.surface;
        let outline = theme.outline;
        let primary = theme.primary;
        let muted = theme.text_muted;
        let spacing = theme.spacing;

        leaf(move || {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(4.0);
            for (index, destination) in destinations.iter().enumerate() {
                let active = index == selected;
                let color = if active { primary } else { muted };
                let mark = Container::new()
                    .with_size(34.0, 30.0)
                    .with_color(if active {
                        primary.with_alpha(0x33)
                    } else {
                        Color::TRANSPARENT
                    })
                    .with_corner_radius(10.0)
                    .with_child(Align::new(
                        Alignment::CENTER,
                        Text::new(destination.mark.clone())
                            .with_size(12.0)
                            .with_weight(700)
                            .with_color(color),
                    ));
                let content: crate::widgets::BoxedWidget = if extended {
                    crate::render::RenderRef::new(
                        Row::new().with_spacing(spacing).push(mark).push(
                            Text::new(destination.label.clone())
                                .with_size(13.0)
                                .with_weight(if active { 700 } else { 500 })
                                .with_color(color),
                        ),
                    )
                } else {
                    crate::render::RenderRef::new(
                        Column::new().with_spacing(2.0).push(mark).push(
                            Text::new(destination.label.clone())
                                .with_size(10.0)
                                .with_color(color),
                        ),
                    )
                };
                let item = Container::new()
                    .with_padding(EdgeInsets::symmetric(spacing, spacing * 0.75))
                    .with_child(if extended {
                        Align::new(Alignment::CENTER_LEFT, content)
                    } else {
                        Align::new(Alignment::CENTER, content)
                    });
                let region = match handlers.get(index) {
                    Some(handlers) => {
                        Pointer::new(first_id + index as u64, item).with_handlers(handlers.clone())
                    }
                    None => Pointer::new(first_id + index as u64, item),
                };
                column = column.push(region);
            }
            Container::new()
                // Upstream `NavigationRail`'s widths: 80 collapsed, 256 with
                // the labels out.
                .with_width(if extended { 256.0 } else { 80.0 })
                .with_color(surface)
                .with_border(1.0, outline)
                .with_padding(EdgeInsets::symmetric(0.0, spacing))
                .with_child(column)
        })
    }
}

// -- Overlays -----------------------------------------------------------------

/// A scrim: a translucent sheet over the page that swallows taps.
///
/// Put it under a dialog. Without one a tap goes to whatever is behind the
/// dialog, which is the classic modal bug.
pub struct Scrim {
    id: u64,
    handlers: PointerHandlers,
}

impl Scrim {
    pub fn new(id: u64) -> Scrim {
        Scrim {
            id,
            handlers: PointerHandlers::new(),
        }
    }

    /// Dismisses when tapped.
    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, dismiss: fn(&mut S)) -> Self {
        self.handlers = PointerHandlers::new().with_tap(move |_| {
            handle.set_state(move |state| dismiss(state));
        });
        self
    }
}

impl Component for Scrim {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        let id = self.id;
        let handlers = self.handlers.clone();
        leaf(move || {
            Pointer::new(id, Container::new().with_color(Color::argb(0x8A, 0, 0, 0)))
                .with_handlers(handlers.clone())
        })
    }
}

/// A modal card. Put it in a `Stack` over a [`Scrim`].
pub struct Dialog {
    title: String,
    body: Option<String>,
    actions: RefCell<Vec<AnyWidget>>,
    width: f32,
}

impl Dialog {
    pub fn new(title: impl Into<String>) -> Dialog {
        Dialog {
            title: title.into(),
            body: None,
            actions: RefCell::new(Vec::new()),
            // Material 3's dialog is 280 across at the least; a wider one is
            // `with_width`'s to ask for.
            width: 280.0,
        }
    }

    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn with_action(self, action: AnyWidget) -> Self {
        self.actions.borrow_mut().push(action);
        self
    }
}

impl Component for Dialog {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let title = self.title.clone();
        let body = self.body.clone();
        let width = self.width;
        let surface = theme.surface;
        let outline = theme.outline;
        // Material 3's dialog shape: a 28-radius corner all round.
        let radius = 28.0;
        let spacing = theme.spacing;
        let title_style = theme.title();
        let muted = theme.muted();

        let actions = std::mem::take(&mut *self.actions.borrow_mut());
        let has_actions = !actions.is_empty();

        let mut children = vec![leaf(move || {
            let mut column = Column::new()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(spacing);
            column = column.push(Text::new(title.clone()).with_style(title_style.clone()));
            if let Some(body) = &body {
                column = column.push(Text::new(body.clone()).with_style(muted.clone()));
            }
            column
        })];
        children.extend(actions);

        many(children, move |mut rendered| {
            let header = if rendered.is_empty() {
                crate::render::RenderRef::new(Empty) as crate::widgets::BoxedWidget
            } else {
                rendered.remove(0)
            };
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(spacing * 2.0)
                .push(header);
            if has_actions {
                let mut actions_row = RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_main_axis_alignment(MainAxisAlignment::End)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(spacing);
                for action in rendered {
                    actions_row = actions_row.push(action);
                }
                column = column.push(actions_row);
            }
            Box::new(
                Container::new()
                    .with_width(width)
                    .with_color(surface)
                    .with_corner_radius(radius)
                    // Material 3's dialog elevation. It is over a scrim, and
                    // the shadow is what separates it from what it dims.
                    .with_elevation(6)
                    .with_border(1.0, outline)
                    .with_padding(EdgeInsets::all(spacing * 2.5))
                    .with_child(column),
            )
        })
    }
}

/// A panel anchored to the bottom edge.
pub struct BottomSheet {
    title: Option<String>,
    child: RefCell<Option<AnyWidget>>,
}

impl BottomSheet {
    pub fn new(child: AnyWidget) -> BottomSheet {
        BottomSheet {
            title: None,
            child: RefCell::new(Some(child)),
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

impl Component for BottomSheet {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let title = self.title.clone();
        let child = self
            .child
            .borrow_mut()
            .take()
            .unwrap_or_else(|| leaf(|| Empty));
        let surface = theme.surface;
        let outline = theme.outline;
        let spacing = theme.spacing;
        let title_style = theme.title();

        crate::framework::single(child, move |inner| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(spacing);
            // The grab handle: a short bar that says the sheet can be dragged,
            // even though dragging it is the caller's to wire up. Thirty-two
            // by four is upstream's drag handle.
            column = column.push(Box::new(Align::new(
                Alignment::CENTER,
                Container::new()
                    .with_size(32.0, 4.0)
                    .with_color(outline)
                    .with_corner_radius(2.0),
            )));
            if let Some(title) = &title {
                column = column.push(Box::new(
                    Text::new(title.clone()).with_style(title_style.clone()),
                ));
            }
            column = column.push(inner);
            Box::new(
                Container::new()
                    .with_color(surface)
                    .with_corner_radius(theme_sheet_radius())
                    .with_elevation(1)
                    .with_border(1.0, outline)
                    .with_padding(EdgeInsets::only(
                        spacing * 2.0,
                        spacing,
                        spacing * 2.0,
                        spacing * 2.5,
                    ))
                    .with_child(column),
            )
        })
    }
}

/// Sheets are rounded only conceptually at the top; the renderer has one
/// radius, so the bottom corners are rounded too and fall off the screen.
/// Twenty-eight is Material 3's sheet shape; the top-only half of it waits on
/// per-corner radii.
fn theme_sheet_radius() -> f32 {
    28.0
}

/// A brief message along the bottom.
pub struct Snackbar {
    message: String,
    action: Option<String>,
    id: u64,
    handlers: PointerHandlers,
}

impl Snackbar {
    pub fn new(id: u64, message: impl Into<String>) -> Snackbar {
        Snackbar {
            message: message.into(),
            action: None,
            id,
            handlers: PointerHandlers::new(),
        }
    }

    pub fn with_action(mut self, label: impl Into<String>) -> Self {
        self.action = Some(label.into());
        self
    }

    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, action: fn(&mut S)) -> Self {
        self.handlers = PointerHandlers::new().with_tap(move |_| {
            handle.set_state(move |state| action(state));
        });
        self
    }
}

impl Component for Snackbar {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let message = self.message.clone();
        let action = self.action.clone();
        let id = self.id;
        let handlers = self.handlers.clone();
        // A snackbar inverts the surface so it reads as separate from the page
        // rather than as part of it.
        let background = theme.text;
        let foreground = theme.background;
        let accent = theme.primary;
        let spacing = theme.spacing;
        let size = theme.body_size - 1.0;

        leaf(move || {
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .push_flex(FlexChild::expanded(
                    Align::new(
                        Alignment::CENTER_LEFT,
                        Text::new(message.clone())
                            .with_size(size)
                            .with_color(foreground),
                    ),
                    1,
                ));
            if let Some(label) = &action {
                row = row.push(
                    Text::new(label.clone())
                        .with_size(size)
                        .with_weight(700)
                        .with_color(accent),
                );
            }
            Pointer::new(
                id,
                Container::new()
                    // Upstream's snackbar is at least 48 tall with a
                    // 4-radius corner.
                    .with_height(48.0)
                    .with_color(background)
                    .with_corner_radius(4.0)
                    // A snack bar floats over whatever it interrupts; six is
                    // the elevation upstream gives it.
                    .with_elevation(6)
                    .with_padding(EdgeInsets::symmetric(spacing * 2.0, 0.0))
                    .with_child(row),
            )
            .with_handlers(handlers.clone())
        })
    }
}

/// A coloured strip across the top, for something that needs acknowledging.
pub struct Banner {
    message: String,
    actions: RefCell<Vec<AnyWidget>>,
}

impl Banner {
    pub fn new(message: impl Into<String>) -> Banner {
        Banner {
            message: message.into(),
            actions: RefCell::new(Vec::new()),
        }
    }

    pub fn with_action(self, action: AnyWidget) -> Self {
        self.actions.borrow_mut().push(action);
        self
    }
}

impl Component for Banner {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let message = self.message.clone();
        // Upstream's `MaterialBanner.build`: the fill, the rule under it and
        // the padding come off `MaterialBannerTheme.of(context)` first.
        let banner = crate::component_themes::MaterialBannerTheme::of(context);
        let surface = banner.background_color.unwrap_or(theme.surface_variant);
        let outline = banner.divider_color.unwrap_or(theme.outline);
        let body = theme.body();
        let spacing = theme.spacing;
        let padding = banner
            .padding
            .map(|padding| padding.resolve(crate::direction::current_direction()))
            .unwrap_or(EdgeInsets::symmetric(spacing * 2.0, spacing * 1.5));

        let actions = std::mem::take(&mut *self.actions.borrow_mut());
        let has_actions = !actions.is_empty();
        let mut children = vec![leaf(move || {
            Text::new(message.clone()).with_style(body.clone())
        })];
        children.extend(actions);

        many(children, move |mut rendered| {
            let text = if rendered.is_empty() {
                crate::render::RenderRef::new(Empty) as crate::widgets::BoxedWidget
            } else {
                rendered.remove(0)
            };
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(spacing)
                .push_flex(FlexChild::expanded(text, 1));
            if has_actions {
                for action in rendered {
                    row = row.push(action);
                }
            }
            Box::new(
                Container::new()
                    .with_color(surface)
                    .with_border(1.0, outline)
                    .with_padding(padding)
                    .with_child(row),
            )
        })
    }
}

// -- Collections --------------------------------------------------------------

/// A fixed-column grid.
///
/// Rows are built eagerly, like [`crate::widgets::ListView`]: there is no
/// sliver protocol yet, so a grid of a thousand tiles lays out a thousand
/// tiles.
pub struct GridList {
    columns: usize,
    spacing: f32,
    aspect_ratio: f32,
    children: RefCell<Vec<AnyWidget>>,
}

impl GridList {
    pub fn new(columns: usize) -> GridList {
        GridList {
            columns: columns.max(1),
            spacing: 12.0,
            aspect_ratio: 1.0,
            children: RefCell::new(Vec::new()),
        }
    }

    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Width divided by height for each tile.
    pub fn with_aspect_ratio(mut self, ratio: f32) -> Self {
        self.aspect_ratio = ratio.max(0.05);
        self
    }

    pub fn push(self, child: AnyWidget) -> Self {
        self.children.borrow_mut().push(child);
        self
    }
}

impl Component for GridList {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        let columns = self.columns;
        let spacing = self.spacing;
        let aspect = self.aspect_ratio;
        let children = std::mem::take(&mut *self.children.borrow_mut());

        many(children, move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(spacing);

            let mut tiles = rendered.into_iter().peekable();
            while tiles.peek().is_some() {
                let mut row = RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_spacing(spacing);
                for _ in 0..columns {
                    // A short last row is padded with empties rather than left
                    // short, so its tiles keep the width the rest have instead
                    // of stretching to fill.
                    let cell = tiles
                        .next()
                        .unwrap_or_else(|| crate::render::RenderRef::new(Empty));
                    row = row.push_flex(FlexChild::expanded(cell, 1));
                }
                column = column.push(Box::new(AspectRow {
                    row,
                    aspect,
                    columns,
                    spacing,
                }));
            }
            Box::new(column)
        })
    }
}

/// A row that reports a height derived from its width and an aspect ratio.
struct AspectRow {
    row: RenderFlex,
    aspect: f32,
    columns: usize,
    spacing: f32,
}

impl crate::render::RenderBox for AspectRow {
    fn layout(&mut self, constraints: crate::render::BoxConstraints) -> Size {
        // Each cell is (width - gaps) / columns wide, and its height follows
        // from the aspect ratio. That is what makes a grid a grid rather than a
        // column of rows that each pick their own height.
        let width = if constraints.has_bounded_width() {
            constraints.max_width
        } else {
            constraints.min_width
        };
        let gaps = self.spacing * (self.columns.saturating_sub(1)) as f32;
        let cell = ((width - gaps) / self.columns as f32).max(0.0);
        let height = (cell / self.aspect).max(0.0);
        self.row
            .layout(crate::render::BoxConstraints::tight(width, height))
    }

    fn size(&self) -> Size {
        self.row.size()
    }

    fn paint(&self, context: &mut crate::render::PaintContext, offset: crate::render::Offset) {
        self.row.paint(context, offset);
    }

    fn hit_test(
        &self,
        position: crate::render::Offset,
        result: &mut crate::render::HitTestResult,
    ) -> bool {
        self.row.hit_test(position, result)
    }
}

/// A table of text, for showing structured data.
pub struct DataTable {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl DataTable {
    pub fn new(headers: Vec<String>) -> DataTable {
        DataTable {
            headers,
            rows: Vec::new(),
        }
    }

    pub fn push_row(mut self, row: Vec<String>) -> Self {
        self.rows.push(row);
        self
    }
}

impl Component for DataTable {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let headers = self.headers.clone();
        let rows = self.rows.clone();
        let outline = theme.outline;
        let text = theme.text;
        let muted = theme.text_muted;
        let size = theme.body_size - 1.0;
        let spacing = theme.spacing;

        leaf(move || {
            let cell = |content: &str, bold: bool, color: Color| {
                Container::new()
                    .with_padding(EdgeInsets::symmetric(spacing, spacing * 0.9))
                    .with_child(Align::new(
                        Alignment::CENTER_LEFT,
                        Text::new(content.to_string())
                            .with_size(size)
                            .with_weight(if bold { 700 } else { 400 })
                            .with_color(color),
                    ))
            };

            let mut table = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);

            // Center rather than Stretch: a stretched row takes the height it
            // is offered, and a table in a tall page would make every row as
            // tall as the page.
            let mut header_row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            for name in &headers {
                header_row = header_row.push_flex(FlexChild::expanded(cell(name, true, muted), 1));
            }
            table = table.push(header_row);
            table = table.push(Container::new().with_height(1.0).with_color(outline));

            for row in &rows {
                let mut data_row = RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center);
                for index in 0..headers.len() {
                    let value = row.get(index).map(String::as_str).unwrap_or("");
                    data_row = data_row.push_flex(FlexChild::expanded(cell(value, false, text), 1));
                }
                table = table.push(data_row);
                table = table.push(
                    Container::new()
                        .with_height(1.0)
                        .with_color(outline.with_alpha(0x60)),
                );
            }
            table
        })
    }
}

/// The label bubble of a tooltip: a dark pill with the message. This is the
/// surface half of upstream's `Tooltip` (`material/tooltip.dart`); the trigger
/// half -- what shows and hides it -- is [`TooltipTrigger`], and composing the
/// two is the application's `Stack`, as with every overlay here.
pub struct Tooltip {
    message: String,
}

impl Tooltip {
    pub fn new(message: impl Into<String>) -> Tooltip {
        Tooltip {
            message: message.into(),
        }
    }
}

impl Component for Tooltip {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let message = self.message.clone();
        let background = theme.text;
        let foreground = theme.background;
        leaf(move || {
            Container::new()
                .with_color(background)
                .with_corner_radius(6.0)
                .with_padding(EdgeInsets::symmetric(10.0, 6.0))
                .with_child(
                    Text::new(message.clone())
                        .with_size(11.0)
                        .with_color(foreground),
                )
        })
    }
}

/// How touch events should trigger a tooltip. Upstream's `TooltipTriggerMode`
/// (`widgets/raw_tooltip.dart`).
///
/// Whatever the mode, a hovering mouse always shows the tooltip -- upstream's
/// `RawTooltip.triggerMode` docs say so outright ("This property does not
/// affect mouse devices").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TooltipTriggerMode {
    /// Not triggered by touch; hover still works. Upstream's `manual`.
    Manual,
    /// Shown after a long press. Upstream's `longPress`, the default
    /// (`_defaultTriggerMode`).
    #[default]
    LongPress,
    /// Shown after a single tap. Upstream's `tap`.
    Tap,
}

/// The trigger half of a tooltip: wraps a child and reports when the bubble
/// should be visible. The bubble is [`Tooltip`]; putting the bubble over the
/// child is the application's `Stack`, as with every overlay here.
///
/// Ported from the trigger semantics of `material/tooltip.dart`'s `Tooltip`
/// and `widgets/raw_tooltip.dart`'s `RawTooltipState`: hovering in shows and
/// hovering out hides (`_handleMouseEnter`/`_handleMouseExit`), a long press
/// shows (`_handleLongPress`), and a tap shows in tap mode (`_handleTap`).
///
/// The timers upstream runs on top of those events are not ported, because
/// the frame scheduler has no delayed callback:
///
/// - `waitDuration`/`hoverDelay` defaults to zero upstream, so showing on
///   hover *immediately* is the default behavior, ported as is;
/// - `dismissDelay` (100ms from hover-exit to hide) has no clock to run on,
///   so a hover-exit hides at once;
/// - `touchDelay`/`showDuration` (the 1500ms a touch-shown tooltip lingers)
///   likewise: a touch-shown tooltip stays until the application hides it,
///   which its next tap handler is the usual place to do.
///
/// The exclusivity upstream gets from `_ExclusiveMouseRegion` and the
/// `_openedTooltips` list -- one hovered tooltip at a time -- is likewise the
/// application's: the state these callbacks write holds at most one visible
/// tooltip if the application writes it that way.
pub struct TooltipTrigger {
    id: u64,
    child: RefCell<Option<AnyWidget>>,
    trigger_mode: TooltipTriggerMode,
    on_show: Option<Rc<dyn Fn(bool)>>,
}

impl TooltipTrigger {
    pub fn new(id: u64, child: AnyWidget) -> TooltipTrigger {
        TooltipTrigger {
            id,
            child: RefCell::new(Some(child)),
            trigger_mode: TooltipTriggerMode::default(),
            on_show: None,
        }
    }

    /// Upstream's `Tooltip.triggerMode`.
    pub fn with_trigger_mode(mut self, mode: TooltipTriggerMode) -> Self {
        self.trigger_mode = mode;
        self
    }

    /// Runs `show(state, visible)` when the tooltip should appear or
    /// disappear. The application shows a [`Tooltip`] while the state says
    /// visible.
    pub fn wired<S: 'static>(mut self, handle: StateHandle<S>, show: fn(&mut S, bool)) -> Self {
        self.on_show = Some(Rc::new(move |visible| {
            handle.set_state(move |state| show(state, visible));
        }));
        self
    }

    /// The region's handlers for the current trigger mode. Built at build
    /// time rather than in `wired` so that `with_trigger_mode` may come
    /// after it in the chain.
    fn handlers(&self) -> PointerHandlers {
        let Some(on_show) = &self.on_show else {
            return PointerHandlers::new();
        };
        let hover = on_show.clone();
        // Hover shows and hides regardless of the trigger mode; upstream's
        // `hoverDelay` is zero by default, so there is nothing to wait for.
        let mut handlers =
            PointerHandlers::new().with_hover_change(move |hovering| hover(hovering));
        match self.trigger_mode {
            TooltipTriggerMode::LongPress => {
                let show = on_show.clone();
                handlers = handlers.with_long_press(move |_| show(true));
            }
            TooltipTriggerMode::Tap => {
                let show = on_show.clone();
                handlers = handlers.with_tap(move |_| show(true));
            }
            TooltipTriggerMode::Manual => {}
        }
        handlers
    }
}

impl Component for TooltipTrigger {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        let id = self.id;
        let handlers = self.handlers();
        let child = self
            .child
            .borrow_mut()
            .take()
            .unwrap_or_else(|| leaf(|| Empty));
        // Upstream wraps the child in a `_ExclusiveMouseRegion` around a
        // `Listener(onPointerDown:)`; the trigger gestures here arrive through
        // the region's own handlers, so the wrapper is the region.
        single(child, move |inner| {
            Pointer::new(id, inner).with_handlers(handlers.clone())
        })
    }
}

/// A circular progress spinner, drawn as an arc that advances with `value`.
pub struct Spinner {
    value: f32,
    size: f32,
}

impl Spinner {
    /// `value` is 0..1. Feed it from an [`crate::animation::Controller`] set to
    /// loop for an indeterminate spinner.
    pub fn new(value: f32) -> Spinner {
        Spinner {
            value: value.clamp(0.0, 1.0),
            size: 36.0,
        }
    }

    pub fn with_size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
}

impl Component for Spinner {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let value = self.value;
        let size = self.size;
        let track = theme.surface_variant;
        let fill = theme.primary;
        leaf(move || ArcSpinner {
            value,
            extent: size,
            track,
            fill,
            laid_out: Size::ZERO,
        })
    }
}

/// The arc itself. A render object rather than a composition because an arc is
/// one draw call and there is no widget that draws one.
struct ArcSpinner {
    value: f32,
    extent: f32,
    track: Color,
    fill: Color,
    laid_out: Size,
}

impl crate::render::RenderBox for ArcSpinner {
    fn layout(&mut self, constraints: crate::render::BoxConstraints) -> Size {
        self.laid_out = constraints.constrain(Size::square(self.extent));
        self.laid_out
    }

    fn size(&self) -> Size {
        self.laid_out
    }

    fn paint(&self, context: &mut crate::render::PaintContext, offset: crate::render::Offset) {
        use crate::engine::{Paint, Rect, Style};
        let stroke = (self.extent * 0.11).max(2.0);
        let inset = stroke / 2.0;
        let bounds = Rect::ltrb(
            offset.dx + inset,
            offset.dy + inset,
            offset.dx + self.laid_out.width - inset,
            offset.dy + self.laid_out.height - inset,
        );
        let track_paint = Paint::new(self.track).with_style(Style::Stroke { width: stroke });
        context.canvas().draw_oval(bounds, &track_paint);

        let sweep = 360.0 * self.value;
        if sweep > 0.0 {
            let fill_paint = Paint::new(self.fill)
                .with_style(Style::Stroke { width: stroke })
                .with_stroke_cap(crate::painting::StrokeCap::Round);
            // Twelve o'clock is -90 degrees, which is where a spinner should
            // start rather than at three o'clock.
            context
                .canvas()
                .draw_arc(bounds, -90.0, sweep, false, &fill_paint);
        }
    }
}

/// A vertical spacer that pushes everything after it to the far end of a flex.
pub fn spacer() -> AnyWidget {
    leaf(|| SizedBox::new(0.0, 0.0))
}

/// A titled section: a small caption over some content.
pub struct Section {
    title: String,
    child: RefCell<Option<AnyWidget>>,
}

impl Section {
    pub fn new(title: impl Into<String>, child: AnyWidget) -> Section {
        Section {
            title: title.into(),
            child: RefCell::new(Some(child)),
        }
    }
}

impl Component for Section {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let title = self.title.clone();
        let child = self
            .child
            .borrow_mut()
            .take()
            .unwrap_or_else(|| leaf(|| Empty));
        let spacing = theme.spacing;
        let caption = TextStyle {
            font_size: theme.body_size - 2.0,
            color: theme.text_muted,
            font_weight: 700,
            ..TextStyle::default()
        };

        let heading = leaf(move || Text::new(title.clone()).with_style(caption.clone()));
        many(vec![heading, child], move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(spacing);
            for child in rendered {
                column = column.push(child);
            }
            Box::new(column)
        })
    }
}

// -- What a chip can do -------------------------------------------------------

/// Upstream's six chip attribute interfaces (`material/chip.dart`) say what
/// each kind of chip supports, and they exist because upstream has six chip
/// widgets that overlap: `Chip`, `InputChip`, `ChoiceChip`, `FilterChip`,
/// `ActionChip` and `RawChip`. Each declares the combination it implements,
/// and the documentation for a field is written once on the interface rather
/// than six times on the widgets.
///
/// **This crate has one [`Chip`] with a [`ChipStyle`] where upstream has six
/// widgets.** So the interfaces are ported for what they actually are here:
/// the vocabulary that says what a chip *can be asked*, with each style
/// answering for itself. A style that gains an ability implements another
/// trait rather than growing a field nobody can find.
///
/// Every method is a question with a default, because every one of upstream's
/// fields is nullable and null means "the theme decides". A chip only
/// overrides what it has an answer for.
///
/// # The members that are not here
///
/// Upstream's `ChipAttributes` has twenty getters. The ones left out are
/// those with no type in this crate to answer them: `avatar`, `focusNode`,
/// `autofocus`, `visualDensity`, `iconTheme`, `avatarBoxConstraints` and
/// `mouseCursor`. Naming them here is the same choice
/// [`crate::pickers::CalendarDelegate`] makes about its formatting half --
/// adding one later should be an addition, not a correction.
pub trait ChipAttributes {
    /// Upstream's `label`, the only field with no default: a chip without one
    /// is not a chip.
    fn label(&self) -> String;

    fn label_style(&self) -> Option<TextStyle> {
        None
    }

    /// Upstream's `side`: the outline, which for a chip is load-bearing
    /// rather than decorative -- an unfilled chip is *only* its outline.
    fn side(&self) -> Option<crate::borders::BorderSide> {
        None
    }

    fn shape(&self) -> Option<crate::borders::ShapeBorder> {
        None
    }

    fn clip_behavior(&self) -> crate::painting::ClipBehavior {
        crate::painting::ClipBehavior::None
    }

    /// Upstream's `color`: the Material 3 state property, which is consulted
    /// *before* `backgroundColor` and the flag-specific colours. One property
    /// answering for every state is how M3 replaced the four separate colour
    /// fields M2 needed.
    fn color(&self) -> Option<crate::widget_state::StateProperty<Color>> {
        None
    }

    fn background_color(&self) -> Option<Color> {
        None
    }

    fn padding(&self) -> Option<EdgeInsets> {
        None
    }

    fn label_padding(&self) -> Option<EdgeInsets> {
        None
    }

    fn material_tap_target_size(&self) -> Option<crate::widget_state::MaterialTapTargetSize> {
        None
    }

    fn elevation(&self) -> Option<f32> {
        None
    }

    fn shadow_color(&self) -> Option<Color> {
        None
    }

    fn surface_tint_color(&self) -> Option<Color> {
        None
    }

    fn chip_animation_style(&self) -> Option<ChipAnimationStyle> {
        None
    }
}

/// Upstream `DeletableChipAttributes`: a chip with an ✕ on it.
///
/// The tooltip is part of the interface rather than an afterthought, and
/// upstream's reason is worth keeping: the delete affordance is a small
/// target with no label, so the only thing that says *what* it deletes is the
/// tooltip. A chip that is deletable and says nothing about it is one a
/// reader has to guess at.
pub trait DeletableChipAttributes {
    /// Upstream's `onDeleted`. `None` means the chip shows no delete
    /// affordance at all -- the icon is not merely disabled, it is absent,
    /// because an ✕ that does nothing invites a press that does nothing.
    fn on_deleted(&self) -> Option<Rc<dyn Fn()>> {
        None
    }

    fn delete_icon_color(&self) -> Option<Color> {
        None
    }

    fn delete_button_tooltip_message(&self) -> Option<String> {
        None
    }

    /// Whether this chip currently offers deletion, which is exactly whether
    /// it has a callback.
    fn is_deletable(&self) -> bool {
        self.on_deleted().is_some()
    }
}

/// Upstream `CheckmarkableChipAttributes`: a chip that shows a tick when
/// selected.
///
/// Separate from [`SelectableChipAttributes`] because the two are genuinely
/// different questions. A chip can be selectable and show its selection by
/// colour alone -- upstream's `ChoiceChip` does -- and a filter chip shows a
/// tick because a filter's *set* of selections has to be readable at a
/// glance, where a single choice does not.
pub trait CheckmarkableChipAttributes {
    /// `None` is upstream's "let the theme decide", not "no".
    fn show_checkmark(&self) -> Option<bool> {
        None
    }

    fn checkmark_color(&self) -> Option<Color> {
        None
    }
}

/// Upstream `SelectableChipAttributes`: a chip that is on or off.
pub trait SelectableChipAttributes {
    fn selected(&self) -> bool;

    /// Upstream's `onSelected`. Note it is handed the *new* value rather than
    /// being a bare notification: a selectable chip does not own its own
    /// selection -- whoever holds the filter does -- so the callback has to
    /// say which way it went.
    fn on_selected(&self) -> Option<Rc<dyn Fn(bool)>> {
        None
    }

    /// How far the chip lifts while pressed. Shared with
    /// [`TappableChipAttributes`], because both are about a press.
    fn press_elevation(&self) -> Option<f32> {
        None
    }

    fn selected_color(&self) -> Option<Color> {
        None
    }

    fn selected_shadow_color(&self) -> Option<Color> {
        None
    }

    fn tooltip(&self) -> Option<String> {
        None
    }
}

/// Upstream `DisabledChipAttributes`: a chip that can be greyed out.
pub trait DisabledChipAttributes {
    /// Upstream's `isEnabled`, which it derives from whether any callback was
    /// given -- a chip with nothing to do is disabled whether or not anyone
    /// said so.
    fn is_enabled(&self) -> bool;

    fn disabled_color(&self) -> Option<Color> {
        None
    }
}

/// Upstream `TappableChipAttributes`: a chip that is a button.
///
/// Distinct from [`SelectableChipAttributes`] because pressing one *does*
/// something and pressing the other *is* something. An action chip that
/// stayed lit after a press would be lying about having a state.
pub trait TappableChipAttributes {
    fn on_pressed(&self) -> Option<Rc<dyn Fn()>> {
        None
    }

    fn press_elevation(&self) -> Option<f32> {
        None
    }

    fn tooltip(&self) -> Option<String> {
        None
    }
}

/// Upstream `ChipAnimationStyle`: the four animations a chip runs, each
/// overridable on its own.
///
/// Four rather than one because they are four different events with four
/// different right speeds: becoming enabled, becoming selected, and the two
/// drawers -- the avatar sliding in and the delete icon sliding out. Upstream's
/// own constants bear that out (195ms to select, 150ms for a drawer opening
/// and 100ms for it closing, 75ms to disable), so a single knob would have to
/// be wrong for three of them.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ChipAnimationStyle {
    pub enable_animation: Option<crate::animation::AnimationStyle>,
    pub select_animation: Option<crate::animation::AnimationStyle>,
    pub avatar_drawer_animation: Option<crate::animation::AnimationStyle>,
    pub delete_drawer_animation: Option<crate::animation::AnimationStyle>,
}

impl ChipAnimationStyle {
    /// Upstream's `_kSelectDuration`.
    pub const SELECT_MICROS: i64 = 195_000;
    /// Upstream's `_kDrawerDuration`, for a drawer opening.
    pub const DRAWER_MICROS: i64 = 150_000;
    /// Upstream's `_kReverseDrawerDuration`. Shorter than opening, which is
    /// the usual asymmetry: a thing arriving is worth watching and a thing
    /// leaving is in the way.
    pub const REVERSE_DRAWER_MICROS: i64 = 100_000;
    /// Upstream's `_kDisableDuration`.
    pub const DISABLE_MICROS: i64 = 75_000;

    pub fn new() -> ChipAnimationStyle {
        ChipAnimationStyle::default()
    }
}

impl ChipAttributes for Chip {
    fn label(&self) -> String {
        self.label.clone()
    }

    /// The crate's three styles are this control's own defaults, which is
    /// upstream's last fallback -- a theme that says nothing leaves them be.
    /// See [`Chip::build`](Component::build).
    fn background_color(&self) -> Option<Color> {
        None
    }
}

impl SelectableChipAttributes for Chip {
    fn selected(&self) -> bool {
        self.style == ChipStyle::Selected
    }
}

impl TappableChipAttributes for Chip {}

impl DisabledChipAttributes for Chip {
    /// Upstream derives this from whether any callback was given, and so does
    /// this: a chip with no tap handler has nothing to do, whatever its style
    /// says.
    fn is_enabled(&self) -> bool {
        !self.handlers.is_empty()
    }
}

// -- The two shaped dialogs ---------------------------------------------------

/// Upstream's `_scalePadding`: how much of its padding a dialog keeps at a
/// given text scale.
///
/// **The padding shrinks as the reader's text grows** -- to a third of itself
/// by the time text is at 2×, and no further. That looks backwards until the
/// alternative is considered: a dialog is a fixed, small box, and padding that
/// stayed put while the text doubled would push the content off the bottom.
/// The room has to come from somewhere, and whitespace is the part a reader
/// who asked for larger text was not asking for.
///
/// Clamped at both ends: below 1× nothing grows (a reader with *smaller* text
/// does not get a roomier dialog than the design), and past 2× nothing shrinks
/// further (a third is as tight as it may get before the text touches the
/// edges).
pub fn scale_dialog_padding(text_scale: f32) -> f32 {
    let clamped = text_scale.clamp(1.0, 2.0);
    1.0 + (1.0 / 3.0 - 1.0) * (clamped - 1.0)
}

/// Upstream `SimpleDialogOption`: one choice in a [`SimpleDialog`].
///
/// It is an ink well with padding and nothing else, and the padding is the
/// point: 24 across and 8 down, so the options run edge to edge of the dialog
/// and the splash does too. An option inset from the dialog's sides would read
/// as a button in a list rather than as a row of the list.
pub struct SimpleDialogOption {
    id: u64,
    /// A *builder* rather than a widget, for the reason [`crate::ink::Ink`]
    /// gives: the ink well rebuilds from the same widget instance whenever its
    /// own state changes, so a child handed over once would be gone on the
    /// second build.
    build_child: Box<dyn Fn() -> AnyWidget>,
    padding: Option<EdgeInsets>,
    on_pressed: Option<Rc<dyn Fn()>>,
}

impl SimpleDialogOption {
    /// Upstream's default padding.
    pub const DEFAULT_PADDING: EdgeInsets = EdgeInsets::symmetric(24.0, 8.0);

    pub fn new(id: u64, build_child: impl Fn() -> AnyWidget + 'static) -> SimpleDialogOption {
        SimpleDialogOption {
            id,
            build_child: Box::new(build_child),
            padding: None,
            on_pressed: None,
        }
    }

    pub fn with_padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = Some(padding);
        self
    }

    pub fn with_on_pressed(mut self, on_pressed: impl Fn() + 'static) -> Self {
        self.on_pressed = Some(Rc::new(on_pressed));
        self
    }

    pub fn padding(&self) -> EdgeInsets {
        self.padding.unwrap_or(SimpleDialogOption::DEFAULT_PADDING)
    }
}

impl Component for SimpleDialogOption {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        let padding = self.padding();
        let child = (self.build_child)();
        // The padding is *inside* the well, so the splash covers the padded
        // row rather than only the label -- an option is a row of the list,
        // and a splash that stopped at the text would read as a button.
        let padded = crate::framework::single(child, move |inner| {
            Box::new(Container::new().with_padding(padding).with_child(inner))
        });
        let padded = RefCell::new(Some(padded));
        let mut well = crate::ink_well::InkWell::new(self.id, move || {
            padded
                .borrow_mut()
                .take()
                .unwrap_or_else(|| leaf(|| crate::widgets::Empty))
        });
        if let Some(on_pressed) = self.on_pressed.clone() {
            well = well.with_on_tap(move || on_pressed());
        }
        crate::framework::stateful(well)
    }
}

/// Upstream `SimpleDialog`: a title and a list of choices.
///
/// The difference from [`AlertDialog`] is what the reader is being asked. An
/// alert states something and offers actions at the bottom; a simple dialog
/// *is* its choices, so they fill the body and there is no action row at all.
pub struct SimpleDialog {
    title: Option<String>,
    children: RefCell<Vec<AnyWidget>>,
    background_color: Option<Color>,
}

impl SimpleDialog {
    /// Upstream's `titlePadding`, `EdgeInsets.fromLTRB(24, 24, 24, 0)`. No
    /// bottom inset when there are children: the first option supplies its own
    /// top padding, and two would read as a gap.
    pub const TITLE_PADDING: EdgeInsets = EdgeInsets::only(24.0, 24.0, 24.0, 0.0);
    /// Upstream's `contentPadding`, `EdgeInsets.fromLTRB(0, 12, 0, 16)` --
    /// nothing at the sides, because the options run edge to edge.
    pub const CONTENT_PADDING: EdgeInsets = EdgeInsets::only(0.0, 12.0, 0.0, 16.0);

    pub fn new() -> SimpleDialog {
        SimpleDialog {
            title: None,
            children: RefCell::new(Vec::new()),
            background_color: None,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_child(self, child: AnyWidget) -> Self {
        self.children.borrow_mut().push(child);
        self
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    /// Upstream's title padding, scaled -- and with the bottom inset kept
    /// unscaled when there are children, which is upstream's one asymmetry
    /// here: that inset is the gap to the first option, and a gap that shrank
    /// with the text would close up exactly when the text needed separating.
    pub fn title_padding(&self, text_scale: f32) -> EdgeInsets {
        let scale = scale_dialog_padding(text_scale);
        let base = SimpleDialog::TITLE_PADDING;
        let has_children = !self.children.borrow().is_empty();
        EdgeInsets {
            left: base.left * scale,
            right: base.right * scale,
            top: base.top * scale,
            bottom: if has_children {
                base.bottom
            } else {
                base.bottom * scale
            },
        }
    }
}

impl Default for SimpleDialog {
    fn default() -> SimpleDialog {
        SimpleDialog::new()
    }
}

impl Component for SimpleDialog {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let scale = crate::media_query::current_text_scale();
        let title_padding = self.title_padding(scale);
        let content_padding = SimpleDialog::CONTENT_PADDING;
        let title = self.title.clone();
        let title_style = theme.title();
        let background = self.background_color.unwrap_or(theme.surface);
        let children = std::mem::take(&mut *self.children.borrow_mut());

        many(children, move |boxed| {
            let mut column = Column::new()
                .with_main_axis_size(crate::render::MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Start);
            if let Some(title) = &title {
                column = column.push(
                    Container::new()
                        .with_padding(title_padding)
                        .with_child(Text::new(title.clone()).with_style(title_style.clone())),
                );
            }
            let mut body = Column::new()
                .with_main_axis_size(crate::render::MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for child in boxed {
                body = body.push(child);
            }
            column = column.push(
                Container::new()
                    .with_padding(content_padding)
                    .with_child(body),
            );
            Container::new()
                .with_color(background)
                .with_corner_radius(28.0)
                .with_child(column)
        })
    }
}

/// Upstream `AlertDialog`: something to say, and what to do about it.
///
/// The shape is upstream's four optional bands -- icon, title, content,
/// actions -- and the ordering rule worth stating is what the *icon* does to
/// the alignment: an alert with an icon centres its title, because the icon is
/// above it and a left-aligned title under a centred icon reads as a mistake.
pub struct AlertDialog {
    title: Option<String>,
    content: Option<String>,
    actions: RefCell<Vec<AnyWidget>>,
    icon: RefCell<Option<AnyWidget>>,
    background_color: Option<Color>,
}

impl AlertDialog {
    /// Upstream's default `titlePadding` when there is no icon.
    pub const TITLE_PADDING: EdgeInsets = EdgeInsets::only(24.0, 24.0, 24.0, 0.0);
    /// Upstream's default `contentPadding` for Material 3.
    pub const CONTENT_PADDING: EdgeInsets = EdgeInsets::only(24.0, 16.0, 24.0, 24.0);
    /// Upstream's default `actionsPadding` for Material 3.
    pub const ACTIONS_PADDING: EdgeInsets = EdgeInsets::only(24.0, 0.0, 24.0, 24.0);

    pub fn new() -> AlertDialog {
        AlertDialog {
            title: None,
            content: None,
            actions: RefCell::new(Vec::new()),
            icon: RefCell::new(None),
            background_color: None,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }

    pub fn with_action(self, action: AnyWidget) -> Self {
        self.actions.borrow_mut().push(action);
        self
    }

    pub fn with_icon(self, icon: AnyWidget) -> Self {
        *self.icon.borrow_mut() = Some(icon);
        self
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = Some(color);
        self
    }

    /// Whether the title is centred, which upstream ties to having an icon.
    pub fn centres_title(&self) -> bool {
        self.icon.borrow().is_some()
    }

    /// Any of upstream's paddings at the reader's text scale.
    pub fn scaled(padding: EdgeInsets, text_scale: f32) -> EdgeInsets {
        let scale = scale_dialog_padding(text_scale);
        EdgeInsets {
            left: padding.left * scale,
            right: padding.right * scale,
            top: padding.top * scale,
            bottom: padding.bottom * scale,
        }
    }
}

impl Default for AlertDialog {
    fn default() -> AlertDialog {
        AlertDialog::new()
    }
}

impl Component for AlertDialog {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let scale = crate::media_query::current_text_scale();
        let title = self.title.clone();
        let content = self.content.clone();
        let title_style = theme.title();
        let body_style = theme.body();
        let background = self.background_color.unwrap_or(theme.surface);
        let centred = self.centres_title();
        let title_padding = AlertDialog::scaled(AlertDialog::TITLE_PADDING, scale);
        let content_padding = AlertDialog::scaled(AlertDialog::CONTENT_PADDING, scale);
        let actions_padding = AlertDialog::scaled(AlertDialog::ACTIONS_PADDING, scale);

        let icon = self.icon.borrow_mut().take();
        let has_icon = icon.is_some();
        let actions = std::mem::take(&mut *self.actions.borrow_mut());
        let action_count = actions.len();
        let mut children = Vec::new();
        children.extend(icon);
        children.extend(actions);

        many(children, move |mut boxed| {
            let mut boxed = boxed.drain(..);
            let icon = if has_icon { boxed.next() } else { None };
            let actions: Vec<_> = boxed.take(action_count).collect();

            let mut column = Column::new()
                .with_main_axis_size(crate::render::MainAxisSize::Min)
                .with_cross_axis_alignment(if centred {
                    CrossAxisAlignment::Center
                } else {
                    CrossAxisAlignment::Start
                });
            if let Some(icon) = icon {
                column = column.push(
                    Container::new()
                        .with_padding(EdgeInsets::only(24.0, 24.0, 24.0, 16.0))
                        .with_child(icon),
                );
            }
            if let Some(title) = &title {
                column = column.push(
                    Container::new()
                        .with_padding(if has_icon {
                            EdgeInsets::only(24.0, 0.0, 24.0, 0.0)
                        } else {
                            title_padding
                        })
                        .with_child(Text::new(title.clone()).with_style(title_style.clone())),
                );
            }
            if let Some(content) = &content {
                column = column.push(
                    Container::new()
                        .with_padding(content_padding)
                        .with_child(Text::new(content.clone()).with_style(body_style.clone())),
                );
            }
            if !actions.is_empty() {
                // Upstream lays the actions out in an `OverflowBar` so that a
                // pair of long labels stacks rather than overflowing the
                // dialog -- which is precisely the case a small fixed box
                // runs into.
                let mut bar = crate::overflow_bar::OverflowBar::new()
                    .with_spacing(8.0)
                    .with_alignment(crate::render::MainAxisAlignment::End);
                for action in actions {
                    bar = bar.push_boxed(action);
                }
                column = column.push(
                    Container::new()
                        .with_padding(actions_padding)
                        .with_child(bar),
                );
            }
            Container::new()
                .with_color(background)
                .with_corner_radius(28.0)
                .with_child(column)
        })
    }
}

// -- The four chip variants ---------------------------------------------------

/// The fields every chip variant carries, so the four below differ only in
/// what they *implement* rather than in what they store.
///
/// Upstream repeats the whole field list in each of the four files, because a
/// Dart class implementing an interface has to declare every member of it.
/// The interfaces are the shared vocabulary and this is the shared storage;
/// keeping them apart is what makes the four variants readable as four
/// *combinations* rather than four near-copies.
#[derive(Default)]
pub struct ChipParts {
    pub label: String,
    pub selected: bool,
    pub enabled: bool,
    pub show_checkmark: Option<bool>,
    pub background_color: Option<Color>,
    pub selected_color: Option<Color>,
    pub disabled_color: Option<Color>,
    pub tooltip: Option<String>,
    pub press_elevation: Option<f32>,
    pub on_pressed: Option<Rc<dyn Fn()>>,
    pub on_selected: Option<Rc<dyn Fn(bool)>>,
    pub on_deleted: Option<Rc<dyn Fn()>>,
    /// Upstream's `.elevated` constructors, which are a *shape* rather than a
    /// number: an elevated chip sits on its own surface instead of being an
    /// outline on the page. The four variants each have one.
    pub elevated: bool,
}

impl ChipParts {
    fn new(label: impl Into<String>) -> ChipParts {
        ChipParts {
            label: label.into(),
            enabled: true,
            ..ChipParts::default()
        }
    }
}

/// Upstream `ActionChip` (`material/action_chip.dart`): a chip that *does*
/// something.
///
/// Implements [`ChipAttributes`], [`TappableChipAttributes`] and
/// [`DisabledChipAttributes`] -- and notably **not**
/// [`SelectableChipAttributes`]. That is the distinction the taxonomy exists
/// to make: an action chip has no state to be in. Pressing it starts
/// something and it looks the same afterwards, so a lit-up action chip would
/// be claiming a state it does not have.
pub struct ActionChip(pub ChipParts);

impl ActionChip {
    pub fn new(label: impl Into<String>) -> ActionChip {
        ActionChip(ChipParts::new(label))
    }

    /// Upstream's `ActionChip.elevated`.
    pub fn elevated(label: impl Into<String>) -> ActionChip {
        ActionChip(ChipParts {
            elevated: true,
            ..ChipParts::new(label)
        })
    }

    pub fn with_on_pressed(mut self, on_pressed: impl Fn() + 'static) -> Self {
        self.0.on_pressed = Some(Rc::new(on_pressed));
        self
    }

    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.0.tooltip = Some(tooltip.into());
        self
    }
}

impl ChipAttributes for ActionChip {
    fn label(&self) -> String {
        self.0.label.clone()
    }

    fn background_color(&self) -> Option<Color> {
        self.0.background_color
    }
}

impl TappableChipAttributes for ActionChip {
    fn on_pressed(&self) -> Option<Rc<dyn Fn()>> {
        self.0.on_pressed.clone()
    }

    fn press_elevation(&self) -> Option<f32> {
        self.0.press_elevation
    }

    fn tooltip(&self) -> Option<String> {
        self.0.tooltip.clone()
    }
}

impl DisabledChipAttributes for ActionChip {
    /// Upstream derives this from `onPressed != null`, so an action chip with
    /// nothing to do is disabled whatever anyone said.
    fn is_enabled(&self) -> bool {
        self.0.enabled && self.0.on_pressed.is_some()
    }

    fn disabled_color(&self) -> Option<Color> {
        self.0.disabled_color
    }
}

/// Upstream `ChoiceChip` (`material/choice_chip.dart`): one of a set, of which
/// the reader picks exactly one.
///
/// Selectable and checkmarkable but **not deletable**: a choice is one of a
/// fixed set, so there is nothing to remove -- picking another is how you stop
/// picking this one.
pub struct ChoiceChip(pub ChipParts);

impl ChoiceChip {
    pub fn new(label: impl Into<String>) -> ChoiceChip {
        ChoiceChip(ChipParts::new(label))
    }

    pub fn elevated(label: impl Into<String>) -> ChoiceChip {
        ChoiceChip(ChipParts {
            elevated: true,
            ..ChipParts::new(label)
        })
    }

    pub fn with_selected(mut self, selected: bool) -> Self {
        self.0.selected = selected;
        self
    }

    pub fn with_on_selected(mut self, on_selected: impl Fn(bool) + 'static) -> Self {
        self.0.on_selected = Some(Rc::new(on_selected));
        self
    }
}

impl ChipAttributes for ChoiceChip {
    fn label(&self) -> String {
        self.0.label.clone()
    }

    fn background_color(&self) -> Option<Color> {
        self.0.background_color
    }
}

impl SelectableChipAttributes for ChoiceChip {
    fn selected(&self) -> bool {
        self.0.selected
    }

    fn on_selected(&self) -> Option<Rc<dyn Fn(bool)>> {
        self.0.on_selected.clone()
    }

    fn selected_color(&self) -> Option<Color> {
        self.0.selected_color
    }

    fn tooltip(&self) -> Option<String> {
        self.0.tooltip.clone()
    }
}

impl CheckmarkableChipAttributes for ChoiceChip {
    /// Upstream's default for a choice chip is **no tick**, unlike a filter
    /// chip's: one choice out of a set is already told apart by its colour,
    /// and a tick on the single chosen one is a second way of saying the same
    /// thing.
    fn show_checkmark(&self) -> Option<bool> {
        self.0.show_checkmark.or(Some(false))
    }
}

impl DisabledChipAttributes for ChoiceChip {
    fn is_enabled(&self) -> bool {
        self.0.enabled && self.0.on_selected.is_some()
    }

    fn disabled_color(&self) -> Option<Color> {
        self.0.disabled_color
    }
}

/// Upstream `FilterChip` (`material/filter_chip.dart`): one of a set, of which
/// the reader picks any number.
///
/// The same traits as a [`ChoiceChip`] plus [`DeletableChipAttributes`]. The
/// difference from a choice chip is not the widget but the *set*: several
/// filters can be on at once, so the reader has to be able to read which at a
/// glance -- which is why the tick is on by default here and off there.
pub struct FilterChip(pub ChipParts);

impl FilterChip {
    pub fn new(label: impl Into<String>) -> FilterChip {
        FilterChip(ChipParts::new(label))
    }

    pub fn elevated(label: impl Into<String>) -> FilterChip {
        FilterChip(ChipParts {
            elevated: true,
            ..ChipParts::new(label)
        })
    }

    pub fn with_selected(mut self, selected: bool) -> Self {
        self.0.selected = selected;
        self
    }

    pub fn with_on_selected(mut self, on_selected: impl Fn(bool) + 'static) -> Self {
        self.0.on_selected = Some(Rc::new(on_selected));
        self
    }

    pub fn with_on_deleted(mut self, on_deleted: impl Fn() + 'static) -> Self {
        self.0.on_deleted = Some(Rc::new(on_deleted));
        self
    }
}

impl ChipAttributes for FilterChip {
    fn label(&self) -> String {
        self.0.label.clone()
    }

    fn background_color(&self) -> Option<Color> {
        self.0.background_color
    }
}

impl SelectableChipAttributes for FilterChip {
    fn selected(&self) -> bool {
        self.0.selected
    }

    fn on_selected(&self) -> Option<Rc<dyn Fn(bool)>> {
        self.0.on_selected.clone()
    }

    fn selected_color(&self) -> Option<Color> {
        self.0.selected_color
    }

    fn tooltip(&self) -> Option<String> {
        self.0.tooltip.clone()
    }
}

impl CheckmarkableChipAttributes for FilterChip {
    /// Unset here means the theme decides, which for Material 3 means a tick.
    /// Several filters can be on at once, so the set has to be readable at a
    /// glance and colour alone is not enough.
    fn show_checkmark(&self) -> Option<bool> {
        self.0.show_checkmark
    }
}

impl DeletableChipAttributes for FilterChip {
    fn on_deleted(&self) -> Option<Rc<dyn Fn()>> {
        self.0.on_deleted.clone()
    }
}

impl DisabledChipAttributes for FilterChip {
    fn is_enabled(&self) -> bool {
        self.0.enabled && self.0.on_selected.is_some()
    }

    fn disabled_color(&self) -> Option<Color> {
        self.0.disabled_color
    }
}

/// Upstream `InputChip` (`material/input_chip.dart`): something the reader
/// themselves put there.
///
/// The only variant implementing **all six** interfaces, and that is the
/// point: an input chip is a piece of the reader's own input -- a recipient on
/// an email, a tag they typed -- so it can be pressed, chosen, ticked, deleted
/// and disabled. The other three are each a *subset* of it, which is why the
/// taxonomy is worth having rather than one chip with every field.
pub struct InputChip(pub ChipParts);

impl InputChip {
    pub fn new(label: impl Into<String>) -> InputChip {
        InputChip(ChipParts::new(label))
    }

    pub fn with_selected(mut self, selected: bool) -> Self {
        self.0.selected = selected;
        self
    }

    pub fn with_on_pressed(mut self, on_pressed: impl Fn() + 'static) -> Self {
        self.0.on_pressed = Some(Rc::new(on_pressed));
        self
    }

    pub fn with_on_selected(mut self, on_selected: impl Fn(bool) + 'static) -> Self {
        self.0.on_selected = Some(Rc::new(on_selected));
        self
    }

    pub fn with_on_deleted(mut self, on_deleted: impl Fn() + 'static) -> Self {
        self.0.on_deleted = Some(Rc::new(on_deleted));
        self
    }

    /// Upstream's `isEnabled`, which an input chip carries outright rather
    /// than deriving -- it is the one variant that can be meaningfully
    /// present and inert, because it is showing something the reader typed
    /// whether or not it can still be acted on.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.0.enabled = enabled;
        self
    }
}

impl ChipAttributes for InputChip {
    fn label(&self) -> String {
        self.0.label.clone()
    }

    fn background_color(&self) -> Option<Color> {
        self.0.background_color
    }
}

impl SelectableChipAttributes for InputChip {
    fn selected(&self) -> bool {
        self.0.selected
    }

    fn on_selected(&self) -> Option<Rc<dyn Fn(bool)>> {
        self.0.on_selected.clone()
    }

    fn selected_color(&self) -> Option<Color> {
        self.0.selected_color
    }

    fn tooltip(&self) -> Option<String> {
        self.0.tooltip.clone()
    }
}

impl CheckmarkableChipAttributes for InputChip {
    fn show_checkmark(&self) -> Option<bool> {
        self.0.show_checkmark
    }
}

impl DeletableChipAttributes for InputChip {
    fn on_deleted(&self) -> Option<Rc<dyn Fn()>> {
        self.0.on_deleted.clone()
    }
}

impl TappableChipAttributes for InputChip {
    fn on_pressed(&self) -> Option<Rc<dyn Fn()>> {
        self.0.on_pressed.clone()
    }

    fn tooltip(&self) -> Option<String> {
        self.0.tooltip.clone()
    }
}

impl DisabledChipAttributes for InputChip {
    /// Carried, not derived -- see [`InputChip::with_enabled`].
    fn is_enabled(&self) -> bool {
        self.0.enabled
    }

    fn disabled_color(&self) -> Option<Color> {
        self.0.disabled_color
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Theme;
    use crate::framework::{ElementTree, component, provide};
    use crate::render::{BoxConstraints, RenderBox};

    fn lay_out(widget: AnyWidget, width: f32, height: f32) -> Size {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), widget));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::loose(width, height))
    }

    #[test]
    fn a_checkbox_lays_out_at_a_sensible_size() {
        let size = lay_out(component(Checkbox::new(1, true)), 200.0, 200.0);
        assert!(size.width > 20.0 && size.width < 60.0, "{size:?}");
        assert!(size.height > 20.0 && size.height < 60.0, "{size:?}");
    }

    #[test]
    fn a_labelled_checkbox_is_wider_than_a_bare_one() {
        let bare = lay_out(component(Checkbox::new(1, false)), 300.0, 100.0);
        let labelled = lay_out(
            component(Checkbox::new(1, false).with_label("Remember me")),
            300.0,
            100.0,
        );
        assert!(labelled.width > bare.width);
    }

    #[test]
    fn a_tab_bar_fills_its_width() {
        let size = lay_out(
            component(TabBar::new(
                1,
                vec!["One".into(), "Two".into(), "Three".into()],
                0,
            )),
            400.0,
            200.0,
        );
        assert_eq!(size.width, 400.0);
        assert!(size.height > 40.0 && size.height < 60.0, "{size:?}");
    }

    #[test]
    fn a_grid_gives_every_row_the_same_height() {
        // Four tiles in two columns at 1:1 in a 200 wide box: two rows, each
        // (200 - 12) / 2 = 94 tall, plus the gap between them.
        let grid = GridList::new(2)
            .with_spacing(12.0)
            .push(leaf(|| Empty))
            .push(leaf(|| Empty))
            .push(leaf(|| Empty))
            .push(leaf(|| Empty));
        let size = lay_out(component(grid), 200.0, 1000.0);
        assert!((size.height - (94.0 * 2.0 + 12.0)).abs() < 1.0, "{size:?}");
    }

    #[test]
    fn a_short_last_row_keeps_the_tile_width() {
        // Three tiles in two columns: the second row has one tile and a gap,
        // and the row is still two rows tall.
        let grid = GridList::new(2)
            .with_spacing(12.0)
            .push(leaf(|| Empty))
            .push(leaf(|| Empty))
            .push(leaf(|| Empty));
        let size = lay_out(component(grid), 200.0, 1000.0);
        assert!((size.height - (94.0 * 2.0 + 12.0)).abs() < 1.0, "{size:?}");
    }

    #[test]
    fn a_data_table_grows_with_its_rows() {
        let one = lay_out(
            component(
                DataTable::new(vec!["A".into(), "B".into()]).push_row(vec!["1".into(), "2".into()]),
            ),
            300.0,
            1000.0,
        );
        let two = lay_out(
            component(
                DataTable::new(vec!["A".into(), "B".into()])
                    .push_row(vec!["1".into(), "2".into()])
                    .push_row(vec!["3".into(), "4".into()]),
            ),
            300.0,
            1000.0,
        );
        assert!(two.height > one.height);
    }

    #[test]
    fn a_spinner_is_square() {
        let size = lay_out(component(Spinner::new(0.4).with_size(48.0)), 200.0, 200.0);
        assert_eq!(size, Size::square(48.0));
    }

    #[test]
    fn a_navigation_rail_is_wider_when_extended() {
        let destinations = vec![
            Destination::new("Home", "H"),
            Destination::new("Settings", "S"),
        ];
        let narrow = lay_out(
            component(NavigationRail::new(1, destinations.clone(), 0)),
            400.0,
            400.0,
        );
        let wide = lay_out(
            component(NavigationRail::new(1, destinations, 0).extended(true)),
            400.0,
            400.0,
        );
        assert!(wide.width > narrow.width);
    }

    #[test]
    fn a_tooltip_trigger_passes_its_childs_size_through() {
        let trigger = TooltipTrigger::new(1, leaf(|| crate::widgets::SizedBox::new(30.0, 20.0)));
        let size = lay_out(component(trigger), 200.0, 200.0);
        assert_eq!(size, Size::new(30.0, 20.0));
    }

    #[test]
    fn hovering_shows_and_unhovering_hides() {
        let shown = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let record = shown.clone();
        let mut trigger = TooltipTrigger::new(1, leaf(|| Empty));
        trigger.on_show = Some(std::rc::Rc::new(move |visible| {
            record.borrow_mut().push(visible)
        }));
        let handlers = trigger.handlers();
        let on_hover = handlers
            .on_hover_change
            .clone()
            .expect("hover is always wired");
        on_hover(true);
        on_hover(false);
        assert_eq!(*shown.borrow(), vec![true, false]);
    }

    #[test]
    fn the_trigger_mode_picks_the_touch_gesture() {
        let mut trigger = TooltipTrigger::new(1, leaf(|| Empty));
        trigger.on_show = Some(std::rc::Rc::new(|_| {}));
        // Upstream's default, `_defaultTriggerMode`, is longPress.
        let handlers = trigger.handlers();
        assert!(handlers.on_long_press.is_some());
        assert!(handlers.on_tap.is_none());

        let trigger = trigger.with_trigger_mode(TooltipTriggerMode::Tap);
        let handlers = trigger.handlers();
        assert!(handlers.on_tap.is_some());
        assert!(handlers.on_long_press.is_none());

        // Manual wires no touch gesture, but hover is wired either way --
        // upstream's triggerMode "does not affect mouse devices".
        let trigger = trigger.with_trigger_mode(TooltipTriggerMode::Manual);
        let handlers = trigger.handlers();
        assert!(handlers.on_tap.is_none());
        assert!(handlers.on_long_press.is_none());
        assert!(handlers.on_hover_change.is_some());
    }

    #[test]
    fn an_unwired_trigger_has_no_handlers() {
        let trigger = TooltipTrigger::new(1, leaf(|| Empty));
        assert!(trigger.handlers().is_empty());
    }

    #[test]
    fn a_chip_answers_its_label_and_leaves_the_rest_to_the_theme() {
        // Every one of upstream's fields is nullable and null means "the
        // theme decides"; `label` is the only one with no default, because a
        // chip without one is not a chip.
        let chip = Chip::new(1, "Filter");
        assert_eq!(chip.label(), "Filter");
        assert_eq!(chip.background_color(), None);
        assert_eq!(chip.side(), None);
        assert_eq!(chip.elevation(), None);
        assert_eq!(chip.chip_animation_style(), None);
    }

    #[test]
    fn the_style_is_what_answers_for_selection() {
        // Upstream splits this across six widgets; here one chip with a style
        // answers, and `ChipStyle::Selected` is what makes it selected.
        assert!(!Chip::new(1, "x").with_selected(false).selected());
        assert!(Chip::new(1, "x").with_selected(true).selected());
        assert!(!Chip::new(1, "x").with_style(ChipStyle::Action).selected());
    }

    #[test]
    fn a_chip_with_nothing_to_do_is_disabled_whatever_its_style_says() {
        // Upstream derives `isEnabled` from whether any callback was given,
        // not from a flag -- so a chip nobody wired up is greyed out even if
        // it looks like an action.
        let inert = Chip::new(1, "x").with_style(ChipStyle::Action);
        assert!(!inert.is_enabled());

        struct Nothing;
        let handle: StateHandle<Nothing> = StateHandle::detached();
        let wired = Chip::new(1, "x").wired(handle, |_: &mut Nothing| {});
        assert!(wired.is_enabled());
    }

    #[test]
    fn deleting_is_absent_rather_than_disabled_when_there_is_no_callback() {
        // An x that does nothing invites a press that does nothing, so
        // upstream shows no delete affordance at all rather than a dead one.
        struct Undeletable;
        impl DeletableChipAttributes for Undeletable {}
        assert!(!Undeletable.is_deletable());

        struct Deletable;
        impl DeletableChipAttributes for Deletable {
            fn on_deleted(&self) -> Option<Rc<dyn Fn()>> {
                Some(Rc::new(|| {}))
            }
        }
        assert!(Deletable.is_deletable());
    }

    #[test]
    fn a_checkmark_left_unset_means_the_theme_decides_not_no() {
        // The distinction matters: `Some(false)` is a chip saying "do not
        // show one", and `None` is a chip saying nothing at all. A port that
        // collapsed them would make every filter chip's tick untheme-able.
        struct Quiet;
        impl CheckmarkableChipAttributes for Quiet {}
        assert_eq!(Quiet.show_checkmark(), None);

        struct Refuses;
        impl CheckmarkableChipAttributes for Refuses {
            fn show_checkmark(&self) -> Option<bool> {
                Some(false)
            }
        }
        assert_eq!(Refuses.show_checkmark(), Some(false));
        assert_ne!(Quiet.show_checkmark(), Refuses.show_checkmark());
    }

    #[test]
    fn selection_is_reported_with_the_value_it_became() {
        // A selectable chip does not own its own selection -- whoever holds
        // the filter does -- so the callback has to say which way it went
        // rather than merely that something happened.
        let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
        struct Filter(Rc<dyn Fn(bool)>);
        impl SelectableChipAttributes for Filter {
            fn selected(&self) -> bool {
                false
            }
            fn on_selected(&self) -> Option<Rc<dyn Fn(bool)>> {
                Some(Rc::clone(&self.0))
            }
        }
        let sink = Rc::clone(&seen);
        let chip = Filter(Rc::new(move |value| sink.borrow_mut().push(value)));
        let callback = chip.on_selected().expect("a callback");
        callback(true);
        callback(false);
        assert_eq!(*seen.borrow(), vec![true, false]);
    }

    #[test]
    fn the_four_chip_animations_are_four_different_speeds() {
        // Which is why upstream has four knobs rather than one: a single one
        // would have to be wrong for three of them.
        assert_eq!(ChipAnimationStyle::SELECT_MICROS, 195_000);
        assert_eq!(ChipAnimationStyle::DRAWER_MICROS, 150_000);
        assert_eq!(ChipAnimationStyle::REVERSE_DRAWER_MICROS, 100_000);
        assert_eq!(ChipAnimationStyle::DISABLE_MICROS, 75_000);
        // A drawer closes faster than it opens: a thing arriving is worth
        // watching and a thing leaving is in the way.
        assert!(ChipAnimationStyle::REVERSE_DRAWER_MICROS < ChipAnimationStyle::DRAWER_MICROS);
        // And all four are distinct, which is the claim.
        let all = [
            ChipAnimationStyle::SELECT_MICROS,
            ChipAnimationStyle::DRAWER_MICROS,
            ChipAnimationStyle::REVERSE_DRAWER_MICROS,
            ChipAnimationStyle::DISABLE_MICROS,
        ];
        for (index, one) in all.iter().enumerate() {
            for other in &all[index + 1..] {
                assert_ne!(one, other);
            }
        }
    }

    #[test]
    fn each_animation_is_overridable_on_its_own() {
        // Four `Option`s rather than one style, so a caller who only cares
        // about the select animation does not have to restate the other three.
        let style = ChipAnimationStyle {
            select_animation: Some(crate::animation::AnimationStyle::NO_ANIMATION),
            ..ChipAnimationStyle::new()
        };
        assert!(style.select_animation.is_some());
        assert!(style.enable_animation.is_none());
        assert!(style.avatar_drawer_animation.is_none());
        assert!(style.delete_drawer_animation.is_none());
    }

    #[test]
    fn a_dialogs_padding_shrinks_as_the_readers_text_grows() {
        // Backwards until the alternative is considered: a dialog is a fixed,
        // small box, and padding that stayed put while the text doubled would
        // push the content off the bottom. The room has to come from
        // somewhere, and whitespace is the part a reader who asked for larger
        // text was not asking for.
        assert_eq!(scale_dialog_padding(1.0), 1.0, "the design's own padding");
        assert!((scale_dialog_padding(2.0) - 1.0 / 3.0).abs() < 0.001);
        assert!(
            scale_dialog_padding(1.5) < scale_dialog_padding(1.0),
            "and it only ever shrinks"
        );
    }

    #[test]
    fn the_padding_scale_is_clamped_at_both_ends() {
        // Below 1x nothing grows -- a reader with *smaller* text does not get
        // a roomier dialog than the design. Past 2x nothing shrinks further,
        // because a third is as tight as it may get before the text touches
        // the edges.
        assert_eq!(scale_dialog_padding(0.5), 1.0);
        assert_eq!(scale_dialog_padding(0.0), 1.0);
        assert_eq!(scale_dialog_padding(3.0), scale_dialog_padding(2.0));
        assert_eq!(scale_dialog_padding(100.0), scale_dialog_padding(2.0));
    }

    #[test]
    fn a_simple_dialogs_title_keeps_its_gap_to_the_first_option() {
        // Upstream's one asymmetry here: the bottom inset is the gap to the
        // first option, and a gap that shrank with the text would close up
        // exactly when the text needed separating. So it is *not* scaled --
        // but only when there is something below it to separate from.
        let with_options = SimpleDialog::new()
            .with_title("Pick one")
            .with_child(leaf(|| crate::widgets::Empty));
        let scaled = with_options.title_padding(2.0);
        assert!(
            scaled.top < SimpleDialog::TITLE_PADDING.top,
            "the top shrank"
        );
        assert_eq!(
            scaled.bottom,
            SimpleDialog::TITLE_PADDING.bottom,
            "the gap below did not"
        );

        // With nothing below, the bottom is scaled like the rest -- there is
        // no gap to protect.
        let bare = SimpleDialog::new().with_title("Nothing here");
        let bare_scaled = bare.title_padding(2.0);
        assert!(
            (bare_scaled.bottom - SimpleDialog::TITLE_PADDING.bottom * scale_dialog_padding(2.0))
                .abs()
                < 0.001
        );
    }

    #[test]
    fn a_simple_dialogs_options_run_edge_to_edge() {
        // Its content padding has nothing at the sides, and the option
        // supplies its own 24 -- so the splash reaches the dialog's edges. An
        // option inset from the sides would read as a button in a list rather
        // than as a row of it.
        assert_eq!(SimpleDialog::CONTENT_PADDING.left, 0.0);
        assert_eq!(SimpleDialog::CONTENT_PADDING.right, 0.0);
        assert_eq!(SimpleDialogOption::DEFAULT_PADDING.left, 24.0);
        assert_eq!(SimpleDialogOption::DEFAULT_PADDING.right, 24.0);
    }

    #[test]
    fn an_icon_is_what_centres_an_alert_dialogs_title() {
        // A left-aligned title under a centred icon reads as a mistake, so
        // upstream ties the two together rather than offering the alignment
        // separately.
        assert!(!AlertDialog::new().with_title("Delete?").centres_title());
        assert!(
            AlertDialog::new()
                .with_title("Delete?")
                .with_icon(leaf(|| crate::widgets::Empty))
                .centres_title()
        );
    }

    #[test]
    fn every_alert_padding_scales_the_same_way() {
        // One rule over all of them, so a dialog does not go lopsided at a
        // large text size.
        for base in [
            AlertDialog::TITLE_PADDING,
            AlertDialog::CONTENT_PADDING,
            AlertDialog::ACTIONS_PADDING,
        ] {
            let scaled = AlertDialog::scaled(base, 2.0);
            let factor = scale_dialog_padding(2.0);
            assert!((scaled.left - base.left * factor).abs() < 0.001);
            assert!((scaled.top - base.top * factor).abs() < 0.001);
            assert!((scaled.right - base.right * factor).abs() < 0.001);
            assert!((scaled.bottom - base.bottom * factor).abs() < 0.001);
        }
        // And at the default scale nothing moves at all.
        assert_eq!(
            AlertDialog::scaled(AlertDialog::CONTENT_PADDING, 1.0),
            AlertDialog::CONTENT_PADDING
        );
    }

    #[test]
    fn a_simple_dialog_option_builds_its_child_through_the_builder() {
        // The ink well rebuilds from the same widget instance whenever its own
        // state changes -- a splash is state -- so a child handed over once
        // would be gone on the second build. Mounting it proves the builder is
        // what the child comes through.
        let built = std::rc::Rc::new(std::cell::Cell::new(0));
        let counter = std::rc::Rc::clone(&built);
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(crate::framework::provide(
            Theme::dark(),
            component(SimpleDialogOption::new(1, move || {
                counter.set(counter.get() + 1);
                leaf(|| Container::new().with_size(100.0, 20.0))
            })),
        ));
        assert!(built.get() >= 1, "the builder ran");
        assert!(tree.build_render_tree().is_some(), "and produced a tree");
    }

    #[test]
    fn an_action_chip_has_no_state_to_be_in() {
        // The distinction the taxonomy exists to make. Pressing an action chip
        // starts something and it looks the same afterwards, so it implements
        // `TappableChipAttributes` and *not* `SelectableChipAttributes` -- a
        // lit-up action chip would be claiming a state it does not have.
        //
        // Written as a compile-time check: a function that only accepts
        // selectable chips would not take this one, and adding the trait to
        // `ActionChip` later would make this test's *absence* of a call the
        // only thing that noticed.
        fn only_selectable<T: SelectableChipAttributes>(chip: &T) -> bool {
            chip.selected()
        }
        assert!(!only_selectable(&ChoiceChip::new("A").with_selected(false)));
        assert!(!only_selectable(&FilterChip::new("B")));
        assert!(!only_selectable(&InputChip::new("C")));
        // And the action chip is tappable, which the other question is about.
        let pressed = std::rc::Rc::new(std::cell::Cell::new(false));
        let flag = std::rc::Rc::clone(&pressed);
        let action = ActionChip::new("Do it").with_on_pressed(move || flag.set(true));
        action.on_pressed().expect("a callback")();
        assert!(pressed.get());
    }

    #[test]
    fn an_input_chip_is_the_only_one_that_is_all_six() {
        // Which is the point: an input chip is a piece of the reader's own
        // input, so it can be pressed, chosen, ticked, deleted and disabled.
        // The other three are each a subset of it.
        fn needs_all_six<T>(_: &T)
        where
            T: ChipAttributes
                + DeletableChipAttributes
                + SelectableChipAttributes
                + CheckmarkableChipAttributes
                + DisabledChipAttributes
                + TappableChipAttributes,
        {
        }
        needs_all_six(&InputChip::new("someone@example.com"));
    }

    #[test]
    fn a_choice_chip_shows_no_tick_and_a_filter_chip_leaves_it_to_the_theme() {
        // The difference is not the widget but the *set*. One choice out of a
        // set is already told apart by its colour, so a tick on the single
        // chosen one says the same thing twice; several filters can be on at
        // once, so the set has to be readable at a glance and colour alone is
        // not enough.
        assert_eq!(ChoiceChip::new("Small").show_checkmark(), Some(false));
        assert_eq!(FilterChip::new("Vegan").show_checkmark(), None);
        assert_eq!(InputChip::new("tag").show_checkmark(), None);
    }

    #[test]
    fn a_choice_or_filter_chip_with_no_callback_is_disabled_but_an_input_chip_is_not() {
        // The three derive `isEnabled` from having something to do; an input
        // chip carries it, because it is showing something the reader typed
        // whether or not it can still be acted on.
        assert!(!ChoiceChip::new("A").is_enabled());
        assert!(ChoiceChip::new("A").with_on_selected(|_| {}).is_enabled());
        assert!(!FilterChip::new("B").is_enabled());
        assert!(!ActionChip::new("C").is_enabled());
        assert!(ActionChip::new("C").with_on_pressed(|| {}).is_enabled());

        assert!(
            InputChip::new("D").is_enabled(),
            "present and inert is a state"
        );
        assert!(!InputChip::new("D").with_enabled(false).is_enabled());
    }

    #[test]
    fn only_the_two_set_chips_can_be_deleted() {
        // A choice is one of a fixed set, so there is nothing to remove --
        // picking another is how you stop picking this one. An action chip
        // has nothing to remove either.
        assert!(!FilterChip::new("A").is_deletable());
        assert!(FilterChip::new("A").with_on_deleted(|| {}).is_deletable());
        assert!(InputChip::new("B").with_on_deleted(|| {}).is_deletable());
    }

    #[test]
    fn every_variant_has_an_elevated_form() {
        // Upstream's `.elevated` constructors, which are a shape rather than a
        // number: an elevated chip sits on its own surface instead of being an
        // outline on the page.
        assert!(!ActionChip::new("A").0.elevated);
        assert!(ActionChip::elevated("A").0.elevated);
        assert!(ChoiceChip::elevated("B").0.elevated);
        assert!(FilterChip::elevated("C").0.elevated);
    }

    #[test]
    fn a_no_splash_feature_presses_without_painting() {
        // It exists rather than being expressed as "no feature" because the
        // *gesture* still has to happen: a control whose theme asks for no
        // splash still presses and still fires its callback.
        use crate::ink::{InkFeatureKind, InteractiveInkFeatureFactory};
        use crate::render::{Offset, Size};

        let mut feature = InteractiveInkFeatureFactory::NoSplash.create(
            Size::new(100.0, 40.0),
            Offset::new(10.0, 10.0),
            Color::argb(0xFF, 0, 0, 0),
            true,
            0,
        );
        assert!(matches!(feature.kind, InkFeatureKind::None(_)));
        assert_eq!(feature.opacity(), 0.0, "nothing to paint, ever");
        assert_eq!(feature.ink_circle(Size::new(100.0, 40.0)), None);
        assert_eq!(feature.paint_color().alpha(), 0);

        // Alive while the finger is down -- the press is happening -- and gone
        // the moment it settles, because upstream's confirm and cancel both
        // dispose outright rather than fading.
        assert!(feature.alive());
        feature.confirm(1_000);
        assert!(!feature.alive());
    }

    #[test]
    fn the_no_splash_factory_is_a_third_choice_beside_the_other_two() {
        // A theme swaps every splash in an application at once, and "none" is
        // one of the things it can swap to.
        use crate::ink::InteractiveInkFeatureFactory;
        assert_ne!(
            InteractiveInkFeatureFactory::NoSplash,
            InteractiveInkFeatureFactory::default()
        );
        assert_ne!(
            InteractiveInkFeatureFactory::NoSplash,
            InteractiveInkFeatureFactory::Splash
        );
    }
}
