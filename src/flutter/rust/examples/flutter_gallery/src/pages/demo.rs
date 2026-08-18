// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The demo page scaffold.
//!
//! Ported from `lib/pages/demo.dart` (flutter/gallery @ d12640d): the
//! transparent app bar with the back button and the section icons, the info
//! section above the demo, and `DemoWrapper`, which runs every demo inside
//! `MaterialDemoThemeData` -- always light, Material purple -- the way upstream
//! does, so a demo shows the component the way a plain Material app would.
//!
//! What is not here, each logged in PORTING.md:
//!
//! * The **code** icon is present but disabled: the code viewer is batch M-H,
//!   and with it the desktop code-background crossfade.
//! * The **documentation** icon is present but disabled: upstream opens the
//!   API docs through `url_launcher`, which has no counterpart here.
//! * The **options** section (`_DemoState.options`) is unreachable: the
//!   catalogue flattens upstream's multi-configuration demos to one entry, so
//!   no demo here has more than one configuration to pick between.
//! * `DemoWrapper`'s clip is upstream's `BorderRadius.vertical(top: 10,
//!   bottom: 2)`; the framework's `ClipRRect` rounds all four corners alike,
//!   so the bottom corners take the top's radius.
//! * The foldable `TwoPane` layout is unreachable
//!   (`adaptive_layout::is_display_foldable` is always false).

use rustflutter::framework::{component, leaf, many, provide, single, AnyWidget, StateHandle};
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{Alignment, CrossAxisAlignment, FlexChild, MainAxisSize, RenderFlex};
use rustflutter::widgets::{Align, ClipRRect, Container, Pointer};

use crate::app::{self, ids, GalleryState};
use crate::constants::DESKTOP_DISPLAY1_FONT_DELTA;
use crate::data::demos as catalog;
use crate::data::demos::Demo;
use crate::demos::cupertino;
use crate::demos::material as demos;
use crate::demos::reference;
use crate::pages::splash;
use crate::themes::gallery_theme_data::{text, Scheme};
use crate::themes::material_demo_theme_data::MaterialDemoThemeData;

/// Upstream's `_DemoState`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DemoSection {
    /// Just the demo.
    #[default]
    Normal,
    /// The per-configuration picker. Unreachable here: the catalogue carries
    /// one configuration per demo (see the module header).
    #[allow(dead_code)]
    Options,
    /// The title and description above the demo.
    Info,
    /// The code viewer. Batch M-H; the icon is present but disabled.
    #[allow(dead_code)]
    Code,
    /// Desktop only: the demo without a section beside it.
    Fullscreen,
}

/// Upstream's `GalleryDemoPage`.
pub fn page(
    demo: &'static Demo,
    state: &GalleryState,
    handle: StateHandle<GalleryState>,
    is_desktop: bool,
) -> AnyWidget {
    let scheme = state.scheme();
    // Upstream's `_resolveState`: a desktop page never sits in `normal`.
    let section = match state.demo_section {
        DemoSection::Normal if is_desktop => DemoSection::Info,
        other => other,
    };

    let bar = component(DemoBar {
        scheme,
        section,
        is_desktop,
        pressed: state.pressed,
        handle: handle.clone(),
    });

    let demo_content = demo_wrapper(demo, state, handle.clone());

    let body = if is_desktop {
        // Upstream's desktop layout: the section and the demo side by side,
        // or the demo alone when fullscreen.
        let fullscreen = section == DemoSection::Fullscreen;
        let mut children = vec![];
        if !fullscreen {
            children.push(section_widget(demo, section, scheme, is_desktop));
        }
        children.push(demo_content);
        many(children, move |mut rendered| {
            let demo = rendered.pop().expect("the demo content");
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Start);
            if let Some(section) = rendered.pop() {
                row = row
                    .push_flex(FlexChild::expanded(section, 1))
                    .push(Container::new().with_size(48.0, 1.0));
            }
            let row = row.push_flex(FlexChild::expanded(demo, 1));
            // Upstream's `SafeArea(Padding(top: 56, ...))`.
            Box::new(
                Container::new()
                    .with_padding(EdgeInsets::only(0.0, 56.0, 0.0, 0.0))
                    .with_child(row),
            )
        })
    } else {
        // Upstream's mobile layout: the section above the demo, collapsing on
        // a tap of the demo.
        let mut rows: Vec<AnyWidget> = Vec::new();
        if section != DemoSection::Normal {
            rows.push(section_widget(demo, section, scheme, is_desktop));
        }
        let dismiss = PointerHandlers::new().with_tap({
            let handle = handle.clone();
            move |_| {
                handle.set_state(|state| {
                    if state.demo_section != DemoSection::Normal {
                        state.demo_section = DemoSection::Normal;
                    }
                });
            }
        });
        rows.push(single(demo_content, move |rendered| {
            Box::new(Pointer::new(ids::DEMO_CHROME + 5, rendered).with_handlers(dismiss.clone()))
        }));
        let body = app::scrolling_body(rows, 0.0, 0.0, state, handle.clone());
        // The bar floats over the body; the body starts below it.
        single(body, |rendered| {
            Box::new(
                Container::new()
                    .with_padding(EdgeInsets::only(0.0, 56.0, 0.0, 0.0))
                    .with_child(rendered),
            )
        })
    };

    let page = many(vec![bar, body], move |mut rendered| {
        let body = rendered.pop().expect("two children");
        let bar = rendered.pop().expect("two children");
        Box::new(
            rustflutter::render::RenderStack::new()
                .push_positioned(body, rustflutter::render::StackPosition::fill())
                .push_positioned(
                    bar,
                    rustflutter::render::StackPosition {
                        left: Some(0.0),
                        top: Some(0.0),
                        right: Some(0.0),
                        ..rustflutter::render::StackPosition::default()
                    },
                ),
        )
    });

    // Upstream's page background, and on desktop the splash layer around it.
    let background = scheme.background;
    let page = single(page, move |rendered| {
        Box::new(Container::new().with_color(background).with_child(rendered))
    });
    if is_desktop {
        splash::page(state, handle, true, page)
    } else {
        page
    }
}

