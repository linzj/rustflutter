// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/cupertino/cupertino_activity_indicator_demo.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! Upstream's `CupertinoProgressIndicatorDemo` is a stateless
//! `CupertinoPageScaffold`: a nav bar titled
//! `demoCupertinoActivityIndicatorTitle` over a centred
//! `CupertinoActivityIndicator`. The framework's [`CupertinoActivityIndicator`]
//! spins itself on the frame clock -- its `advance` asks for the next frame,
//! and the frame walk honours that for any mounted element -- so this slug
//! animates without an entry in `app.rs`'s `ANIMATED_DEMOS` (that list feeds
//! the `SpinnerValue` the material progress demo reads, which this demo does
//! not use).
//!
//! Divergences, each also marked at its site:
//!
//! - **the scaffold is a fixed height.** Upstream's `DemoWrapper`
//!   (`lib/pages/demo.dart`) gives the demo the page's content height, so the
//!   indicator is centred in a screen-sized area; the demo page here renders
//!   each stage in a scrolling column at its intrinsic height, so the scaffold
//!   gets [`DEMO_HEIGHT`] to stand in for the screen's remainder -- the same
//!   trade `material/bottom_app_bar_demo.rs` makes.

use rustflutter::prelude::*;
use rustflutter::widgets::Center;

use crate::l10n::gallery_localizations::GalleryLocalizations;

/// The height the scaffold stands in for: the demo page's content height,
/// which upstream's `DemoWrapper` computes from the `MediaQuery` and this
/// port's page does not hand down. About a phone's content area at the
/// default window size.
const DEMO_HEIGHT: f32 = 700.0;

/// The demo body for the `cupertino-activity-indicator` slug: upstream's
/// `CupertinoProgressIndicatorDemo.build`.
pub(super) fn stage() -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    // Upstream's `Center(child: CupertinoActivityIndicator())`.
    let body = single(stateful(CupertinoActivityIndicator::new()), |indicator| {
        Box::new(Center::new(indicator))
    });
    let scaffold = component(
        CupertinoPageScaffold::new(body).with_navigation_bar(component(
            // `automaticallyImplyLeading: false` is the default here: the
            // framework's bar shows a back chevron only when asked.
            CupertinoNavigationBar::new()
                .with_middle(l10n.demo_cupertino_activity_indicator_title()),
        )),
    );
    // Upstream's `DemoWrapper` wraps every demo in a light `CupertinoTheme`
    // (`lib/pages/demo.dart`), whatever the app's brightness.
    provide(
        CupertinoTheme::light(),
        single(scaffold, |scaffold| {
            // The fixed height stands in for the content height; see the
            // module header.
            Box::new(
                Container::new()
                    .with_height(DEMO_HEIGHT)
                    .with_child(scaffold),
            )
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflutter::framework::ElementTree;
    use rustflutter::render::{BoxConstraints, RenderBox, Size};

    fn lay_out(widget: AnyWidget, width: f32) -> Size {
        let mut tree = ElementTree::new();
        tree.rebuild(widget);
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::new(0.0, width, 0.0, f32::INFINITY))
    }

    #[test]
    fn the_stage_is_a_scaffold_at_the_stand_in_height() {
        let size = lay_out(stage(), 428.0);
        assert_eq!(size.height, DEMO_HEIGHT);
        assert_eq!(size.width, 428.0);
    }

    #[test]
    fn the_indicator_asks_for_frames_on_its_own() {
        // Why this slug needs no `ANIMATED_DEMOS` entry: the mounted
        // indicator's advance keeps the frame clock coming.
        let indicator = CupertinoActivityIndicator::new();
        let mut state = rustflutter::cupertino::CupertinoActivityIndicatorState::default();
        assert!(indicator.advance(&mut state, 0));
        assert!(indicator.advance(&mut state, 16_000));
    }
}
