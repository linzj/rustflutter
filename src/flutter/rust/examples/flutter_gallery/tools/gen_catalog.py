#!/usr/bin/env python3
"""Writes src/data/demos.rs from the JSON pulled out of upstream.

Generated rather than hand-written because it is 47 entries of upstream's own
wording, and anything retyped drifts from the thing it is a port of.

This script never touches the upstream repo directly: the data reaches it as
`upstream_catalog.json` / `upstream_icons.json`, written next to this script by
extract_catalog.py (which takes --upstream / $GALLERY_UPSTREAM). Run that first
when the upstream pin moves. The example's own location is derived from this
script's path, so the repo can live anywhere.
"""

import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
EXAMPLE = os.path.dirname(HERE)

CATALOG = json.load(open(os.path.join(HERE, 'upstream_catalog.json'), encoding='utf-8'))
ICONS = json.load(open(os.path.join(HERE, 'upstream_icons.json'), encoding='utf-8'))

# Four entries in upstream's icon table point at Material's own icon font
# rather than at GalleryIcons, so they need the other family.
MATERIAL_ICONS = {
    'appbar': 0xE6DE,          # Icons.web_asset
    'divider': 0xE19F,         # Icons.credit_card
    'navigationRail': 0xE69F,  # Icons.vertical_split
    'search': 0xE567,          # Icons.search
}

OUT = os.path.join(EXAMPLE, 'src', 'data', 'demos.rs')

CATEGORY = {'material': 'Material', 'cupertino': 'Cupertino', 'other': 'Reference'}

# Card colours and text colours are upstream's, from pages/home.dart and each
# study's own colors.dart.
STUDY_CARDS = {
    'reply':       ('reply',       0xFF344955, 0xFF1D2327, 0xFFFFFFFF),
    'shrine':      ('shrine',      0xFFFEDBD0, 0xFF543B3C, 0xFF442B2D),
    'rally':       ('rally',       0xFFD1F2E6, 0xFF253538, 0xFF005D57),
    'crane':       ('crane',       0xFFFBF6F8, 0xFF591946, 0xFF720D5D),
    'fortnightly': ('fortnightly', 0xFFFFFFFF, 0xFF1F1F1F, 0xFF000000),
    'starterApp':  ('starter',     0xFFFAF6FE, 0xFF3F3D45, 0xFF000000),
}

# The accent each demo row's icon is tinted with. Upstream tints them all with
# the theme's primary; a per-category hue reads better against a list this long
# and costs nothing.
ACCENT = {'material': 'BLUE', 'cupertino': 'GREEN', 'other': 'AMBER', 'study': 'TEAL'}


def rust_string(text):
  """A Rust string literal, wrapped with line continuations so the source stays
  inside a sensible width without changing the text."""
  text = text.replace('\\', '\\\\').replace('"', '\\"')
  if len(text) <= 62:
    return '"%s"' % text
  words, lines, line = text.split(' '), [], ''
  for word in words:
    if line and len(line) + 1 + len(word) > 62:
      lines.append(line)
      line = word
    else:
      line = (line + ' ' + word) if line else word
  if line:
    lines.append(line)
  body = ('\\\n' + ' ' * 22).join(l + ' ' if i < len(lines) - 1 else l
                                  for i, l in enumerate(lines))
  return '"%s"' % body


def icon_const(name):
  """(glyph, family constant) for an upstream icon name."""
  if name in MATERIAL_ICONS:
    return '\\u{%x}' % MATERIAL_ICONS[name], 'MATERIAL_ICONS'
  code = ICONS.get(name)
  if code is None:
    return None
  return '\\u{%s}' % code[2:], 'GALLERY_ICONS'


