// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/reference/motion_demo_container_transition.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! Upstream's `OpenContainerTransformDemo` is a `Navigator` whose home route
//! lists every kind of `OpenContainer` closed child -- the `_DetailsCard`,
//! the `_DetailsListTile`, the two- and three-up `_SmallDetailsCard` rows and
//! ten avatar list tiles, with the FAB floating over them -- each opening
//! `_DetailsPage` through the container transform, in the `fade` or
//! `fadeThrough` mode the settings gear's modal bottom sheet picks. The
//! opacity choreography is the `animations` package's
//! ([`transitions::open_container_open_opacity`] and
//! [`transitions::open_container_closed_opacity`]) at `OpenContainer`'s
//! 300ms.
//!
//! Divergences, each also marked at its site:
//!
//! * The demo is one of six sections stacked on the single `motion` stage
//!   (see `mod.rs`'s header), so its route is a state of the section, its
//!   list scrolls inside a bounded window ([`BODY_HEIGHT`]), and the details
//!   page's back affordance is an explicit arrow (the framework's `AppBar`
//!   has no implied leading).
//! * The container's rect-to-rect morph is approximated by the opacity
//!   choreography alone: `OpenContainer` measures the closed child's bounds
//!   and animates the container between the two rects, and a build here has
//!   no geometry to measure. The details page covers the section's body
//!   area while the package's opacity tweens play.
//! * The `ToggleButtons` in the settings sheet are two `Button`s, filled for
//!   the selected mode and outlined for the other -- the framework widget
//!   gap the material demos document (PORTING.md, M-D).
//! * The placeholder artwork (`placeholders/placeholder_image.png`,
//!   `placeholders/avatar_logo.png`) ships in `assets/placeholders/`, copied
//!   from `flutter_gallery_assets` (the assets/README.md convention).

use rustflutter::framework::BuildContext;
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, CrossAxisAlignment, FlexChild, MainAxisSize, RenderFlex, StackPosition,
};
use rustflutter::widgets::{Align, Empty, ImageView, Opacity, Pointer, Positioned, Stack};

use crate::app::ids;
use crate::data::demos::{icon, MATERIAL_ICONS};
use crate::l10n::gallery_localizations::GalleryLocalizations;
use crate::themes::material_demo_theme_data::COLOR_SCHEME;

use super::{screen_column, transitions};

/// The hit-test ids this section's controls take from.
const ID_BASE: u64 = ids::DEMO_LOCAL + 1000;

/// `OpenContainer`'s default `transitionDuration`.
const TRANSITION_MICROS: i64 = 300_000;

/// The height the closed list and the details page stand in at; see the
/// module header.
const BODY_HEIGHT: f32 = 560.0;

/// Upstream's `_fabDimension`.
const FAB_DIMENSION: f32 = 56.0;

/// The image fills and text greys upstream states: `Colors.black38` for the
/// image areas, `Colors.black54` for the details text.
const IMAGE_FILL: Color = Color(0x6100_0000);
const DETAILS_TEXT: Color = Color(0x8A00_0000);
/// The settings sheet's barrier, `showModalBottomSheet`'s default
/// `Colors.black54` scrim.
const BARRIER_COLOR: Color = Color(0x8A00_0000);

/// The placeholder artwork (see the module header).
const PLACEHOLDER_IMAGE: &[u8] =
    include_bytes!("../../../assets/placeholders/placeholder_image.png");
const AVATAR_LOGO: &[u8] = include_bytes!("../../../assets/placeholders/avatar_logo.png");
const PLACEHOLDER_CACHE_KEY: &str = "placeholders/placeholder_image.png";
const AVATAR_CACHE_KEY: &str = "placeholders/avatar_logo.png";

