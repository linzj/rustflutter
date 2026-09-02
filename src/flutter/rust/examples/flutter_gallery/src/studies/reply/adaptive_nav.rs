// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/reply/adaptive_nav.dart` (flutter/gallery @
//! d12640d): the study's chrome -- the mailbox under a notched bottom app bar
//! with the compose button docked into the notch.
//!
//! Upstream's `AdaptiveNav` branches on `isDisplayDesktop` into `_DesktopNav`
//! (a navigation rail beside a two-pane list/detail) and `_MobileNav`. **Only
//! the mobile arm is here**, at every width: the desktop arm is a second
//! navigator, a rail and a mail-view pane, none of which exist yet, and a
//! half-built rail beside an empty pane would read as broken rather than as
//! unported.
//!
//! What the mobile arm does render, and what it leaves out:
//!
//! * **The bar and its notch are upstream's.** The waterfall notch is drawn
//!   from `waterfall_notched_rectangle.rs`'s path -- the Bezier maths was
//!   already ported and this is its first reader. The bar is painted as a
//!   filled path rather than clipped, which is what lets the bite be real:
//!   `bottom_app_bar_demo.rs` fakes its circular notch with a background
//!   coloured circle because the framework's clips are rectangles, and a
//!   painter has no such limit.
//! * **The drawer does not open.** Upstream's arrow toggles a bottom drawer
//!   of the six mailboxes and six folders, opening 40% on the first tap and
//!   flinging fully open on the second. `bottom_drawer.rs` is still a
//!   skeleton, and its six destination icons (`reply/icons/twotone_*.png`)
//!   are not among the vendored assets. The arrow is drawn, and does nothing.
//! * **The bar does not hide on scroll.** Upstream listens for a
//!   `UserScrollNotification` and runs a `SizeTransition`; the body here does
//!   not scroll yet (see below), so there is nothing to hide for.
//! * **The search button is drawn and inert**, as `compose` is: both lead to
//!   screens this batch does not build.
//! * **The bar does not hide on scroll**, as above; it is pinned to the
//!   bottom of the study's box the way upstream pins it to the window's, with
//!   the mail scrolling under it (`extendBody: true`).

use rustflutter::engine::{Canvas, Color, Paint, Rect};
use rustflutter::framework::{AnyWidget, BuildContext, Component, StateHandle, leaf, many};
use rustflutter::painting::{Image, RenderPath};
use rustflutter::platform::Brightness;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, BoxFit, CrossAxisAlignment, CustomPainter, EdgeInsets, MainAxisSize, RenderFlex,
    RenderPadding, RenderStack, StackPosition,
};
use rustflutter::widgets::{Container, CustomPaint, ImageView, Size};

use crate::app::GalleryState;
use crate::data::demos::{MATERIAL_ICONS, icon};

use super::colors::reply_colors;
use super::mailbox_body::MailboxBody;
use super::theme;
use super::waterfall_notched_rectangle;

/// Upstream's `kToolbarHeight`, the bar's own height.
const BAR_HEIGHT: f32 = 56.0;

/// Upstream's `Padding(padding: EdgeInsetsDirectional.only(top: 2))` above
/// the bar, which is what leaves room for the notch's shoulders.
const BAR_TOP_PADDING: f32 = 2.0;

/// Upstream's `_mobileFabDimension`.
const FAB_SIZE: f32 = 56.0;

/// Upstream's `notchMargin: 6` -- the gap the notch leaves around the button.
const NOTCH_MARGIN: f32 = 6.0;

/// Upstream's `Padding(padding: EdgeInsetsDirectional.only(bottom: 8))`
/// around the mobile FAB.
const FAB_BOTTOM_PADDING: f32 = 8.0;

/// The leading run of the bar: `SizedBox(width: 16)`, the arrow, `8`, the
/// logo, `10`, the mailbox label.
const BAR_LEADING_INSET: f32 = 16.0;
const ARROW_TO_LOGO: f32 = 8.0;
const LOGO_TO_LABEL: f32 = 10.0;

/// Upstream's `_ReplyLogo`: `ImageIcon(size: 32)`.
const LOGO_SIZE: f32 = 32.0;

/// Upstream's `reply/reply_logo.png`, compiled in; see `assets/README.md`.
const LOGO: (&str, &[u8]) = (
    "reply/reply_logo.png",
    include_bytes!("../../../assets/reply/reply_logo.png"),
);

/// One of the bar's icons, in the bundled Material font -- upstream draws
/// these as `Icon`s at the default 24.
///
/// The same `material_icon` `text_field_demo.rs` has. The codepoints are the
/// font's own, which are not upstream's legacy ones: see
/// `data::demos::icon`'s note.
fn material_icon(glyph: &'static str, color: Color) -> rustflutter::render::RenderParagraph {
    Text::new(glyph)
        .with_font_family(MATERIAL_ICONS)
        .with_size(24.0)
        .with_color(color)
}

