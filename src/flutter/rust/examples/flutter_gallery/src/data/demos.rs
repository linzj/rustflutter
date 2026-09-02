// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! What the gallery contains.
//!
//! Maps to upstream `lib/data/demos.dart` (flutter/gallery @ d12640d).
//! Generated from that file and its English localisation by
//! `tools/gen_catalog.py`; do not edit by hand. The titles, subtitles and
//! descriptions are upstream's own words rather than retyped ones, because
//! forty-seven retyped entries drift from the thing they are a port of.
//!
//! The localisation catalogue is ported (`l10n/gallery_localizations.rs`)
//! but English-backed only, so the strings here resolve to en and are
//! baked in rather than looked up.

use rustflutter::engine::Color;

/// Which section of the gallery an entry belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// `Study` is never constructed: the studies are their own list, and the
// category exists so that the enum matches upstream's, which
// `the_catalogue_matches_upstream` checks. Deleting it to quiet the warning
// would make this catalogue disagree with the one it is a port of.
#[allow(dead_code)]
pub enum Category {
    /// The full-screen sample apps, shown as the home page carousel.
    Study,
    Material,
    Cupertino,
    /// Upstream calls this `other` and labels it "STYLES & OTHER".
    Reference,
}

impl Category {
    /// The header upstream puts above the category, uppercased the way
    /// `GalleryDemoCategory.toString` does it. Studies have none: they are
    /// the carousel, not a list with a heading.
    pub fn title(self) -> Option<&'static str> {
        match self {
            Category::Study => None,
            Category::Material => Some("MATERIAL"),
            Category::Cupertino => Some("CUPERTINO"),
            Category::Reference => Some("STYLES & OTHER"),
        }
    }

    /// The icon asset beside that header.
    pub fn icon(self) -> Option<&'static [u8]> {
        match self {
            Category::Study => None,
            Category::Material => Some(include_bytes!("../../assets/icons/material.png")),
            Category::Cupertino => Some(include_bytes!("../../assets/icons/cupertino.png")),
            Category::Reference => Some(include_bytes!("../../assets/icons/reference.png")),
        }
    }
}

/// One demo: a component, or one of the reference screens.
#[derive(Clone, Copy, Debug)]
pub struct Demo {
    /// Route argument. Stable, because it is what a route is pushed with.
    pub slug: &'static str,
    pub title: &'static str,
    pub subtitle: &'static str,
    /// The longer text shown on the demo's own page.
    pub description: &'static str,
    pub category: Category,
    /// The glyph upstream shows, as a private-use codepoint. Drawn as text
    /// in `icon_family` -- which is all an icon is.
    pub icon: &'static str,
    pub icon_family: &'static str,
    /// The tint the deleted description card used. Nothing reads it since
    /// the demo page's info section replaced that card; kept so the
    /// catalogue keeps carrying the palette (see PORTING.md).
    #[allow(dead_code)]
    pub accent: Color,
}

/// The grid-list demo's variants, upstream's three
/// `GalleryDemoConfiguration`s (`demoGridListsImageOnlyTitle` and friends).
const GRID_LIST_CONFIGURATIONS: &[&str] = &["Image only", "With header", "With footer"];

impl Demo {
    /// Upstream's `GalleryDemo.configurations`: the variants the demo page's
    /// options section switches between. Only the demos with more than one
    /// get the tune icon and the section; here that is grid-lists alone --
    /// every other upstream multi-configuration demo is flattened to one
    /// entry per configuration (PORTING.md).
    pub fn configurations(&self) -> &'static [&'static str] {
        match self.slug {
            "grid-lists" => GRID_LIST_CONFIGURATIONS,
            _ => &[],
        }
    }
}

/// One study: a whole sample app, with the card the home page shows for it.
#[derive(Clone, Copy, Debug)]
pub struct Study {
    pub slug: &'static str,
    pub title: &'static str,
    pub subtitle: &'static str,
    /// Upstream's own card artwork, light and dark.
    pub card: &'static [u8],
    pub card_dark: &'static [u8],
    /// The colour behind the artwork while it loads, and the colour the
    /// title is written in. Both upstream's.
    pub fill: Color,
    pub fill_dark: Color,
    pub text: Color,
}

