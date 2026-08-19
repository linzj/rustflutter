// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/reference/motion_demo_shared_x_axis_transition.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! Upstream's `SharedXAxisTransitionDemo` is a sign-in page and a course
//! page traded through a `PageTransitionSwitcher` running a horizontal
//! `SharedAxisTransition` (300ms): NEXT signs in, BACK signs out, and the
//! switcher's `reverse` mirrors the slide. The sign-in page is the avatar,
//! "Hi David Park", the email field and the FORGOT EMAIL?/CREATE ACCOUNT
//! text buttons; the course page is "Streamline your courses" over five
//! `_CourseSwitch`es. The transition is reproduced here by
//! [`transitions::shared_axis_enter`] and [`transitions::shared_axis_exit`].
//!
//! Divergences, each also marked at its site:
//!
//! * The demo is one of six sections stacked on the single `motion` stage
//!   (see `mod.rs`'s header), so its pages are height-bounded
//!   ([`BODY_HEIGHT`]) rather than filling a screen, and the pages' own
//!   scrollables are not carried -- both pages fit their window.
//! * The email `TextField`'s `InputDecoration` (label and suffix icon)
//!   becomes a caption above the field, as in every material text-field port
//!   (PORTING.md, M-D: labels become caption-above). The visibility suffix
//!   icon is omitted with it.
//! * FORGOT EMAIL? and CREATE ACCOUNT are unwired: their `onPressed`s are
//!   empty upstream.
//! * The `_CourseSwitch` row is a title/subtitle with a trailing `Switch`
//!   rather than upstream's `SwitchListTile`, the framework's `ListTile`
//!   having no control slot.

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, CrossAxisAlignment, FlexChild, MainAxisAlignment, MainAxisSize, RenderFlex,
};
use rustflutter::widgets::{Align, Empty, FullWidth, ImageView, Opacity, Stack, Transform};

use crate::app::ids;
use crate::l10n::gallery_localizations::GalleryLocalizations;

use super::{
    screen_column,
    transitions::{self, SharedAxis},
};

/// The hit-test ids this section's controls take from. The leaving page's
/// copies are shifted by [`LEAVING_SHIFT`] so the two pages' controls never
/// share an id while a transition shows both.
const ID_BASE: u64 = ids::DEMO_LOCAL + 1100;
const LEAVING_SHIFT: u64 = 60;

/// The switcher's duration, upstream's explicit
/// `PageTransitionSwitcher(duration: 300ms)`.
const TRANSITION_MICROS: i64 = 300_000;

/// The height the pages stand in at; see the module header.
const BODY_HEIGHT: f32 = 430.0;

/// The 12pt grey of both pages' subtitles, upstream's
/// `TextStyle(fontSize: 12, color: Colors.grey)`.
const GREY: Color = Color(0xFF9E9E9E);

/// The sign-in page's avatar, upstream's `placeholders/avatar_logo.png`.
const AVATAR_LOGO: &[u8] = include_bytes!("../../../assets/placeholders/avatar_logo.png");
const AVATAR_CACHE_KEY: &str = "placeholders/avatar_logo.png";

/// The demo's section: upstream's `SharedXAxisTransitionDemo`.
pub(super) fn section() -> AnyWidget {
    stateful(SharedXAxisTransitionDemo)
}

struct SharedXAxisTransitionDemo;

/// Upstream's `_SharedXAxisTransitionDemoState`, plus the switcher's clock
/// and the course switches' values (upstream's per-row
/// `_CourseSwitchState._isCourseBundled`).
struct SharedXAxisDemoState {
    /// Upstream's `_isLoggedIn`.
    logged_in: bool,
    /// Which way the current transition runs: forward into the course page,
    /// backward back to sign-in (the switcher's `reverse: !_isLoggedIn`).
    forward: bool,
    progress: f32,
    running: bool,
    /// The five course switches, upstream's `_isCourseBundled = true` each.
    bundled: [bool; 5],
    /// The email field's text.
    email: String,
    last_frame_micros: Option<i64>,
    pressed: Option<u64>,
}

