// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/cupertino/cupertino_tab_bar_demo.dart` (flutter/
//! gallery @ d12640d), aligned with upstream.
//!
//! Upstream's `CupertinoTabBarDemo` is a `CupertinoTabScaffold` with three
//! tabs -- Home, Chat, Profile (`cupertinoTabBarHomeTab` and friends) -- each
//! tab a `CupertinoTabView` holding a `_CupertinoDemoTab`: a page scaffold
//! with an empty navigation bar and the tab's icon centered at size 100. All
//! of that is here as one per-demo [`StatefulComponent`] holding the selected
//! index, which upstream's scaffold carries in its own `CupertinoTabController`.
//!
//! Divergences, each commented at its site as well:
//!
//! * **No icon font.** Upstream's icons are `CupertinoIcons` codepoints; the
//!   framework tier has no icon font (cupertino.rs's module docs), so the tab
//!   bar's icons are the caller-supplied marks [`CupertinoTabItem`] documents
//!   and the centered 100px icon is drawn ([`TabGlyph`]), the way the tier
//!   draws its back chevron.
//! * **No per-tab navigators.** Upstream's `CupertinoTabView` is a `Navigator`
//!   per tab; the framework's `CupertinoTabScaffold` is the layout only, so
//!   the body is the selected tab's page directly.
//! * Restoration (`restorationId`/`restorationScopeId`, `defaultTitle`) is
//!   not carried: nothing here restores.

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::render::{BoxConstraints, Offset, PaintContext, RenderBox, Size};
use rustflutter::widgets::Center;

use crate::app::{ids, GalleryState};

/// The hit-test id of the tab bar's first item; the rest follow consecutively
/// (`CupertinoTabBar`'s rule). From the demo-local block (PORTING.md: fixed
/// bases, no counters).
const TAB_BAR_FIRST: u64 = ids::DEMO_LOCAL;

/// The demo body for the `cupertino-tab-bar` slug.
///
/// `state` is read for the resolved brightness only: upstream's demo runs
/// under the app's `CupertinoTheme`, which the gallery derives from the
/// options' brightness, so the same theme is provided over the stage here.
pub(super) fn stage(state: &GalleryState) -> AnyWidget {
    let theme = match state.options.resolved_brightness() {
        Brightness::Light => CupertinoTheme::light(),
        Brightness::Dark => CupertinoTheme::dark(),
    };
    provide(theme, stateful(CupertinoTabBarDemo))
}

/// Which tab. Upstream carries an `IconData`; with no icon font here (see the
/// module header) the kind says what [`TabGlyph`] draws and what mark the tab
/// bar shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TabKind {
    /// `CupertinoIcons.home`.
    Home,
    /// `CupertinoIcons.conversation_bubble`.
    Chat,
    /// `CupertinoIcons.profile_circled`.
    Profile,
}

/// Upstream's `_TabInfo`: the tab's title and icon.
struct TabInfo {
    title: &'static str,
    kind: TabKind,
    /// The one-character mark the tab bar draws in the icon's slot, standing
    /// in for the `CupertinoIcons` glyph (see the module header).
    mark: &'static str,
}

/// Upstream's `tabInfo` list: `cupertinoTabBarHomeTab`, `cupertinoTabBarChatTab`
/// and `cupertinoTabBarProfileTab` resolve to "Home", "Chat" and "Profile".
const TABS: [TabInfo; 3] = [
    TabInfo {
        title: "Home",
        kind: TabKind::Home,
        mark: "H",
    },
    TabInfo {
        title: "Chat",
        kind: TabKind::Chat,
        mark: "C",
    },
    TabInfo {
        title: "Profile",
        kind: TabKind::Profile,
        mark: "P",
    },
];

/// Upstream's `CupertinoTabBarDemo`.
struct CupertinoTabBarDemo;

/// The selected tab. Upstream's scaffold keeps this in its own
/// `CupertinoTabController` (default 0); with the framework's scaffold being
/// the layout only, the demo holds it.
#[derive(Default)]
struct TabBarDemoState {
    selected: usize,
}

impl StatefulComponent for CupertinoTabBarDemo {
    type State = TabBarDemoState;

    fn build(
        &self,
        state: &TabBarDemoState,
        handle: StateHandle<TabBarDemoState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let items = TABS
            .iter()
            .map(|tab| CupertinoTabItem::new(tab.title, tab.mark))
            .collect();
        // Upstream's `CupertinoTabBar(items: ...)`; which item is active is
        // the demo's state here (see the module header).
        let tab_bar = component(
            CupertinoTabBar::new(TAB_BAR_FIRST, items, state.selected).wired(handle, |s, index| {
                s.selected = index;
            }),
        );
        // Upstream's `tabBuilder` -> `CupertinoTabView` -> `_CupertinoDemoTab`;
        // the per-tab navigator is not carried (see the module header).
        let body = component(CupertinoDemoTab {
            index: state.selected,
        });

        // Upstream's `DefaultTextStyle(CupertinoTheme.of(context).textTheme.
        // textStyle, ...)` covers the tab labels, which the framework's tab
        // bar styles itself out of the same theme.
        component(CupertinoTabScaffold::new(tab_bar, body))
    }
}

/// Upstream's `_CupertinoDemoTab`: a page scaffold with an empty navigation
/// bar, a `systemBackground` body, and the tab's icon centered at 100.
struct CupertinoDemoTab {
    index: usize,
}