/// Upstream's `_loremIpsumParagraph`.
const LOREM_IPSUM_PARAGRAPH: &str =
    "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod \
     tempor incididunt ut labore et dolore magna aliqua. Vulputate dignissim \
     suspendisse in est. Ut ornare lectus sit amet. Eget nunc lobortis mattis \
     aliquam faucibus purus in. Hendrerit gravida rutrum quisque non tellus \
     orci ac auctor. Mattis aliquam faucibus purus in massa. Tellus rutrum \
     tellus pellentesque eu tincidunt tortor. Nunc eget lorem dolor sed. Nulla \
     at volutpat diam ut venenatis tellus in metus. Tellus cras adipiscing enim \
     eu turpis. Pretium fusce id velit ut tortor. Adipiscing enim eu turpis \
     egestas pretium. Quis varius quam quisque id. Blandit aliquam etiam erat \
     velit scelerisque. In nisl nisi scelerisque eu. Semper risus in hendrerit \
     gravida rutrum quisque. Suspendisse in est ante in nibh mauris cursus \
     mattis molestie. Adipiscing elit duis tristique sollicitudin nibh sit \
     amet commodo nulla. Pretium viverra suspendisse potenti nullam ac tortor \
     vitae.\n\
     \n\
     Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod \
     tempor incididunt ut labore et dolore magna aliqua. Vulputate dignissim \
     suspendisse in est. Ut ornare lectus sit amet. Eget nunc lobortis mattis \
     aliquam faucibus purus in. Hendrerit gravida rutrum quisque non tellus \
     orci ac auctor. Mattis aliquam faucibus purus in massa. Tellus rutrum \
     tellus pellentesque eu tincidunt tortor. Nunc eget lorem dolor sed. Nulla \
     at volutpat diam ut venenatis tellus in metus. Tellus cras adipiscing enim \
     eu turpis. Pretium fusce id velit ut tortor. Adipiscing enim eu turpis \
     egestas pretium. Quis varius quam quisque id. Blandit aliquam etiam erat \
     velit scelerisque. In nisl nisi scelerisque eu. Semper risus in hendrerit \
     gravida rutrum quisque. Suspendisse in est ante in nibh mauris cursus \
     mattis molestie. Adipiscing elit duis tristique sollicitudin nibh sit \
     amet commodo nulla. Pretium viverra suspendisse potenti nullam ac tortor \
     vitae";

/// Upstream's `ContainerTransitionType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransitionKind {
    Fade,
    FadeThrough,
}

/// The demo's section: upstream's `OpenContainerTransformDemo`.
pub(super) fn section() -> AnyWidget {
    stateful(OpenContainerTransformDemo)
}

struct OpenContainerTransformDemo;

/// Upstream's `_OpenContainerTransformDemoState`, plus the open route and
/// the transition's clock (upstream's `Navigator`'s and route's).
struct ContainerDemoState {
    /// Upstream's `_transitionType`.
    kind: TransitionKind,
    /// Whether `_DetailsPage` is pushed, and how far through the morph the
    /// route is.
    open: bool,
    progress: f32,
    running: bool,
    /// Whether the fade-mode sheet is up.
    sheet_open: bool,
    /// The closed list's and the details page's scroll positions.
    closed_scroll: Scroll,
    details_scroll: Scroll,
    last_frame_micros: Option<i64>,
    pressed: Option<u64>,
}

impl Default for ContainerDemoState {
    fn default() -> Self {
        ContainerDemoState {
            // Upstream's `_transitionType = ContainerTransitionType.fade`.
            kind: TransitionKind::Fade,
            open: false,
            progress: 0.0,
            running: false,
            sheet_open: false,
            closed_scroll: Scroll::default(),
            details_scroll: Scroll::default(),
            last_frame_micros: None,
            pressed: None,
        }
    }
}

/// Any closed child's tap: upstream's `openContainer` -- the route goes on,
/// the morph runs forward.
fn open_details(state: &mut ContainerDemoState) {
    state.open = true;
    state.progress = 0.0;
    state.running = true;
}

/// The details page's back arrow: the route pops, the morph reverses.
fn close_details(state: &mut ContainerDemoState) {
    state.open = false;
    state.progress = 0.0;
    state.running = true;
}

/// The sheet's gear.
fn open_sheet(state: &mut ContainerDemoState) {
    state.sheet_open = true;
}

/// The barrier's dismiss, upstream's modal bottom sheet default.
fn close_sheet(state: &mut ContainerDemoState) {
    state.sheet_open = false;
}

/// The sheet's `ToggleButtons.onPressed`: the picked mode, with the sheet
/// staying up the way upstream's `StatefulBuilder` leaves it.
fn pick_fade(state: &mut ContainerDemoState) {
    state.kind = TransitionKind::Fade;
}
fn pick_fade_through(state: &mut ContainerDemoState) {
    state.kind = TransitionKind::FadeThrough;
}

impl StatefulComponent for OpenContainerTransformDemo {
    type State = ContainerDemoState;

