// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The component demos.
//!
//! Ported from `new_gallery/lib/demos/material/`, one function per demo instead
//! of one file. Upstream each demo is a StatefulWidget with its own State;
//! here the state they need is a handful of fields on [`DemoState`], because a
//! demo that owned its own element would still have to be found by the router,
//! and one shared struct is less machinery than a registry of them.
//!
//! Every demo is interactive. A demo that only drew a picture of a control
//! would be a screenshot with extra steps.

use rustflutter::animation::Curve;
use rustflutter::components::theme_of;
use rustflutter::framework::{AnyWidget, BuildContext, StateHandle, component, leaf, many};
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, CrossAxisAlignment, FlexChild, MainAxisAlignment, MainAxisSize, RenderFlex,
};
use rustflutter::widgets::{Align, Container, Empty, Pointer};

use crate::app::{self, GalleryState, ids};
use crate::catalog::Demo;

/// Everything the demos need to remember between frames.
///
/// One struct rather than one per demo: a demo is opened, used and left, and
/// the router resets this on every open, so a field that another demo also uses
/// cannot leak state across a visit.
#[derive(Clone, Debug)]
pub struct DemoState {
    pub checkbox_a: bool,
    pub checkbox_b: bool,
    pub radio: usize,
    pub switch: bool,
    pub slider: f32,
    pub tab: usize,
    pub bottom_nav: usize,
    pub chips: Vec<bool>,
    pub dialog_open: bool,
    pub sheet_open: bool,
    pub snackbar_open: bool,
    pub banner_open: bool,
    pub rail: usize,
    pub rail_extended: bool,
    pub tooltip_pressed: bool,
    pub counter: i32,
}

impl Default for DemoState {
    fn default() -> DemoState {
        DemoState {
            checkbox_a: true,
            checkbox_b: false,
            radio: 0,
            switch: true,
            slider: 0.35,
            tab: 0,
            bottom_nav: 0,
            chips: vec![true, false, false, false],
            dialog_open: false,
            sheet_open: false,
            snackbar_open: false,
            banner_open: true,
            rail: 1,
            rail_extended: false,
            tooltip_pressed: false,
            counter: 0,
        }
    }
}

/// Builds a demo's page: the description, then the demo itself.
pub fn page(
    demo: &'static Demo,
    state: &GalleryState,
    handle: StateHandle<GalleryState>,
) -> AnyWidget {
    let body = app::scrolling_body(
        vec![
            component(Description { text: demo.description, accent: demo.accent }),
            component(Stage { demo, state: state.demo.clone(), pressed: state.pressed, handle: handle.clone() }),
        ],
        16.0,
        16.0,
    );

    let page = app::scaffold(demo.title, Some(demo.subtitle), state, handle.clone(), body);
    app::with_overlay(page, overlay(demo, state, handle))
}

/// The paragraph at the top of every demo page.
struct Description {
    text: &'static str,
    accent: Color,
}

impl Component for Description {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let text = self.text;
        let accent = self.accent;
        let muted = theme.muted();
        let surface = theme.surface_variant;
        let radius = theme.radius;
        let spacing = theme.spacing;

        leaf(move || {
            Container::new()
                .with_color(surface)
                .with_corner_radius(radius)
                .with_padding(EdgeInsets::all(spacing * 1.75))
                .with_child(
                    Column::new()
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(spacing)
                        // A coloured rule above the text rather than beside it.
                        // Beside would need the rule to be as tall as the text,
                        // and inside a scroll viewport there is no height to
                        // stretch to until the text has been laid out.
                        .push(
                            Container::new()
                                .with_width(34.0)
                                .with_height(3.0)
                                .with_color(accent)
                                .with_corner_radius(2.0),
                        )
                        .push(Text::new(text).with_style(muted.clone())),
                )
        })
    }
}

/// The demo itself, dispatched by slug.
struct Stage {
    demo: &'static Demo,
    state: DemoState,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
}

impl Component for Stage {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let state = &self.state;
        let handle = self.handle.clone();
        let pressed = self.pressed;

        let content: AnyWidget = match self.demo.slug {
            "app-bar" => app_bar(state, pressed, handle),
            "banner" => banner(state, handle),
            "bottom-navigation" => bottom_navigation(state, handle),
            "bottom-sheet" => sheet_launcher(state, pressed, handle),
            "button" => buttons(state, pressed, handle),
            "card" => cards(),
            "chip" => chips(state, handle),
            "data-table" => data_table(),
            "dialog" => dialog_launcher(state, pressed, handle),
            "divider" => dividers(),
            "grid-lists" => grid_lists(),
            "lists" => lists(),
            "nav_rail" => navigation_rail(state, pressed, handle),
            "progress-indicator" => progress(state, context),
            "selection-controls" => selection_controls(state, handle),
            "sliders" => sliders(state, handle),
            "snackbars" => snackbar_launcher(state, pressed, handle),
            "tabs" => tabs(state, handle),
            "tooltip" => tooltips(state, handle),
            "colors" => colors(context),
            "typography" => typography(context),
            "motion" => motion(context),
            "2d-transformations" => layout_demo(),
            other => {
                let slug = other.to_string();
                leaf(move || {
                    Text::new(format!("The demo for {slug} is not written yet.")).with_size(13.0)
                })
            }
        };

