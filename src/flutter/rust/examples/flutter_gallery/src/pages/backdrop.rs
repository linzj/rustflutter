// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The two-layer container the home page and the settings panel live in.
//!
//! Ported from `lib/pages/backdrop.dart` (flutter/gallery @ d12640d). The
//! earlier batches made settings a route; this is upstream's real structure --
//! a `Stack` holding the settings page behind the home page, with both sliding
//! on one controller -- and it replaces that route. The divergence log entry
//! for settings-as-a-route is removed in PORTING.md.
//!
//! Upstream's mobile layout slides the settings page down from above the top
//! edge and the home page down until only the header strip shows, both over
//! the first 40% of the panel controller; the remaining 60% staggers the
//! settings items in (see `pages/settings.rs`). On desktop the panel instead
//! scales in at the top end over a modal barrier.
//!
//! What is not ported:
//!
//! * The settings icon's custom painter (`pages/settings_icon/`), which morphs
//!   a gear into a close cross through a set of gradient sticks. Here the gear
//!   glyph rotates and cross-fades to the close glyph over the same 500ms.
//! * The Escape-key listener and the semantics announcements around the panel;
//!   the example wires no keyboard or semantics service.
//!
//! Upstream's controller-duration expression is inverted
//! (`isDesktop ? settingsPanelMobileAnimationDuration
//! : settingsPanelDesktopAnimationDuration`), so on mobile the panel takes
//! 600ms. That is upstream's code, kept as written; see
//! `app::GalleryState::backdrop_panel`.

use rustflutter::animation::Curve;
use rustflutter::framework::{component, leaf, many, AnyWidget, StateHandle};
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{Alignment, RenderBox, Size, StackPosition};
use rustflutter::widgets::{Container, Pointer, Stack, Transform};

use crate::app::{self, ids, GalleryState};
use crate::constants::{DESKTOP_SETTINGS_WIDTH, GALLERY_HEADER_HEIGHT};
use crate::data::demos as catalog;
use crate::pages::{home, settings};
use crate::themes::gallery_theme_data::Scheme;

/// Upstream's `_settingsButtonWidth`.
const SETTINGS_BUTTON_WIDTH: f32 = 64.0;
/// Upstream's `_settingsButtonHeightDesktop` / `_settingsButtonHeightMobile`.
const SETTINGS_BUTTON_HEIGHT_DESKTOP: f32 = 56.0;
const SETTINGS_BUTTON_HEIGHT_MOBILE: f32 = 40.0;
/// Upstream's desktop panel: `maxHeight: 560`, `maxWidth/minWidth:
/// desktopSettingsWidth`, corner 40.
const DESKTOP_PANEL_HEIGHT: f32 = 560.0;
const DESKTOP_PANEL_RADIUS: f32 = 40.0;

/// Upstream's `Interval(0.0, 0.4, curve: Curves.ease)`, the slice of the panel
/// controller that slides the two layers.
pub fn slide_interval(value: f32) -> f32 {
    Curve::EASE.transform(((value - 0.0) / 0.4).clamp(0.0, 1.0))
}

/// Upstream's `Interval(0.4, 1.0, curve: Curves.ease)`, the slice that staggers
/// the settings items.
pub fn stagger_interval(value: f32) -> f32 {
    Curve::EASE.transform(((value - 0.4) / 0.6).clamp(0.0, 1.0))
}

/// The backdrop: settings behind, home in front, the settings icon on top.
///
/// `panel` is the panel controller (shared with the settings page, which reads
/// the stagger interval off it), `icon` the icon controller.
pub fn page(
    state: &GalleryState,
    handle: StateHandle<GalleryState>,
    is_desktop: bool,
) -> AnyWidget {
    let scheme = state.scheme();
    let panel = state.backdrop_panel.value();
    let settings_page = settings::panel(state, handle.clone(), is_desktop);
    let home_page = home::page(state, handle.clone(), is_desktop);
    let icon = component(SettingsIcon {
        id: ids::SETTINGS,
        scheme,
        is_desktop,
        open: state.settings_open,
        progress: state.icon.value(),
        pressed: state.pressed,
        handle: handle.clone(),
    });

    let stacked = if is_desktop {
        desktop_stack(settings_page, home_page, icon, state, handle.clone(), panel)
    } else {
        let slide = slide_interval(panel);
        many(vec![settings_page, home_page, icon], move |mut rendered| {
            let icon = rendered.pop().expect("three children");
            let home = rendered.pop().expect("three children");
            let settings = rendered.pop().expect("three children");
            Box::new(BackdropStack {
                settings,
                home,
                icon,
                slide,
                size: Size::ZERO,
                icon_size: Size::ZERO,
            })
        })
    };

    // The about dialog sits over both layers, as upstream's `showDialog` sits
    // over the whole navigator.
    app::with_overlay(stacked, crate::pages::about::overlay(state, handle))
}

