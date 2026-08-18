// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/reference/motion_demo_fade_through_transition.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! Upstream's `FadeThroughTransitionDemo` is a `Scaffold` whose body is a
//! `PageTransitionSwitcher` running `FadeThroughTransition` between three
//! pages -- `_AlbumsPage` (three rows of two `_ExampleCard`s), `_PhotosPage`
//! (two cards) and `_SearchPage` (ten list tiles) -- and whose bottom
//! navigation bar switches the page. The transition is the `animations`
//! package's, reproduced here by [`transitions::fade_through_enter`] and
//! [`transitions::fade_through_exit`] at the switcher's 300ms default.
//!
//! Divergences, each also marked at its site:
//!
//! * The demo is one of six sections stacked on the single `motion` stage
//!   (see `mod.rs`'s header), so its pages are height-bounded
//!   ([`BODY_HEIGHT`]) rather than filling a screen.
//! * The bottom navigation destinations carry one-letter marks instead of
//!   their `Icons.photo_library`/`Icons.photo`/`Icons.search` glyphs: the
//!   framework's `Destination` mark is text in the theme's font, and the
//!   Material Icons glyphs would draw nothing in it.
//! * The cards' `InkWell` splash overlay is not carried: its `onTap` is
//!   empty upstream, so all it could do here is splash.

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::render::{Alignment, CrossAxisAlignment, FlexChild, MainAxisSize, RenderFlex};
use rustflutter::widgets::{Align, ImageView, Opacity, Stack, Transform};

use crate::app::ids;
use crate::l10n::gallery_localizations::GalleryLocalizations;

use super::{screen_column, transitions};

/// The hit-test ids this section's controls take from.
const ID_BASE: u64 = ids::DEMO_LOCAL + 1400;

/// `PageTransitionSwitcher`'s default duration.
const TRANSITION_MICROS: i64 = 300_000;

/// The height the pages stand in at; see the module header.
const BODY_HEIGHT: f32 = 430.0;

/// Upstream's `_ExampleCard` image fill, `Colors.black26`.
const IMAGE_FILL: Color = Color(0x1F00_0000);

/// The `flutter_gallery_assets` placeholder both pages' images are.
const PLACEHOLDER_IMAGE: &[u8] =
    include_bytes!("../../../assets/placeholders/placeholder_image.png");
const AVATAR_LOGO: &[u8] = include_bytes!("../../../assets/placeholders/avatar_logo.png");

/// Which page is showing, upstream's `_pageIndex` into `_pageList`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Albums,
    Photos,
    Search,
}

/// The pages in the bottom bar's order, upstream's `_pageList`.
const PAGES: [Page; 3] = [Page::Albums, Page::Photos, Page::Search];

/// The demo's section: upstream's `FadeThroughTransitionDemo`.
pub(super) fn section() -> AnyWidget {
    stateful(FadeThroughTransitionDemo)
}

struct FadeThroughTransitionDemo;

/// Upstream's `_FadeThroughTransitionDemoState`, plus the outgoing page and
/// the switcher's clock: `PageTransitionSwitcher` keeps the old child around
/// to play the exit under the entrance.
struct FadeThroughDemoState {
    page: Page,
    /// The page leaving, while the transition runs.
    leaving: Option<Page>,
    /// The transition's progress, 0..1 over [`TRANSITION_MICROS`].
    progress: f32,
    running: bool,
    last_frame_micros: Option<i64>,
}

impl Default for FadeThroughDemoState {
    fn default() -> Self {
        FadeThroughDemoState {
            page: Page::Albums,
            leaving: None,
            progress: 0.0,
            running: false,
            last_frame_micros: None,
        }
    }
}

/// A tap on a destination: upstream's `onTap` + `PageTransitionSwitcher`'s
/// reaction to the new child -- the old page starts leaving and the clock
/// restarts. A tap on the current destination is a no-op upstream too.
fn select(state: &mut FadeThroughDemoState, index: usize) {
    let page = PAGES[index];
    if page == state.page && !state.running {
        return;
    }
    state.leaving = Some(state.page);
    state.page = page;
    state.progress = 0.0;
    state.running = true;
}