impl Default for SharedXAxisDemoState {
    fn default() -> Self {
        SharedXAxisDemoState {
            logged_in: false,
            forward: true,
            progress: 0.0,
            running: false,
            bundled: [true; 5],
            email: String::new(),
            last_frame_micros: None,
            pressed: None,
        }
    }
}

/// Upstream's `_toggleLoginStatus`, with the switcher's reaction to the new
/// child: the clock restarts, forwards on NEXT, backwards on BACK.
fn toggle_login(state: &mut SharedXAxisDemoState) {
    state.logged_in = !state.logged_in;
    state.forward = state.logged_in;
    state.progress = 0.0;
    state.running = true;
}

/// One course switch's toggle. `Switch::wired` takes a plain `fn`, so each
/// row's is its own.
fn toggle_course(state: &mut SharedXAxisDemoState, index: usize) {
    state.bundled[index] = !state.bundled[index];
}
fn toggle_course_0(state: &mut SharedXAxisDemoState) {
    toggle_course(state, 0);
}
fn toggle_course_1(state: &mut SharedXAxisDemoState) {
    toggle_course(state, 1);
}
fn toggle_course_2(state: &mut SharedXAxisDemoState) {
    toggle_course(state, 2);
}
fn toggle_course_3(state: &mut SharedXAxisDemoState) {
    toggle_course(state, 3);
}
fn toggle_course_4(state: &mut SharedXAxisDemoState) {
    toggle_course(state, 4);
}

/// The toggles in row order, upstream's course order.
const COURSE_TOGGLES: [fn(&mut SharedXAxisDemoState); 5] = [
    toggle_course_0,
    toggle_course_1,
    toggle_course_2,
    toggle_course_3,
    toggle_course_4,
];

impl StatefulComponent for SharedXAxisTransitionDemo {
    type State = SharedXAxisDemoState;

    fn advance(&self, state: &mut SharedXAxisDemoState, frame_time_micros: i64) -> bool {
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
        }
        true
    }

    fn build(
        &self,
        state: &SharedXAxisDemoState,
        handle: StateHandle<SharedXAxisDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let l10n = GalleryLocalizations::en();
        let theme = theme_of(context);
        let canvas = theme.background;

        let app_bar = component(
            AppBar::new(l10n.demo_shared_x_axis_title())
                .with_subtitle(format!("({})", l10n.demo_shared_x_axis_demo_instructions())),
        );

        // The body: the arriving page over the leaving one while the
        // transition runs. `reverse` mirrors the pattern; see `toggle_login`.
        let reverse = !state.forward;
        let body = if state.running {
            let enter =
                transitions::shared_axis_enter(state.progress, SharedAxis::Horizontal, reverse);
            let exit =
                transitions::shared_axis_exit(state.progress, SharedAxis::Horizontal, reverse);
            let arriving = if state.logged_in {
                course_page(state, &handle, ID_BASE)
            } else {
                sign_in_page(&handle, context, ID_BASE)
            };
            let leaving = if state.logged_in {
                sign_in_page(&handle, context, ID_BASE + LEAVING_SHIFT)
            } else {
                course_page(state, &handle, ID_BASE + LEAVING_SHIFT)
            };
            many(vec![leaving, arriving], move |mut rendered| {
                let arriving = rendered.pop().unwrap_or_else(|| boxed(Empty));
                let leaving = rendered.pop().unwrap_or_else(|| boxed(Empty));
                Box::new(
                    Stack::new()
                        .push(Opacity::new(
                            exit.opacity,
                            Transform::matrix([1.0, 0.0, 0.0, 1.0, exit.dx, 0.0], leaving),
                        ))
                        .push(Opacity::new(
                            enter.opacity,
                            Transform::matrix([1.0, 0.0, 0.0, 1.0, enter.dx, 0.0], arriving),
                        )),
                )
            })
        } else if state.logged_in {
            course_page(state, &handle, ID_BASE)
        } else {
            sign_in_page(&handle, context, ID_BASE)
        };
        let body = single(body, move |inner| {
            Box::new(
                Container::new()
                    .with_height(BODY_HEIGHT)
                    .with_color(canvas)
                    .with_child(inner),
            )
        });

        // The bottom row: upstream's BACK/NEXT padding and arrangement.
        let back = component(
            Button::new(ID_BASE, l10n.demo_shared_x_axis_back_button_text())
                .with_style(ButtonVariant::Text)
                .with_enabled(state.logged_in)
                .with_pressed(state.pressed == Some(ID_BASE))
                .wired(handle.clone(), |s| &mut s.pressed, toggle_login),
        );
        let next = component(
            Button::new(ID_BASE + 1, l10n.demo_shared_x_axis_next_button_text())
                .with_enabled(!state.logged_in)
                .with_pressed(state.pressed == Some(ID_BASE + 1))
                .wired(handle.clone(), |s| &mut s.pressed, toggle_login),
        );
        let bottom_bar = many(vec![back, next], move |rendered| {
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            for button in rendered {
                row = row.push(button);
            }
            Box::new(
                Container::new()
                    .with_padding(EdgeInsets::symmetric(15.0, 20.0))
                    .with_child(row),
            )
        });

        screen_column(vec![app_bar, body, bottom_bar])
    }
}