    fn advance(&self, state: &mut ContainerDemoState, frame_time_micros: i64) -> bool {
        let elapsed = match state.last_frame_micros.replace(frame_time_micros) {
            Some(previous) => (frame_time_micros - previous).clamp(0, crate::app::MAX_FRAME_MICROS),
            None => 0,
        };
        let mut active = false;
        if state.running {
            state.progress = (state.progress + elapsed as f32 / TRANSITION_MICROS as f32).min(1.0);
            if state.progress >= 1.0 {
                state.running = false;
            }
            active = true;
        }
        active |= state.closed_scroll.advance(frame_time_micros);
        active |= state.details_scroll.advance(frame_time_micros);
        active
    }

    fn build(
        &self,
        state: &ContainerDemoState,
        handle: StateHandle<ContainerDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let l10n = GalleryLocalizations::en();
        let theme = theme_of(context);
        let surface = theme.surface;
        let canvas = theme.background;
        let on_surface = theme.text;

        // The home route's app bar: the two-line title and the settings gear.
        let gear = icon_button(ID_BASE, icon::SETTINGS, DETAILS_TEXT, &handle, open_sheet);
        let app_bar = component(
            AppBar::new(l10n.demo_container_transform_title())
                .with_subtitle(format!(
                    "({})",
                    l10n.demo_container_transform_demo_instructions()
                ))
                .with_trailing(gear),
        );

        // The body: the closed list with the FAB over it, and the details
        // page over that while the morph is mid-flight or complete. The
        // package's opacity tweens play on both; the morph itself is
        // approximated (see the module header). For the fade type the closed
        // content stays at full opacity underneath; the details page's own
        // fill is what covers it.
        let fade_through = state.kind == TransitionKind::FadeThrough;
        let mut layers: Vec<AnyWidget> = vec![closed_list(state, &handle), fab(&handle)];
        if state.open || state.running {
            layers.push(details_page(state, &handle, surface, canvas, on_surface));
        }
        let progress = state.progress;
        let body = many(layers, move |mut rendered| {
            let details = if rendered.len() > 2 {
                rendered.pop()
            } else {
                None
            };
            let fab = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let closed = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let mut stack = Stack::new()
                .push_positioned(closed, Positioned::fill())
                .push_positioned(
                    fab,
                    StackPosition {
                        right: Some(16.0),
                        bottom: Some(16.0),
                        ..Default::default()
                    },
                );
            if let Some(details) = details {
                let open_opacity = transitions::open_container_open_opacity(progress, fade_through);
                stack =
                    stack.push_positioned(Opacity::new(open_opacity, details), Positioned::fill());
            }
            Box::new(
                Container::new()
                    .with_height(BODY_HEIGHT)
                    .with_color(canvas)
                    .with_child(stack),
            )
        });

        let screen = screen_column(vec![app_bar, body]);

        // The fade-mode sheet over the section: barrier, then the panel
        // anchored bottom -- upstream's `showModalBottomSheet`.
        if !state.sheet_open {
            return screen;
        }
        let barrier = leaf({
            let handle = handle.clone();
            move || {
                let barrier_handle = handle.clone();
                Pointer::new(ID_BASE + 90, Container::new().with_color(BARRIER_COLOR))
                    .with_handlers(PointerHandlers::new().with_tap(move |_| {
                        barrier_handle.set_state(close_sheet);
                    }))
            }
        });
        let sheet = fade_mode_sheet(state, &handle, surface, on_surface);
        many(vec![screen, barrier, sheet], move |mut rendered| {
            let sheet = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let barrier = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let screen = rendered.pop().unwrap_or_else(|| boxed(Empty));
            Box::new(
                Stack::new()
                    .push(screen)
                    .push_positioned(barrier, Positioned::fill())
                    .push_positioned(
                        sheet,
                        StackPosition {
                            left: Some(0.0),
                            right: Some(0.0),
                            bottom: Some(0.0),
                            ..Default::default()
                        },
                    ),
            )
        })
    }
}

/// An icon button's target: the glyph centered in a padded, tappable box.
fn icon_button(
    id: u64,
    glyph: &'static str,
    color: Color,
    handle: &StateHandle<ContainerDemoState>,
    action: fn(&mut ContainerDemoState),
) -> AnyWidget {
    let handle = handle.clone();
    leaf(move || {
        let tap_handle = handle.clone();
        Pointer::new(
            id,
            Container::new()
                .with_padding(EdgeInsets::all(12.0))
                .with_child(
                    Text::new(glyph)
                        .with_font_family(MATERIAL_ICONS)
                        .with_size(24.0)
                        .with_color(color),
                ),
        )
        .with_handlers(PointerHandlers::new().with_tap(move |_| {
            tap_handle.set_state(action);
        }))
    })
}