impl StatefulComponent for FadeThroughTransitionDemo {
    type State = FadeThroughDemoState;

    fn advance(&self, state: &mut FadeThroughDemoState, frame_time_micros: i64) -> bool {
        let elapsed = match state.last_frame_micros.replace(frame_time_micros) {
            Some(previous) => (frame_time_micros - previous).clamp(0, crate::app::MAX_FRAME_MICROS),
            None => 0,
        };
        if !state.running {
            return false;
        }
        state.progress = (state.progress + elapsed as f32 / TRANSITION_MICROS as f32).min(1.0);
        if state.progress >= 1.0 {
            state.running = false;
            state.leaving = None;
        }
        true
    }

    fn build(
        &self,
        state: &FadeThroughDemoState,
        handle: StateHandle<FadeThroughDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let l10n = GalleryLocalizations::en();
        let theme = theme_of(context);
        // The transition's `fillColor` default, `Theme.canvasColor`.
        let canvas = theme.background;

        let app_bar = component(
            AppBar::new(l10n.demo_fade_through_title())
                .with_subtitle(format!("({})", l10n.demo_fade_through_demo_instructions())),
        );

        // The body: the current page, with the leaving page underneath it
        // while the transition runs -- the exiting child is behind the
        // entering one in the switcher's stack too.
        let mut pages: Vec<AnyWidget> = Vec::new();
        if let Some(leaving) = state.leaving {
            let exit = transitions::fade_through_exit(state.progress);
            pages.push(leaf(move || Opacity::new(exit.opacity, page_body(leaving))));
        }
        let current = state.page;
        let progress = state.progress;
        let running = state.running;
        pages.push(leaf(move || {
            // The rest placement is the identity, so the settled page reads
            // the same code as the entering one.
            let enter = if running {
                transitions::fade_through_enter(progress)
            } else {
                transitions::Placement::REST
            };
            Opacity::new(
                enter.opacity,
                Transform::scale(enter.scale, page_body(current)),
            )
        }));
        let body = many(pages, move |rendered| {
            let mut stack = Stack::new();
            for page in rendered {
                stack = stack.push(page);
            }
            Box::new(
                Container::new()
                    .with_height(BODY_HEIGHT)
                    .with_color(canvas)
                    .with_child(stack),
            )
        });

        // Upstream's `bottomNavigationBar`. The marks stand in for the icon
        // glyphs; see the module header.
        let selected = PAGES
            .iter()
            .position(|page| *page == state.page)
            .unwrap_or(0);
        let destinations = vec![
            Destination::new(l10n.demo_fade_through_albums_destination(), "A"),
            Destination::new(l10n.demo_fade_through_photos_destination(), "P"),
            Destination::new(l10n.demo_fade_through_search_destination(), "S"),
        ];
        let bottom_bar = component(
            BottomNavigation::new(ID_BASE, destinations, selected)
                .wired(handle, |state, index| select(state, index)),
        );

        screen_column(vec![app_bar, body, bottom_bar])
    }
}

/// One page's contents, upstream's `_pageList` entries.
fn page_body(page: Page) -> impl rustflutter::render::RenderBox {
    match page {
        Page::Albums => albums_page(),
        Page::Photos => photos_page(),
        Page::Search => search_page(),
    }
}

/// `_AlbumsPage`: three rows of two cards.
fn albums_page() -> RenderFlex {
    let mut column = RenderFlex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    for _ in 0..3 {
        column = column.push_flex(FlexChild::expanded(
            RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .push_flex(FlexChild::expanded(example_card(), 1))
                .push_flex(FlexChild::expanded(example_card(), 1)),
            1,
        ));
    }
    column
}

