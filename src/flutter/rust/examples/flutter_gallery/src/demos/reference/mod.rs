// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The reference demos.
//!
//! Mirrors `lib/demos/reference/` (flutter/gallery @ d12640d): one child
//! module per upstream file. What the ported demos share stays here: the
//! `stage()`/`overlay()` dispatch the demo page routes the reference slugs
//! to, and the layout and transition helpers more than one of them builds
//! with.
//!
//! The catalogue carries four reference slugs (`src/data/demos.rs`):
//! `colors`, `typography`, `motion` and `2d-transformations`. Upstream's
//! fifth, the two-pane demo, is a `DeferredWidget` demo and is excluded from
//! the catalogue (PORTING.md, "deferred loading is synchronous"), so
//! `two_pane_demo` is wired here but unrouted.
//!
//! Upstream's `motion` slug is one catalogue entry with six
//! `GalleryDemoConfiguration`s (`lib/data/demos.dart`), one per transition
//! pattern; the catalogue here is flattened to one configuration per demo
//! (PORTING.md, "demo options section is unreachable"), so the six patterns
//! render stacked as six sections on the one stage, in upstream's
//! configuration order: container transform, shared x-axis, shared y-axis,
//! shared z-axis, fade through, fade scale. The transition math they share
//! -- the `animations` package's curves, intervals and offsets -- is the
//! [`transitions`] module below.

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, MainAxisSize, RenderFlex};

use crate::app::GalleryState;
use crate::data::demos::Demo;

mod colors_demo;
mod motion_demo_container_transition;
mod motion_demo_fade_scale_transition;
mod motion_demo_fade_through_transition;
mod motion_demo_shared_x_axis_transition;
mod motion_demo_shared_y_axis_transition;
mod motion_demo_shared_z_axis_transition;
mod transformations_demo;
mod transformations_demo_board;
mod transformations_demo_color_picker;
mod transformations_demo_edit_board_point;
mod two_pane_demo;
mod typography_demo;

/// The value the motion demo's clock is at, published by the app.
///
/// The pre-split aggregate motion demo read it to drive its curve tracks;
/// the six ported demos drive their own controllers through
/// `StatefulComponent::advance` instead, so nothing reads it any more. It
/// stays published because `app.rs` owns the publish and the animated-slug
/// list, and stays exported from here so `app.rs` keeps one import path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionValue(pub f32);

/// The reference slugs the catalogue carries. The demo page routes on this
/// list (no shared prefix distinguishes the reference demos the way
/// `cupertino-` does the Cupertino ones).
pub const SLUGS: &[&str] = &["colors", "typography", "motion", "2d-transformations"];

/// Builds the demo itself, dispatched by slug.
///
/// The signature mirrors `demos::material::stage` so the demo page's routing
/// stays one shape; no reference demo reads the shared `DemoState` today --
/// per-demo state is a per-demo `StatefulComponent`'s, the way upstream's
/// per-widget `State`s are.
pub fn stage(
    demo: &'static Demo,
    state: &GalleryState,
    handle: StateHandle<GalleryState>,
) -> AnyWidget {
    let _ = (state, handle);
    component(Stage { demo })
}

/// The demo itself, dispatched by slug.
struct Stage {
    demo: &'static Demo,
}

impl Component for Stage {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);

        let content: AnyWidget = match self.demo.slug {
            "colors" => colors_demo::stage(),
            "typography" => typography_demo::stage(),
            // Upstream's six motion configurations, stacked; see the module
            // header. Each section is one upstream demo file's port.
            "motion" => column(
                vec![
                    motion_demo_container_transition::section(),
                    motion_demo_shared_x_axis_transition::section(),
                    motion_demo_shared_y_axis_transition::section(),
                    motion_demo_shared_z_axis_transition::section(),
                    motion_demo_fade_through_transition::section(),
                    motion_demo_fade_scale_transition::section(),
                ],
                16.0,
            ),
            "2d-transformations" => transformations_demo::stage(),
            other => not_written_yet(other),
        };

        // The same stage chrome `demos::material`'s stage gives every demo:
        // the surface card the demo's content sits in.
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
///
/// No reference demo has a modal today; the signature mirrors
/// `demos::material::overlay` for the same reason `stage`'s does.
pub fn overlay(
    _demo: &'static Demo,
    _state: &GalleryState,
    _handle: StateHandle<GalleryState>,
) -> Option<AnyWidget> {
    None
}