/// The home route's body: upstream's `ListView` of closed containers.
fn closed_list(state: &ContainerDemoState, handle: &StateHandle<ContainerDemoState>) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let offset = state.closed_scroll.offset;
    let extent = state.closed_scroll.extent.clone();

    let mut children: Vec<AnyWidget> = Vec::new();
    // `_DetailsCard`.
    children.push(leaf({
        let handle = handle.clone();
        move || details_card(ID_BASE + 10, handle.clone())
    }));
    children.push(leaf(|| Container::new().with_height(16.0)));
    // `_DetailsListTile`.
    children.push(leaf({
        let handle = handle.clone();
        move || details_list_tile(ID_BASE + 11, handle.clone())
    }));
    children.push(leaf(|| Container::new().with_height(16.0)));
    // The two-up row of `_SmallDetailsCard`s, "Secondary text".
    children.push(small_card_row(
        &[ID_BASE + 12, ID_BASE + 13],
        l10n.demo_motion_placeholder_subtitle(),
        handle,
    ));
    children.push(leaf(|| Container::new().with_height(16.0)));
    // The three-up row, "Secondary".
    children.push(small_card_row(
        &[ID_BASE + 14, ID_BASE + 15, ID_BASE + 16],
        l10n.demo_motion_small_placeholder_subtitle(),
        handle,
    ));
    children.push(leaf(|| Container::new().with_height(16.0)));
    // The ten avatar list tiles.
    for index in 0..10 {
        children.push(list_tile(ID_BASE + 20 + index as u64, index, handle));
    }

    // The list's drag and wheel, against the state's `Scroll`.
    let down_handle = handle.clone();
    let drag_handle = handle.clone();
    let end_handle = handle.clone();
    let wheel_handle = handle.clone();
    let handlers = PointerHandlers::new()
        .with_pointer_down(move |_| {
            down_handle.set_state(|s| s.closed_scroll.stop());
        })
        .with_drag_update(move |drag| {
            let delta = drag.delta.dy;
            drag_handle.set_state(move |s| s.closed_scroll.scroll_by(-delta));
        })
        .with_drag_end(move |end| {
            let velocity = end.velocity.dy;
            end_handle.set_state(move |s| s.closed_scroll.fling(-velocity));
        })
        .with_scroll(move |scroll| {
            let delta = scroll.delta.dy;
            wheel_handle.set_state(move |s| s.closed_scroll.scroll_by(delta));
        });

    many(children, move |rendered| {
        let mut column = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        for child in rendered {
            column = column.push(child);
        }
        let list = rustflutter::widgets::ListView::new()
            .with_offset(offset)
            .with_extent_sink(extent.clone())
            .push(column);
        Box::new(
            Container::new()
                .with_padding(EdgeInsets::all(8.0))
                .with_child(Pointer::new(ID_BASE + 9, list).with_handlers(handlers.clone())),
        )
    })
}

/// The FAB: upstream's `OpenContainer` closed as the 56 circle in the
/// scheme's secondary with the add glyph, elevation 6. Tapping it opens
/// `_DetailsPage`.
fn fab(handle: &StateHandle<ContainerDemoState>) -> AnyWidget {
    let handle = handle.clone();
    leaf(move || {
        let tap_handle = handle.clone();
        Pointer::new(
            ID_BASE + 40,
            Container::new()
                .with_size(FAB_DIMENSION, FAB_DIMENSION)
                .with_color(COLOR_SCHEME.secondary)
                .with_corner_radius(FAB_DIMENSION / 2.0)
                .with_elevation(6)
                .with_child(Align::new(
                    Alignment::CENTER,
                    Text::new(icon::ADD)
                        .with_font_family(MATERIAL_ICONS)
                        .with_size(24.0)
                        .with_color(COLOR_SCHEME.on_secondary),
                )),
        )
        .with_handlers(PointerHandlers::new().with_tap(move |_| {
            tap_handle.set_state(open_details);
        }))
    })
}