/// `_PhotosPage`: two cards stacked.
fn photos_page() -> RenderFlex {
    RenderFlex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .push_flex(FlexChild::expanded(example_card(), 1))
        .push_flex(FlexChild::expanded(example_card(), 1))
}

/// `_SearchPage`: ten list tiles with the avatar leading. The stage's body
/// is shorter than the list; the overflow clips, the page being a fixed
/// window here (see the module header) rather than a scrollable.
fn search_page() -> RenderFlex {
    let l10n = GalleryLocalizations::en();
    let mut column = RenderFlex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
    for index in 0..10 {
        let title = format!("{} {}", l10n.demo_motion_list_tile_title(), index + 1);
        let subtitle = l10n.demo_motion_placeholder_subtitle();
        let mut avatar = Container::new().with_size(40.0, 40.0);
        if let Some(image) = Image::shared(AVATAR_CACHE_KEY, AVATAR_LOGO) {
            avatar = avatar.with_child(ImageView::new(image));
        }
        column = column.push(
            Container::new()
                .with_padding(EdgeInsets::symmetric(16.0, 8.0))
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
                                .push(Text::new(title.clone()).with_size(15.0))
                                .push(Text::new(subtitle).with_size(12.0)),
                        ),
                ),
        );
    }
    column
}

/// The avatar's cache key, upstream's asset name.
const AVATAR_CACHE_KEY: &str = "placeholders/avatar_logo.png";

/// `_ExampleCard`: the image area over two lines of "123 photos".
fn example_card() -> impl rustflutter::render::RenderBox {
    let l10n = GalleryLocalizations::en();
    let image = Image::shared(PLACEHOLDER_CACHE_KEY, PLACEHOLDER_IMAGE);
    let mut image_area = Container::new()
        .with_color(IMAGE_FILL)
        .with_padding(EdgeInsets::all(30.0));
    if let Some(image) = image {
        image_area = image_area.with_child(Align::new(Alignment::CENTER, ImageView::new(image)));
    }
    RenderFlex::column()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .push_flex(FlexChild::expanded(image_area, 1))
        .push(
            Container::new()
                .with_padding(EdgeInsets::all(8.0))
                .with_child(
                    Column::new()
                        .with_main_axis_size(MainAxisSize::Min)
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .push(Text::new(l10n.demo_fade_through_text_placeholder()).with_size(15.0))
                        .push(Text::new(l10n.demo_fade_through_text_placeholder()).with_size(12.0)),
                ),
        )
}

/// The placeholder's cache key, upstream's asset name.
const PLACEHOLDER_CACHE_KEY: &str = "placeholders/placeholder_image.png";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tap_on_another_page_starts_the_transition() {
        let mut state = FadeThroughDemoState::default();
        select(&mut state, 2);
        assert_eq!(state.page, Page::Search);
        assert_eq!(state.leaving, Some(Page::Albums));
        assert!(state.running);
        assert_eq!(state.progress, 0.0);
    }

    #[test]
    fn a_tap_on_the_current_page_is_a_no_op() {
        let mut state = FadeThroughDemoState::default();
        select(&mut state, 0);
        assert!(!state.running);
        assert_eq!(state.leaving, None);
    }

    #[test]
    fn the_transition_lands_and_clears_the_leaving_page() {
        let mut state = FadeThroughDemoState::default();
        select(&mut state, 1);
        // The frame clock ticks in ~16ms frames, each clamped to
        // `MAX_FRAME_MICROS`; step 300ms' worth.
        let mut now = 1_000_000;
        FadeThroughTransitionDemo.advance(&mut state, now);
        for _ in 0..20 {
            now += 16_667;
            FadeThroughTransitionDemo.advance(&mut state, now);
        }
        assert_eq!(state.progress, 1.0);
        assert!(!state.running);
        assert_eq!(state.leaving, None);
        // The tick after settling is idle.
        assert!(!FadeThroughTransitionDemo.advance(&mut state, now + 16_667));
    }
}
