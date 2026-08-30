// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The component library, and the theme mechanism under it.
//!
//! Every control on screen reads its colours from a [`Theme`] published at the
//! root with `provide`. The switch in the title bar swaps that theme, which is
//! the point of the example: one `set_state` at the top changes a value, and
//! every component below it repaints in the new palette without any of them
//! knowing about each other.
//!
//! `--png <path>` renders headlessly; `--light` starts in the light theme.

use std::os::raw::{c_char, c_int};

use rustflutter::components::{gap, stack_column, stack_row};
use rustflutter::framework::{ElementTree, provide};
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, BoxConstraints, CrossAxisAlignment, MainAxisSize, Offset, RenderBox,
    RenderConstrainedBox, RenderDecoratedBox, RenderFlex,
};
use rustflutter::widgets::{Align, Container, Empty, ListView};

const WIDTH: i32 = 460;
const HEIGHT: i32 = 700;

// Hit-test identities. Handed out by hand rather than by an IdSource so that
// the same control keeps the same id across rebuilds -- an id that shifted
// would break a press that is in progress.
const ID_THEME_SWITCH: u64 = 1;
const ID_PRIMARY: u64 = 2;
const ID_OUTLINED: u64 = 3;
const ID_TEXT: u64 = 4;
const ID_DANGER: u64 = 5;
const ID_DISABLED: u64 = 6;
const ID_SLIDER: u64 = 7;
const ID_NOTIFICATIONS: u64 = 8;
const ID_TILE: u64 = 9;

#[derive(Default)]
struct ShowcaseState {
    light: bool,
    pressed: Option<u64>,
    volume: f32,
    notifications: bool,
    taps: u32,
}

struct Showcase {
    start_light: bool,
}

impl StatefulComponent for Showcase {
    type State = ShowcaseState;

    fn build(
        &self,
        state: &ShowcaseState,
        handle: StateHandle<ShowcaseState>,
        _context: &mut rustflutter::framework::BuildContext,
    ) -> AnyWidget {
        // The very first build has default state, which is dark; honour the
        // command line by flipping it once.
        let light = state.light || (self.start_light && state.taps == 0 && !state.light);
        let theme = if light { Theme::light() } else { Theme::dark() };

        provide(
            theme,
            component(Page {
                light,
                pressed: state.pressed,
                volume: if state.volume == 0.0 {
                    0.4
                } else {
                    state.volume
                },
                notifications: state.notifications,
                taps: state.taps,
                handle,
            }),
        )
    }
}

struct Page {
    light: bool,
    pressed: Option<u64>,
    volume: f32,
    notifications: bool,
    taps: u32,
    handle: StateHandle<ShowcaseState>,
}

impl Component for Page {
    fn build(&self, context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let spacing = theme.spacing;
        let handle = self.handle.clone();

        let app_bar = component(
            AppBar::new("Components")
                .with_subtitle(if self.light {
                    "light theme"
                } else {
                    "dark theme"
                })
                .with_trailing(component(
                    Switch::new(ID_THEME_SWITCH, self.light)
                        .wired(handle.clone(), |state| state.light = !state.light),
                )),
        );

        let body = component(Body {
            pressed: self.pressed,
            volume: self.volume,
            notifications: self.notifications,
            taps: self.taps,
            handle,
            spacing,
        });

        component(Scaffold::new(body).with_app_bar(app_bar))
    }
}

struct Body {
    pressed: Option<u64>,
    volume: f32,
    notifications: bool,
    taps: u32,
    handle: StateHandle<ShowcaseState>,
    spacing: f32,
}