/// `_InkWellOverlay`: the fixed-height tappable wrapper every closed card
/// wears.
fn tappable_card(
    id: u64,
    height: f32,
    handle: StateHandle<ContainerDemoState>,
    child: impl rustflutter::render::RenderBox + 'static,
) -> impl rustflutter::render::RenderBox {
    Pointer::new(id, Container::new().with_height(height).with_child(child)).with_handlers(
        PointerHandlers::new().with_tap(move |_| {
            handle.set_state(open_details);
        }),
    )
}

/// The image area every closed card shares: `Colors.black38` with the
/// placeholder centered at `width`.
fn image_area(width: f32) -> impl rustflutter::render::RenderBox {
    let mut area = Container::new().with_color(IMAGE_FILL);
    if let Some(image) = Image::shared(PLACEHOLDER_CACHE_KEY, PLACEHOLDER_IMAGE) {
        area = area.with_child(Align::new(
            Alignment::CENTER,
            rustflutter::widgets::SizedBox::width(width).with_child(ImageView::new(image)),
        ));
    }
    area
}

/// `_DetailsCard`: image area, title/subtitle tile, lorem snippet; 300 tall.
fn details_card(
    id: u64,
    handle: StateHandle<ContainerDemoState>,
) -> impl rustflutter::render::RenderBox {
    let l10n = GalleryLocalizations::en();
    tappable_card(
        id,
        300.0,
        handle,
        RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .push_flex(FlexChild::expanded(image_area(100.0), 1))
            .push(
                Container::new()
                    .with_padding(EdgeInsets::all(16.0))
                    .with_child(
                        Column::new()
                            .with_main_axis_size(MainAxisSize::Min)
                            .with_cross_axis_alignment(CrossAxisAlignment::Start)
                            .with_spacing(4.0)
                            .push(Text::new(l10n.demo_motion_placeholder_title()).with_size(15.0))
                            .push(
                                Text::new(l10n.demo_motion_placeholder_subtitle()).with_size(12.0),
                            ),
                    ),
            )
            .push(
                Container::new()
                    .with_padding(EdgeInsets::only(16.0, 0.0, 16.0, 16.0))
                    .with_child(
                        Text::new(
                            "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do \
                             eiusmod tempor.",
                        )
                        .with_style(TextStyle {
                            font_size: 14.0,
                            color: DETAILS_TEXT,
                            ..TextStyle::default()
                        }),
                    ),
            ),
    )
}

/// `_SmallDetailsCard`: a 150 image area over the padded title/subtitle; 225
/// tall. One row's worth comes from [`small_card_row`].
fn small_details_card(
    id: u64,
    subtitle: &'static str,
    handle: &StateHandle<ContainerDemoState>,
) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let handle = handle.clone();
    leaf(move || {
        tappable_card(
            id,
            225.0,
            handle.clone(),
            RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .push(
                    Container::new()
                        .with_height(150.0)
                        .with_child(image_area(80.0)),
                )
                .push_flex(FlexChild::expanded(
                    Container::new()
                        .with_padding(EdgeInsets::all(10.0))
                        .with_child(
                            Column::new()
                                .with_main_axis_size(MainAxisSize::Min)
                                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                                .with_spacing(4.0)
                                .push(
                                    Text::new(l10n.demo_motion_placeholder_title()).with_size(20.0),
                                )
                                .push(Text::new(subtitle).with_size(12.0)),
                        ),
                    1,
                )),
        )
    })
}

/// One of the two `Row`s of `_SmallDetailsCard`s, `Expanded` each with 8
/// between.
fn small_card_row(
    ids: &[u64],
    subtitle: &'static str,
    handle: &StateHandle<ContainerDemoState>,
) -> AnyWidget {
    let cards: Vec<AnyWidget> = ids
        .iter()
        .map(|&id| small_details_card(id, subtitle, handle))
        .collect();
    many(cards, move |rendered| {
        let mut row = RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(8.0);
        for card in rendered {
            row = row.push_flex(FlexChild::expanded(card, 1));
        }
        Box::new(row)
    })
}

/// `_DetailsListTile`: the square image area beside the padded
/// title/snippet; 120 tall.
fn details_list_tile(
    id: u64,
    handle: StateHandle<ContainerDemoState>,
) -> impl rustflutter::render::RenderBox {
    let l10n = GalleryLocalizations::en();
    tappable_card(
        id,
        120.0,
        handle,
        RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .push(
                Container::new()
                    .with_size(120.0, 120.0)
                    .with_child(image_area(60.0)),
            )
            .push_flex(FlexChild::expanded(
                Container::new()
                    .with_padding(EdgeInsets::all(20.0))
                    .with_child(
                        Column::new()
                            .with_main_axis_size(MainAxisSize::Min)
                            .with_cross_axis_alignment(CrossAxisAlignment::Start)
                            .with_spacing(8.0)
                            .push(Text::new(l10n.demo_motion_placeholder_title()).with_size(15.0))
                            .push(
                                Text::new(
                                    "Lorem ipsum dolor sit amet, consectetur adipiscing elit,",
                                )
                                .with_size(12.0),
                            ),
                    ),
                1,
            )),
    )
}