/// The two icon fonts, and the families they are registered under.
///
/// Both have to be registered before the first frame. An unregistered
/// family falls back to a system face, which has nothing at a private-use
/// codepoint and draws a blank rather than complaining.
pub const GALLERY_ICON_FONT: &[u8] = include_bytes!("../../assets/fonts/GalleryIcons.ttf");
pub const MATERIAL_ICON_FONT: &[u8] =
    include_bytes!("../../assets/fonts/MaterialIcons-Regular.otf");
pub const GALLERY_ICONS: &str = "GalleryIcons";
pub const MATERIAL_ICONS: &str = "MaterialIcons";

/// The two text faces upstream sets the gallery in.
///
/// Upstream fetches these at runtime through `google_fonts`; they ship
/// with `flutter_gallery_assets` too, which is where these came from.
/// Four weights of one and two of the other, because that is what the
/// text theme asks for -- a weight that is not registered is synthesised
/// by smearing the nearest one, which looks like a different font.
pub const TEXT_FONTS: &[(&str, &[u8])] = &[
    (
        MONTSERRAT,
        include_bytes!("../../assets/fonts/Montserrat-Regular.ttf"),
    ),
    (
        MONTSERRAT,
        include_bytes!("../../assets/fonts/Montserrat-Medium.ttf"),
    ),
    (
        MONTSERRAT,
        include_bytes!("../../assets/fonts/Montserrat-SemiBold.ttf"),
    ),
    (
        MONTSERRAT,
        include_bytes!("../../assets/fonts/Montserrat-Bold.ttf"),
    ),
    (
        OSWALD,
        include_bytes!("../../assets/fonts/Oswald-Medium.ttf"),
    ),
    (
        OSWALD,
        include_bytes!("../../assets/fonts/Oswald-SemiBold.ttf"),
    ),
];
pub const MONTSERRAT: &str = "Montserrat";
pub const OSWALD: &str = "Oswald";

/// Registers every font the gallery draws with. Call once, before the
/// first frame: an unregistered family falls back to a system face, which
/// has nothing at a private-use codepoint and draws a blank rather than
/// complaining.
pub fn register_fonts() {
    rustflutter::engine::register_font(GALLERY_ICON_FONT, GALLERY_ICONS);
    rustflutter::engine::register_font(MATERIAL_ICON_FONT, MATERIAL_ICONS);
    for (family, bytes) in TEXT_FONTS {
        rustflutter::engine::register_font(bytes, family);
    }
}

/// The chrome icons: back arrows, the settings gear, chevrons. Upstream
/// takes these from Material rather than from its own font, so they all
/// live in [`MATERIAL_ICONS`].
#[allow(dead_code)] // The complete set upstream uses; not every screen
// that will want one exists yet.
pub mod icon {
    pub const ARROW_BACK: &str = "\u{e092}";
    pub const SETTINGS: &str = "\u{e57f}";
    pub const CLOSE: &str = "\u{e16a}";
    pub const CHEVRON_RIGHT: &str = "\u{e15f}";
    pub const ARROW_DOWN: &str = "\u{e353}";
    pub const ARROW_UP: &str = "\u{e356}";
    pub const PLAY: &str = "\u{e4cd}";
    pub const SEARCH: &str = "\u{e567}";
    pub const MENU: &str = "\u{e3dc}";
    pub const MORE: &str = "\u{e404}";
    pub const CHECK: &str = "\u{e156}";
    pub const ADD: &str = "\u{e047}";
    pub const REMOVE: &str = "\u{e516}";
    pub const FAVORITE: &str = "\u{e25b}";
    pub const STAR: &str = "\u{e5f9}";
    pub const INFO: &str = "\u{e33c}";
    pub const ARROW_DROP_DOWN: &str = "\u{e098}";
    pub const INFO_OUTLINE: &str = "\u{e33d}";
    pub const FEEDBACK: &str = "\u{e260}";
    pub const TUNE: &str = "\u{e683}";
    pub const CODE: &str = "\u{e176}";
    pub const LIBRARY_BOOKS: &str = "\u{e377}";
    pub const FULLSCREEN: &str = "\u{e2cb}";
    pub const ARROW_BACK_IOS: &str = "\u{e093}";
    pub const ARROW_FORWARD_IOS: &str = "\u{e09c}";
    // The text-field demo's decoration icons (upstream's `Icons.person` and
    // friends on the fields, and the password's visibility toggle). The
    // codepoints are this font's -- the bundled `MaterialIcons-Regular.otf`
    // uses the new mapping, not the legacy one (`Icons.person` is 0xe7fd
    // upstream, 0xe491 here).
    pub const PERSON: &str = "\u{e491}";
    pub const PHONE: &str = "\u{e4a2}";
    pub const EMAIL: &str = "\u{e22a}";
    pub const VISIBILITY: &str = "\u{e6bd}";
    pub const VISIBILITY_OFF: &str = "\u{e6be}";
    // Reply's bottom app bar: the drawer's drop arrow and the compose
    // button's pencil (upstream's `Icons.arrow_drop_up` and `Icons.create`).
    // Its search action is `SEARCH`, already here.
    pub const ARROW_DROP_UP: &str = "\u{e09a}";
    pub const CREATE: &str = "\u{e19d}";
}