impl Component for CupertinoDemoTab {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let tab = &TABS[self.index];
        // Upstream's `Icon(icon, size: 100)` takes the ambient icon color,
        // which under the Cupertino theme is the label color.
        let color = theme.resolve(CupertinoColors::LABEL);
        let kind = tab.kind;
        let icon = single(
            leaf(move || TabGlyph {
                kind,
                color,
                laid_out: Size::ZERO,
            }),
            |glyph| Box::new(Center::new(glyph)),
        );
        component(
            CupertinoPageScaffold::new(icon)
                // Upstream's `navigationBar: const CupertinoNavigationBar()`:
                // a bare bar, no middle.
                .with_navigation_bar(component(CupertinoNavigationBar::new()))
                .with_background_color(theme.resolve(CupertinoColors::SYSTEM_BACKGROUND)),
        )
    }
}

/// The tab's icon, drawn. Upstream's glyphs are `CupertinoIcons` codepoints;
/// with no icon font here (see the module header) each is a few strokes and
/// circles on the canvas, the way cupertino.rs draws its back chevron. The
/// shapes trace the glyphs' outlines at 100px, not the font's geometry.
struct TabGlyph {
    kind: TabKind,
    color: Color,
    laid_out: Size,
}

impl TabGlyph {
    /// The stroke width at a drawn size: 5 at 100, scaling down.
    fn stroke(size: f32) -> f32 {
        (size / 20.0).max(1.5)
    }
}

/// Paints the arc of the circle at (`cx`, `cy`) with radius `r` between the
/// given angles (degrees, 0 at the start edge, positive downward) as a
/// polyline: the canvas has no arc primitive, and twelve segments read as
/// round at this size.
fn stroke_arc(
    canvas: &mut rustflutter::engine::Canvas,
    cx: f32,
    cy: f32,
    r: f32,
    from_degrees: f32,
    to_degrees: f32,
    paint: &Paint,
) {
    const SEGMENTS: usize = 12;
    let point = |i: usize| {
        let angle =
            (from_degrees + (to_degrees - from_degrees) * i as f32 / SEGMENTS as f32).to_radians();
        (cx + r * angle.cos(), cy + r * angle.sin())
    };
    for i in 0..SEGMENTS {
        canvas.draw_line(point(i), point(i + 1), paint);
    }
}

impl RenderBox for TabGlyph {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Upstream's `size: 100`.
        self.laid_out = constraints.constrain(Size::new(100.0, 100.0));
        self.laid_out
    }

    fn size(&self) -> Size {
        self.laid_out
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let s = self.laid_out.width.min(self.laid_out.height);
        let paint = Paint::new(self.color)
            .with_style(Style::Stroke {
                width: Self::stroke(s),
            })
            .with_stroke_cap(StrokeCap::Round);
        let canvas = context.canvas();
        // All coordinates are fractions of the drawn size, the glyph's box.
        let x = |f: f32| offset.dx + f * s;
        let y = |f: f32| offset.dy + f * s;
        match self.kind {
            // `CupertinoIcons.home`: a roof over a square body.
            TabKind::Home => {
                canvas.draw_line((x(0.15), y(0.48)), (x(0.5), y(0.13)), &paint);
                canvas.draw_line((x(0.5), y(0.13)), (x(0.85), y(0.48)), &paint);
                canvas.draw_line((x(0.24), y(0.44)), (x(0.24), y(0.87)), &paint);
                canvas.draw_line((x(0.24), y(0.87)), (x(0.76), y(0.87)), &paint);
                canvas.draw_line((x(0.76), y(0.87)), (x(0.76), y(0.44)), &paint);
            }
            // `CupertinoIcons.conversation_bubble`: a rounded bubble with a
            // tail at the lower start corner.
            TabKind::Chat => {
                canvas.draw_rounded_rect(
                    Rect::ltrb(x(0.16), y(0.2), x(0.84), y(0.64)),
                    0.1 * s,
                    &paint,
                );
                canvas.draw_line((x(0.34), y(0.64)), (x(0.26), y(0.84)), &paint);
            }
            // `CupertinoIcons.profile_circled`: a circle with a head and
            // shoulders inside it.
            TabKind::Profile => {
                canvas.draw_circle(x(0.5), y(0.5), 0.38 * s, &paint);
                canvas.draw_circle(x(0.5), y(0.4), 0.11 * s, &paint);
                // The shoulders: the top arc of a circle centered below the
                // glyph's middle, entering at the outer circle's sides.
                stroke_arc(canvas, x(0.5), y(0.98), 0.34 * s, 215.0, 325.0, &paint);
            }
        }
    }

    fn hit_test_self(&self, _position: Offset) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tabs_are_upstreams() {
        // `cupertinoTabBarHomeTab` / `cupertinoTabBarChatTab` /
        // `cupertinoTabBarProfileTab`, in order.
        let titles: Vec<&str> = TABS.iter().map(|tab| tab.title).collect();
        assert_eq!(titles, ["Home", "Chat", "Profile"]);
        let kinds: Vec<TabKind> = TABS.iter().map(|tab| tab.kind).collect();
        assert_eq!(kinds, [TabKind::Home, TabKind::Chat, TabKind::Profile]);
    }

    #[test]
    fn the_first_tab_is_selected_initially() {
        // Upstream's `CupertinoTabController` default.
        assert_eq!(TabBarDemoState::default().selected, 0);
    }
}