        let surface = theme.surface;
        let outline = theme.outline;
        let radius = theme.radius;
        let spacing = theme.spacing;
        rustflutter::framework::single(content, move |inner| {
            Box::new(
                Container::new()
                    .with_color(surface)
                    .with_corner_radius(radius)
                    .with_border(1.0, outline)
                    .with_padding(EdgeInsets::all(spacing * 2.0))
                    .with_child(inner),
            )
        })
    }
}

/// The modal a demo puts over its own page, if it has one open.
fn overlay(
    demo: &'static Demo,
    state: &GalleryState,
    handle: StateHandle<GalleryState>,
) -> Option<AnyWidget> {
    match demo.slug {
        "dialogs" if state.demo.dialog_open => Some(dialog_overlay(state, handle)),
        "bottom-sheet" if state.demo.sheet_open => Some(sheet_overlay(handle)),
        "snackbars" if state.demo.snackbar_open => Some(snackbar_overlay(handle)),
        _ => None,
    }
}

// -- Layout helpers -----------------------------------------------------------

fn column(children: Vec<AnyWidget>, spacing: f32) -> AnyWidget {
    many(children, move |rendered| {
        // Start rather than Stretch. Stretch forces a tight cross-axis
        // constraint, which overrides a child's own width -- a 48px switch
        // becomes as wide as the page. Under Start a child with no width of
        // its own still takes the full width, because it takes the biggest
        // size it is offered.
        let mut flex = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(spacing);
        for child in rendered {
            flex = flex.push(child);
        }
        Box::new(flex)
    })
}

fn row(children: Vec<AnyWidget>, spacing: f32) -> AnyWidget {
    many(children, move |rendered| {
        let mut flex = RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(spacing);
        for child in rendered {
            flex = flex.push(child);
        }
        Box::new(flex)
    })
}

fn caption(text: impl Into<String>) -> AnyWidget {
    let text = text.into();
    component(Caption { text })
}

struct Caption {
    text: String,
}

impl Component for Caption {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let text = self.text.clone();
        let style = TextStyle {
            font_size: theme.body_size - 2.0,
            color: theme.text_muted,
            font_weight: 700,
            ..TextStyle::default()
        };
        leaf(move || Text::new(text.clone()).with_style(style.clone()))
    }
}

// -- The demos ----------------------------------------------------------------

fn app_bar(state: &DemoState, pressed: Option<u64>, handle: StateHandle<GalleryState>) -> AnyWidget {
    let count = state.counter;
    column(
        vec![
            caption("An app bar with a title, a subtitle and an action"),
            component(
                AppBar::new("Inbox")
                    .with_subtitle(format!("{count} unread"))
                    .with_trailing(component(
                        Button::new(ids::DEMO_LOCAL, "Mark read")
                            .with_style(ButtonStyle::Text)
                            .with_pressed(pressed == Some(ids::DEMO_LOCAL))
                            .wired(handle, |s| &mut s.pressed, |s| s.demo.counter += 1),
                    )),
            ),
        ],
        12.0,
    )
}

fn banner(state: &DemoState, handle: StateHandle<GalleryState>) -> AnyWidget {
    if !state.banner_open {
        let restore = handle;
        return column(
            vec![
                caption("Dismissed"),
                component(
                    Button::new(ids::DEMO_LOCAL, "Show the banner again")
                        .with_style(ButtonStyle::Outlined)
                        .wired(restore, |s| &mut s.pressed, |s| s.demo.banner_open = true),
                ),
            ],
            12.0,
        );
    }

    column(
        vec![
            caption("A banner stays until it is answered"),
            component(
                Banner::new("Your account is not verified. Some features are limited.")
                    .with_action(component(
                        Button::new(ids::DEMO_LOCAL + 1, "Dismiss")
                            .with_style(ButtonStyle::Text)
                            .wired(handle, |s| &mut s.pressed, |s| s.demo.banner_open = false),
                    )),
            ),
        ],
        12.0,
    )
}

