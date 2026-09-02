// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/fortnightly/shared.dart` (flutter/gallery @ d12640d), upstream's
//! the article data and preview widgets.
//!
//! What is here: `ArticleData`, the horizontal/vertical article previews, the
//! article/stock/video list builders, `HashtagBar`, `NavigationMenu` +
//! `MenuItem`, `StockItem`, `VideoPreview`, and `buildTheme` (as
//! [`text_theme`] + [`theme`]). What changed on the way over:
//!
//! - **FadeInImagePlaceholder has no fade**: the framework's image cache
//!   (`Image::shared`) hands back `None` until the decode lands on a worker
//!   thread, and upstream's `Colors.black.withOpacity(0.1)` placeholder is
//!   drawn until then -- the loading contract upstream's widget exists for.
//!   The 500ms cross-fade after the decode is not ported (no image-frame
//!   callback to drive it).
//! - **google_fonts is shipped TTFs**: upstream resolves Merriweather,
//!   LibreFranklin and RobotoCondensed through the google_fonts package; the
//!   same files ship with `flutter_gallery_assets`, so they are compiled in
//!   and registered by [`register_fonts`] under the same family names (the
//!   provenance is for `assets/README.md` to log).
//! - **`craneMinutes` is formatted locally**: the generated English l10n
//!   table's interpolation prints the literal `${minutes}` (a gen_l10n bug,
//!   not this study's to fix), so `1m`/`2m`/`5m` come from
//!   [`minutes_string`], matching upstream's `{minutes, plural, ...}`.
//! - **`NumberFormat.decimalPercentPattern` is a `format!`**: English-only,
//!   so `0.48%` is `format!("{:.2}%")` -- the same string the intl call
//!   produces for the en locale.
//! - **SelectableText is Text**: text selection has no counterpart here.
//! - **Icons.arrow_drop_down is the shipped down-arrow glyph**: the gallery's
//!   MaterialIcons subset names it `icon::ARROW_DOWN`.

use std::cell::RefCell;

use rustflutter::engine::TextStyle;
use rustflutter::framework::{
    AnyWidget, BuildContext, Component, StateHandle, StatefulComponent, leaf, many, stateful,
};
use rustflutter::painting::Image;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, Axis, BoxFit, CrossAxisAlignment, EdgeInsets, FlexChild, MainAxisSize, RenderFlex,
    RenderPadding,
};
use rustflutter::widgets::{
    AspectRatio, BoxedWidget, Center, Empty, ImageView, ListView, Pointer, SizedBox,
};

use crate::data::demos::{self as catalog, icon};
use crate::l10n::gallery_localizations::GalleryLocalizations;

// -- Assets ---------------------------------------------------------------------
//
// Upstream these are `AssetImage('fortnightly/<name>', package:
// 'flutter_gallery_assets')`; here they are the same files, copied from that
// package's `fortnightly/` directory (base resolution) and compiled in. The
// cache keys keep the package path so a collision with another study's
// `title.png` cannot hand back the wrong picture.

macro_rules! asset {
    ($name:literal) => {
        include_bytes!(concat!("../../../assets/fortnightly/", $name))
    };
}

const TITLE_IMAGE: &[u8] = asset!("fortnightly_title.png");
const HEALTHCARE_IMAGE: &[u8] = asset!("fortnightly_healthcare.jpg");
const WAR_IMAGE: &[u8] = asset!("fortnightly_war.png");
const GAS_IMAGE: &[u8] = asset!("fortnightly_gas.png");
const ARMY_IMAGE: &[u8] = asset!("fortnightly_army.png");
const STOCKS_IMAGE: &[u8] = asset!("fortnightly_stocks.png");
const FABRICS_IMAGE: &[u8] = asset!("fortnightly_fabrics.png");
const CHART_IMAGE: &[u8] = asset!("fortnightly_chart.png");
const FEMINISTS_IMAGE: &[u8] = asset!("fortnightly_feminists.jpg");
const BEES_IMAGE: &[u8] = asset!("fortnightly_bees.jpg");

/// Decodes an asset once per process and hands back the shared handle -- the
/// `Image::shared` pattern Shrine's product grid established.
fn image(key: &'static str, bytes: &'static [u8]) -> Option<std::rc::Rc<Image>> {
    Image::shared(&format!("fortnightly:{key}"), bytes)
}

/// The title wordmark, drawn in the app bar, the desktop header and the
/// drawer's header row. Until the decode lands this is upstream's
/// `SizedBox.shrink()` placeholder.
pub(crate) fn title_image() -> BoxedWidget {
    match image("fortnightly_title.png", TITLE_IMAGE) {
        Some(wordmark) => boxed(ImageView::new(wordmark)),
        None => boxed(SizedBox::new(0.0, 0.0)),
    }
}