// -- Layout helpers -----------------------------------------------------------
//
// The same two helpers `demos::material`'s demos build with; its copies are
// private to that module.

fn column(children: Vec<AnyWidget>, spacing: f32) -> AnyWidget {
    many(children, move |rendered| {
        // Start rather than Stretch, as in `demos::material`: Stretch forces a
        // tight cross-axis constraint, which overrides a child's own width.
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

/// One demo screen's frame: its children stretched to the stage's width in a
/// column. The motion sections build with this -- the app bar, the bounded
/// body and any bottom chrome of one upstream demo screen.
fn screen_column(children: Vec<AnyWidget>) -> AnyWidget {
    many(children, move |rendered| {
        let mut flex = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        for child in rendered {
            flex = flex.push(child);
        }
        Box::new(flex)
    })
}

/// The stand-in every unwritten demo renders -- the same one
/// `demos::material` uses (its copy is private to that module).
fn not_written_yet(slug: &str) -> AnyWidget {
    let slug = slug.to_string();
    leaf(move || Text::new(format!("The demo for {slug} is not written yet.")).with_size(13.0))
}

// -- Transition specs ---------------------------------------------------------
//
// The `animations` package (2.2.0, the gallery's pinned version) has no
// counterpart in the framework, so the transition patterns the six motion
// demos use are spelled out here as pure functions of a 0..1 progress,
// mirroring the package's builders:
//
// * `fade_scale_enter`/`fade_scale_exit` -- `FadeScaleTransition`
//   (fade_scale_transition.dart's `_fadeInTransition`, `_scaleInTransition`
//   and `_fadeOutTransition`).
// * `fade_through_enter`/`fade_through_exit` -- `FadeThroughTransition`'s
//   `_ZoomedFadeIn` and `_FadeOut` (fade_through_transition.dart).
// * `shared_axis_enter`/`shared_axis_exit` -- `SharedAxisTransition`'s
//   `_EnterTransition` and `_ExitTransition` (shared_axis_transition.dart).
// * `open_container_open_opacity`/`open_container_closed_opacity` --
//   `OpenContainer`'s `_getOpenOpacityTween` and `_getClosedOpacityTween`
//   (open_container.dart). The container's rect-to-rect morph is not
//   portable -- it is a measured-geometry animation and a build here has no
//   geometry -- so the demos approximate it; see the container demo's header.
pub(super) mod transitions {
    use rustflutter::animation::Curve;

    /// Flutter's `Easing.legacy`, material's standard curve
    /// (`Curves.fastOutSlowIn` under its deprecated name).
    pub const LEGACY: Curve = Curve::FAST_OUT_SLOW_IN;
    /// Flutter's `Easing.legacyAccelerate` (`Curves.fastLinearToSlowEaseIn`).
    pub const LEGACY_ACCELERATE: Curve = Curve::Cubic(0.4, 0.0, 1.0, 1.0);
    /// Flutter's `Easing.legacyDecelerate` (`Curves.linearToSlowEaseOut`).
    pub const LEGACY_DECELERATE: Curve = Curve::Cubic(0.0, 0.0, 0.2, 1.0);

    /// The fade-through patterns' own curves: `_ZoomedFadeIn._inCurve` and
    /// `_FadeOut._outCurve`.
    const FADE_THROUGH_IN: Curve = Curve::Cubic(0.0, 0.0, 0.2, 1.0);
    const FADE_THROUGH_OUT: Curve = Curve::Cubic(0.4, 0.0, 1.0, 1.0);

    /// Upstream's `Interval`: `t` rescaled onto `begin..end`, clamped.
    pub fn interval(t: f32, begin: f32, end: f32) -> f32 {
        ((t - begin) / (end - begin)).clamp(0.0, 1.0)
    }

    fn lerp(begin: f32, end: f32, t: f32) -> f32 {
        begin + (end - begin) * t
    }

    /// Where a transitioning child is: its opacity, its pixel offset and its
    /// uniform scale. The identity placement is `opacity: 1`, no offset, no
    /// scale, which is also what both ends of every pattern resolve to.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Placement {
        pub opacity: f32,
        pub dx: f32,
        pub dy: f32,
        pub scale: f32,
    }

    impl Placement {
        pub const REST: Placement = Placement {
            opacity: 1.0,
            dx: 0.0,
            dy: 0.0,
            scale: 1.0,
        };
    }

    /// `FadeScaleTransition`'s forward builder: a quick linear fade-in over
    /// the first 30% and an 80% -> 100% scale under `legacyDecelerate`.
    pub fn fade_scale_enter(t: f32) -> Placement {
        Placement {
            opacity: interval(t, 0.0, 0.3),
            scale: lerp(0.8, 1.0, LEGACY_DECELERATE.transform(t)),
            ..Placement::REST
        }
    }

    /// Its reverse builder: a plain linear fade-out, no scale.
    pub fn fade_scale_exit(t: f32) -> Placement {
        Placement {
            opacity: 1.0 - t.clamp(0.0, 1.0),
            ..Placement::REST
        }
    }

    /// `FadeThroughTransition`'s entering half: nothing for the first 30%
    /// (the exit is still playing), then a 0.92 -> 1 zoom with the fade.
    pub fn fade_through_enter(t: f32) -> Placement {
        let eased = FADE_THROUGH_IN.transform(interval(t, 0.3, 1.0));
        Placement {
            opacity: eased,
            scale: lerp(0.92, 1.0, eased),
            ..Placement::REST
        }
    }

    /// Its exiting half: out over the first 30% under `_outCurve`, gone
    /// after.
    pub fn fade_through_exit(t: f32) -> Placement {
        Placement {
            opacity: 1.0 - FADE_THROUGH_OUT.transform(interval(t, 0.0, 0.3)),
            ..Placement::REST
        }
    }

    /// Which axis a shared-axis transition moves along, upstream's
    /// `SharedAxisTransitionType`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum SharedAxis {
        Horizontal,
        Vertical,
        Scaled,
    }

    /// The 30-pixel slide of the shared-axis patterns.
    const SHARED_AXIS_SHIFT: f32 = 30.0;

    /// `SharedAxisTransition`'s `_EnterTransition`: the fade runs over the
    /// last 70% under `legacyDecelerate`; the slide or scale runs the whole
    /// way under `legacy`. `reverse` plays the pattern's mirrored form, the
    /// package's `reverse` parameter.
    pub fn shared_axis_enter(t: f32, axis: SharedAxis, reverse: bool) -> Placement {
        let eased = LEGACY.transform(t);
        let mut placement = Placement {
            opacity: LEGACY_DECELERATE.transform(interval(t, 0.3, 1.0)),
            ..Placement::REST
        };
        match axis {
            SharedAxis::Horizontal => {
                let from = if reverse {
                    -SHARED_AXIS_SHIFT
                } else {
                    SHARED_AXIS_SHIFT
                };
                placement.dx = lerp(from, 0.0, eased);
            }
            SharedAxis::Vertical => {
                let from = if reverse {
                    -SHARED_AXIS_SHIFT
                } else {
                    SHARED_AXIS_SHIFT
                };
                placement.dy = lerp(from, 0.0, eased);
            }
            SharedAxis::Scaled => {
                let from = if reverse { 1.10 } else { 0.80 };
                placement.scale = lerp(from, 1.0, eased);
            }
        }
        placement
    }

    /// `SharedAxisTransition`'s `_ExitTransition`: out over the first 30%
    /// under the flipped `legacyAccelerate`; the slide or scale runs the
    /// whole way under `legacy`, away from the incoming side.
    pub fn shared_axis_exit(t: f32, axis: SharedAxis, reverse: bool) -> Placement {
        let eased = LEGACY.transform(t);
        let mut placement = Placement {
            opacity: 1.0 - LEGACY_ACCELERATE.transform(interval(t, 0.0, 0.3)),
            ..Placement::REST
        };
        match axis {
            SharedAxis::Horizontal => {
                let to = if reverse {
                    SHARED_AXIS_SHIFT
                } else {
                    -SHARED_AXIS_SHIFT
                };
                placement.dx = lerp(0.0, to, eased);
            }
            SharedAxis::Vertical => {
                let to = if reverse {
                    SHARED_AXIS_SHIFT
                } else {
                    -SHARED_AXIS_SHIFT
                };
                placement.dy = lerp(0.0, to, eased);
            }
            SharedAxis::Scaled => {
                let to = if reverse { 0.80 } else { 1.10 };
                placement.scale = lerp(1.0, to, eased);
            }
        }
        placement
    }

    /// `OpenContainer`'s `_openOpacityTween`: the incoming content fades in
    /// over the second fifth for the fade type, over the last four fifths
    /// for fade-through.
    pub fn open_container_open_opacity(t: f32, fade_through: bool) -> f32 {
        if fade_through {
            interval(t, 0.2, 1.0)
        } else {
            interval(t, 0.2, 0.4)
        }
    }

    /// Its `_closedOpacityTween`: for the fade type the closed container
    /// itself becomes the open one, so the closed content stays put; for
    /// fade-through it is gone within the first fifth.
    pub fn open_container_closed_opacity(t: f32, fade_through: bool) -> f32 {
        if fade_through {
            1.0 - interval(t, 0.0, 0.2)
        } else {
            1.0
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn every_pattern_restores_the_identity_at_its_end() {
            for axis in [
                SharedAxis::Horizontal,
                SharedAxis::Vertical,
                SharedAxis::Scaled,
            ] {
                for reverse in [false, true] {
                    assert_eq!(shared_axis_enter(1.0, axis, reverse), Placement::REST);
                    // The exit ends invisible; offset and scale finish at the
                    // pattern's own end, which is not the identity.
                    assert_eq!(shared_axis_exit(1.0, axis, reverse).opacity, 0.0);
                    assert_eq!(shared_axis_enter(0.0, axis, reverse).opacity, 0.0);
                    assert_eq!(shared_axis_exit(0.0, axis, reverse), Placement::REST);
                }
            }
            assert_eq!(fade_scale_enter(1.0), Placement::REST);
            assert_eq!(fade_scale_exit(0.0), Placement::REST);
            assert_eq!(fade_scale_exit(1.0).opacity, 0.0);
            assert_eq!(fade_through_enter(1.0), Placement::REST);
            assert_eq!(fade_through_exit(0.0), Placement::REST);
            assert_eq!(fade_through_exit(1.0).opacity, 0.0);
        }

        #[test]
        fn the_fade_through_halves_take_turns() {
            // The exit is done before the entrance starts: the middle of the
            // transition is the background showing through, which is the
            // "through" in the name.
            assert_eq!(fade_through_exit(0.3).opacity, 0.0);
            assert_eq!(fade_through_enter(0.3).opacity, 0.0);
            assert!(fade_through_enter(0.5).opacity > 0.0);
        }

        #[test]
        fn the_fade_scale_entrance_is_quick_and_scaled() {
            // All of the fade is in the first 30%...
            assert_eq!(fade_scale_enter(0.3).opacity, 1.0);
            // ...while the scale is still arriving.
            let halfway = fade_scale_enter(0.5);
            assert!(halfway.scale > 0.9 && halfway.scale < 1.0);
            assert_eq!(fade_scale_enter(0.0).scale, 0.8);
        }

        #[test]
        fn the_shared_axis_slide_is_thirty_pixels_and_mirrored_in_reverse() {
            let enter = shared_axis_enter(0.0, SharedAxis::Horizontal, false);
            assert_eq!(enter.dx, 30.0);
            assert_eq!(
                shared_axis_enter(0.0, SharedAxis::Horizontal, true).dx,
                -30.0
            );
            assert_eq!(
                shared_axis_exit(1.0, SharedAxis::Horizontal, false).dx,
                -30.0
            );
            assert_eq!(shared_axis_exit(1.0, SharedAxis::Horizontal, true).dx, 30.0);
            assert_eq!(shared_axis_enter(0.0, SharedAxis::Vertical, false).dy, 30.0);
        }

        #[test]
        fn the_scaled_axis_zooms_the_right_way() {
            // Forward: the incoming page grows into place, the outgoing one
            // grows out of it. Reverse mirrors both.
            assert_eq!(
                shared_axis_enter(0.0, SharedAxis::Scaled, false).scale,
                0.80
            );
            assert_eq!(shared_axis_enter(0.0, SharedAxis::Scaled, true).scale, 1.10);
            assert_eq!(shared_axis_exit(1.0, SharedAxis::Scaled, false).scale, 1.10);
            assert_eq!(shared_axis_exit(1.0, SharedAxis::Scaled, true).scale, 0.80);
        }

        #[test]
        fn the_container_opacities_match_the_package_tweens() {
            // Fade: the closed content never fades, the open content is all
            // in by two fifths.
            assert_eq!(open_container_closed_opacity(0.5, false), 1.0);
            assert_eq!(open_container_open_opacity(0.2, false), 0.0);
            assert_eq!(open_container_open_opacity(0.4, false), 1.0);
            // Fade through: the closed content is gone in the first fifth,
            // the open content fades over the rest.
            assert_eq!(open_container_closed_opacity(0.2, true), 0.0);
            assert_eq!(open_container_open_opacity(0.2, true), 0.0);
            assert!(open_container_open_opacity(0.6, true) > 0.0);
        }
    }
}
