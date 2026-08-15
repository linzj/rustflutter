// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! What the gallery contains.
//!
//! Ported from `new_gallery/lib/data/demos.dart`, which is one long table of
//! metadata that the home page, the category lists and the demo pages all read
//! from. Keeping it a table here too is what stops the three of them drifting:
//! a demo that is added to the list is a demo the router can reach.
//!
//! Titles and subtitles are upstream's, in English. The localisation machinery
//! behind them is not ported -- there is no message catalogue and no locale
//! plumbing yet -- so the strings sit here rather than in a generated file.

use rustflutter::engine::Color;

/// Which section of the gallery a demo belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    /// The full-screen sample apps. Upstream these are Reply, Shrine, Rally,
    /// Crane, Fortnightly and Starter.
    Study,
    /// The component demos. Upstream's Material section.
    Components,
    /// Layout and painting demos, which upstream files under Reference.
    Reference,
}

impl Category {
    pub fn title(self) -> &'static str {
        match self {
            Category::Study => "Studies",
            Category::Components => "Components",
            Category::Reference => "Reference",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Category::Study => "Complete apps built from the component library",
            Category::Components => "Building blocks, one demo each",
            Category::Reference => "Layout, painting and motion",
        }
    }
}

/// One entry in the gallery.
#[derive(Clone, Copy, Debug)]
pub struct Demo {
    /// Route argument. Stable, because it is what a route is pushed with.
    pub slug: &'static str,
    pub title: &'static str,
    pub subtitle: &'static str,
    /// The longer text shown on the demo's own page.
    pub description: &'static str,
    pub category: Category,
    /// A one- or two-letter mark, standing in for upstream's icon. There is no
    /// icon font here, and a missing glyph draws nothing at all.
    pub mark: &'static str,
    pub accent: Color,
}

const BLUE: Color = Color::rgb(0x54, 0xC5, 0xF8);
const GREEN: Color = Color::rgb(0x7B, 0xD3, 0x89);
const AMBER: Color = Color::rgb(0xF2, 0xB1, 0x4F);
const PINK: Color = Color::rgb(0xE0, 0x7A, 0x9B);
const PURPLE: Color = Color::rgb(0x9B, 0x8C, 0xF0);
const TEAL: Color = Color::rgb(0x4F, 0xC8, 0xB0);
const CORAL: Color = Color::rgb(0xF0, 0x8A, 0x6E);