fn bottom_navigation(state: &DemoState, handle: StateHandle<GalleryState>) -> AnyWidget {
    let selected = state.bottom_nav;
    let labels = ["Everything in one place", "What you saved", "Who you follow"];
    let body = labels
        .get(selected)
        .copied()
        .unwrap_or("Nothing here")
        .to_string();

    column(
        vec![
            caption("The view above cross-fades when the destination changes"),
            component(FadedPanel { text: body, key_index: selected }),
            component(
                BottomNavigation::new(
                    ids::DEMO_LOCAL,
                    vec![
                        Destination::new("Home", "H"),
                        Destination::new("Saved", "S"),
                        Destination::new("People", "P"),
                    ],
                    selected,
                )
                .wired(handle, |s, index| s.demo.bottom_nav = index),
            ),
        ],
        12.0,
    )
}

/// A panel whose key changes with the selection.
///
/// The key is the point: without one the element is reused and the text simply
/// changes, and with one it is replaced -- which is what a cross-fade would
/// animate between if the framework had an implicit one.
struct FadedPanel {
    text: String,
    key_index: usize,
}

impl Component for FadedPanel {
    fn key(&self) -> rustflutter::framework::Key {
        Some(self.key_index as u64)
    }

    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let text = self.text.clone();
        let surface = theme.surface_variant;
        let radius = theme.radius;
        let body = theme.body();
        leaf(move || {
            Container::new()
                .with_height(96.0)
                .with_color(surface)
                .with_corner_radius(radius)
                .with_child(Align::new(
                    Alignment::CENTER,
                    Text::new(text.clone()).with_style(body.clone()),
                ))
        })
    }
}

fn sheet_launcher(
    state: &DemoState,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
) -> AnyWidget {
    let _ = state;
    column(
        vec![
            caption("A modal sheet draws over a scrim that swallows taps"),
            component(
                Button::new(ids::DEMO_LOCAL, "Show the sheet")
                    .with_pressed(pressed == Some(ids::DEMO_LOCAL))
                    .wired(handle, |s| &mut s.pressed, |s| s.demo.sheet_open = true),
            ),
        ],
        12.0,
    )
}

fn sheet_overlay(handle: StateHandle<GalleryState>) -> AnyWidget {
    let scrim_handle = handle.clone();
    let close_handle = handle;
    many(
        vec![
            component(Scrim::new(ids::SCRIM).wired(scrim_handle, |s| s.demo.sheet_open = false)),
            component(BottomSheet::new(column(
                vec![
                    caption("Anything can go in a sheet"),
                    component(
                        Button::new(ids::DEMO_LOCAL + 1, "Close")
                            .with_style(ButtonStyle::Outlined)
                            .wired(close_handle, |s| &mut s.pressed, |s| s.demo.sheet_open = false),
                    ),
                ],
                12.0,
            ))
            .with_title("Share")),
        ],
        |mut rendered| {
            let sheet = rendered.pop().unwrap_or_else(|| Box::new(Empty));
            let scrim = rendered.pop().unwrap_or_else(|| Box::new(Empty));
            Box::new(
                rustflutter::widgets::Stack::new()
                    .push_positioned(scrim, rustflutter::widgets::Positioned::fill())
                    .push_positioned(
                        sheet,
                        rustflutter::render::StackPosition {
                            left: Some(0.0),
                            right: Some(0.0),
                            bottom: Some(0.0),
                            ..Default::default()
                        },
                    ),
            )
        },
    )
}

fn buttons(state: &DemoState, pressed: Option<u64>, handle: StateHandle<GalleryState>) -> AnyWidget {
    let taps = state.counter;
    let base = ids::DEMO_LOCAL;
    column(
        vec![
            caption(format!("{taps} taps so far")),
            row(
                vec![
                    component(
                        Button::new(base, "Filled")
                            .with_pressed(pressed == Some(base))
                            .wired(handle.clone(), |s| &mut s.pressed, |s| s.demo.counter += 1),
                    ),
                    component(
                        Button::new(base + 1, "Outlined")
                            .with_style(ButtonStyle::Outlined)
                            .with_pressed(pressed == Some(base + 1))
                            .wired(handle.clone(), |s| &mut s.pressed, |s| s.demo.counter += 1),
                    ),
                ],
                10.0,
            ),
            row(
                vec![
                    component(
                        Button::new(base + 2, "Text")
                            .with_style(ButtonStyle::Text)
                            .with_pressed(pressed == Some(base + 2))
                            .wired(handle.clone(), |s| &mut s.pressed, |s| s.demo.counter += 1),
                    ),
                    component(
                        Button::new(base + 3, "Danger")
                            .with_style(ButtonStyle::Danger)
                            .with_pressed(pressed == Some(base + 3))
                            .wired(handle, |s| &mut s.pressed, |s| s.demo.counter = 0),
                    ),
                    component(
                        Button::new(base + 4, "Disabled")
                            .with_style(ButtonStyle::Outlined)
                            .with_enabled(false),
                    ),
                ],
                10.0,
            ),
        ],
        12.0,
    )
}

