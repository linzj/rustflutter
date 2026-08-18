// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The splash page: the Flutter-logo layer behind the home page.
//!
//! Ported from `lib/pages/splash.dart` (flutter/gallery @ d12640d). Upstream
//! the splash is not a launch screen at this commit -- the controller starts
//! dismissed, so the home page covers it -- it is a pull-down layer: dragging
//! down from the top strip of the home page slides the front layer away and
//! shows the logo underneath, and a tap or an upward fling slides it back.
//! That is what this is.
//!
//! Deltas, logged in PORTING.md:
//!
//! * Upstream's back layer plays one of ten animated `splash_effect_N.gif`s
//!   behind the logo, chosen at random. The image pipeline decodes a GIF's
//!   first frame only and the effects are not shipped, so the back layer is
//!   the logo alone on the GIFs' background colour.
//! * The foldable `TwoPane` branch is unreachable:
//!   `adaptive_layout::is_display_foldable` is always false here.

use rustflutter::animation::Curve;
use rustflutter::framework::{leaf, many, single, AnyWidget, StateHandle};
use rustflutter::gestures::PointerHandlers;
use rustflutter::painting::Image;
use rustflutter::prelude::*;
use rustflutter::render::{Alignment, RenderBox, Size};
use rustflutter::widgets::{Align, ClipRRect, Container, IgnorePointer, ImageView, Pointer};

use crate::app::{ids, GalleryState};

/// Upstream's `homePeekDesktop` / `homePeekMobile`: how much of the front
/// layer stays on screen while the splash is showing.
pub const HOME_PEEK_DESKTOP: f32 = 210.0;
pub const HOME_PEEK_MOBILE: f32 = 60.0;

/// Upstream's `assets/logo/flutter_logo.png`, from `flutter_gallery_assets`
/// 1.0.2 (see `assets/README.md`).
pub const FLUTTER_LOGO: &[u8] = include_bytes!("../../assets/logo/flutter_logo.png");
/// Upstream's `assets/logo/flutter_logo_color.png`, which the desktop home
/// footer shows in the light theme.
pub const FLUTTER_LOGO_COLOR: &[u8] = include_bytes!("../../assets/logo/flutter_logo_color.png");

/// The GIFs' background colour, which upstream paints the back layer in.
const BACKGROUND: Color = Color(0xFF030303);

/// Upstream's `SplashPage`: `child` in front, the logo layer behind, and the
/// slide between them driven by `GalleryState::splash`.
pub fn page(
    state: &GalleryState,
    handle: StateHandle<GalleryState>,
    is_desktop: bool,
    child: AnyWidget,
) -> AnyWidget {
    let peek = if is_desktop {
        HOME_PEEK_DESKTOP
    } else {
        HOME_PEEK_MOBILE
    };
    // Upstream runs the rect tween through `Curves.easeInOut`.
    let progress = Curve::EASE_IN_OUT.transform(state.splash.value());
    // Upstream's `_isSplashVisible`: showing, or on its way to showing.
    let visible = progress > 0.0
        || (state.splash.is_running()
            && state.splash.direction() == rustflutter::animation::Direction::Forward);

    let back = back_layer(handle.clone(), is_desktop, !visible, peek);

    // The front layer: rounded and inset on desktop, inert while the splash
    // shows.
    let mut front = child;
    if is_desktop {
        front = single(front, move |rendered| {
            Box::new(
                Container::new()
                    .with_padding(EdgeInsets::only(0.0, 136.0, 0.0, 0.0))
                    .with_child(ClipRRect::new(40.0, rendered)),
            )
        });
    }
    if visible {
        front = single(front, |rendered| Box::new(IgnorePointer::new(rendered)));
    }

    // While the splash shows, the front layer takes the tap (or the upward
    // fling) that closes it. A sheet rather than a wrapper because
    // `SplashStack` is what positions the front layer.
    let sheet = visible.then(|| {
        let close = PointerHandlers::new()
            .with_tap({
                let handle = handle.clone();
                move |_| {
                    handle.set_state(|state| {
                        state.splash.reverse();
                    });
                }
            })
            .with_drag_end(move |end| {
                if end.velocity.dy < -200.0 {
                    handle.set_state(|state| {
                        state.splash.reverse();
                    });
                }
            });
        leaf(move || {
            Pointer::new(
                ids::SPLASH_FRONT,
                Container::new().with_color(Color::TRANSPARENT),
            )
            .with_handlers(close.clone())
        })
    });

    let children = match sheet {
        Some(sheet) => vec![back, front, sheet],
        None => vec![
            back,
            front,
            leaf(|| Container::new().with_color(Color::TRANSPARENT)),
        ],
    };
    many(children, move |mut rendered| {
        let sheet = rendered.pop().expect("three children");
        let front = rendered.pop().expect("three children");
        let back = rendered.pop().expect("three children");
        Box::new(SplashStack {
            back,
            front,
            sheet,
            visible,
            peek,
            progress,
            size: Size::ZERO,
        })
    })
}