/// Upstream's desktop branch: home full-screen, a barrier while the panel is
/// open, and the panel scaling in at the top end.
fn desktop_stack(
    settings_page: AnyWidget,
    home_page: AnyWidget,
    icon: AnyWidget,
    state: &GalleryState,
    handle: StateHandle<GalleryState>,
    panel: f32,
) -> AnyWidget {
    let scheme = state.scheme();

    // Upstream's `ModalBarrier(dismissible: false)` with an `onPointerDown`
    // that closes the panel: a dim sheet that swallows the tap and toggles.
    let barrier = if state.settings_open {
        let close = PointerHandlers::new().with_pointer_down(move |_| {
            handle.set_state(|state| state.toggle_settings());
        });
        leaf(move || {
            Pointer::new(
                ids::SCRIM,
                Container::new().with_color(Color::argb(0x8A, 0, 0, 0)),
            )
            .with_handlers(close.clone())
        })
    } else {
        leaf(|| rustflutter::widgets::Empty)
    };

    // Upstream's `ScaleTransition` over `Curves.fastOutSlowIn`, aligned to the
    // top end, clipping to a 40-radius card of fixed width.
    let scale = Curve::FAST_OUT_SLOW_IN.transform(panel);
    let fill = scheme.secondary_container;
    let card = rustflutter::framework::single(settings_page, move |page| {
        Box::new(
            Transform::scale(
                scale.max(f32::EPSILON),
                Container::new()
                    .with_size(DESKTOP_SETTINGS_WIDTH, DESKTOP_PANEL_HEIGHT)
                    .with_color(fill)
                    .with_corner_radius(DESKTOP_PANEL_RADIUS)
                    .with_child(rustflutter::widgets::ClipRRect::new(
                        DESKTOP_PANEL_RADIUS,
                        page,
                    )),
            )
            .with_origin(Alignment::TOP_RIGHT),
        )
    });

    many(vec![home_page, barrier, card, icon], move |mut rendered| {
        let icon = rendered.pop().expect("four children");
        let card = rendered.pop().expect("four children");
        let barrier = rendered.pop().expect("four children");
        let home = rendered.pop().expect("four children");
        Box::new(
            Stack::new()
                .push(home)
                .push_positioned(barrier, StackPosition::fill())
                .push_positioned(
                    card,
                    StackPosition {
                        top: Some(0.0),
                        right: Some(0.0),
                        ..StackPosition::default()
                    },
                )
                .push_positioned(
                    icon,
                    StackPosition {
                        top: Some(0.0),
                        right: Some(0.0),
                        ..StackPosition::default()
                    },
                ),
        )
    })
}

/// Upstream's mobile branch, a render object for the same reason
/// `app::TransitionStack` is one: the slides are fractions of a height that
/// only exists once layout has run.
struct BackdropStack {
    settings: rustflutter::render::BoxedRender,
    home: rustflutter::render::BoxedRender,
    icon: rustflutter::render::BoxedRender,
    /// The slide interval's value: 0 closed, 1 open.
    slide: f32,
    size: Size,
    icon_size: Size,
}

impl BackdropStack {
    /// Where the settings layer's top edge sits: above the screen when closed,
    /// at the top when open. Upstream's `_slideDownSettingsPageAnimation`.
    fn settings_dy(&self) -> f32 {
        -self.size.height * (1.0 - self.slide)
    }

    /// Where the home layer's top edge sits: at the top when closed, a header
    /// strip from the bottom when open. Upstream's
    /// `_slideDownHomePageAnimation`.
    fn home_dy(&self) -> f32 {
        (self.size.height - GALLERY_HEADER_HEIGHT) * self.slide
    }
}

impl RenderBox for BackdropStack {
    fn layout(
        &mut self,
        constraints: rustflutter::render::BoxConstraints,
    ) -> rustflutter::render::Size {
        let tight = rustflutter::render::BoxConstraints::tight_for(constraints.biggest());
        self.settings.layout(tight);
        self.home.layout(tight);
        self.icon_size = self.icon.layout(constraints.loosen());
        self.size = constraints.biggest();
        self.size
    }

    fn size(&self) -> rustflutter::render::Size {
        self.size
    }

    fn paint(
        &self,
        context: &mut rustflutter::render::PaintContext,
        offset: rustflutter::render::Offset,
    ) {
        self.settings.paint(
            context,
            rustflutter::render::Offset::new(offset.dx, offset.dy + self.settings_dy()),
        );
        self.home.paint(
            context,
            rustflutter::render::Offset::new(offset.dx, offset.dy + self.home_dy()),
        );
        self.icon.paint(
            context,
            rustflutter::render::Offset::new(
                offset.dx + self.size.width - self.icon_size.width,
                offset.dy,
            ),
        );
    }