const BLUE: Color = Color::rgb(0x54, 0xC5, 0xF8);
const GREEN: Color = Color::rgb(0x7B, 0xD3, 0x89);
const AMBER: Color = Color::rgb(0xF2, 0xB1, 0x4F);
// Upstream's fifth accent. Nothing here uses it yet; kept so the palette is
// the whole palette rather than the part that happens to be referenced.
#[allow(dead_code)]
const TEAL: Color = Color::rgb(0x4F, 0xC8, 0xB0);

/// The studies, in the order the carousel shows them.
pub const STUDIES: &[Study] = &[
    Study {
        slug: "reply",
        title: "Reply",
        subtitle: "An efficient, focused email app",
        card: include_bytes!("../../assets/studies/reply_card.png"),
        card_dark: include_bytes!("../../assets/studies/reply_card_dark.png"),
        fill: Color(0xFF344955),
        fill_dark: Color(0xFF1D2327),
        text: Color(0xFFFFFFFF),
    },
    Study {
        slug: "shrine",
        title: "Shrine",
        subtitle: "A fashionable retail app",
        card: include_bytes!("../../assets/studies/shrine_card.png"),
        card_dark: include_bytes!("../../assets/studies/shrine_card_dark.png"),
        fill: Color(0xFFFEDBD0),
        fill_dark: Color(0xFF543B3C),
        text: Color(0xFF442B2D),
    },
    Study {
        slug: "rally",
        title: "Rally",
        subtitle: "A personal finance app",
        card: include_bytes!("../../assets/studies/rally_card.png"),
        card_dark: include_bytes!("../../assets/studies/rally_card_dark.png"),
        fill: Color(0xFFD1F2E6),
        fill_dark: Color(0xFF253538),
        text: Color(0xFF005D57),
    },
    Study {
        slug: "crane",
        title: "Crane",
        subtitle: "A personalized travel app",
        card: include_bytes!("../../assets/studies/crane_card.png"),
        card_dark: include_bytes!("../../assets/studies/crane_card_dark.png"),
        fill: Color(0xFFFBF6F8),
        fill_dark: Color(0xFF591946),
        text: Color(0xFF720D5D),
    },
    Study {
        slug: "fortnightly",
        title: "Fortnightly",
        subtitle: "A content-focused news app",
        card: include_bytes!("../../assets/studies/fortnightly_card.png"),
        card_dark: include_bytes!("../../assets/studies/fortnightly_card_dark.png"),
        fill: Color(0xFFFFFFFF),
        fill_dark: Color(0xFF1F1F1F),
        text: Color(0xFF000000),
    },
    Study {
        slug: "starterApp",
        title: "Starter app",
        subtitle: "A responsive starter layout",
        card: include_bytes!("../../assets/studies/starter_card.png"),
        card_dark: include_bytes!("../../assets/studies/starter_card_dark.png"),
        fill: Color(0xFFFAF6FE),
        fill_dark: Color(0xFF3F3D45),
        text: Color(0xFF000000),
    },
];