/// Every demo, in the order the gallery lists them.
pub const DEMOS: &[Demo] = &[
    // -- Studies --------------------------------------------------------------
    Demo {
        slug: "rally",
        title: "Rally",
        subtitle: "A personal finance app",
        description: "A dashboard of accounts, bills and budgets. Every figure \
                      is a real layout: the bars are sized from their values, \
                      and the rows are a flex with an expanded label.",
        category: Category::Study,
        mark: "R",
        accent: TEAL,
    },
    Demo {
        slug: "shrine",
        title: "Shrine",
        subtitle: "A shopping app",
        description: "A product grid with a filter row and a cart count. The \
                      grid is a fixed-column layout with a tile aspect ratio, \
                      and tapping a chip filters it.",
        category: Category::Study,
        mark: "S",
        accent: PINK,
    },
    Demo {
        slug: "crane",
        title: "Crane",
        subtitle: "A travel app",
        description: "Tabs over a list of destinations. Switching tabs replaces \
                      the list without rebuilding the page around it.",
        category: Category::Study,
        mark: "C",
        accent: PURPLE,
    },
    // -- Components -----------------------------------------------------------
    Demo {
        slug: "app-bar",
        title: "App bar",
        subtitle: "Displays information and actions at the top of a screen",
        description: "The app bar sits at the top and holds the screen's title \
                      and its actions. Here it carries a trailing control.",
        category: Category::Components,
        mark: "A",
        accent: BLUE,
    },
    Demo {
        slug: "banner",
        title: "Banner",
        subtitle: "Displaying a banner within a list",
        description: "A banner displays an important message and the actions \
                      that answer it. It stays until it is dismissed.",
        category: Category::Components,
        mark: "B",
        accent: AMBER,
    },
    Demo {
        slug: "bottom-navigation",
        title: "Bottom navigation",
        subtitle: "Bottom navigation with cross-fading views",
        description: "A bar of destinations along the bottom edge. The view \
                      above it cross-fades when the destination changes.",
        category: Category::Components,
        mark: "BN",
        accent: GREEN,
    },
    Demo {
        slug: "bottom-sheet",
        title: "Bottom sheet",
        subtitle: "Persistent and modal bottom sheets",
        description: "A sheet anchored to the bottom edge. A modal one draws \
                      over a scrim that swallows taps meant for the page.",
        category: Category::Components,
        mark: "BS",
        accent: PURPLE,
    },
    Demo {
        slug: "buttons",
        title: "Buttons",
        subtitle: "Text, elevated, outlined, and more",
        description: "Four styles, one pressed state. The state lives in the \
                      page rather than the button, because a button is rebuilt \
                      every frame and cannot remember anything.",
        category: Category::Components,
        mark: "Bt",
        accent: BLUE,
    },
    Demo {
        slug: "cards",
        title: "Cards",
        subtitle: "Baseline cards with rounded corners",
        description: "A card is a surface with a border and padding, used to \
                      group things that belong together.",
        category: Category::Components,
        mark: "Cd",
        accent: TEAL,
    },
    Demo {
        slug: "chips",
        title: "Chips",
        subtitle: "Compact elements that represent an input, attribute, or action",
        description: "Chips are compact and tappable. A filter chip records a \
                      choice; an action chip performs one.",
        category: Category::Components,
        mark: "Ch",
        accent: CORAL,
    },
    Demo {
        slug: "data-table",
        title: "Data tables",
        subtitle: "Rows and columns of information",
        description: "A table of text with a header row. Columns share the \
                      width evenly through the row's flex.",
        category: Category::Components,
        mark: "DT",
        accent: BLUE,
    },
    Demo {
        slug: "dialogs",
        title: "Dialogs",
        subtitle: "Simple, alert, and fullscreen",
        description: "A dialog interrupts to ask something. The scrim under it \
                      is what makes it modal -- without one the taps go to the \
                      page behind.",
        category: Category::Components,
        mark: "D",
        accent: PINK,
    },
    Demo {
        slug: "dividers",
        title: "Dividers",
        subtitle: "A divider is a thin line that groups content in lists and layouts",
        description: "A one-pixel rule. Worth a demo because a rule that is not \
                      exactly one physical pixel looks wrong on every screen.",
        category: Category::Components,
        mark: "Dv",
        accent: GREEN,
    },
    Demo {
        slug: "grid-lists",
        title: "Grid lists",
        subtitle: "Row and column layout",
        description: "Tiles in fixed columns, each sized by an aspect ratio. \
                      The last row is padded so its tiles keep their width.",
        category: Category::Components,
        mark: "G",
        accent: AMBER,
    },
    Demo {
        slug: "lists",
        title: "Lists",
        subtitle: "Scrolling list layouts",
        description: "A scrollable column of tiles. The viewport lays its child \
                      out unbounded and shows a window onto it.",
        category: Category::Components,
        mark: "L",
        accent: PURPLE,
    },
    Demo {
        slug: "navigation-rail",
        title: "Navigation rail",
        subtitle: "Displaying a navigation rail within an app",
        description: "A column of destinations along the leading edge, for \
                      windows too wide for a bottom bar.",
        category: Category::Components,
        mark: "NR",
        accent: TEAL,
    },
    Demo {
        slug: "progress",
        title: "Progress indicators",
        subtitle: "Linear, circular, indeterminate",
        description: "Determinate indicators show how much is done; \
                      indeterminate ones only show that something is. The \
                      spinning one is a looping animation controller.",
        category: Category::Components,
        mark: "P",
        accent: BLUE,
    },
    Demo {
        slug: "selection-controls",
        title: "Selection controls",
        subtitle: "Checkboxes, radio buttons, and switches",
        description: "Three shapes for three meanings: a checkbox for several \
                      independent choices, a radio for one of a set, a switch \
                      for something that takes effect immediately.",
        category: Category::Components,
        mark: "SC",
        accent: GREEN,
    },
    Demo {
        slug: "sliders",
        title: "Sliders",
        subtitle: "Widgets for selecting a value by swiping",
        description: "A slider's hit area is deliberately taller than its \
                      track: the thing you can hit should be bigger than the \
                      thing you can see.",
        category: Category::Components,
        mark: "Sl",
        accent: CORAL,
    },
    Demo {
        slug: "snackbars",
        title: "Snackbars",
        subtitle: "Snackbars show messages at the bottom of the screen",
        description: "A brief message with at most one action. It inverts the \
                      surface so it reads as separate from the page.",
        category: Category::Components,
        mark: "Sn",
        accent: PINK,
    },
    Demo {
        slug: "tabs",
        title: "Tabs",
        subtitle: "Tabs with independently scrollable views",
        description: "Tabs switch between views at the same level. The \
                      indicator is a child of the tab, so it moves with the \
                      tab's own layout rather than being placed twice.",
        category: Category::Components,
        mark: "T",
        accent: PURPLE,
    },
    Demo {
        slug: "tooltips",
        title: "Tooltips",
        subtitle: "Short message displayed on long press or hover",
        description: "There is no hover yet, so this one appears while a region \
                      is pressed. The shape is the same either way.",
        category: Category::Components,
        mark: "Tt",
        accent: AMBER,
    },
    // -- Reference ------------------------------------------------------------
    Demo {
        slug: "colors",
        title: "Colors",
        subtitle: "All of the predefined colors",
        description: "The theme's palette, and what each colour is for. \
                      Switching the theme in settings changes every swatch.",
        category: Category::Reference,
        mark: "Co",
        accent: PINK,
    },
    Demo {
        slug: "typography",
        title: "Typography",
        subtitle: "All of the predefined text styles",
        description: "Text is shaped by the engine's own txt and skparagraph \
                      stack -- the same one Flutter uses -- and drawn into a \
                      real display list.",
        category: Category::Reference,
        mark: "Ty",
        accent: BLUE,
    },
    Demo {
        slug: "motion",
        title: "Motion",
        subtitle: "Curves, and what each of them is for",
        description: "Every curve maps 0 to 0 and 1 to 1, so an animation \
                      always starts where it started and ends where it ends, \
                      however strangely it gets there.",
        category: Category::Reference,
        mark: "M",
        accent: TEAL,
    },
    Demo {
        slug: "layout",
        title: "Layout",
        subtitle: "Constraints go down, sizes come up",
        description: "The protocol the whole render layer turns on. A parent \
                      hands each child constraints; the child picks a size \
                      inside them; the parent decides where it sits.",
        category: Category::Reference,
        mark: "Ly",
        accent: GREEN,
    },
];