/// One of the ten `OpenContainer` list tiles: the 40 avatar, "List item N",
/// "Secondary text".
fn list_tile(id: u64, index: usize, handle: &StateHandle<ContainerDemoState>) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let handle = handle.clone();
    leaf(move || {
        let tap_handle = handle.clone();
        let mut avatar = Container::new().with_size(40.0, 40.0);
        if let Some(image) = Image::shared(AVATAR_CACHE_KEY, AVATAR_LOGO) {
            avatar = avatar.with_child(ImageView::new(image));
        }
        Pointer::new(
            id,
            Container::new()
                .with_padding(EdgeInsets::symmetric(8.0, 8.0))
                .with_child(
                    RenderFlex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_spacing(16.0)
                        .push(avatar)
                        .push(
                            Column::new()
                                .with_main_axis_size(MainAxisSize::Min)
                                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                                .with_spacing(2.0)
                                .push(
                                    Text::new(format!(
                                        "{} {}",
                                        l10n.demo_motion_list_tile_title(),
                                        index + 1
                                    ))
                                    .with_size(15.0),
                                )
                                .push(
                                    Text::new(l10n.demo_motion_placeholder_subtitle())
                                        .with_size(12.0),
                                ),
                        ),
                ),
        )
        .with_handlers(PointerHandlers::new().with_tap(move |_| {
            tap_handle.set_state(open_details);
        }))
    })
}

/// `_DetailsPage`: the titled app bar (with the back arrow; see the module
/// header) over the 250 image area, the 30pt title and the lorem paragraphs,
/// scrolling inside the window.
fn details_page(
    state: &ContainerDemoState,
    handle: &StateHandle<ContainerDemoState>,
    surface: Color,
    canvas: Color,
    on_surface: Color,
) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let back = icon_button(
        ID_BASE + 50,
        icon::ARROW_BACK,
        DETAILS_TEXT,
        handle,
        close_details,
    );
    let offset = state.details_scroll.offset;
    let extent = state.details_scroll.extent.clone();

    let down_handle = handle.clone();
    let drag_handle = handle.clone();
    let end_handle = handle.clone();
    let wheel_handle = handle.clone();
    let handlers = PointerHandlers::new()
        .with_pointer_down(move |_| {
            down_handle.set_state(|s| s.details_scroll.stop());
        })
        .with_drag_update(move |drag| {
            let delta = drag.delta.dy;
            drag_handle.set_state(move |s| s.details_scroll.scroll_by(-delta));
        })
        .with_drag_end(move |end| {
            let velocity = end.velocity.dy;
            end_handle.set_state(move |s| s.details_scroll.fling(-velocity));
        })
        .with_scroll(move |scroll| {
            let delta = scroll.delta.dy;
            wheel_handle.set_state(move |s| s.details_scroll.scroll_by(delta));
        });

    // The route's fill: the details page is opaque over the closed list, so
    // for the fade type the closed content staying at full opacity
    // underneath is invisible anyway (see the module header).
    many(vec![back], move |rendered| {
        let back = rendered.into_iter().next().unwrap_or_else(|| boxed(Empty));
        let title_bar = Container::new().with_color(surface).with_child(
            RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(4.0)
                .push(back)
                .push(
                    Text::new(l10n.demo_motion_details_page_title())
                        .with_size(20.0)
                        .with_color(on_surface),
                ),
        );
        let mut hero = Container::new().with_height(250.0).with_color(IMAGE_FILL);
        if let Some(image) = Image::shared(PLACEHOLDER_CACHE_KEY, PLACEHOLDER_IMAGE) {
            hero = hero.with_child(Align::new(
                Alignment::CENTER,
                Container::new()
                    .with_padding(EdgeInsets::all(70.0))
                    .with_child(ImageView::new(image)),
            ));
        }
        let list = rustflutter::widgets::ListView::new()
            .with_offset(offset)
            .with_extent_sink(extent.clone())
            .push(hero)
            .push(
                Container::new()
                    .with_padding(EdgeInsets::all(20.0))
                    .with_child(
                        Column::new()
                            .with_main_axis_size(MainAxisSize::Min)
                            .with_cross_axis_alignment(CrossAxisAlignment::Start)
                            .with_spacing(10.0)
                            .push(Text::new(l10n.demo_motion_placeholder_title()).with_style(
                                TextStyle {
                                    font_size: 30.0,
                                    color: DETAILS_TEXT,
                                    ..TextStyle::default()
                                },
                            ))
                            .push(Text::new(LOREM_IPSUM_PARAGRAPH).with_style(TextStyle {
                                font_size: 16.0,
                                color: DETAILS_TEXT,
                                height: Some(1.5),
                                ..TextStyle::default()
                            })),
                    ),
            );
        Box::new(
            Container::new().with_color(canvas).with_child(
                Column::new()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .push(title_bar)
                    .push_flex(FlexChild::expanded(
                        Pointer::new(ID_BASE + 51, list).with_handlers(handlers.clone()),
                        1,
                    )),
            ),
        )
    })
}