/// Every demo, in the order the gallery lists them.
pub const DEMOS: &[Demo] = &[
    // -- Material ---
    Demo {
        slug: "app-bar",
        title: "App bar",
        subtitle: "Displays information and actions relating to the current \
                      screen",
        description: "The App bar provides content and actions related to the \
                      current screen. It's used for branding, screen titles, \
                      navigation, and actions",
        category: Category::Material,
        icon: "\u{e6de}",
        icon_family: MATERIAL_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "banner",
        title: "Banner",
        subtitle: "Displaying a banner within a list",
        description: "A banner displays an important, succinct message, and provides \
                      actions for users to address (or dismiss the banner). A user \
                      action is required for it to be dismissed.",
        category: Category::Material,
        icon: "\u{e927}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "bottom-app-bar",
        title: "Bottom app bar",
        subtitle: "Displays navigation and actions at the bottom",
        description: "Bottom app bars provide access to a bottom navigation drawer \
                      and up to four actions, including the floating action button.",
        category: Category::Material,
        icon: "\u{e925}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "bottom-navigation",
        title: "Bottom navigation",
        subtitle: "Bottom navigation with cross-fading views",
        description: "Bottom navigation bars display three to five destinations at \
                      the bottom of a screen. Each destination is represented by an \
                      icon and an optional text label. When a bottom navigation icon \
                      is tapped, the user is taken to the top-level navigation \
                      destination associated with that icon.",
        category: Category::Material,
        icon: "\u{e91b}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "bottom-sheet",
        title: "Bottom sheet",
        subtitle: "Persistent and modal bottom sheets",
        description: "A persistent bottom sheet shows information that supplements \
                      the primary content of the app. A persistent bottom sheet \
                      remains visible even when the user interacts with other parts \
                      of the app.",
        category: Category::Material,
        icon: "\u{e91a}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "button",
        title: "Buttons",
        subtitle: "Text, elevated, outlined, and more",
        description: "A text button displays an ink splash on press but does not \
                      lift. Use text buttons on toolbars, in dialogs and inline with \
                      padding",
        category: Category::Material,
        icon: "\u{e923}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "card",
        title: "Cards",
        subtitle: "Baseline cards with rounded corners",
        description: "A card is a sheet of Material used to represent some related \
                      information, for example an album, a geographical location, a \
                      meal, contact details, etc.",
        category: Category::Material,
        icon: "\u{e918}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "chip",
        title: "Chips",
        subtitle: "Compact elements that represent an input, attribute, or action",
        description: "Action chips are a set of options which trigger an action \
                      related to primary content. Action chips should appear \
                      dynamically and contextually in a UI.",
        category: Category::Material,
        icon: "\u{e916}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "data-table",
        title: "Data Tables",
        subtitle: "Rows and columns of information",
        description: "Data tables display information in a grid-like format of rows \
                      and columns. They organize information in a way that's easy to \
                      scan, so that users can look for patterns and insights.",
        category: Category::Material,
        icon: "\u{e913}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "dialog",
        title: "Dialogs",
        subtitle: "Simple, alert, and fullscreen",
        description: "An alert dialog informs the user about situations that require \
                      acknowledgement. An alert dialog has an optional title and an \
                      optional list of actions.",
        category: Category::Material,
        icon: "\u{e912}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "divider",
        title: "Divider",
        subtitle: "A divider is a thin line that groups content in lists and \
                      layouts.",
        description: "Dividers can be used in lists, drawers, and elsewhere to \
                      separate content.",
        category: Category::Material,
        icon: "\u{e19f}",
        icon_family: MATERIAL_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "grid-lists",
        title: "Grid Lists",
        subtitle: "Row and column layout",
        description: "Grid Lists are best suited for presenting homogeneous data, \
                      typically images. Each item in a grid list is called a tile.",
        category: Category::Material,
        icon: "\u{e90e}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "lists",
        title: "Lists",
        subtitle: "Scrolling list layouts",
        description: "A single fixed-height row that typically contains some text as \
                      well as a leading or trailing icon.",
        category: Category::Material,
        icon: "\u{e90d}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "menu",
        title: "Menu",
        subtitle: "Menu buttons and simple menus",
        description: "A menu displays a list of choices on a temporary surface. They \
                      appear when users interact with a button, action, or other \
                      control.",
        category: Category::Material,
        icon: "\u{e90b}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "nav_drawer",
        title: "Navigation Drawer",
        subtitle: "Displaying a drawer within appbar",
        description: "A Material Design panel that slides in horizontally from the \
                      edge of the screen to show navigation links in an application.",
        category: Category::Material,
        icon: "\u{e90c}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "nav_rail",
        title: "Navigation Rail",
        subtitle: "Displaying a Navigation Rail within an app",
        description: "A material widget that is meant to be displayed at the left or \
                      right of an app to navigate between a small number of views, \
                      typically between three and five.",
        category: Category::Material,
        icon: "\u{e69f}",
        icon_family: MATERIAL_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "pickers",
        title: "Pickers",
        subtitle: "Date and time selection",
        description: "Shows a dialog containing a Material Design date picker.",
        category: Category::Material,
        icon: "\u{e910}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "progress-indicator",
        title: "Progress indicators",
        subtitle: "Linear, circular, indeterminate",
        description: "A Material Design circular progress indicator, which spins to \
                      indicate that the application is busy.",
        category: Category::Material,
        icon: "\u{e908}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "selection-controls",
        title: "Selection controls",
        subtitle: "Checkboxes, radio buttons, and switches",
        description: "Checkboxes allow the user to select multiple options from a \
                      set. A normal checkbox's value is true or false and a tristate \
                      checkbox's value can also be null.",
        category: Category::Material,
        icon: "\u{e917}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "sliders",
        title: "Sliders",
        subtitle: "Widgets for selecting a value by swiping",
        description: "Sliders reflect a range of values along a bar, from which \
                      users may select a single value. They are ideal for adjusting \
                      settings such as volume, brightness, or applying image \
                      filters.",
        category: Category::Material,
        icon: "\u{e904}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "snackbars",
        title: "Snackbars",
        subtitle: "Snackbars show messages at the bottom of the screen",
        description: "Snackbars inform users of a process that an app has performed \
                      or will perform. They appear temporarily, towards the bottom \
                      of the screen. They shouldn't interrupt the user experience, \
                      and they don't require user input to disappear.",
        category: Category::Material,
        icon: "\u{e91e}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "tabs",
        title: "Tabs",
        subtitle: "Tabs with independently scrollable views",
        description: "Tabs organize content across different screens, data sets, and \
                      other interactions.",
        category: Category::Material,
        icon: "\u{e902}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "text-field",
        title: "Text fields",
        subtitle: "Single line of editable text and numbers",
        description: "Text fields allow users to enter text into a UI. They \
                      typically appear in forms and dialogs.",
        category: Category::Material,
        icon: "\u{e901}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    Demo {
        slug: "tooltip",
        title: "Tooltips",
        subtitle: "Short message displayed on long press or hover",
        description: "Tooltips provide text labels that help explain the function of \
                      a button or other user interface action. Tooltips display \
                      informative text when users hover over, focus on, or long \
                      press an element.",
        category: Category::Material,
        icon: "\u{e900}",
        icon_family: GALLERY_ICONS,
        accent: BLUE,
    },
    // -- Cupertino ---
    Demo {
        slug: "cupertino-activity-indicator",
        title: "Activity indicator",
        subtitle: "iOS-style activity indicators",
        description: "An iOS-style activity indicator that spins clockwise.",
        category: Category::Cupertino,
        icon: "\u{e920}",
        icon_family: GALLERY_ICONS,
        accent: GREEN,
    },
    Demo {
        slug: "cupertino-alerts",
        title: "Alerts",
        subtitle: "iOS-style alert dialogs",
        description: "An alert dialog informs the user about situations that require \
                      acknowledgement. An alert dialog has an optional title, \
                      optional content, and an optional list of actions. The title \
                      is displayed above the content and the actions are displayed \
                      below the content.",
        category: Category::Cupertino,
        icon: "\u{e912}",
        icon_family: GALLERY_ICONS,
        accent: GREEN,
    },
    Demo {
        slug: "cupertino-buttons",
        title: "Buttons",
        subtitle: "iOS-style buttons",
        description: "An iOS-style button. It takes in text and/or an icon that \
                      fades out and in on touch. May optionally have a background.",
        category: Category::Cupertino,
        icon: "\u{e923}",
        icon_family: GALLERY_ICONS,
        accent: GREEN,
    },
    Demo {
        slug: "cupertino-context-menu",
        title: "Context Menu",
        subtitle: "iOS-style context menu",
        description: "An iOS-style full screen contextual menu that appears when an \
                      element is long-pressed.",
        category: Category::Cupertino,
        icon: "\u{e90b}",
        icon_family: GALLERY_ICONS,
        accent: GREEN,
    },
    Demo {
        slug: "cupertino-navigation-bar",
        title: "Navigation bar",
        subtitle: "iOS-style navigation bar",
        description: "An iOS-styled navigation bar. The navigation bar is a toolbar \
                      that minimally consists of a page title, in the middle of the \
                      toolbar.",
        category: Category::Cupertino,
        icon: "\u{e926}",
        icon_family: GALLERY_ICONS,
        accent: GREEN,
    },
    Demo {
        slug: "cupertino-picker",
        title: "Pickers",
        subtitle: "iOS-style pickers",
        description: "An iOS-style picker widget that can be used to select strings, \
                      dates, times, or both date and time.",
        category: Category::Cupertino,
        icon: "\u{e90d}",
        icon_family: GALLERY_ICONS,
        accent: GREEN,
    },
    Demo {
        slug: "cupertino-scrollbar",
        title: "Scrollbar",
        subtitle: "iOS-style scrollbar",
        description: "A scrollbar that wraps the given child",
        category: Category::Cupertino,
        icon: "\u{e90d}",
        icon_family: GALLERY_ICONS,
        accent: GREEN,
    },
    Demo {
        slug: "cupertino-segmented-control",
        title: "Segmented control",
        subtitle: "iOS-style segmented control",
        description: "Used to select between a number of mutually exclusive options. \
                      When one option in the segmented control is selected, the \
                      other options in the segmented control cease to be selected.",
        category: Category::Cupertino,
        icon: "\u{e902}",
        icon_family: GALLERY_ICONS,
        accent: GREEN,
    },
    Demo {
        slug: "cupertino-slider",
        title: "Slider",
        subtitle: "iOS-style slider",
        description: "A slider can be used to select from either a continuous or a \
                      discrete set of values.",
        category: Category::Cupertino,
        icon: "\u{e904}",
        icon_family: GALLERY_ICONS,
        accent: GREEN,
    },
    Demo {
        slug: "cupertino-switch",
        title: "Switch",
        subtitle: "iOS-style switch",
        description: "A switch is used to toggle the on/off state of a single \
                      setting.",
        category: Category::Cupertino,
        icon: "\u{e922}",
        icon_family: GALLERY_ICONS,
        accent: GREEN,
    },
    Demo {
        slug: "cupertino-tab-bar",
        title: "Tab bar",
        subtitle: "iOS-style bottom tab bar",
        description: "An iOS-style bottom navigation tab bar. Displays multiple tabs \
                      with one tab being active, the first tab by default.",
        category: Category::Cupertino,
        icon: "\u{e91b}",
        icon_family: GALLERY_ICONS,
        accent: GREEN,
    },
    Demo {
        slug: "cupertino-text-field",
        title: "Text fields",
        subtitle: "iOS-style text fields",
        description: "A text field lets the user enter text, either with a hardware \
                      keyboard or with an onscreen keyboard.",
        category: Category::Cupertino,
        icon: "\u{e901}",
        icon_family: GALLERY_ICONS,
        accent: GREEN,
    },
    Demo {
        slug: "cupertino-search-text-field",
        title: "Search text field",
        subtitle: "iOS-style search text field",
        description: "A search text field that lets the user search by entering \
                      text, and that can offer and filter suggestions.",
        category: Category::Cupertino,
        icon: "\u{e567}",
        icon_family: MATERIAL_ICONS,
        accent: GREEN,
    },
    // -- Reference ---
    Demo {
        slug: "motion",
        title: "Motion",
        subtitle: "All of the predefined transition patterns",
        description: "The container transform pattern is designed for transitions \
                      between UI elements that include a container. This pattern \
                      creates a visible connection between two UI elements",
        category: Category::Reference,
        icon: "\u{e91c}",
        icon_family: GALLERY_ICONS,
        accent: AMBER,
    },
    Demo {
        slug: "colors",
        title: "Colors",
        subtitle: "All of the predefined colors",
        description: "Color and color swatch constants which represent Material \
                      Design's color palette.",
        category: Category::Reference,
        icon: "\u{e915}",
        icon_family: GALLERY_ICONS,
        accent: AMBER,
    },
    Demo {
        slug: "typography",
        title: "Typography",
        subtitle: "All of the predefined text styles",
        description: "Definitions for the various typographical styles found in \
                      Material Design.",
        category: Category::Reference,
        icon: "\u{e914}",
        icon_family: GALLERY_ICONS,
        accent: AMBER,
    },
    Demo {
        slug: "2d-transformations",
        title: "2D transformations",
        subtitle: "Pan and zoom",
        description: "Tap to edit tiles, and use gestures to move around the scene. \
                      Drag to pan and pinch with two fingers to zoom. Press the \
                      reset button to return to the starting orientation.",
        category: Category::Reference,
        icon: "\u{e90e}",
        icon_family: GALLERY_ICONS,
        accent: AMBER,
    },
];