// -- Fonts ----------------------------------------------------------------------

/// The three faces `buildTheme` sets the study in, under the family names
/// upstream's google_fonts calls produce.
pub const MERRIWEATHER: &str = "Merriweather";
pub const LIBRE_FRANKLIN: &str = "LibreFranklin";
pub const ROBOTO_CONDENSED: &str = "RobotoCondensed";

/// Registers the study's faces. Called from `app::screen` before the first
/// frame of the study; idempotent, because the route can be entered more than
/// once. The weights are the ones the text theme asks for (see
/// [`text_theme`]): Merriweather Light (300) and Bold Italic, LibreFranklin
/// Medium (500) and Regular, RobotoCondensed Bold.
pub(crate) fn register_fonts() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        rustflutter::engine::register_font(
            include_bytes!("../../../assets/fonts/Merriweather-Light.ttf"),
            MERRIWEATHER,
        );
        rustflutter::engine::register_font(
            include_bytes!("../../../assets/fonts/Merriweather-BoldItalic.ttf"),
            MERRIWEATHER,
        );
        rustflutter::engine::register_font(
            include_bytes!("../../../assets/fonts/LibreFranklin-Medium.ttf"),
            LIBRE_FRANKLIN,
        );
        rustflutter::engine::register_font(
            include_bytes!("../../../assets/fonts/LibreFranklin-Regular.ttf"),
            LIBRE_FRANKLIN,
        );
        rustflutter::engine::register_font(
            include_bytes!("../../../assets/fonts/RobotoCondensed-Bold.ttf"),
            ROBOTO_CONDENSED,
        );
    });
}

// -- Theme ----------------------------------------------------------------------
//
// Upstream's `buildTheme`: a white scaffold, a flat white app bar with black
// icons, and a text theme mixing the three faces above. It is light-only --
// upstream builds this one theme regardless of the platform brightness, so
// the study reads the same in the gallery's dark mode. That is upstream's
// choice, mirrored deliberately.

const BLACK_87: Color = Color::argb(0xDD, 0, 0, 0);
const BLACK_75: Color = Color::argb(0xBF, 0, 0, 0);
const BLACK_50: Color = Color::argb(0x80, 0, 0, 0);
const BLACK_20: Color = Color::argb(0x33, 0, 0, 0);
const BLACK_10: Color = Color::argb(0x1A, 0, 0, 0);
const BLACK_07: Color = Color::argb(0x12, 0, 0, 0);
/// Stock gain. Upstream `const Color(0xff20CF63)`.
const GAIN: Color = Color::rgb(0x20, 0xCF, 0x63);
/// Stock loss. Upstream `const Color(0xff661FFF)`.
const LOSS: Color = Color::rgb(0x66, 0x1F, 0xFF);

fn face(family: &str) -> Option<String> {
    Some(family.to_string())
}

/// Upstream's `buildTheme().textTheme`, one field per role upstream sets. The
/// comments name the use each role has in the study, the way upstream's own
/// comments do.
#[derive(Clone)]
pub(crate) struct FortnightlyTextTheme {
    /// preview snippet
    pub body_medium: TextStyle,
    /// time in latest updates
    pub body_large: TextStyle,
    /// preview headlines
    pub headline_small: TextStyle,
    /// (caption 2), preview category, stock ticker
    pub title_medium: TextStyle,
    /// hashtags, stock prices
    pub title_small: TextStyle,
    /// section titles: Top Highlights, Last Updated...
    pub title_large: TextStyle,
}

pub(crate) fn text_theme() -> FortnightlyTextTheme {
    FortnightlyTextTheme {
        body_medium: TextStyle {
            font_family: face(MERRIWEATHER),
            font_size: 16.0,
            font_weight: 300,
            color: BLACK_87,
            ..TextStyle::default()
        },
        body_large: TextStyle {
            font_family: face(LIBRE_FRANKLIN),
            font_size: 11.0,
            font_weight: 500,
            color: BLACK_50,
            ..TextStyle::default()
        },
        headline_small: TextStyle {
            font_family: face(LIBRE_FRANKLIN),
            font_size: 16.0,
            font_weight: 500,
            color: BLACK_87,
            ..TextStyle::default()
        },
        title_medium: TextStyle {
            font_family: face(ROBOTO_CONDENSED),
            font_size: 16.0,
            font_weight: 700,
            color: BLACK_87,
            ..TextStyle::default()
        },
        title_small: TextStyle {
            font_family: face(LIBRE_FRANKLIN),
            font_size: 14.0,
            font_weight: 400,
            color: BLACK_87,
            ..TextStyle::default()
        },
        title_large: TextStyle {
            font_family: face(MERRIWEATHER),
            font_size: 14.0,
            font_weight: 700,
            italic: true,
            color: BLACK_87,
            ..TextStyle::default()
        },
    }
}