fn cards() -> AnyWidget {
    column(
        vec![
            caption("A card groups things that belong together"),
            component(Card::new(column(
                vec![
                    component(Label::title("Weekly report")),
                    component(Label::muted("Sales are up 12% on last week")),
                ],
                6.0,
            ))),
            component(Card::new(column(
                vec![
                    component(Label::title("Storage")),
                    component(Label::muted("48.2 GB of 64 GB used")),
                    component(ProgressBar::new(0.75).with_width(260.0)),
                ],
                8.0,
            ))),
        ],
        12.0,
    )
}

fn chips(state: &DemoState, handle: StateHandle<GalleryState>) -> AnyWidget {
    let labels = ["All", "Unread", "Flagged", "Archived"];
    let mut children: Vec<AnyWidget> = Vec::new();
    for (index, label) in labels.iter().enumerate() {
        let selected = state.chips.get(index).copied().unwrap_or(false);
        let chip = Chip::new(ids::DEMO_LOCAL + index as u64, *label).selected(selected);
        // A fn pointer cannot capture, so the index is dispatched here. Four
        // arms is less machinery than boxing the callback.
        children.push(component(match index {
            0 => chip.wired(handle.clone(), |s| toggle_chip(s, 0)),
            1 => chip.wired(handle.clone(), |s| toggle_chip(s, 1)),
            2 => chip.wired(handle.clone(), |s| toggle_chip(s, 2)),
            _ => chip.wired(handle.clone(), |s| toggle_chip(s, 3)),
        }));
    }

    let chosen: Vec<&str> = labels
        .iter()
        .enumerate()
        .filter(|(index, _)| state.chips.get(*index).copied().unwrap_or(false))
        .map(|(_, label)| *label)
        .collect();
    let summary = if chosen.is_empty() {
        "Nothing selected".to_string()
    } else {
        chosen.join(", ")
    };

    column(
        vec![caption(summary), row(children, 8.0)],
        12.0,
    )
}

fn toggle_chip(state: &mut GalleryState, index: usize) {
    if let Some(value) = state.demo.chips.get_mut(index) {
        *value = !*value;
    }
}

fn data_table() -> AnyWidget {
    column(
        vec![
            caption("Columns share the width through the row's flex"),
            component(
                DataTable::new(vec!["Dessert".into(), "Calories".into(), "Fat".into()])
                    .push_row(vec!["Frozen yoghurt".into(), "159".into(), "6.0".into()])
                    .push_row(vec!["Ice cream sandwich".into(), "237".into(), "9.0".into()])
                    .push_row(vec!["Eclair".into(), "262".into(), "16.0".into()])
                    .push_row(vec!["Cupcake".into(), "305".into(), "3.7".into()]),
            ),
        ],
        12.0,
    )
}

fn dialog_launcher(
    state: &DemoState,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
) -> AnyWidget {
    let count = state.counter;
    column(
        vec![
            caption(if count == 0 {
                "Not confirmed yet".to_string()
            } else {
                format!("Confirmed {count} times")
            }),
            component(
                Button::new(ids::DEMO_LOCAL, "Delete everything")
                    .with_style(ButtonStyle::Danger)
                    .with_pressed(pressed == Some(ids::DEMO_LOCAL))
                    .wired(handle, |s| &mut s.pressed, |s| s.demo.dialog_open = true),
            ),
        ],
        12.0,
    )
}

fn dialog_overlay(state: &GalleryState, handle: StateHandle<GalleryState>) -> AnyWidget {
    let pressed = state.pressed;
    let scrim_handle = handle.clone();
    let cancel_handle = handle.clone();
    let confirm_handle = handle;

    many(
        vec![
            component(Scrim::new(ids::SCRIM).wired(scrim_handle, |s| s.demo.dialog_open = false)),
            component(
                Dialog::new("Delete everything?")
                    .with_body("This cannot be undone. Everything in this demo will go.")
                    .with_action(component(
                        Button::new(ids::DEMO_LOCAL + 1, "Cancel")
                            .with_style(ButtonStyle::Text)
                            .with_pressed(pressed == Some(ids::DEMO_LOCAL + 1))
                            .wired(cancel_handle, |s| &mut s.pressed, |s| s.demo.dialog_open = false),
                    ))
                    .with_action(component(
                        Button::new(ids::DEMO_LOCAL + 2, "Delete")
                            .with_style(ButtonStyle::Danger)
                            .with_pressed(pressed == Some(ids::DEMO_LOCAL + 2))
                            .wired(confirm_handle, |s| &mut s.pressed, |s| {
                                s.demo.counter += 1;
                                s.demo.dialog_open = false;
                            }),
                    )),
            ),
        ],
        |mut rendered| {
            let dialog = rendered.pop().unwrap_or_else(|| Box::new(Empty));
            let scrim = rendered.pop().unwrap_or_else(|| Box::new(Empty));
            Box::new(
                rustflutter::widgets::Stack::new()
                    .push_positioned(scrim, rustflutter::widgets::Positioned::fill())
                    .push(rustflutter::widgets::Center::new(dialog)),
            )
        },
    )
}