/// Looks a demo up by its route argument.
pub fn find(slug: &str) -> Option<&'static Demo> {
    DEMOS.iter().find(|demo| demo.slug == slug)
}

/// Looks a study up by its route argument.
pub fn find_study(slug: &str) -> Option<&'static Study> {
    STUDIES.iter().find(|study| study.slug == slug)
}

/// Every demo in a category, in order.
pub fn in_category(category: Category) -> impl Iterator<Item = &'static Demo> {
    DEMOS.iter().filter(move |demo| demo.category == category)
}

/// How many demos a category holds.
#[cfg(test)]
pub fn count(category: Category) -> usize {
    in_category(category).count()
}

/// The categories the home page lists, in order. Studies are not among
/// them: they are the carousel above the list.
pub const CATEGORIES: &[Category] = &[Category::Material, Category::Cupertino, Category::Reference];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_slug_is_unique() {
        // Slugs are what routes carry, so a duplicate would silently route
        // two entries to one demo.
        let mut seen: Vec<&str> = Vec::new();
        for slug in DEMOS
            .iter()
            .map(|d| d.slug)
            .chain(STUDIES.iter().map(|s| s.slug))
        {
            assert!(!seen.contains(&slug), "duplicate slug {slug}");
            seen.push(slug);
        }
    }

    #[test]
    fn every_demo_is_in_a_listed_category() {
        for demo in DEMOS {
            assert!(
                CATEGORIES.contains(&demo.category),
                "{} is unreachable",
                demo.slug
            );
        }
    }

    #[test]
    fn lookup_finds_what_the_list_holds() {
        for demo in DEMOS {
            assert!(find(demo.slug).is_some(), "{} is not findable", demo.slug);
        }
        for study in STUDIES {
            assert!(
                find_study(study.slug).is_some(),
                "{} is not findable",
                study.slug
            );
        }
        assert!(find("not-a-demo").is_none());
    }

    #[test]
    fn the_catalogue_matches_upstream() {
        // The counts upstream has, so a botched regeneration is caught here
        // rather than by someone noticing a missing row.
        assert_eq!(STUDIES.len(), 6);
        assert_eq!(count(Category::Material), 24);
        assert_eq!(count(Category::Cupertino), 13);
        assert_eq!(count(Category::Reference), 4);
        let total: usize = CATEGORIES.iter().map(|c| count(*c)).sum();
        assert_eq!(total, DEMOS.len());
    }

    #[test]
    fn every_demo_has_a_real_icon_codepoint() {
        for demo in DEMOS {
            let mut chars = demo.icon.chars();
            let glyph = chars.next().expect("an icon is one character");
            assert!(chars.next().is_none(), "{} has more than one", demo.slug);
            // The private use area, which is where an icon font puts them.
            assert!(
                (0xE000..=0xF8FF).contains(&(glyph as u32)),
                "{} is not a private-use codepoint",
                demo.slug
            );
        }
    }
}