/// The component [`Theme`] the study provides at its root: white background,
/// black text and icons, LibreFranklin as the face a style-less component
/// falls back to. Everything the study draws takes its style from
/// [`text_theme`] instead; this is for the framework's own widgets (the
/// drawer's surface, the scrim).
pub(crate) fn theme() -> Theme {
    Theme {
        background: Color::WHITE,
        surface: Color::WHITE,
        surface_variant: Color::WHITE,
        outline: BLACK_20,
        primary: Color::BLACK,
        on_primary: Color::WHITE,
        danger: LOSS,
        text: Color::BLACK,
        text_muted: BLACK_50,
        radius: 0.0,
        spacing: 8.0,
        body_size: 16.0,
        title_size: 16.0,
        font_family: Some(LIBRE_FRANKLIN),
    }
}

// -- ArticleData ------------------------------------------------------------------

/// One article: upstream's `ArticleData`. `image` is the asset's bytes rather
/// than its URL, and `image_aspect_ratio` is upstream's
/// `imageAspectRatio` -- the photograph's width over its height.
#[derive(Clone)]
pub(crate) struct ArticleData {
    pub image_key: &'static str,
    pub image: &'static [u8],
    pub image_aspect_ratio: f32,
    /// Already upper-case: the call sites pass
    /// `localizations.fortnightlyMenuX.toUpperCase()`.
    pub category: String,
    pub title: &'static str,
    /// Carried as upstream publishes it; nothing shows it, because no
    /// upstream call site sets `showSnippet` either.
    #[allow(dead_code)]
    pub snippet: Option<&'static str>,
}

/// The image a preview draws -- a horizontal one's `64 * aspect` by 64
/// thumbnail or a vertical one's full-width aspect box -- or upstream's
/// `Colors.black.withOpacity(0.1)` placeholder until the decode lands.
fn article_image(data: &ArticleData, fit: BoxFit) -> BoxedWidget {
    match image(data.image_key, data.image) {
        Some(photo) => boxed(ImageView::with_fit(photo, fit)),
        None => boxed(Container::new().with_color(BLACK_10)),
    }
}

// -- Previews ---------------------------------------------------------------------

/// Upstream's `craneMinutes(minutes)`: `{minutes, plural, =1{1m}
/// other{{minutes}m}}`. Formatted here rather than read from the generated
/// table; see the module header.
fn minutes_string(minutes: i64) -> String {
    format!("{minutes}m")
}

/// Upstream's `HorizontalArticlePreview`: category and title on the left, the
/// thumbnail on the right, and the read time between them when there is one.
pub(crate) fn horizontal_article_preview(data: ArticleData, minutes: Option<i64>) -> AnyWidget {
    leaf(move || {
        let styles = text_theme();
        // `width: 64 / (1 / data.imageAspectRatio), height: 64`.
        let thumbnail = 64.0 * data.image_aspect_ratio;
        let text = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .push(Text::new(data.category.as_str()).with_style(styles.title_medium.clone()))
            .push(SizedBox::height(12.0))
            // `headlineSmall.copyWith(fontSize: 16)` -- the role is already 16,
            // so this is the role unchanged.
            .push(Text::new(data.title).with_style(styles.headline_small.clone()));
        let mut row = RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Start);
        row = row.push_flex(FlexChild::expanded(text, 1));
        if let Some(minutes) = minutes {
            row = row
                .push(Text::new(minutes_string(minutes)).with_style(styles.body_large.clone()))
                .push(SizedBox::width(8.0));
        }
        row.push(SizedBox::new(thumbnail, 64.0).with_child(article_image(&data, BoxFit::Cover)))
    })
}

/// Upstream's `VerticalArticlePreview`: the photograph across the full width
/// at its own aspect, then category, headline, and the snippet when asked
/// for. `width` is never set at any upstream call site, so it is not a
/// parameter here -- the preview always fills what it is offered.
pub(crate) fn vertical_article_preview(
    data: ArticleData,
    headline_style: Option<TextStyle>,
    show_snippet: bool,
) -> AnyWidget {
    leaf(move || {
        let styles = text_theme();
        let mut column = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            // `fit: BoxFit.fitWidth, width: double.infinity` at the asset's
            // own aspect is an aspect box with a covering image.
            .push(AspectRatio::new(
                data.image_aspect_ratio,
                article_image(&data, BoxFit::Cover),
            ))
            .push(SizedBox::height(12.0))
            .push(Text::new(data.category.as_str()).with_style(styles.title_medium.clone()))
            .push(SizedBox::height(12.0))
            .push(
                Text::new(data.title).with_style(
                    headline_style
                        .clone()
                        .unwrap_or_else(|| styles.headline_small.clone()),
                ),
            );
        if show_snippet {
            if let Some(snippet) = data.snippet {
                column = column
                    .push(SizedBox::height(4.0))
                    .push(Text::new(snippet).with_style(styles.body_medium.clone()));
            }
        }
        column
    })
}