/// The study's screen: upstream's `_MobileNav`.
pub(crate) fn screen(
    state: &GalleryState,
    handle: StateHandle<GalleryState>,
    brightness: Brightness,
) -> AnyWidget {
    let store = &state.study.reply;
    let emails = store.selected_mailbox_emails();
    let destination = store.selected_mailbox_page.name();
    // Upstream's bar label is the selected destination's `textLabel`, which
    // for the six mailboxes is the name with a capital.
    let label = store.selected_mailbox_page.label();

    let body = rustflutter::framework::component(MailboxBody {
        emails,
        destination,
        offset: state.study.reply_scroll.offset,
        link: state.study.reply_scroll.link(),
        handle: handle.clone(),
    });
    let bar = rustflutter::framework::component(BottomBar { label, brightness });

    // Upstream's `Scaffold(extendBody: true, bottomNavigationBar:,
    // floatingActionButton:, floatingActionButtonLocation: centerDocked)`.
    // Composed here rather than asked of a `Scaffold`, because the docked
    // location is a `Scaffold` layout delegate upstream and this port's
    // `Scaffold` has no floating-action-button slot -- the same composition
    // `bottom_app_bar_demo.rs` makes.
    many(vec![body, bar], move |mut rendered| {
        let bar = rendered.pop().expect("the bar");
        let body = rendered.pop().expect("the body");
        Box::new(
            RenderStack::new()
                .with_fit(rustflutter::render::StackFit::Expand)
                // The mail fills the box and scrolls under the bar --
                // upstream's `extendBody: true`.
                .push(body)
                .push_positioned(
                    bar,
                    StackPosition {
                        left: Some(0.0),
                        right: Some(0.0),
                        bottom: Some(0.0),
                        ..StackPosition::default()
                    },
                ),
        )
    })
}

/// Upstream's `_AnimatedBottomAppBar`, without the animations it is named for
/// (see the module header): the notched bar, its leading run and the docked
/// compose button.
struct BottomBar {
    label: &'static str,
    brightness: Brightness,
}

impl Component for BottomBar {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = super::app::reply_theme_of(context);
        let fill = theme::bottom_app_bar(self.brightness);
        let label = self.label;
        let label_style = TextStyle {
            // Upstream's `bodyLarge`, in white whatever the brightness.
            font_size: 18.0,
            color: reply_colors::WHITE50,
            font_family: theme.font_family.map(str::to_string),
            ..TextStyle::default()
        };
        let fab_fill = theme::secondary(self.brightness);
        let fab_icon = theme::on_secondary();

        leaf(move || {
            // The whole strip: the bar's height plus the shoulder padding,
            // with the button standing half out of the top.
            let strip_height = BAR_TOP_PADDING + BAR_HEIGHT;

            let painter: std::rc::Rc<dyn CustomPainter> = std::rc::Rc::new(NotchedBar { fill });

            // Upstream's leading `Row`: 16, the drop arrow, 8, the logo, 10,
            // the mailbox label. The arrow is `Icons.arrow_drop_up`.
            let mut leading = RenderFlex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .push(Container::new().with_size(BAR_LEADING_INSET, 1.0))
                .push(material_icon(icon::ARROW_DROP_UP, reply_colors::WHITE50))
                .push(Container::new().with_size(ARROW_TO_LOGO, 1.0));
            let mut logo = Container::new().with_size(LOGO_SIZE, LOGO_SIZE);
            if let Some(image) = Image::shared(LOGO.0, LOGO.1) {
                logo = logo.with_child(ImageView::with_fit(image, BoxFit::Contain));
            }
            leading = leading
                .push(logo)
                .push(Container::new().with_size(LOGO_TO_LABEL, 1.0))
                .push(Text::new(label).with_style(label_style.clone()));

            // The trailing search button, drawn and inert.
            let search = material_icon(icon::SEARCH, reply_colors::WHITE50);

            let row = RenderFlex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .push(leading)
                .push_flex(rustflutter::render::FlexChild::expanded(
                    rustflutter::render::RenderRef::new(Container::new()),
                    1,
                ))
                .push(search)
                .push(Container::new().with_size(BAR_LEADING_INSET, 1.0));

            // The compose button, docked centre: a circle standing so its
            // middle is on the bar's top edge, which is where the notch is cut.
            let fab = Container::new()
                .with_size(FAB_SIZE, FAB_SIZE)
                .with_color(fab_fill)
                .with_corner_radius(FAB_SIZE / 2.0)
                .with_child(rustflutter::widgets::Center::new(material_icon(
                    icon::CREATE,
                    fab_icon,
                )));

            // The bar: one box of a known height with the notch painted
            // behind its contents. Every size here is definite, which is what
            // lets the painter be handed the box it thinks it has -- it draws
            // the host rectangle from `size`, so a stack that sized itself to
            // its children would have it drawing the wrong shape.
            let bar = CustomPaint::new(
                Container::new()
                    .with_size(f32::INFINITY, strip_height)
                    .with_padding(EdgeInsets::only(0.0, BAR_TOP_PADDING, 0.0, 0.0))
                    .with_child(row),
            )
            .with_painter(painter);

            // How far the button stands above the strip. The whole thing is
            // that much taller than the bar, and the button is docked so its
            // middle sits on the bar's top edge -- where the notch is cut.
            let overhang = (FAB_SIZE / 2.0 - (BAR_TOP_PADDING + FAB_BOTTOM_PADDING)).max(0.0);

            Container::new()
                .with_size(f32::INFINITY, strip_height + overhang)
                .with_child(
                    RenderStack::new()
                        .with_fit(rustflutter::render::StackFit::Expand)
                        .push_positioned(
                            bar,
                            StackPosition {
                                left: Some(0.0),
                                right: Some(0.0),
                                bottom: Some(0.0),
                                height: Some(strip_height),
                                ..StackPosition::default()
                            },
                        )
                        // Centred and docked: upstream's `centerDocked`.
                        .push_positioned(
                            rustflutter::render::RenderAlign::new(Alignment::new(0.0, -1.0), fab),
                            StackPosition {
                                left: Some(0.0),
                                right: Some(0.0),
                                top: Some(0.0),
                                height: Some(FAB_SIZE),
                                ..StackPosition::default()
                            },
                        ),
                )
        })
    }
}

