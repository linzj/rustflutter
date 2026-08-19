// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Selection controls, navigation surfaces and overlays.
//!
//! The second half of the component library. [`crate::components`] has the
//! pieces a page is built from -- surfaces, text, buttons; this has the pieces
//! a page is *operated* by.
//!
//! # Overlays are the app's, not the framework's
//!
//! A dialog, a sheet and a snackbar all mean "draw this over everything and
//! take the taps first". That is a `Stack` with the overlay as its last child,
//! which the application already knows how to write, so there is no overlay
//! manager here -- only the things that go in one. What the framework would add
//! is a place to put an overlay from a callback that has no access to the build,
//! and every callback here has a `StateHandle` instead.

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

    pub fn selected(self, selected: bool) -> Self {
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
        let (fill, text_color, border) = match self.style {
            ChipStyle::Filter => (Color::TRANSPARENT, theme.text, Some(theme.outline)),
            ChipStyle::Selected => (
                theme.primary.with_alpha(0x33),
                theme.primary,
                Some(theme.primary),
            ),
            ChipStyle::Action => (theme.surface_variant, theme.text, None),
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
            if let Some(border) = border {
                container = container.with_border(1.0, border);
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
        let surface = theme.surface_variant;
        let outline = theme.outline;
        let body = theme.body();
        let spacing = theme.spacing;

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
                    .with_padding(EdgeInsets::symmetric(spacing * 2.0, spacing * 1.5))
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
}