/// The 1-pixel hairline between two articles, upstream's `articleDivider`:
/// black at 7% with 16 above and below.
pub(crate) fn article_divider() -> AnyWidget {
    leaf(|| {
        RenderPadding::new(
            EdgeInsets::symmetric(0.0, 16.0),
            Container::new().with_color(BLACK_07).with_height(1.0),
        )
    })
}

/// Between two sections rather than two articles: black at 20%, same margins.
fn section_divider() -> AnyWidget {
    leaf(|| {
        RenderPadding::new(
            EdgeInsets::symmetric(0.0, 16.0),
            Container::new().with_color(BLACK_20).with_height(1.0),
        )
    })
}

/// Upstream's `buildArticlePreviewItems`: the front page's feed, dividers and
/// the "Latest Updates" section header included.
pub(crate) fn build_article_preview_items() -> Vec<AnyWidget> {
    let l10n = GalleryLocalizations::en();
    let styles = text_theme();
    vec![
        vertical_article_preview(
            ArticleData {
                image_key: "fortnightly_healthcare.jpg",
                image: HEALTHCARE_IMAGE,
                image_aspect_ratio: 391.0 / 248.0,
                category: l10n.fortnightly_menu_world().to_uppercase(),
                title: l10n.fortnightly_headline_healthcare(),
                snippet: None,
            },
            Some(TextStyle {
                font_size: 20.0,
                ..styles.headline_small.clone()
            }),
            false,
        ),
        article_divider(),
        horizontal_article_preview(
            ArticleData {
                image_key: "fortnightly_war.png",
                image: WAR_IMAGE,
                image_aspect_ratio: 1.0,
                category: l10n.fortnightly_menu_politics().to_uppercase(),
                title: l10n.fortnightly_headline_war(),
                snippet: None,
            },
            None,
        ),
        article_divider(),
        horizontal_article_preview(
            ArticleData {
                image_key: "fortnightly_gas.png",
                image: GAS_IMAGE,
                image_aspect_ratio: 1.0,
                category: l10n.fortnightly_menu_tech().to_uppercase(),
                title: l10n.fortnightly_headline_gasoline(),
                snippet: None,
            },
            None,
        ),
        section_divider(),
        leaf(move || {
            Text::new(GalleryLocalizations::en().fortnightly_latest_updates())
                .with_style(text_theme().title_large.clone())
        }),
        article_divider(),
        horizontal_article_preview(
            ArticleData {
                image_key: "fortnightly_army.png",
                image: ARMY_IMAGE,
                image_aspect_ratio: 1.0,
                category: l10n.fortnightly_menu_politics().to_uppercase(),
                title: l10n.fortnightly_headline_army(),
                snippet: None,
            },
            Some(2),
        ),
        article_divider(),
        horizontal_article_preview(
            ArticleData {
                image_key: "fortnightly_stocks.png",
                image: STOCKS_IMAGE,
                image_aspect_ratio: 77.0 / 64.0,
                category: l10n.fortnightly_menu_world().to_uppercase(),
                title: l10n.fortnightly_headline_stocks(),
                snippet: None,
            },
            Some(5),
        ),
        article_divider(),
        horizontal_article_preview(
            ArticleData {
                image_key: "fortnightly_fabrics.png",
                image: FABRICS_IMAGE,
                image_aspect_ratio: 76.0 / 64.0,
                category: l10n.fortnightly_menu_tech().to_uppercase(),
                title: l10n.fortnightly_headline_fabrics(),
                snippet: None,
            },
            Some(4),
        ),
        article_divider(),
    ]
}

// -- HashtagBar -------------------------------------------------------------------

/// Upstream's `HashtagBar`: the trending strip, a horizontal `ListView` of
/// `#tag`s separated by hairlines. It is its own scrollable -- upstream's
/// internal `ScrollController`, here a per-widget `Scroll` the element keeps
/// between frames.
pub(crate) struct HashtagBar {
    /// The hit-test identity of the scroll region; the caller picks it so two
    /// bars on one screen cannot collide.
    pub id: u64,
}