/// The app bar: back, then the section icons. Upstream's `AppBar` in
/// `_GalleryDemoPageState.build`, transparent with the back button leading.
struct DemoBar {
    scheme: Scheme,
    section: DemoSection,
    is_desktop: bool,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
}

impl DemoBar {
    /// One icon in the bar. `action` toggles a section; `None` is a disabled
    /// icon, drawn muted with no handler.
    #[allow(clippy::too_many_arguments)]
    fn icon(
        id: u64,
        glyph: &'static str,
        scheme: Scheme,
        ink: Color,
        enabled: bool,
        pressed: Option<u64>,
        handle: StateHandle<GalleryState>,
        action: Option<fn(&mut GalleryState)>,
    ) -> AnyWidget {
        let held = pressed == Some(id);
        let handlers = match action {
            Some(action) if enabled => PointerHandlers::new()
                .with_tap({
                    let handle = handle.clone();
                    move |_| {
                        handle.set_state(move |state| action(state));
                    }
                })
                .with_press_change({
                    let handle = handle.clone();
                    move |down| {
                        handle.set_state(move |state| {
                            state.pressed = if down { Some(id) } else { None };
                        });
                    }
                }),
            _ => PointerHandlers::new(),
        };
        let color = if enabled { ink } else { ink.with_alpha(0x61) };
        leaf(move || {
            Pointer::new(
                id,
                Container::new()
                    .with_size(48.0, 48.0)
                    .with_corner_radius(24.0)
                    .with_color(if held {
                        scheme.on_surface.with_alpha(0x18)
                    } else {
                        Color::TRANSPARENT
                    })
                    .with_child(Align::new(
                        Alignment::CENTER,
                        Text::new(glyph)
                            .with_font_family(catalog::MATERIAL_ICONS)
                            .with_size(24.0)
                            .with_color(color),
                    )),
            )
            .with_handlers(handlers.clone())
        })
    }
}

impl Component for DemoBar {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let scheme = self.scheme;
        let section = self.section;
        let is_desktop = self.is_desktop;
        let icon_color = scheme.on_surface;
        let selected = scheme.primary;

        let back = Self::icon(
            ids::BACK,
            catalog::icon::ARROW_BACK,
            scheme,
            icon_color,
            true,
            self.pressed,
            self.handle.clone(),
            Some(|s| s.back()),
        );
        // The tune icon appears only for a demo with more than one
        // configuration; the catalogue carries exactly one per demo, so it
        // never does.
        let info = Self::icon(
            ids::DEMO_CHROME,
            catalog::icon::INFO,
            scheme,
            if section == DemoSection::Info {
                selected
            } else {
                icon_color
            },
            true,
            self.pressed,
            self.handle.clone(),
            Some(|s| {
                s.demo_section = if s.demo_section == DemoSection::Info {
                    DemoSection::Normal
                } else {
                    DemoSection::Info
                };
            }),
        );
        // Present but disabled until the code viewer lands (batch M-H).
        let code = Self::icon(
            ids::DEMO_CHROME + 1,
            catalog::icon::CODE,
            scheme,
            icon_color,
            false,
            self.pressed,
            self.handle.clone(),
            None,
        );
        // Present but disabled: upstream opens the API docs through
        // `url_launcher`, which has no counterpart here.
        let docs = Self::icon(
            ids::DEMO_CHROME + 2,
            catalog::icon::LIBRARY_BOOKS,
            scheme,
            icon_color,
            false,
            self.pressed,
            self.handle.clone(),
            None,
        );