impl Component for Body {
    fn build(&self, context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let spacing = self.spacing;
        let handle = &self.handle;

        let buttons = component(Card::new(stack_column(
            vec![
                component(Label::title("Buttons")),
                component(Label::muted(format!("{} taps so far", self.taps))),
                gap(1.0),
                stack_row(
                    vec![
                        component(
                            Button::new(ID_PRIMARY, "Primary")
                                .with_pressed(self.pressed == Some(ID_PRIMARY))
                                .wired(handle.clone(), |s| &mut s.pressed, |s| s.taps += 1),
                        ),
                        component(
                            Button::new(ID_OUTLINED, "Outlined")
                                .with_style(ButtonVariant::Outlined)
                                .with_pressed(self.pressed == Some(ID_OUTLINED))
                                .wired(handle.clone(), |s| &mut s.pressed, |s| s.taps += 1),
                        ),
                    ],
                    spacing,
                ),
                stack_row(
                    vec![
                        component(
                            Button::new(ID_TEXT, "Text")
                                .with_style(ButtonVariant::Text)
                                .with_pressed(self.pressed == Some(ID_TEXT))
                                .wired(handle.clone(), |s| &mut s.pressed, |s| s.taps += 1),
                        ),
                        component(
                            Button::new(ID_DANGER, "Danger")
                                .with_style(ButtonVariant::Danger)
                                .with_pressed(self.pressed == Some(ID_DANGER))
                                .wired(handle.clone(), |s| &mut s.pressed, |s| s.taps = 0),
                        ),
                        component(
                            Button::new(ID_DISABLED, "Disabled")
                                .with_style(ButtonVariant::Outlined)
                                .with_enabled(false),
                        ),
                    ],
                    spacing,
                ),
            ],
            spacing,
        )));

        let controls = component(Card::new(stack_column(
            vec![
                component(Label::title("Controls")),
                gap(0.5),
                component(
                    ListTile::new("Notifications")
                        .with_subtitle("delivered while the app is closed")
                        .with_accent(theme.primary)
                        .with_trailing(component(
                            Switch::new(ID_NOTIFICATIONS, self.notifications)
                                .wired(handle.clone(), |state| {
                                    state.notifications = !state.notifications
                                }),
                        )),
                ),
                component(Divider::new()),
                gap(0.5),
                component(Label::muted(format!(
                    "Volume {}%",
                    (self.volume * 100.0).round() as i32
                ))),
                component(SliderRow {
                    value: self.volume,
                    handle: handle.clone(),
                }),
                gap(0.5),
                component(Label::muted("Progress follows the slider")),
                component(ProgressRow { value: self.volume }),
            ],
            spacing,
        )));

        // One paragraph with three styles in it, which is what a rich text is
        // for: the line has to break as a sentence, not as three texts.
        let body = theme.body();
        let accent = theme.primary;
        let mixed = component(Card::new(stack_column(
            vec![
                component(Label::title("Rich text")),
                gap(0.5),
                leaf(move || {
                    Text::rich_spans(vec![
                        TextSpan::new("Hold ", body.clone()),
                        TextSpan::bold("Shift", &body),
                        TextSpan::new(" to select a range, or ", body.clone()),
                        TextSpan::new(
                            "Esc",
                            TextStyle {
                                color: accent,
                                font_weight: 700,
                                ..body.clone()
                            },
                        ),
                        TextSpan::new(" to give up. One paragraph, three styles.", body.clone()),
                    ])
                }),
            ],
            spacing,
        )));

        // The reader's text size, and the two ways a subtree can differ from
        // it. Upstream this is `MediaQuery.textScaler`, and the reason it is
        // per-subtree rather than global is on screen here: the first line is
        // a reading size and grows; the second is a logotype and does not.
        let scaled = component(Card::new(stack_column(
            vec![
                component(Label::title("Text scale")),
                gap(0.5),
                MediaQuery::clamped_text_scaling(
                    1.6,
                    1.6,
                    component(Label::new("This subtree reads at 1.6x")),
                ),
                MediaQuery::no_text_scaling(component(Label::muted(
                    "and this one opted out entirely",
                ))),
            ],
            spacing,
        )));

        let badges = component(Card::new(stack_column(
            vec![
                component(Label::title("Badges")),
                gap(0.5),
                stack_row(
                    vec![
                        component(Badge::new("layout")),
                        component(Badge::new("input").with_color(theme.danger)),
                        component(Badge::new("theme").with_color(theme.text_muted)),
                    ],
                    spacing,
                ),
            ],
            spacing,
        )));

        // The sizing widgets, each showing the one effect it owns: a clamp on
        // an unbounded axis, a scale-to-fit, a shared baseline line, a
        // fraction of the child, a child let out of its frame, and a
        // paint-only-this-child index.
        let primary = theme.primary;
        let danger = theme.danger;
        let demuted = theme.text_muted;
        let sizing = component(Card::new(stack_column(
            vec![
                component(Label::title("Sizing")),
                component(Label::muted("LimitedBox clamps an unbounded row at 90")),
                leaf(move || {
                    RenderFlex::row()
                        .with_main_axis_size(MainAxisSize::Min)
                        .push(LimitedBox::new(swatch(240.0, 10.0, primary)).with_max_width(90.0))
                }),
                component(Label::muted("FittedBox contain fits a swatch into 56x56")),
                leaf(move || {
                    RenderConstrainedBox::tight(56.0, 56.0)
                        .with_child(FittedBox::new(swatch(96.0, 36.0, danger)))
                }),
                component(Label::muted("Baseline puts every bottom edge on one line")),
                leaf(move || {
                    RenderFlex::row()
                        .with_main_axis_size(MainAxisSize::Min)
                        .with_spacing(6.0)
                        .push(Baseline::new(28.0, swatch(8.0, 28.0, primary)))
                        .push(Baseline::new(28.0, swatch(48.0, 12.0, danger)))
                        .push(Baseline::new(28.0, swatch(24.0, 20.0, demuted)))
                }),
                component(Label::muted(
                    "FractionallySizedBox is half its child's size",
                )),
                leaf(move || {
                    FractionallySizedBox::new(swatch(60.0, 24.0, primary))
                        .with_width_factor(0.5)
                        .with_height_factor(1.0)
                }),
                component(Label::muted(
                    "OverflowBox lets a 200px bar out of a 96px frame",
                )),
                leaf(move || {
                    RenderConstrainedBox::tight(96.0, 36.0).with_child(
                        OverflowBox::new(swatch(200.0, 12.0, danger))
                            .with_alignment(Alignment::CENTER),
                    )
                }),
                component(Label::muted(
                    "SizedOverflowBox is 64 wide, its child is not",
                )),
                leaf(move || {
                    SizedOverflowBox::new(Size::new(64.0, 32.0), swatch(140.0, 10.0, primary))
                        .with_alignment(Alignment::CENTER)
                }),
                component(Label::muted("IndexedStack paints child 1 of 2")),
                leaf(move || {
                    IndexedStack::new()
                        .with_index(Some(1))
                        .push(swatch(28.0, 28.0, demuted))
                        .push(swatch(28.0, 28.0, primary))
                }),
            ],
            spacing,
        )));

        let tile = component(Card::new(stack_column(
            vec![
                component(Label::title("Tappable tile")),
                gap(0.5),
                component(
                    ListTile::new("Tap anywhere on this row")
                        .with_subtitle("the whole tile is one hit-test region")
                        .with_accent(theme.primary)
                        .tappable(
                            ID_TILE,
                            rustflutter::gestures::PointerHandlers::new().with_tap({
                                let handle = handle.clone();
                                move |_| {
                                    handle.set_state(|s| s.taps += 1);
                                }
                            }),
                        ),
                ),
            ],
            spacing,
        )));

        // A ListView so the page scrolls if the window is short.
        let cards = vec![buttons, mixed, scaled, sizing, controls, badges, tile];
        many(cards, move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(spacing * 1.5);
            for child in rendered {
                column = column.push(child);
            }
            let mut list = ListView::new();
            list = list.push(column);
            Box::new(
                Container::new()
                    .with_padding(rustflutter::render::EdgeInsets::all(spacing * 1.5))
                    .with_child(list),
            )
        })
    }
}