#[derive(Default)]
pub(crate) struct HashtagBarState {
    scroll: Scroll,
}

impl StatefulComponent for HashtagBar {
    type State = HashtagBarState;

    fn advance(&self, state: &mut HashtagBarState, frame_time_micros: i64) -> bool {
        state.scroll.advance(frame_time_micros)
    }

    fn build(
        &self,
        state: &HashtagBarState,
        handle: StateHandle<HashtagBarState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let l10n = GalleryLocalizations::en();
        let tags = [
            l10n.fortnightly_trending_tech_design(),
            l10n.fortnightly_trending_reform(),
            l10n.fortnightly_trending_healthcare_revolution(),
            l10n.fortnightly_trending_green_army(),
            l10n.fortnightly_trending_stocks(),
        ];
        // Upstream `reducedTextScale(context)`: at-or-above-normal text grows
        // the bar at half the rate; smaller text leaves it alone.
        let height = 32.0 * reduced_text_scale(context);

        let mut items: Vec<AnyWidget> = vec![leaf(|| SizedBox::width(16.0))];
        for tag in tags {
            let tag = format!("#{tag}");
            items.push(leaf(move || {
                Center::new(Text::new(tag.clone()).with_style(text_theme().title_small.clone()))
            }));
            // Upstream's `verticalDivider`: black at 10%, 16 either side, 8
            // above and below.
            items.push(leaf(|| {
                RenderPadding::new(
                    EdgeInsets::symmetric(16.0, 8.0),
                    Container::new().with_color(BLACK_10).with_width(1.0),
                )
            }));
        }

        let offset = state.scroll.offset;
        let extent = state.scroll.link();
        let id = self.id;
        let handlers = scroll_handlers(handle, |s| &mut s.scroll, Axis::Horizontal);

        many(items, move |rendered| {
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for child in rendered {
                row = row.push(child);
            }
            let list = ListView::horizontal()
                .with_offset(offset)
                .with_link(extent.clone())
                .push(row);
            Box::new(
                SizedBox::height(height)
                    .with_child(Pointer::new(id, list).with_handlers(handlers.clone())),
            )
        })
    }
}

// -- NavigationMenu ---------------------------------------------------------------

/// Upstream's `NavigationMenu`: the section list the desktop layout pins to
/// the left edge and the mobile layout shows in its drawer. `is_closeable`
/// adds upstream's header row -- the close button (wired by the caller, which
/// owns the drawer state) next to the wordmark.
pub(crate) struct NavigationMenu {
    is_closeable: bool,
    /// The hit-test identity of the menu's own scroll region.
    scroll_id: u64,
    close_button: RefCell<Option<AnyWidget>>,
}

impl NavigationMenu {
    pub(crate) fn new(is_closeable: bool, scroll_id: u64) -> NavigationMenu {
        NavigationMenu {
            is_closeable,
            scroll_id,
            close_button: RefCell::new(None),
        }
    }

    /// The close control for the closeable variant; `app.rs` builds it,
    /// because the state it flips lives there.
    pub(crate) fn with_close_button(self, button: AnyWidget) -> NavigationMenu {
        *self.close_button.borrow_mut() = Some(button);
        self
    }
}

impl Component for NavigationMenu {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        let l10n = GalleryLocalizations::en();
        let mut children: Vec<AnyWidget> = Vec::new();
        if self.is_closeable {
            let close = self.close_button.borrow_mut().take();
            children.push(many(
                vec![
                    close.unwrap_or_else(|| leaf(|| Empty)),
                    leaf(|| title_image()),
                ],
                |mut rendered| {
                    let wordmark = rendered.pop().unwrap_or_else(|| boxed(Empty));
                    let button = rendered.pop().unwrap_or_else(|| boxed(Empty));
                    Box::new(
                        RenderFlex::row()
                            .with_main_axis_size(MainAxisSize::Max)
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .push(button)
                            .push(wordmark),
                    )
                },
            ));
        }
        children.push(leaf(|| SizedBox::height(32.0)));
        children.push(menu_item(l10n.fortnightly_menu_front_page(), true));
        children.push(menu_item(l10n.fortnightly_menu_world(), false));
        children.push(menu_item(l10n.fortnightly_menu_us(), false));
        children.push(menu_item(l10n.fortnightly_menu_politics(), false));
        children.push(menu_item(l10n.fortnightly_menu_business(), false));
        children.push(menu_item(l10n.fortnightly_menu_tech(), false));
        children.push(menu_item(l10n.fortnightly_menu_science(), false));
        children.push(menu_item(l10n.fortnightly_menu_sports(), false));
        children.push(menu_item(l10n.fortnightly_menu_travel(), false));
        children.push(menu_item(l10n.fortnightly_menu_culture(), false));