/// The settings sheet: upstream's modal bottom sheet -- the "Fade mode"
/// caption and the two modes as filled/outlined buttons (see the module
/// header for the `ToggleButtons` note).
fn fade_mode_sheet(
    state: &ContainerDemoState,
    handle: &StateHandle<ContainerDemoState>,
    surface: Color,
    on_surface: Color,
) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let fade = component(
        Button::new(ID_BASE + 91, l10n.demo_container_transform_type_fade())
            .with_style(if state.kind == TransitionKind::Fade {
                ButtonStyle::Filled
            } else {
                ButtonStyle::Outlined
            })
            .with_pressed(state.pressed == Some(ID_BASE + 91))
            .wired(handle.clone(), |s| &mut s.pressed, pick_fade),
    );
    let fade_through = component(
        Button::new(
            ID_BASE + 92,
            l10n.demo_container_transform_type_fade_through(),
        )
        .with_style(if state.kind == TransitionKind::FadeThrough {
            ButtonStyle::Filled
        } else {
            ButtonStyle::Outlined
        })
        .with_pressed(state.pressed == Some(ID_BASE + 92))
        .wired(handle.clone(), |s| &mut s.pressed, pick_fade_through),
    );
    many(vec![fade, fade_through], move |rendered| {
        let mut modes = RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(8.0);
        for mode in rendered {
            modes = modes.push(mode);
        }
        Box::new(
            Container::new()
                .with_height(125.0)
                .with_color(surface)
                .with_padding(EdgeInsets::all(15.0))
                .with_child(
                    Column::new()
                        .with_main_axis_size(MainAxisSize::Min)
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(12.0)
                        .push(
                            Text::new(l10n.demo_container_transform_modal_bottom_sheet_title())
                                .with_style(TextStyle {
                                    font_size: 12.0,
                                    color: on_surface,
                                    ..TextStyle::default()
                                }),
                        )
                        .push(modes),
                ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mode_starts_at_fade_as_upstream_does() {
        assert_eq!(ContainerDemoState::default().kind, TransitionKind::Fade);
    }

    #[test]
    fn opening_and_closing_run_the_morph_both_ways() {
        let mut state = ContainerDemoState::default();
        open_details(&mut state);
        assert!(state.open && state.running && state.progress == 0.0);
        close_details(&mut state);
        assert!(!state.open && state.running, "the pop animates back out");
    }

    #[test]
    fn the_sheet_picks_the_mode_and_stays_up() {
        let mut state = ContainerDemoState::default();
        open_sheet(&mut state);
        assert!(state.sheet_open);
        pick_fade_through(&mut state);
        assert_eq!(state.kind, TransitionKind::FadeThrough);
        assert!(state.sheet_open, "upstream's StatefulBuilder leaves it up");
        close_sheet(&mut state);
        assert!(!state.sheet_open);
    }

    #[test]
    fn the_lorem_is_upstream_two_paragraphs() {
        // The doubled paragraph with the blank line between, as upstream's
        // `_loremIpsumParagraph` concatenates it.
        assert_eq!(LOREM_IPSUM_PARAGRAPH.matches("\n\n").count(), 1);
        assert!(LOREM_IPSUM_PARAGRAPH.len() > 1000);
    }
}