fn dividers() -> AnyWidget {
    column(
        vec![
            caption("One physical pixel, whatever the display scale"),
            component(Label::new("Above")),
            component(Divider),
            component(Label::new("Between")),
            component(Divider),
            component(Label::new("Below")),
        ],
        10.0,
    )
}

fn grid_lists() -> AnyWidget {
    let colors = [
        Color::rgb(0x54, 0xC5, 0xF8),
        Color::rgb(0x7B, 0xD3, 0x89),
        Color::rgb(0xF2, 0xB1, 0x4F),
        Color::rgb(0xE0, 0x7A, 0x9B),
        Color::rgb(0x9B, 0x8C, 0xF0),
        Color::rgb(0x4F, 0xC8, 0xB0),
    ];
    let mut grid = GridList::new(3).with_spacing(10.0).with_aspect_ratio(1.2);
    for (index, color) in colors.iter().enumerate() {
        let color = *color;
        grid = grid.push(leaf(move || {
            Container::new()
                .with_color(color.with_alpha(0x3A))
                .with_corner_radius(10.0)
                .with_border(1.0, color)
                .with_child(Align::new(
                    Alignment::CENTER,
                    Text::new(format!("{}", index + 1))
                        .with_size(18.0)
                        .with_weight(700)
                        .with_color(color),
                ))
        }));
    }

    column(
        vec![
            caption("Three columns, tiles at a 1.2 aspect ratio"),
            component(grid),
        ],
        12.0,
    )
}

fn lists() -> AnyWidget {
    let entries = [
        ("Constraints go down", "A parent hands each child a BoxConstraints"),
        ("Sizes come up", "The child picks a size inside them"),
        ("The parent positions the child", "Nothing reads its parent"),
        ("Flex divides what is left", "Inflexible children first"),
        ("A viewport is a window", "Laid out unbounded, shown at an offset"),
        ("Stacks overlay", "Anchor to an edge, or stretch across two"),
        ("Intrinsics ask ahead", "What a box would like, before it knows"),
        ("Hit tests walk back", "Front to back, innermost first"),
    ];
    let mut children: Vec<AnyWidget> = vec![caption("A scrollable column of tiles")];
    for (index, (title, subtitle)) in entries.iter().enumerate() {
        let accent = if index % 2 == 0 {
            Color::rgb(0x54, 0xC5, 0xF8)
        } else {
            Color::rgb(0x7B, 0xD3, 0x89)
        };
        children.push(component(
            ListTile::new(*title).with_subtitle(*subtitle).with_accent(accent),
        ));
        if index + 1 < entries.len() {
            children.push(component(Divider));
        }
    }
    column(children, 4.0)
}

fn navigation_rail(
    state: &DemoState,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
) -> AnyWidget {
    let selected = state.rail;
    let extended = state.rail_extended;
    let toggle_handle = handle.clone();

    let rail = component(
        NavigationRail::new(
            ids::DEMO_LOCAL,
            vec![
                Destination::new("Inbox", "In"),
                Destination::new("Starred", "St"),
                Destination::new("Sent", "Se"),
            ],
            selected,
        )
        .extended(extended)
        .wired(handle, |s, index| s.demo.rail = index),
    );

    let names = ["Inbox", "Starred", "Sent"];
    let title = names.get(selected).copied().unwrap_or("Inbox").to_string();
    let body = component(RailBody { title });

    column(
        vec![
            caption("A rail suits a window too wide for a bottom bar"),
            many(vec![rail, body], |mut rendered| {
                let body = rendered.pop().unwrap_or_else(|| Box::new(Empty));
                let rail = rendered.pop().unwrap_or_else(|| Box::new(Empty));
                Box::new(
                    RenderFlex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .push(rail)
                        .push_flex(FlexChild::expanded(body, 1)),
                )
            }),
            component(
                Button::new(ids::DEMO_LOCAL + 10, if extended { "Collapse" } else { "Extend" })
                    .with_style(ButtonStyle::Outlined)
                    .with_pressed(pressed == Some(ids::DEMO_LOCAL + 10))
                    .wired(toggle_handle, |s| &mut s.pressed, |s| {
                        s.demo.rail_extended = !s.demo.rail_extended;
                    }),
            ),
        ],
        12.0,
    )
}

struct RailBody {
    title: String,
}

impl Component for RailBody {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let title = self.title.clone();
        let surface = theme.surface_variant;
        let radius = theme.radius;
        let style = theme.title();
        leaf(move || {
            Container::new()
                .with_height(180.0)
                .with_color(surface)
                .with_corner_radius(radius)
                .with_child(Align::new(
                    Alignment::CENTER,
                    Text::new(title.clone()).with_style(style.clone()),
                ))
        })
    }
}