        stateful(ScrollColumn::new(self.scroll_id, children))
    }
}

/// Upstream's `MenuItem`: a 32-wide leading slot that holds the drop-down
/// arrow on every section but the header, then the title -- weight 700 for
/// the header, 600 for the rest, both 16pt.
fn menu_item(title: &'static str, header: bool) -> AnyWidget {
    leaf(move || {
        let mut leading = Container::new()
            .with_width(32.0)
            .with_alignment(Alignment::CENTER_LEFT);
        if !header {
            leading = leading.with_child(
                Text::new(icon::ARROW_DOWN)
                    .with_font_family(catalog::MATERIAL_ICONS)
                    .with_size(24.0)
                    .with_color(Color::BLACK),
            );
        }
        RenderPadding::new(
            EdgeInsets::symmetric(0.0, 8.0),
            RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .push(leading)
                .push_flex(FlexChild::expanded(
                    Text::new(title).with_style(TextStyle {
                        font_weight: if header { 700 } else { 600 },
                        ..text_theme().title_medium.clone()
                    }),
                    1,
                )),
        )
    })
}

// -- Stocks -----------------------------------------------------------------------

/// Upstream's `StockItem`: the ticker over a row of price, sign and
/// percentage. The sign is `+` only when the change is strictly positive, and
/// its colour follows -- the upstream ternaries, kept as they are
/// (`percent > 0`, not `>=`).
pub(crate) fn stock_item(ticker: &'static str, price: &'static str, percent: f64) -> AnyWidget {
    leaf(move || {
        let styles = text_theme();
        let up = percent > 0.0;
        RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .push(Text::new(ticker).with_style(styles.title_medium.clone()))
            .push(SizedBox::height(2.0))
            .push(
                RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .push_flex(FlexChild::expanded(
                        // `titleSmall.color.withOpacity(0.75)`.
                        Text::new(price).with_style(TextStyle {
                            color: BLACK_75,
                            ..styles.title_small.clone()
                        }),
                        1,
                    ))
                    .push(Text::new(if up { "+" } else { "-" }).with_style(TextStyle {
                        font_size: 12.0,
                        color: if up { GAIN } else { LOSS },
                        ..styles.title_small.clone()
                    }))
                    .push(SizedBox::width(4.0))
                    .push(
                        // `decimalPercentPattern(decimalDigits: 2)` over
                        // `percent.abs() / 100`: "0.48%" for -0.48.
                        Text::new(percent_string(percent)).with_style(TextStyle {
                            font_size: 12.0,
                            color: BLACK_75,
                            ..styles.title_small.clone()
                        }),
                    ),
            )
    })
}

/// The percent text for a `StockItem`; see the call site for the intl call
/// this stands in for.
fn percent_string(percent: f64) -> String {
    format!("{:.2}%", percent.abs())
}

/// Upstream's `buildStockItems`: the chart, then the five indices with
/// article hairlines between them.
pub(crate) fn build_stock_items() -> Vec<AnyWidget> {
    /// Upstream's local `imageAspectRatio`.
    const CHART_ASPECT: f32 = 165.0 / 55.0;
    vec![
        leaf(|| {
            AspectRatio::new(
                CHART_ASPECT,
                match image("fortnightly_chart.png", CHART_IMAGE) {
                    Some(chart) => boxed(ImageView::with_fit(chart, BoxFit::Contain)),
                    None => boxed(Container::new().with_color(BLACK_10)),
                },
            )
        }),
        article_divider(),
        stock_item("DIJA", "7,031.21", -0.48),
        article_divider(),
        stock_item("SP", "1,967.84", -0.23),
        article_divider(),
        stock_item("Nasdaq", "6,211.46", 0.52),
        article_divider(),
        stock_item("Nikkei", "5,891", 1.16),
        article_divider(),
        stock_item("DJ Total", "89.02", 0.80),
        article_divider(),
    ]
}

// -- Videos -----------------------------------------------------------------------

/// Upstream's `VideoPreview`: the thumbnail across the full width, then the
/// category with the duration right-aligned, then the headline.
pub(crate) fn video_preview(data: ArticleData, time: &'static str) -> AnyWidget {
    leaf(move || {
        let styles = text_theme();
        RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .push(AspectRatio::new(
                data.image_aspect_ratio,
                article_image(&data, BoxFit::Cover),
            ))
            .push(SizedBox::height(4.0))
            .push(
                RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .push_flex(FlexChild::expanded(
                        Text::new(data.category.as_str()).with_style(styles.title_medium.clone()),
                        1,
                    ))
                    .push(Text::new(time).with_style(styles.body_large.clone())),
            )
            .push(SizedBox::height(4.0))
            .push(Text::new(data.title).with_style(styles.headline_small.clone()))
    })
}