/// Upstream's `_SplashBackLayer`: the logo on the GIFs' background colour.
///
/// `collapsed` is upstream's `isSplashCollapsed`; when collapsed on mobile the
/// layer is empty (it is fully covered), and on desktop it shows the logo at
/// the top, which taps through to reveal the splash.
fn back_layer(
    handle: StateHandle<GalleryState>,
    is_desktop: bool,
    collapsed: bool,
    peek: f32,
) -> AnyWidget {
    let logo = Image::shared("flutter_logo", FLUTTER_LOGO);

    let content: AnyWidget = if collapsed {
        if is_desktop {
            let reveal = PointerHandlers::new().with_tap(move |_| {
                handle.set_state(|state| {
                    state.splash.forward();
                });
            });
            leaf(move || {
                let logo_view: rustflutter::render::BoxedRender = match logo.clone() {
                    Some(image) => rustflutter::render::RenderRef::new(ImageView::with_fit(
                        image,
                        rustflutter::render::BoxFit::Contain,
                    )),
                    None => rustflutter::render::RenderRef::new(
                        Container::new().with_size(100.0, 100.0),
                    ),
                };
                Container::new()
                    .with_padding(EdgeInsets::only(0.0, 50.0, 0.0, 0.0))
                    .with_child(Align::new(
                        Alignment::TOP_CENTER,
                        Pointer::new(ids::SPLASH_BACK, logo_view).with_handlers(reveal.clone()),
                    ))
            })
        } else {
            leaf(|| Container::new())
        }
    } else {
        leaf(move || match logo.clone() {
            Some(image) => Align::new(
                Alignment::CENTER,
                ImageView::with_fit(image, rustflutter::render::BoxFit::Contain),
            ),
            None => Align::new(Alignment::CENTER, Container::new()),
        })
    };

    single(content, move |rendered| {
        Box::new(
            Container::new()
                .with_color(BACKGROUND)
                .with_padding(EdgeInsets::only(0.0, 0.0, 0.0, peek))
                .with_child(rendered),
        )
    })
}

/// The slide between the layers, a render object for the same reason
/// `app::TransitionStack` is one: the travel is a fraction of a height that
/// only exists once layout has run.
struct SplashStack {
    back: rustflutter::render::BoxedRender,
    front: rustflutter::render::BoxedRender,
    /// The tap sheet over the front layer; dead weight when the splash is
    /// collapsed, so hit testing skips it then.
    sheet: rustflutter::render::BoxedRender,
    visible: bool,
    peek: f32,
    progress: f32,
    size: Size,
}

impl SplashStack {
    /// Upstream's `_getPanelAnimation`: the front layer's top edge runs from
    /// the top of the screen to a peek's height from the bottom.
    fn front_dy(&self) -> f32 {
        (self.size.height - self.peek) * self.progress
    }
}

impl RenderBox for SplashStack {
    fn layout(
        &mut self,
        constraints: rustflutter::render::BoxConstraints,
    ) -> rustflutter::render::Size {
        let tight = rustflutter::render::BoxConstraints::tight_for(constraints.biggest());
        self.back.layout(tight);
        self.front.layout(tight);
        self.sheet.layout(tight);
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
        self.back.paint(context, offset);
        let front_offset = rustflutter::render::Offset::new(offset.dx, offset.dy + self.front_dy());
        self.front.paint(context, front_offset);
        if self.visible {
            self.sheet.paint(context, front_offset);
        }
    }

    fn hit_test(
        &self,
        position: rustflutter::render::Offset,
        result: &mut rustflutter::render::HitTestResult,
    ) -> bool {
        if !self.size.contains(position) {
            return false;
        }
        let dy = self.front_dy();
        let local = rustflutter::render::Offset::new(position.dx, position.dy - dy);
        if self.visible {
            // The sheet speaks for the whole front layer.
            self.sheet.hit_test(local, result);
            return true;
        }
        if self.size.contains(local) {
            self.front.hit_test(local, result);
        }
        // The collapsed back layer's own target (the desktop logo) sits behind
        // the front layer, which covers it completely on mobile.
        self.back.hit_test(position, result);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_peeks_are_upstreams() {
        assert_eq!(HOME_PEEK_DESKTOP, 210.0);
        assert_eq!(HOME_PEEK_MOBILE, 60.0);
    }

    #[test]
    fn the_logo_assets_are_pngs() {
        // A truncated copy fails to decode at first use, which is exactly the
        // bug this assertion exists to be boring about.
        assert_eq!(&FLUTTER_LOGO[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&FLUTTER_LOGO_COLOR[..8], b"\x89PNG\r\n\x1a\n");
    }
}