fn progress(state: &DemoState, context: &mut BuildContext) -> AnyWidget {
    // The looping controller drives the indeterminate spinner. Reading it here
    // rather than storing a copy is what keeps the two in step.
    let spin = context
        .inherited::<SpinnerValue>()
        .map(|value| value.0)
        .unwrap_or(0.0);
    let determinate = state.slider;

    column(
        vec![
            caption("Determinate: how much is done"),
            component(ProgressBar::new(determinate).with_width(280.0)),
            component(Label::muted(format!("{}%", (determinate * 100.0).round() as i32))),
            component(Divider),
            caption("Indeterminate: that something is happening"),
            row(
                vec![
                    component(Spinner::new(spin).with_size(40.0)),
                    component(Spinner::new(spin.min(0.75)).with_size(28.0)),
                ],
                16.0,
            ),
        ],
        12.0,
    )
}

/// The spinner's current value, published by the app so the progress demo can
/// read it without the demo owning the controller.
#[derive(Clone, Copy, Debug)]
pub struct SpinnerValue(pub f32);

fn selection_controls(state: &DemoState, handle: StateHandle<GalleryState>) -> AnyWidget {
    let base = ids::DEMO_LOCAL;
    column(
        vec![
            caption("Checkboxes: several independent choices"),
            component(
                Checkbox::new(base, state.checkbox_a)
                    .with_label("Send me updates")
                    .wired(handle.clone(), |s| s.demo.checkbox_a = !s.demo.checkbox_a),
            ),
            component(
                Checkbox::new(base + 1, state.checkbox_b)
                    .with_label("Share anonymous usage data")
                    .wired(handle.clone(), |s| s.demo.checkbox_b = !s.demo.checkbox_b),
            ),
            component(Divider),
            caption("Radios: one of a set"),
            component(
                Radio::new(base + 2, state.radio == 0)
                    .with_label("Every message")
                    .wired(handle.clone(), |s| s.demo.radio = 0),
            ),
            component(
                Radio::new(base + 3, state.radio == 1)
                    .with_label("Mentions only")
                    .wired(handle.clone(), |s| s.demo.radio = 1),
            ),
            component(
                Radio::new(base + 4, state.radio == 2)
                    .with_label("Nothing")
                    .wired(handle.clone(), |s| s.demo.radio = 2),
            ),
            component(Divider),
            caption("A switch takes effect immediately"),
            component(
                Switch::new(base + 5, state.switch)
                    .wired(handle, |s| s.demo.switch = !s.demo.switch),
            ),
        ],
        8.0,
    )
}

fn sliders(state: &DemoState, handle: StateHandle<GalleryState>) -> AnyWidget {
    let value = state.slider;
    column(
        vec![
            caption(format!("Value {:.2}", value)),
            component(
                Slider::new(ids::DEMO_LOCAL, value)
                    .with_width(280.0)
                    .wired(handle, |s, v| s.demo.slider = v),
            ),
            component(ProgressBar::new(value).with_width(280.0)),
        ],
        14.0,
    )
}

fn snackbar_launcher(
    state: &DemoState,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
) -> AnyWidget {
    let _ = state;
    column(
        vec![
            caption("A snackbar reports something that already happened"),
            component(
                Button::new(ids::DEMO_LOCAL, "Archive")
                    .with_pressed(pressed == Some(ids::DEMO_LOCAL))
                    .wired(handle, |s| &mut s.pressed, |s| s.demo.snackbar_open = true),
            ),
        ],
        12.0,
    )
}

fn snackbar_overlay(handle: StateHandle<GalleryState>) -> AnyWidget {
    let bar = component(
        Snackbar::new(ids::DEMO_LOCAL + 1, "Conversation archived")
            .with_action("Undo")
            .wired(handle, |s| s.demo.snackbar_open = false),
    );
    many(vec![bar], |mut rendered| {
        let bar = rendered.pop().unwrap_or_else(|| Box::new(Empty));
        Box::new(rustflutter::widgets::Stack::new().push_positioned(
            bar,
            rustflutter::render::StackPosition {
                left: Some(16.0),
                right: Some(16.0),
                bottom: Some(16.0),
                ..Default::default()
            },
        ))
    })
}

fn tabs(state: &DemoState, handle: StateHandle<GalleryState>) -> AnyWidget {
    let selected = state.tab;
    let bodies = [
        "Flights leave from three terminals.",
        "Trains run every twelve minutes.",
        "The bus takes forty minutes in traffic.",
    ];
    let body = bodies.get(selected).copied().unwrap_or("").to_string();

    column(
        vec![
            component(
                TabBar::new(
                    ids::DEMO_LOCAL,
                    vec!["Flights".into(), "Trains".into(), "Buses".into()],
                    selected,
                )
                .wired(handle, |s, index| s.demo.tab = index),
            ),
            component(FadedPanel { text: body, key_index: selected }),
        ],
        12.0,
    )
}