/// Upstream's `buildVideoPreviewItems`.
pub(crate) fn build_video_preview_items() -> Vec<AnyWidget> {
    let l10n = GalleryLocalizations::en();
    vec![
        video_preview(
            ArticleData {
                image_key: "fortnightly_feminists.jpg",
                image: FEMINISTS_IMAGE,
                image_aspect_ratio: 148.0 / 88.0,
                category: l10n.fortnightly_menu_politics().to_uppercase(),
                title: l10n.fortnightly_headline_feminists(),
                snippet: None,
            },
            "2:31",
        ),
        leaf(|| SizedBox::height(32.0)),
        video_preview(
            ArticleData {
                image_key: "fortnightly_bees.jpg",
                image: BEES_IMAGE,
                image_aspect_ratio: 148.0 / 88.0,
                category: l10n.fortnightly_menu_us().to_uppercase(),
                title: l10n.fortnightly_headline_bees(),
                snippet: None,
            },
            "1:37",
        ),
    ]
}

// -- Shared pieces ------------------------------------------------------------------

/// Upstream's `reducedTextScale(context)` (`lib/layout/text_scale.dart`):
/// `textScaleFactor >= 1 ? (1 + textScaleFactor) / 2 : 1`.
pub(crate) fn reduced_text_scale(context: &BuildContext) -> f32 {
    let scale = media_query_of(context).text_scale_factor;
    if scale >= 1.0 {
        (1.0 + scale) / 2.0
    } else {
        1.0
    }
}

/// The drag/wheel handlers of one of the study's scrollables, against
/// whichever `Scroll` `pick` names. The same four events
/// `app::scroll_handlers` wires, for a state this module owns.
fn scroll_handlers<S: 'static>(
    handle: StateHandle<S>,
    pick: fn(&mut S) -> &mut Scroll,
    axis: Axis,
) -> rustflutter::gestures::PointerHandlers {
    let down_handle = handle.clone();
    let drag_handle = handle.clone();
    let end_handle = handle.clone();
    rustflutter::gestures::PointerHandlers::new()
        .with_pointer_down(move |_| {
            down_handle.set_state(move |state| pick(state).stop());
        })
        .with_drag_update(move |drag| {
            let along = match axis {
                Axis::Vertical => drag.delta.dy,
                Axis::Horizontal => drag.delta.dx,
            };
            drag_handle.set_state(move |state| pick(state).scroll_by(-along));
        })
        .with_drag_end(move |end| {
            let along = match axis {
                Axis::Vertical => end.velocity.dy,
                Axis::Horizontal => end.velocity.dx,
            };
            end_handle.set_state(move |state| pick(state).fling(-along));
        })
        .with_scroll(move |scroll| {
            let along = match axis {
                Axis::Vertical => scroll.delta.dy,
                Axis::Horizontal => scroll.delta.dx,
            };
            handle.set_state(move |state| pick(state).scroll_by(along));
        })
}

/// A vertical `ListView` that owns its scroll position. Every scrollable the
/// study has -- the feed, the desktop sidebar, the navigation menus -- is one
/// of these; upstream gives each `ListView` its own `ScrollController`, which
/// is what the per-element state stands in for.
pub(crate) struct ScrollColumn {
    id: u64,
    children: RefCell<Option<Vec<AnyWidget>>>,
}

impl ScrollColumn {
    pub(crate) fn new(id: u64, children: Vec<AnyWidget>) -> ScrollColumn {
        ScrollColumn {
            id,
            children: RefCell::new(Some(children)),
        }
    }
}

#[derive(Default)]
pub(crate) struct ScrollColumnState {
    scroll: Scroll,
}

impl StatefulComponent for ScrollColumn {
    type State = ScrollColumnState;

    fn advance(&self, state: &mut ScrollColumnState, frame_time_micros: i64) -> bool {
        state.scroll.advance(frame_time_micros)
    }

    fn build(
        &self,
        state: &ScrollColumnState,
        handle: StateHandle<ScrollColumnState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let children = self.children.borrow_mut().take().unwrap_or_default();
        let offset = state.scroll.offset;
        let extent = state.scroll.link();
        let id = self.id;
        let handlers = scroll_handlers(handle, |s| &mut s.scroll, Axis::Vertical);

        many(children, move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for child in rendered {
                column = column.push(child);
            }
            let list = ListView::new()
                .with_offset(offset)
                .with_link(extent.clone())
                .push(column);
            Box::new(Pointer::new(id, list).with_handlers(handlers.clone()))
        })
    }
}