def main():
  lines = []
  add = lines.append

  add('// Copyright 2013 The Flutter Authors. All rights reserved.')
  add('// Use of this source code is governed by a BSD-style license that can be')
  add('// found in the LICENSE file.')
  add('')
  add('//! What the gallery contains.')
  add('//!')
  add('//! Maps to upstream `lib/data/demos.dart` (flutter/gallery @ d12640d).')
  add('//! Generated from that file and its English localisation by')
  add('//! `tools/gen_catalog.py`; do not edit by hand. The titles, subtitles and')
  add('//! descriptions are upstream\'s own words rather than retyped ones, because')
  add('//! forty-seven retyped entries drift from the thing they are a port of.')
  add('//!')
  add('//! The localisation catalogue is ported (`l10n/gallery_localizations.rs`)')
  add('//! but English-backed only, so the strings here resolve to en and are')
  add('//! baked in rather than looked up.')
  add('')
  add('use rustflutter::engine::Color;')
  add('')
  add('/// Which section of the gallery an entry belongs to.')
  add('#[derive(Clone, Copy, Debug, PartialEq, Eq)]')
  add('// `Study` is never constructed: the studies are their own list, and the')
  add('// category exists so that the enum matches upstream\'s, which')
  add('// `the_catalogue_matches_upstream` checks. Deleting it to quiet the warning')
  add('// would make this catalogue disagree with the one it is a port of.')
  add('#[allow(dead_code)]')
  add('pub enum Category {')
  add('    /// The full-screen sample apps, shown as the home page carousel.')
  add('    Study,')
  add('    Material,')
  add('    Cupertino,')
  add('    /// Upstream calls this `other` and labels it "STYLES & OTHER".')
  add('    Reference,')
  add('}')
  add('')
  add('impl Category {')
  add('    /// The header upstream puts above the category, uppercased the way')
  add('    /// `GalleryDemoCategory.toString` does it. Studies have none: they are')
  add('    /// the carousel, not a list with a heading.')
  add('    pub fn title(self) -> Option<&\'static str> {')
  add('        match self {')
  add('            Category::Study => None,')
  add('            Category::Material => Some("MATERIAL"),')
  add('            Category::Cupertino => Some("CUPERTINO"),')
  add('            Category::Reference => Some("STYLES & OTHER"),')
  add('        }')
  add('    }')
  add('')
  add('    /// The icon asset beside that header.')
  add('    pub fn icon(self) -> Option<&\'static [u8]> {')
  add('        match self {')
  add('            Category::Study => None,')
  add('            Category::Material => Some(include_bytes!("../../assets/icons/material.png")),')
  add('            Category::Cupertino => Some(include_bytes!("../../assets/icons/cupertino.png")),')
  add('            Category::Reference => Some(include_bytes!("../../assets/icons/reference.png")),')
  add('        }')
  add('    }')
  add('}')
  add('')
  add('/// One demo: a component, or one of the reference screens.')
  add('#[derive(Clone, Copy, Debug)]')
  add('pub struct Demo {')
  add('    /// Route argument. Stable, because it is what a route is pushed with.')
  add('    pub slug: &\'static str,')
  add('    pub title: &\'static str,')
  add('    pub subtitle: &\'static str,')
  add('    /// The longer text shown on the demo\'s own page.')
  add('    pub description: &\'static str,')
  add('    pub category: Category,')
  add('    /// The glyph upstream shows, as a private-use codepoint. Drawn as text')
  add('    /// in `icon_family` -- which is all an icon is.')
  add('    pub icon: &\'static str,')
  add('    pub icon_family: &\'static str,')
  add('    /// The tint the deleted description card used. Nothing reads it since')
  add('    /// the demo page\'s info section replaced that card; kept so the')
  add('    /// catalogue keeps carrying the palette (see PORTING.md).')
  add('    #[allow(dead_code)]')
  add('    pub accent: Color,')
  add('}')
  add('')
  add('')
  add('/// One study: a whole sample app, with the card the home page shows for it.')
  add('#[derive(Clone, Copy, Debug)]')
  add('pub struct Study {')
  add('    pub slug: &\'static str,')
  add('    pub title: &\'static str,')
  add('    pub subtitle: &\'static str,')
  add('    /// Upstream\'s own card artwork, light and dark.')
  add('    pub card: &\'static [u8],')
  add('    pub card_dark: &\'static [u8],')
  add('    /// The colour behind the artwork while it loads, and the colour the')
  add('    /// title is written in. Both upstream\'s.')
  add('    pub fill: Color,')
  add('    pub fill_dark: Color,')
  add('    pub text: Color,')
  add('}')
  add('')
  add('/// The two icon fonts, and the families they are registered under.')
  add('///')
  add('/// Both have to be registered before the first frame. An unregistered')
  add('/// family falls back to a system face, which has nothing at a private-use')
  add('/// codepoint and draws a blank rather than complaining.')
  add('pub const GALLERY_ICON_FONT: &[u8] =')
  add('    include_bytes!("../../assets/fonts/GalleryIcons.ttf");')
  add('pub const MATERIAL_ICON_FONT: &[u8] =')
  add('    include_bytes!("../../assets/fonts/MaterialIcons-Regular.otf");')
  add('pub const GALLERY_ICONS: &str = "GalleryIcons";')
  add('pub const MATERIAL_ICONS: &str = "MaterialIcons";')
  add('')
  add('/// The two text faces upstream sets the gallery in.')
  add('///')
  add('/// Upstream fetches these at runtime through `google_fonts`; they ship')
  add('/// with `flutter_gallery_assets` too, which is where these came from.')
  add('/// Four weights of one and two of the other, because that is what the')
  add('/// text theme asks for -- a weight that is not registered is synthesised')
  add('/// by smearing the nearest one, which looks like a different font.')
  add('pub const TEXT_FONTS: &[(&str, &[u8])] = &[')
  for family, face in [('MONTSERRAT', 'Montserrat-Regular'),
                       ('MONTSERRAT', 'Montserrat-Medium'),
                       ('MONTSERRAT', 'Montserrat-SemiBold'),
                       ('MONTSERRAT', 'Montserrat-Bold'),
                       ('OSWALD', 'Oswald-Medium'),
                       ('OSWALD', 'Oswald-SemiBold')]:
    add('    (%s, include_bytes!("../../assets/fonts/%s.ttf")),' % (family, face))
  add('];')
  add('pub const MONTSERRAT: &str = "Montserrat";')
  add('pub const OSWALD: &str = "Oswald";')
  add('')
  add('/// Registers every font the gallery draws with. Call once, before the')
  add('/// first frame: an unregistered family falls back to a system face, which')
  add('/// has nothing at a private-use codepoint and draws a blank rather than')
  add('/// complaining.')
  add('pub fn register_fonts() {')
  add('    rustflutter::engine::register_font(GALLERY_ICON_FONT, GALLERY_ICONS);')
  add('    rustflutter::engine::register_font(MATERIAL_ICON_FONT, MATERIAL_ICONS);')
  add('    for (family, bytes) in TEXT_FONTS {')
  add('        rustflutter::engine::register_font(bytes, family);')
  add('    }')
  add('}')
  add('')
  add('/// The chrome icons: back arrows, the settings gear, chevrons. Upstream')
  add('/// takes these from Material rather than from its own font, so they all')
  add('/// live in [`MATERIAL_ICONS`].')
  add('#[allow(dead_code)] // The complete set upstream uses; not every screen')
  add('                      // that will want one exists yet.')
  add('pub mod icon {')
  for name, code in [('ARROW_BACK', 'e092'), ('SETTINGS', 'e57f'), ('CLOSE', 'e16a'),
                     ('CHEVRON_RIGHT', 'e15f'), ('ARROW_DOWN', 'e353'),
                     ('ARROW_UP', 'e356'), ('PLAY', 'e4cd'), ('SEARCH', 'e567'),
                     ('MENU', 'e3dc'), ('MORE', 'e404'), ('CHECK', 'e156'),
                     ('ADD', 'e047'), ('REMOVE', 'e516'), ('FAVORITE', 'e25b'),
                     ('STAR', 'e5f9'), ('INFO', 'e33c'),
                     # The pages batch (M-C) added these: the settings panel's
                     # chevron and link icons, and the demo page's app bar.
                     ('ARROW_DROP_DOWN', 'e098'), ('INFO_OUTLINE', 'e33d'),
                     ('FEEDBACK', 'e260'), ('TUNE', 'e683'), ('CODE', 'e176'),
                     ('LIBRARY_BOOKS', 'e377'), ('FULLSCREEN', 'e2cb'),
                     ('ARROW_BACK_IOS', 'e093'), ('ARROW_FORWARD_IOS', 'e09c')]:
    add('    pub const %s: &str = "\\u{%s}";' % (name, code))
  add('}')
  add('')
  add('const BLUE: Color = Color::rgb(0x54, 0xC5, 0xF8);')
  add('const GREEN: Color = Color::rgb(0x7B, 0xD3, 0x89);')
  add('const AMBER: Color = Color::rgb(0xF2, 0xB1, 0x4F);')
  add('// Upstream\'s fifth accent. Nothing here uses it yet; kept so the palette is')
  add('// the whole palette rather than the part that happens to be referenced.')
  add('#[allow(dead_code)]')
  add('const TEAL: Color = Color::rgb(0x4F, 0xC8, 0xB0);')
  add('')

  # -- Studies ---------------------------------------------------------------
  add('/// The studies, in the order the carousel shows them.')
  add('pub const STUDIES: &[Study] = &[')
  order = ['reply', 'shrine', 'rally', 'crane', 'fortnightly', 'starterApp']
  by_slug = {s['slug']: s for s in CATALOG['studies']}
  titles = {'reply': 'Reply', 'shrine': 'Shrine', 'rally': 'Rally',
            'crane': 'Crane', 'fortnightly': 'Fortnightly',
            'starterApp': 'Starter app'}
  for slug in order:
    study = by_slug[slug]
    asset, fill, fill_dark, text = STUDY_CARDS[slug]
    add('    Study {')
    add('        slug: "%s",' % slug)
    add('        title: %s,' % rust_string(titles[slug]))
    add('        subtitle: %s,' % rust_string(study['subtitle']))
    add('        card: include_bytes!("../../assets/studies/%s_card.png"),' % asset)
    add('        card_dark: include_bytes!("../../assets/studies/%s_card_dark.png"),' % asset)
    add('        fill: Color(0x%08X),' % fill)
    add('        fill_dark: Color(0x%08X),' % fill_dark)
    add('        text: Color(0x%08X),' % text)
    add('    },')
  add('];')
  add('')

  # -- Demos -----------------------------------------------------------------
  add('/// Every demo, in the order the gallery lists them.')
  add('pub const DEMOS: &[Demo] = &[')
  last = None
  missing_icons = []
  for demo in CATALOG['demos']:
    category = demo['category']
    if category != last:
      add('    // -- %s ---' % CATEGORY[category])
      last = category
    found = icon_const(demo['icon'])
    if found is None:
      missing_icons.append((demo['slug'], demo['icon']))
      found = ('\\u{e900}', 'GALLERY_ICONS')
    glyph, family = found
    add('    Demo {')
    add('        slug: "%s",' % demo['slug'])
    add('        title: %s,' % rust_string(demo['title']))
    add('        subtitle: %s,' % rust_string(demo['subtitle']))
    add('        description: %s,' % rust_string(demo['description']))
    add('        category: Category::%s,' % CATEGORY[category])
    add('        icon: "%s",' % glyph)
    add('        icon_family: %s,' % family)
    add('        accent: %s,' % ACCENT[category])
    add('    },')
  add('];')
  add('')

  add('/// Looks a demo up by its route argument.')
  add('pub fn find(slug: &str) -> Option<&\'static Demo> {')
  add('    DEMOS.iter().find(|demo| demo.slug == slug)')
  add('}')
  add('')
  add('/// Looks a study up by its route argument.')
  add('pub fn find_study(slug: &str) -> Option<&\'static Study> {')
  add('    STUDIES.iter().find(|study| study.slug == slug)')
  add('}')
  add('')
  add('/// Every demo in a category, in order.')
  add('pub fn in_category(category: Category) -> impl Iterator<Item = &\'static Demo> {')
  add('    DEMOS.iter().filter(move |demo| demo.category == category)')
  add('}')
  add('')
  add('/// How many demos a category holds.')
  add('#[cfg(test)]')
  add('pub fn count(category: Category) -> usize {')
  add('    in_category(category).count()')
  add('}')
  add('')
  add('/// The categories the home page lists, in order. Studies are not among')
  add('/// them: they are the carousel above the list.')
  add('pub const CATEGORIES: &[Category] =')
  add('    &[Category::Material, Category::Cupertino, Category::Reference];')
  add('')
  add('#[cfg(test)]')
  add('mod tests {')
  add('    use super::*;')
  add('')
  add('    #[test]')
  add('    fn every_slug_is_unique() {')
  add('        // Slugs are what routes carry, so a duplicate would silently route')
  add('        // two entries to one demo.')
  add('        let mut seen: Vec<&str> = Vec::new();')
  add('        for slug in DEMOS.iter().map(|d| d.slug).chain(STUDIES.iter().map(|s| s.slug)) {')
  add('            assert!(!seen.contains(&slug), "duplicate slug {slug}");')
  add('            seen.push(slug);')
  add('        }')
  add('    }')
  add('')
  add('    #[test]')
  add('    fn every_demo_is_in_a_listed_category() {')
  add('        for demo in DEMOS {')
  add('            assert!(CATEGORIES.contains(&demo.category), "{} is unreachable", demo.slug);')
  add('        }')
  add('    }')
  add('')
  add('    #[test]')
  add('    fn lookup_finds_what_the_list_holds() {')
  add('        for demo in DEMOS {')
  add('            assert!(find(demo.slug).is_some(), "{} is not findable", demo.slug);')
  add('        }')
  add('        for study in STUDIES {')
  add('            assert!(find_study(study.slug).is_some(), "{} is not findable", study.slug);')
  add('        }')
  add('        assert!(find("not-a-demo").is_none());')
  add('    }')
  add('')
  add('    #[test]')
  add('    fn the_catalogue_matches_upstream() {')
  add('        // The counts upstream has, so a botched regeneration is caught here')
  add('        // rather than by someone noticing a missing row.')
  add('        assert_eq!(STUDIES.len(), 6);')
  add('        assert_eq!(count(Category::Material), 24);')
  add('        assert_eq!(count(Category::Cupertino), 13);')
  add('        assert_eq!(count(Category::Reference), 4);')
  add('        let total: usize = CATEGORIES.iter().map(|c| count(*c)).sum();')
  add('        assert_eq!(total, DEMOS.len());')
  add('    }')
  add('')
  add('    #[test]')
  add('    fn every_demo_has_a_real_icon_codepoint() {')
  add('        for demo in DEMOS {')
  add('            let mut chars = demo.icon.chars();')
  add('            let glyph = chars.next().expect("an icon is one character");')
  add('            assert!(chars.next().is_none(), "{} has more than one", demo.slug);')
  add('            // The private use area, which is where an icon font puts them.')
  add('            assert!(')
  add('                (0xE000..=0xF8FF).contains(&(glyph as u32)),')
  add('                "{} is not a private-use codepoint",')
  add('                demo.slug')
  add('            );')
  add('        }')
  add('    }')
  add('}')

  open(OUT, 'w', encoding='utf-8', newline='\n').write('\n'.join(lines) + '\n')
  print('wrote', OUT)
  print('studies:', len(order), 'demos:', len(CATALOG['demos']))
  if missing_icons:
    print('MISSING ICONS:', missing_icons)


if __name__ == '__main__':
  main()