/// Paints the bar as upstream's `WaterfallNotchedRectangle` fills it: the
/// rectangle with a smooth bite taken out around the docked button.
///
/// A painter rather than a clip. `bottom_app_bar_demo.rs` records why its own
/// notch is faked -- the framework's clip shapes are rectangles and rounded
/// rectangles -- and that limit does not reach a canvas, which takes an
/// arbitrary path.
struct NotchedBar {
    fill: Color,
}

impl CustomPainter for NotchedBar {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        let host = Rect::ltrb(0.0, BAR_TOP_PADDING, size.width, size.height);
        // The guest is the button grown by the notch margin, centred on the
        // bar's top edge -- upstream's `Scaffold` geometry for `centerDocked`.
        let guest_size = FAB_SIZE + NOTCH_MARGIN * 2.0;
        let center_x = size.width / 2.0;
        let guest_top = host.top + FAB_BOTTOM_PADDING - guest_size / 2.0;
        let guest = Rect::ltrb(
            center_x - guest_size / 2.0,
            guest_top,
            center_x + guest_size / 2.0,
            guest_top + guest_size,
        );
        let path: RenderPath = waterfall_notched_rectangle::outer_path(host, Some(guest));
        canvas.draw_path(&path, &Paint::new(self.fill));
    }

    fn kind_id(&self) -> std::any::TypeId {
        std::any::TypeId::of::<NotchedBar>()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn should_repaint(&self, old: &dyn CustomPainter) -> bool {
        // Only the colour can change, and only when the brightness does.
        match old.as_any().downcast_ref::<NotchedBar>() {
            Some(old) => old.fill != self.fill,
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_numbers_are_upstreams() {
        // adaptive_nav.dart: kToolbarHeight 56, 2dp above the bar,
        // notchMargin 6, a 56dp button padded 8 from the bottom, and the
        // leading run 16 / 8 / 10 around a 32dp logo.
        assert_eq!(BAR_HEIGHT, 56.0);
        assert_eq!(BAR_TOP_PADDING, 2.0);
        assert_eq!(NOTCH_MARGIN, 6.0);
        assert_eq!(FAB_SIZE, 56.0);
        assert_eq!(FAB_BOTTOM_PADDING, 8.0);
        assert_eq!(BAR_LEADING_INSET, 16.0);
        assert_eq!(ARROW_TO_LOGO, 8.0);
        assert_eq!(LOGO_TO_LABEL, 10.0);
        assert_eq!(LOGO_SIZE, 32.0);
    }

    #[test]
    fn the_notch_is_cut_where_the_button_stands() {
        // The guest the painter hands the Bezier maths is the button grown by
        // the notch margin: 56 + 6 + 6.
        let guest = FAB_SIZE + NOTCH_MARGIN * 2.0;
        assert_eq!(guest, 68.0);
        // And the path it produces has a bite: the notch's radius is half the
        // guest, so the arc reaches 34 either side of centre.
        let host = Rect::ltrb(0.0, 0.0, 400.0, 56.0);
        let guest_rect = Rect::ltrb(166.0, -26.0, 234.0, 42.0);
        let points = waterfall_notched_rectangle::notch_points(host, Some(guest_rect))
            .expect("the button overlaps the bar, so there is a notch");
        assert_eq!(points.notch_radius, 34.0);
    }
}