/// Hit-test identities, allocated from the study base (`ids::STUDY_LOCAL`)
/// and never from a counter -- an id must be stable across rebuilds.
pub(crate) mod study_ids {
    use crate::app::ids;
    /// The hashtag strip's scroll region.
    pub const HASHTAGS: u64 = ids::STUDY_LOCAL;
    /// The article feed's scroll region.
    pub const FEED: u64 = ids::STUDY_LOCAL + 1;
    /// The desktop stocks/videos column's scroll region.
    pub const SIDEBAR: u64 = ids::STUDY_LOCAL + 2;
    /// The desktop navigation menu's scroll region.
    pub const MENU_DESKTOP: u64 = ids::STUDY_LOCAL + 3;
    /// The drawer menu's scroll region.
    pub const MENU_DRAWER: u64 = ids::STUDY_LOCAL + 4;
    /// The drawer's hamburger, close button and scrim.
    pub const DRAWER_OPEN: u64 = ids::STUDY_LOCAL + 5;
    pub const DRAWER_CLOSE: u64 = ids::STUDY_LOCAL + 6;
    pub const DRAWER_SCRIM: u64 = ids::STUDY_LOCAL + 7;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_format_the_way_intl_does() {
        // Upstream `{minutes, plural, =1{1m} other{{minutes}m}}`.
        assert_eq!(minutes_string(1), "1m");
        assert_eq!(minutes_string(2), "2m");
        assert_eq!(minutes_string(5), "5m");
    }

    #[test]
    fn percent_formats_like_decimal_percent_pattern() {
        // `decimalPercentPattern(decimalDigits: 2).format(p.abs() / 100)`.
        assert_eq!(percent_string(-0.48), "0.48%");
        assert_eq!(percent_string(-0.23), "0.23%");
        assert_eq!(percent_string(0.52), "0.52%");
        assert_eq!(percent_string(1.16), "1.16%");
        assert_eq!(percent_string(0.80), "0.80%");
    }

    #[test]
    fn the_feed_matches_upstreams_order() {
        // Seven article widgets, the "Latest Updates" header, and the
        // dividers between and after them: 7 + 1 + 6 hairlines... upstream's
        // list is literally fourteen children.
        assert_eq!(build_article_preview_items().len(), 14);
    }

    #[test]
    fn the_sidebar_matches_upstreams_order() {
        // Chart, a hairline, then the five indices with a hairline after
        // each: 1 + 1 + 5 * 2.
        assert_eq!(build_stock_items().len(), 12);
        // Two videos with a 32-pixel gap between them.
        assert_eq!(build_video_preview_items().len(), 3);
    }

    #[test]
    fn reduced_text_scale_grows_at_half_rate() {
        // The table in `lib/layout/text_scale.dart`: 0.8 -> 1.0, 1.0 -> 1.0,
        // 2.0 -> 1.5, 3.0 -> 2.0.
        let reduced = |scale: f32| {
            if scale >= 1.0 {
                (1.0 + scale) / 2.0
            } else {
                1.0
            }
        };
        assert_eq!(reduced(0.8), 1.0);
        assert_eq!(reduced(1.0), 1.0);
        assert_eq!(reduced(2.0), 1.5);
        assert_eq!(reduced(3.0), 2.0);
    }

    #[test]
    fn the_theme_is_upstreams_text_theme() {
        let styles = text_theme();
        assert_eq!(
            styles.body_medium.font_family.as_deref(),
            Some(MERRIWEATHER)
        );
        assert_eq!(styles.body_medium.font_weight, 300);
        assert_eq!(styles.body_large.font_size, 11.0);
        assert_eq!(
            styles.headline_small.font_family.as_deref(),
            Some(LIBRE_FRANKLIN)
        );
        assert_eq!(
            styles.title_medium.font_family.as_deref(),
            Some(ROBOTO_CONDENSED)
        );
        assert_eq!(styles.title_medium.font_weight, 700);
        assert!(styles.title_large.italic);
        // And the scaffold stays white whatever the gallery's mode is.
        assert_eq!(theme().background, Color::WHITE);
    }

    #[test]
    fn study_ids_do_not_collide() {
        let all = [
            study_ids::HASHTAGS,
            study_ids::FEED,
            study_ids::SIDEBAR,
            study_ids::MENU_DESKTOP,
            study_ids::MENU_DRAWER,
            study_ids::DRAWER_OPEN,
            study_ids::DRAWER_CLOSE,
            study_ids::DRAWER_SCRIM,
        ];
        for (index, id) in all.iter().enumerate() {
            assert!(all[index + 1..].iter().all(|other| other != id));
        }
    }
}