/// Looks a demo up by its route argument.
pub fn find(slug: &str) -> Option<&'static Demo> {
    DEMOS.iter().find(|demo| demo.slug == slug)
}

/// Every demo in a category, in order.
pub fn in_category(category: Category) -> impl Iterator<Item = &'static Demo> {
    DEMOS.iter().filter(move |demo| demo.category == category)
}

/// How many demos a category holds.
pub fn count(category: Category) -> usize {
    in_category(category).count()
}

/// The categories, in the order the home page lists them.
pub const CATEGORIES: &[Category] =
    &[Category::Study, Category::Components, Category::Reference];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_slug_is_unique() {
        // Slugs are what routes carry, so a duplicate would silently route two
        // entries to one demo.
        let mut seen: Vec<&str> = Vec::new();
        for demo in DEMOS {
            assert!(!seen.contains(&demo.slug), "duplicate slug {}", demo.slug);
            seen.push(demo.slug);
        }
    }

    #[test]
    fn every_demo_is_in_a_listed_category() {
        for demo in DEMOS {
            assert!(CATEGORIES.contains(&demo.category), "{} is unreachable", demo.slug);
        }
    }

    #[test]
    fn lookup_finds_what_the_list_holds() {
        for demo in DEMOS {
            assert!(find(demo.slug).is_some(), "{} is not findable", demo.slug);
        }
        assert!(find("not-a-demo").is_none());
    }

    #[test]
    fn the_counts_add_up() {
        let total: usize = CATEGORIES.iter().map(|c| count(*c)).sum();
        assert_eq!(total, DEMOS.len());
    }
}