/// The slider needs the theme's width, which only its own build knows.
struct SliderRow {
    value: f32,
    handle: StateHandle<ShowcaseState>,
}

impl Component for SliderRow {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        component(
            Slider::new(ID_SLIDER, self.value)
                .with_width(300.0)
                .wired(self.handle.clone(), |state, value| state.volume = value),
        )
    }
}

struct ProgressRow {
    value: f32,
}

impl Component for ProgressRow {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        component(ProgressBar::new(self.value).with_width(300.0))
    }
}

/// A coloured box of an exact size -- the visual child for the sizing demos.
fn swatch(width: f32, height: f32, color: Color) -> RenderConstrainedBox {
    RenderConstrainedBox::tight(width, height)
        .with_child(RenderDecoratedBox::new().with_color(color))
}

// -- The sliver list page ------------------------------------------------------

/// The palette a row's band takes its colour from: five hues that cycle every
/// hundred rows, so a screenshot says where in the thousand it was taken.
const SLIVER_BANDS: [Color; 5] = [
    Color::rgb(63, 81, 181),
    Color::rgb(0, 121, 107),
    Color::rgb(183, 28, 28),
    Color::rgb(230, 126, 0),
    Color::rgb(69, 90, 100),
];

/// One row of the sliver list page: a colour band that says which hundred the
/// row is in, and its index set as text.
fn sliver_row(index: usize, body: TextStyle) -> impl RenderBox {
    RenderFlex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(10.0)
        .push(
            RenderConstrainedBox::tight(6.0, 24.0)
                .with_child(RenderDecoratedBox::new().with_color(SLIVER_BANDS[(index / 100) % 5])),
        )
        .push(Text::rich_spans(vec![
            TextSpan::new(format!("Item {index:04}"), body.clone()),
            TextSpan::new(
                "  the window is a screenful, not a thousand",
                TextStyle {
                    color: Color::rgb(120, 120, 120),
                    ..body
                },
            ),
        ]))
}

/// A thousand rows through the sliver protocol, rendered headlessly:
/// `--sliver --png out.png [--scroll 3000]`. The page exists to be looked at
/// as much as measured -- `cargo test` pins the laziness, and the screenshot
/// pins that what is lazily laid out is what lands on the glass, at the top of
/// the list, several screens in, and at the very end.
struct SliverPage {
    scroll: f32,
    body: TextStyle,
}