    fn hit_test(
        &self,
        position: rustflutter::render::Offset,
        result: &mut rustflutter::render::HitTestResult,
    ) -> bool {
        if !self.size.contains(position) {
            return false;
        }
        // The icon is on top in every state: it is how an open panel closes.
        let icon_origin =
            rustflutter::render::Offset::new(self.size.width - self.icon_size.width, 0.0);
        let icon_local = rustflutter::render::Offset::new(
            position.dx - icon_origin.dx,
            position.dy - icon_origin.dy,
        );
        if self.icon_size.contains(icon_local) && self.icon.hit_test(icon_local, result) {
            return true;
        }
        // Home is painted over the settings layer, so it gets the tap first --
        // where it still is. Once it has slid away, the point falls through to
        // the settings layer.
        let home_local =
            rustflutter::render::Offset::new(position.dx, position.dy - self.home_dy());
        if self.size.contains(home_local) && self.home.hit_test(home_local, result) {
            return true;
        }
        let settings_local =
            rustflutter::render::Offset::new(position.dx, position.dy - self.settings_dy());
        if self.size.contains(settings_local) {
            self.settings.hit_test(settings_local, result);
        }
        // The panel swallows what it does not use: while it is open a tap must
        // not reach whatever is behind the backdrop.
        true
    }
}

/// The settings button at the top end. Upstream's `_SettingsIcon`, with the
/// custom-painted morph replaced by a rotating, cross-fading glyph (see the
/// module header).
struct SettingsIcon {
    id: u64,
    scheme: Scheme,
    is_desktop: bool,
    open: bool,
    /// The icon controller's value: 0 shows the gear, 1 the close cross.
    progress: f32,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
}

impl Component for SettingsIcon {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let id = self.id;
        let scheme = self.scheme;
        let height = if self.is_desktop {
            SETTINGS_BUTTON_HEIGHT_DESKTOP
        } else {
            SETTINGS_BUTTON_HEIGHT_MOBILE
        };
        let t = self.progress;
        // Upstream shows a transparent button while the open panel sits still
        // behind it, and the container colour otherwise.
        let filled = !(self.open && t >= 1.0);
        let held = self.pressed == Some(id);

        let handlers = PointerHandlers::new()
            .with_tap({
                let handle = self.handle.clone();
                move |_| {
                    handle.set_state(|state| state.toggle_settings());
                }
            })
            .with_press_change({
                let handle = self.handle.clone();
                move |down| {
                    handle.set_state(move |state| {
                        state.pressed = if down { Some(id) } else { None };
                    });
                }
            });

        leaf(move || {
            // The cross-fade: the gear leaves over the first half of the turn,
            // the close glyph arrives over the second.
            let (glyph, alpha) = if t < 0.5 {
                (catalog::icon::SETTINGS, 1.0 - t * 2.0)
            } else {
                (catalog::icon::CLOSE, (t - 0.5) * 2.0)
            };
            let ink = scheme.on_surface.with_alpha((alpha * 255.0) as u8);
            let icon = Transform::rotate(
                t * 90.0,
                rustflutter::widgets::Align::new(
                    Alignment::CENTER,
                    Text::new(glyph)
                        .with_font_family(catalog::MATERIAL_ICONS)
                        .with_size(22.0)
                        .with_color(ink),
                ),
            );
            Pointer::new(
                id,
                Container::new()
                    .with_size(SETTINGS_BUTTON_WIDTH, height)
                    .with_color(if held {
                        scheme.on_surface.with_alpha(0x24)
                    } else if filled {
                        scheme.secondary_container
                    } else {
                        Color::TRANSPARENT
                    })
                    .with_child(icon),
            )
            .with_handlers(handlers.clone())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_slide_runs_over_the_first_two_fifths() {
        assert_eq!(slide_interval(0.0), 0.0);
        assert_eq!(slide_interval(0.4), 1.0);
        assert_eq!(
            slide_interval(1.0),
            1.0,
            "the slide is done before the stagger starts"
        );
    }

    #[test]
    fn the_stagger_runs_over_the_rest() {
        assert_eq!(stagger_interval(0.0), 0.0);
        assert_eq!(stagger_interval(0.4), 0.0);
        assert_eq!(stagger_interval(1.0), 1.0);
    }

    #[test]
    fn the_controllers_are_constructible_with_upstreams_durations() {
        use rustflutter::animation::Controller;
        let panel = Controller::new(crate::constants::SETTINGS_PANEL_DESKTOP_ANIMATION_DURATION);
        assert_eq!(panel.value(), 0.0);
        let icon = Controller::new(std::time::Duration::from_millis(500));
        assert_eq!(icon.value(), 0.0);
    }
}