/// `_SignInPage`: the avatar, the welcome, the subtitle, the email field and
/// the two text buttons, centered in the page under a max width (upstream's
/// `BoxConstraints(maxWidth: 400)`).
fn sign_in_page(
    handle: &StateHandle<SharedXAxisDemoState>,
    context: &mut BuildContext,
    id_base: u64,
) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let theme = theme_of(context);
    let headline = TextStyle {
        font_size: 24.0,
        color: theme.text,
        ..TextStyle::default()
    };
    let subtitle = TextStyle {
        font_size: 12.0,
        color: GREY,
        ..TextStyle::default()
    };
    let caption_style = TextStyle {
        font_size: 12.0,
        font_weight: 600,
        ..theme.muted()
    };
    let fill = theme.surface_variant;
    let outline = theme.outline;

    // The email field: upstream's `TextField` with an `InputDecoration`
    // label, which lands as a caption above the box here (see the module
    // header).
    let field = stateful(TextField::new(id_base + 2).with_on_changed({
        let handle = handle.clone();
        move |text: &str| {
            let text = text.to_string();
            handle.set_state(move |s| s.email = text);
        }
    }));
    let field_group = single(field, move |field| {
        Box::new(
            Column::new()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(6.0)
                .push(
                    Text::new(
                        GalleryLocalizations::en().demo_shared_x_axis_sign_in_text_field_label(),
                    )
                    .with_style(caption_style.clone()),
                )
                .push(
                    Container::new()
                        .with_color(fill)
                        .with_corner_radius(4.0)
                        .with_border(1.0, outline)
                        .with_padding(EdgeInsets::symmetric(12.0, 10.0))
                        .with_child(FullWidth::new(field)),
                ),
        )
    });

    // Unwired, as upstream's empty `onPressed`s (see the module header).
    let forgot = component(
        Button::new(
            id_base + 3,
            l10n.demo_shared_x_axis_forgot_email_button_text(),
        )
        .with_style(ButtonVariant::Text),
    );
    let create = component(
        Button::new(
            id_base + 4,
            l10n.demo_shared_x_axis_create_account_button_text(),
        )
        .with_style(ButtonVariant::Text),
    );

    let children: Vec<AnyWidget> = vec![
        leaf(move || {
            let mut avatar_box = Container::new().with_size(80.0, 80.0);
            if let Some(image) = Image::shared(AVATAR_CACHE_KEY, AVATAR_LOGO) {
                avatar_box = avatar_box.with_child(ImageView::new(image));
            }
            avatar_box
        }),
        leaf(move || {
            Text::new(GalleryLocalizations::en().demo_shared_x_axis_sign_in_welcome_text())
                .with_style(headline.clone())
        }),
        leaf(move || {
            Text::new(GalleryLocalizations::en().demo_shared_x_axis_sign_in_subtitle_text())
                .with_style(subtitle.clone())
        }),
        field_group,
        many(vec![forgot, create], move |rendered| {
            let mut column = Column::new()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Start);
            for button in rendered {
                column = column.push(button);
            }
            Box::new(column)
        }),
    ];
    many(children, move |rendered| {
        let mut column = Column::new()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(10.0);
        for child in rendered {
            column = column.push(child);
        }
        Box::new(
            Container::new()
                .with_padding(EdgeInsets::symmetric(10.0, 24.0))
                .with_child(Align::new(Alignment::TOP_CENTER, FullWidth::new(column))),
        )
    })
}