fn tooltips(state: &DemoState, handle: StateHandle<GalleryState>) -> AnyWidget {
    let showing = state.tooltip_pressed;
    let press_handle = handle;
    let handlers = PointerHandlers::new().with_press_change(move |down| {
        press_handle.set_state(move |s| s.demo.tooltip_pressed = down);
    });

    let target = component(TooltipTarget { handlers });
    let mut children = vec![
        caption("Press and hold the button"),
        target,
    ];
    if showing {
        children.push(component(Tooltip::new("This is the tooltip")));
    }
    column(children, 12.0)
}

struct TooltipTarget {
    handlers: PointerHandlers,
}

impl Component for TooltipTarget {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let handlers = self.handlers.clone();
        let surface = theme.surface_variant;
        let outline = theme.outline;
        let radius = theme.radius;
        let body = theme.body();
        leaf(move || {
            Pointer::new(
                ids::DEMO_LOCAL,
                Container::new()
                    .with_height(48.0)
                    .with_color(surface)
                    .with_corner_radius(radius)
                    .with_border(1.0, outline)
                    .with_child(Align::new(
                        Alignment::CENTER,
                        Text::new("Hold me").with_style(body.clone()),
                    )),
            )
            .with_handlers(handlers.clone())
        })
    }
}

fn colors(context: &mut BuildContext) -> AnyWidget {
    let theme = theme_of(context);
    let swatches: Vec<(&str, Color, &str)> = vec![
        ("background", theme.background, "Behind everything"),
        ("surface", theme.surface, "Cards, bars, sheets"),
        ("surface variant", theme.surface_variant, "Wells and tracks"),
        ("outline", theme.outline, "Borders and rules"),
        ("primary", theme.primary, "The one action that matters"),
        ("danger", theme.danger, "Destructive actions"),
        ("text", theme.text, "Body copy"),
        ("text muted", theme.text_muted, "Secondary copy"),
    ];

    let mut children: Vec<AnyWidget> = vec![caption("Switch the theme in settings")];
    for (name, color, purpose) in swatches {
        let name = name.to_string();
        let purpose = purpose.to_string();
        let outline = theme.outline;
        let body = theme.body();
        let muted = theme.muted();
        children.push(leaf(move || {
            RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(14.0)
                .push(
                    Container::new()
                        .with_size(44.0, 30.0)
                        .with_color(color)
                        .with_corner_radius(7.0)
                        .with_border(1.0, outline),
                )
                .push_flex(FlexChild::expanded(
                    Column::new()
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(2.0)
                        .push(
                            Text::new(name.clone())
                                .with_style(TextStyle { font_weight: 700, ..body.clone() }),
                        )
                        .push(Text::new(purpose.clone()).with_style(muted.clone())),
                    1,
                ))
        }));
    }
    column(children, 10.0)
}

fn typography(context: &mut BuildContext) -> AnyWidget {
    let theme = theme_of(context);
    let samples: Vec<(&str, f32, i32)> = vec![
        ("Display", 34.0, 700),
        ("Headline", 26.0, 700),
        ("Title", 20.0, 700),
        ("Body", 14.0, 400),
        ("Caption", 12.0, 400),
        ("Overline", 10.0, 700),
    ];

    let mut children: Vec<AnyWidget> = vec![caption("Shaped by txt and skparagraph")];
    for (name, size, weight) in samples {
        let name = name.to_string();
        let text = theme.text;
        let muted = theme.text_muted;
        children.push(leaf(move || {
            RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Baseline)
                .with_spacing(14.0)
                .push(
                    Text::new(format!("{size:.0}"))
                        .with_size(11.0)
                        .with_color(muted),
                )
                .push_flex(FlexChild::expanded(
                    Text::new(name.clone())
                        .with_size(size)
                        .with_weight(weight)
                        .with_color(text),
                    1,
                ))
        }));
    }
    column(children, 8.0)
}

fn motion(context: &mut BuildContext) -> AnyWidget {
    let t = context
        .inherited::<MotionValue>()
        .map(|value| value.0)
        .unwrap_or(0.0);
    let curves: Vec<(&str, Curve, &str)> = vec![
        ("Linear", Curve::Linear, "No easing at all"),
        ("Ease in", Curve::EaseIn, "For something entering"),
        ("Ease out", Curve::EaseOut, "For something leaving"),
        ("Ease in out", Curve::EaseInOut, "For a change in place"),
        ("Decelerate", Curve::Decelerate, "Arriving under its own weight"),
        ("Ease out back", Curve::EaseOutBack, "Overshoots, then settles"),
    ];

    let mut children: Vec<AnyWidget> = vec![caption("The same time, bent six ways")];
    for (name, curve, purpose) in curves {
        children.push(component(CurveTrack {
            name: name.to_string(),
            purpose: purpose.to_string(),
            value: curve.transform(t),
        }));
    }
    column(children, 10.0)
}