impl Component for SliverPage {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let body = self.body.clone();
        component(
            rustflutter::scrolling::SliverListView::new(1000, move |index| {
                rustflutter::render::RenderRef::new(sliver_row(index, body.clone()))
            })
            .with_item_extent(40.0)
            // Padding so the page exercises the SliverPadding in front of the
            // SliverList, exactly as upstream's `ListView(padding:)` builds.
            .with_padding(rustflutter::render::EdgeInsets::only(
                16.0, 24.0, 16.0, 24.0,
            ))
            .with_offset(self.scroll),
        )
    }
}

fn render_png_sliver(path: &str, scroll: f32) -> c_int {
    rustflutter::engine::initialize();

    let theme = Theme::light();
    let mut tree = ElementTree::new();
    tree.rebuild(component(SliverPage {
        scroll,
        body: theme.body(),
    }));
    tree.rebuild_dirty();

    let mut root = tree.build_render_tree().expect("the tree has a root");
    root.layout(BoxConstraints::tight(WIDTH as f32, HEIGHT as f32));

    let mut layer_tree = rustflutter::app::compose_frame(
        WIDTH,
        HEIGHT,
        1.0,
        Size::new(WIDTH as f32, HEIGHT as f32),
        theme.background,
        |context| root.paint(context, Offset::ZERO),
    );

    match layer_tree.write_png(std::path::Path::new(path)) {
        Ok(()) => {
            println!("showcase: wrote {path} (sliver list, scrolled to {scroll})");
            0
        }
        Err(err) => {
            eprintln!("showcase: render failed: {err}");
            1
        }
    }
}

// -- Entry point --------------------------------------------------------------

struct ShowcaseApp {
    light: bool,
}

impl WidgetApplication for ShowcaseApp {
    fn background(&self) -> Color {
        if self.light {
            Theme::light().background
        } else {
            Theme::dark().background
        }
    }

    fn build(&mut self, _context: &BuildContext) -> AnyWidget {
        component(SafeArea::new(stateful(Showcase {
            start_light: self.light,
        })))
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rustflutter_app_main(argc: c_int, argv: *const *const c_char) -> c_int {
    let args = collect_args(argc, argv);
    let light = args.iter().any(|a| a == "--light");
    let sliver = args.iter().any(|a| a == "--sliver");
    let scroll = named_string(&args, "--scroll")
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(0.0);

    if let Some(path) = named_string(&args, "--png") {
        return if sliver {
            render_png_sliver(&path, scroll)
        } else {
            render_png(&path, light)
        };
    }

    register_application(move || Box::new(WidgetHost::new(ShowcaseApp { light })));
    let options = RunOptions {
        width: WIDTH,
        height: HEIGHT,
        title: String::from("Components - rustflutter"),
        ..RunOptions::default()
    };
    match run(&options) {
        Ok(()) => 0,
        Err(code) => {
            eprintln!("showcase: the host exited with {code}");
            1
        }
    }
}

fn render_png(path: &str, light: bool) -> c_int {
    rustflutter::engine::initialize();

    let mut tree = ElementTree::new();
    tree.rebuild(stateful(Showcase { start_light: light }));
    // The first pass builds against default state; a second lets the
    // command-line theme settle in.
    tree.rebuild_dirty();

    let mut root = tree.build_render_tree().expect("the tree has a root");
    root.layout(BoxConstraints::tight(WIDTH as f32, HEIGHT as f32));

    let background = if light {
        Theme::light().background
    } else {
        Theme::dark().background
    };
    let mut layer_tree = rustflutter::app::compose_frame(
        WIDTH,
        HEIGHT,
        1.0,
        Size::new(WIDTH as f32, HEIGHT as f32),
        background,
        |context| root.paint(context, Offset::ZERO),
    );

    match layer_tree.write_png(std::path::Path::new(path)) {
        Ok(()) => {
            println!(
                "showcase: wrote {path} ({})",
                if light { "light" } else { "dark" }
            );
            0
        }
        Err(err) => {
            eprintln!("showcase: render failed: {err}");
            1
        }
    }
}

fn named_string(args: &[String], flag: &str) -> Option<String> {
    let index = args.iter().position(|a| a == flag)?;
    args.get(index + 1).cloned()
}

fn collect_args(argc: c_int, argv: *const *const c_char) -> Vec<String> {
    if argv.is_null() || argc <= 1 {
        return Vec::new();
    }
    // SAFETY: the C++ shim passes main()'s argc/argv unchanged, so argv holds
    // at least `argc` NUL-terminated pointers.
    (1..argc as usize)
        .filter_map(|i| unsafe {
            let ptr = *argv.add(i);
            if ptr.is_null() {
                None
            } else {
                std::ffi::CStr::from_ptr(ptr)
                    .to_str()
                    .ok()
                    .map(str::to_string)
            }
        })
        .collect()
}

#[allow(dead_code)]
fn unused_surface() -> impl Widget {
    Align::new(Alignment::CENTER, Empty)
}