        let mut children = vec![back];
        children.push(info);
        children.push(code);
        children.push(docs);
        if is_desktop {
            children.push(Self::icon(
                ids::DEMO_CHROME + 3,
                catalog::icon::FULLSCREEN,
                scheme,
                if section == DemoSection::Fullscreen {
                    selected
                } else {
                    icon_color
                },
                true,
                self.pressed,
                self.handle.clone(),
                Some(|s| {
                    s.demo_section = if s.demo_section == DemoSection::Fullscreen {
                        DemoSection::Info
                    } else {
                        DemoSection::Fullscreen
                    };
                }),
            ));
        }

        many(children, move |mut rendered| {
            let mut actions = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            // The back button leads; the rest trail.
            let back = rendered.remove(0);
            for icon in rendered {
                actions = actions.push(icon);
            }
            Box::new(
                RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .push(
                        Container::new()
                            .with_padding(EdgeInsets::all(4.0))
                            .with_child(back),
                    )
                    .push_flex(FlexChild::expanded(Container::new(), 1))
                    .push(actions)
                    .push(Container::new().with_size(4.0, 1.0)),
            )
        })
    }
}

/// The section above (mobile) or beside (desktop) the demo. Upstream's
/// `_DemoSectionInfo` is the only reachable one; see the module header.
fn section_widget(
    demo: &'static Demo,
    section: DemoSection,
    scheme: Scheme,
    is_desktop: bool,
) -> AnyWidget {
    debug_assert!(
        section == DemoSection::Info,
        "only the info section is reachable"
    );
    // Upstream's headline medium, with the desktop font delta on desktop.
    let mut title_style = text::HEADLINE_MEDIUM.styled(scheme.on_surface);
    if is_desktop {
        title_style.font_size += DESKTOP_DISPLAY1_FONT_DELTA;
    }
    let body_style = text::BODY_MEDIUM.styled(scheme.on_surface);
    let title = demo.title;
    let description = demo.description;

    leaf(move || {
        Container::new()
            .with_padding(EdgeInsets::only(24.0, 12.0, 24.0, 32.0))
            .with_child(
                RenderFlex::column()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Start)
                    .push(Text::new(title).with_style(title_style.clone()))
                    .push(Container::new().with_size(1.0, 12.0))
                    .push(Text::new(description).with_style(body_style.clone())),
            )
    })
}

/// Upstream's `DemoWrapper`: the demo inside the Material demo theme, padded
/// in and clipped to its card. The demo's own modal (a dialog, a sheet) is
/// part of the content, as upstream's is.
fn demo_wrapper(
    demo: &'static Demo,
    state: &GalleryState,
    handle: StateHandle<GalleryState>,
) -> AnyWidget {
    // The Cupertino demos are their own module; every `cupertino-*` slug
    // routes there. The reference demos have no shared prefix, so they route
    // on `reference::SLUGS`. Everything else stays with the material module.
    let (content, overlay) = if demo.slug.starts_with("cupertino-") {
        (
            cupertino::stage(demo, state, handle.clone()),
            cupertino::overlay(demo, state, handle),
        )
    } else if reference::SLUGS.contains(&demo.slug) {
        (
            reference::stage(demo, state, handle.clone()),
            reference::overlay(demo, state, handle),
        )
    } else {
        (
            demos::stage(demo, state, handle.clone()),
            demos::overlay(demo, state, handle),
        )
    };
    let content = app::with_overlay(content, overlay);
    // Upstream wraps the demo in `MaterialDemoThemeData.themeData.copyWith(
    // platform: ...)`; the platform only keyed typography upstream, which is
    // not carried, so the value is the same either way.
    let themed = provide(MaterialDemoThemeData::theme_data(), content);
    single(themed, |rendered| {
        Box::new(
            Container::new()
                .with_padding(EdgeInsets::only(16.0, 0.0, 16.0, 16.0))
                .with_child(ClipRRect::new(10.0, rendered)),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_section_is_normal() {
        assert_eq!(DemoSection::default(), DemoSection::Normal);
        // Upstream's restore defaults the same way.
        assert_eq!(GalleryState::default().demo_section, DemoSection::Normal);
    }
}