/// The value the motion demo's controller is at, published by the app.
#[derive(Clone, Copy, Debug)]
pub struct MotionValue(pub f32);

struct CurveTrack {
    name: String,
    purpose: String,
    value: f32,
}

impl Component for CurveTrack {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let name = self.name.clone();
        let purpose = self.purpose.clone();
        let value = self.value.clamp(0.0, 1.0);
        let track = theme.surface_variant;
        let dot = theme.primary;
        let body = theme.body();
        let muted = theme.muted();

        leaf(move || {
            RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(12.0)
                .push(
                    Column::new()
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(1.0)
                        .push(
                            Text::new(name.clone())
                                .with_style(TextStyle { font_size: 12.0, font_weight: 700, ..body.clone() }),
                        )
                        .push(
                            Text::new(purpose.clone())
                                .with_style(TextStyle { font_size: 10.0, ..muted.clone() }),
                        ),
                )
                .push_flex(FlexChild::expanded(
                    Container::new()
                        .with_height(22.0)
                        .with_color(track)
                        .with_corner_radius(11.0)
                        .with_child(
                            // The dot's position is the curve's output, so the
                            // six tracks show the same time arriving at six
                            // different places.
                            RenderFlex::row()
                                .with_main_axis_size(MainAxisSize::Max)
                                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                                .push_flex(FlexChild::expanded(
                                    rustflutter::widgets::Empty,
                                    ((value * 1000.0) as u32).max(1),
                                ))
                                .push(
                                    Container::new()
                                        .with_size(14.0, 14.0)
                                        .with_color(dot)
                                        .with_corner_radius(7.0),
                                )
                                .push_flex(FlexChild::expanded(
                                    rustflutter::widgets::Empty,
                                    (((1.0 - value) * 1000.0) as u32).max(1),
                                )),
                        ),
                    1,
                ))
        })
    }
}

fn layout_demo() -> AnyWidget {
    column(
        vec![
            caption("A row of three, one of them flexible"),
            component(LayoutSample),
            component(Divider),
            caption("The same row, main axis alignment changed"),
            component(AlignmentSamples),
        ],
        12.0,
    )
}

struct LayoutSample;

impl Component for LayoutSample {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let a = theme.primary;
        let b = theme.danger;
        let c = theme.text_muted;
        let body = theme.body();
        leaf(move || {
            let block = |color: Color, label: &str, width: Option<f32>| {
                let mut container = Container::new()
                    .with_height(52.0)
                    .with_color(color.with_alpha(0x33))
                    .with_corner_radius(8.0)
                    .with_border(1.0, color)
                    .with_child(Align::new(
                        Alignment::CENTER,
                        Text::new(label.to_string())
                            .with_style(TextStyle { font_size: 11.0, ..body.clone() }),
                    ));
                if let Some(width) = width {
                    container = container.with_width(width);
                }
                container
            };
            RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.0)
                .push(block(a, "fixed 70", Some(70.0)))
                .push_flex(FlexChild::expanded(block(b, "expanded", None), 1))
                .push(block(c, "fixed 60", Some(60.0)))
        })
    }
}

struct AlignmentSamples;

impl Component for AlignmentSamples {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let accent = theme.primary;
        let track = theme.surface_variant;
        let muted = theme.muted();

        let alignments = [
            ("start", MainAxisAlignment::Start),
            ("center", MainAxisAlignment::Center),
            ("end", MainAxisAlignment::End),
            ("space between", MainAxisAlignment::SpaceBetween),
            ("space around", MainAxisAlignment::SpaceAround),
            ("space evenly", MainAxisAlignment::SpaceEvenly),
        ];

        let mut children: Vec<AnyWidget> = Vec::new();
        for (name, alignment) in alignments {
            let name = name.to_string();
            let muted = muted.clone();
            children.push(leaf(move || {
                let dot = || {
                    Container::new()
                        .with_size(16.0, 16.0)
                        .with_color(accent)
                        .with_corner_radius(8.0)
                };
                Column::new()
                    .with_cross_axis_alignment(CrossAxisAlignment::Start)
                    .with_spacing(4.0)
                    .push(Text::new(name.clone()).with_style(muted.clone()))
                    .push(
                        Container::new()
                            .with_height(30.0)
                            .with_color(track)
                            .with_corner_radius(8.0)
                            .with_padding(EdgeInsets::symmetric(6.0, 0.0))
                            .with_child(
                                RenderFlex::row()
                                    .with_main_axis_size(MainAxisSize::Max)
                                    .with_main_axis_alignment(alignment)
                                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                                    .push(dot())
                                    .push(dot())
                                    .push(dot()),
                            ),
                    )
            }));
        }
        column(children, 10.0)
    }
}