/// `_CoursePage`: the title, the subtitle and the five course switches.
fn course_page(
    state: &SharedXAxisDemoState,
    handle: &StateHandle<SharedXAxisDemoState>,
    id_base: u64,
) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let courses = [
        l10n.demo_shared_x_axis_arts_and_crafts_course_title(),
        l10n.demo_shared_x_axis_business_course_title(),
        l10n.demo_shared_x_axis_illustration_course_title(),
        l10n.demo_shared_x_axis_design_course_title(),
        l10n.demo_shared_x_axis_culinary_course_title(),
    ];

    let mut children: Vec<AnyWidget> = vec![
        leaf(|| Container::new().with_height(16.0)),
        component(CenteredLine::new(
            l10n.demo_shared_x_axis_course_page_title(),
            24.0,
            None,
        )),
        leaf(|| Container::new().with_height(10.0)),
        component(CenteredLine::new(
            l10n.demo_shared_x_axis_course_page_subtitle(),
            12.0,
            Some(GREY),
        )),
    ];
    for (index, course) in courses.iter().enumerate() {
        children.push(course_switch(
            id_base + 10 + index as u64,
            index,
            course,
            state.bundled[index],
            handle,
        ));
    }
    many(children, move |rendered| {
        let mut column = Column::new()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(4.0);
        for child in rendered {
            column = column.push(child);
        }
        Box::new(
            Container::new()
                .with_padding(EdgeInsets::symmetric(10.0, 0.0))
                .with_child(column),
        )
    })
}

/// One `_CourseSwitch`: the course name, the bundled/individual subtitle and
/// the switch (see the module header for the `SwitchListTile` note).
fn course_switch(
    id: u64,
    index: usize,
    course: &'static str,
    bundled: bool,
    handle: &StateHandle<SharedXAxisDemoState>,
) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let subtitle = if bundled {
        l10n.demo_shared_x_axis_bundled_course_subtitle()
    } else {
        l10n.demo_shared_x_axis_individual_course_subtitle()
    };
    let switch = component(Switch::new(id, bundled).wired(handle.clone(), COURSE_TOGGLES[index]));
    single(switch, move |switch| {
        Box::new(
            RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(12.0)
                .push_flex(FlexChild::expanded(
                    Column::new()
                        .with_main_axis_size(MainAxisSize::Min)
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(2.0)
                        .push(Text::new(course).with_size(15.0))
                        .push(Text::new(subtitle).with_size(12.0)),
                    1,
                ))
                .push(switch),
        )
    })
}

/// A horizontally centered line of text, for the course page's headings.
struct CenteredLine {
    text: String,
    size: f32,
    color: Option<Color>,
}

impl CenteredLine {
    fn new(text: &str, size: f32, color: Option<Color>) -> CenteredLine {
        CenteredLine {
            text: text.to_string(),
            size,
            color,
        }
    }
}

impl Component for CenteredLine {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let text = self.text.clone();
        let size = self.size;
        let color = self.color.unwrap_or(theme.text);
        leaf(move || {
            FullWidth::new(Text::new(text.clone()).with_style(TextStyle {
                font_size: size,
                color,
                align: TextAlign::Center,
                ..TextStyle::default()
            }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_signs_in_and_back_signs_out() {
        let mut state = SharedXAxisDemoState::default();
        assert!(!state.logged_in);
        toggle_login(&mut state);
        assert!(state.logged_in);
        assert!(state.forward, "NEXT runs the switcher forwards");
        assert!(state.running);
        toggle_login(&mut state);
        assert!(!state.logged_in);
        assert!(!state.forward, "BACK runs it in reverse");
    }

    #[test]
    fn the_courses_start_bundled_and_toggle_individually() {
        let mut state = SharedXAxisDemoState::default();
        assert_eq!(
            state.bundled, [true; 5],
            "upstream's _isCourseBundled = true"
        );
        toggle_course(&mut state, 2);
        assert_eq!(state.bundled, [true, true, false, true, true]);
    }
}
