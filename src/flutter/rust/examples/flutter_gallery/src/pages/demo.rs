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
//! * The **options** section (`_DemoState.options`) is reachable only for a
//!   demo with more than one configuration, as upstream's `_hasOptions` gates
//!   it; the catalogue carries them only for grid-lists
//!   (`data/demos.rs`'s `Demo::configurations`).
//! * `DemoWrapper`'s clip is upstream's `BorderRadius.vertical(top: 10,
//!   bottom: 2)`; the framework's `ClipRRect` rounds all four corners alike,
//!   so the bottom corners take the top's radius.
//! * The foldable `TwoPane` layout is unreachable
//!   (`adaptive_layout::is_display_foldable` is always false).

use rustflutter::framework::{
    component, leaf, many, provide, single, AnyWidget, BuildContext, StateHandle,
};
use rustflutter::gestures::PointerHandlers;
use rustflutter::media_query::size_of;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, BoxConstraints, CrossAxisAlignment, FlexChild, MainAxisSize, RenderConstrainedBox,
    RenderFlex,
};
use rustflutter::widgets::{Align, ClipRRect, Container, Empty, Pointer};

use crate::app::{self, ids, GalleryState};
use crate::constants::DESKTOP_DISPLAY1_FONT_DELTA;
use crate::data::demos as catalog;
use crate::data::demos::Demo;
use crate::demos::cupertino;
use crate::demos::material as demos;
use crate::demos::reference;
use crate::l10n::gallery_localizations::GalleryLocalizations;
use crate::pages::splash;
use crate::themes::gallery_theme_data::{text, Scheme};
use crate::themes::material_demo_theme_data::MaterialDemoThemeData;

/// Upstream's `_DemoState`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DemoSection {
    /// Just the demo.
    #[default]
    Normal,
    /// The per-configuration picker. Reachable only for a demo with more
    /// than one configuration (see the module header).
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
    // Upstream's `_hasOptions`.
    let has_options = demo.configurations().len() > 1;
    // Upstream's `_resolveState`: a desktop page never sits in `normal`, and
    // the section it falls back to is options when the demo has any.
    let section = match state.demo_section {
        DemoSection::Normal if is_desktop => {
            if has_options {
                DemoSection::Options
            } else {
                DemoSection::Info
            }
        }
        other => other,
    };

    let bar = component(DemoBar {
        scheme,
        section,
        is_desktop,
        has_options,
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
            children.push(section_widget(
                demo,
                section,
                scheme,
                is_desktop,
                state.demo_config,
                handle.clone(),
            ));
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
            rows.push(section_widget(
                demo,
                section,
                scheme,
                is_desktop,
                state.demo_config,
                handle.clone(),
            ));
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
        let body = single(body, |rendered| {
            Box::new(
                Container::new()
                    .with_padding(EdgeInsets::only(0.0, 56.0, 0.0, 0.0))
                    .with_child(rendered),
            )
        });
        body
    };

    // Upstream's `GalleryDemoPage` is a `Scaffold`, and so is this: the bar
    // floats over the body (`extendBodyBehindAppBar`), the scaffold paints the
    // background, and -- the reason this page stopped hand-rolling its own
    // frame -- `resizeToAvoidBottomInset` shrinks the body when the keyboard
    // opens. Without that the list under a form still believes it is full
    // height, and a focused field cannot be scrolled out from under the
    // keyboard because there is nowhere to scroll to.
    let page = component(
        rustflutter::components::Scaffold::new(body)
            .with_app_bar(bar)
            .with_extend_body_behind_app_bar(true),
    );
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
    /// Upstream's `_hasOptions`: whether the tune icon shows at all.
    has_options: bool,
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
        action: Option<impl Fn(&mut GalleryState) + 'static>,
    ) -> AnyWidget {
        let held = pressed == Some(id);
        let handlers = match action {
            Some(action) if enabled => {
                // Shared, because the tap handler is `Fn`: every tap applies
                // the same action afresh.
                let action = std::rc::Rc::new(action);
                PointerHandlers::new()
                    .with_tap({
                        let handle = handle.clone();
                        move |_| {
                            let action = std::rc::Rc::clone(&action);
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
                    })
            }
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
            Some(|s: &mut GalleryState| s.back()),
        );
        // The tune icon appears only for a demo with more than one
        // configuration, upstream's `_hasOptions` gate. Tapping it toggles
        // the options section; on desktop `normal` is not allowed, so there
        // the tap only enters the section, never leaves it (`_handleTap`).
        let tune = self.has_options.then(|| {
            Self::icon(
                ids::DEMO_CHROME + 4,
                catalog::icon::TUNE,
                scheme,
                if section == DemoSection::Options {
                    selected
                } else {
                    icon_color
                },
                true,
                self.pressed,
                self.handle.clone(),
                Some(move |s: &mut GalleryState| {
                    s.demo_section = if s.demo_section == DemoSection::Options && !is_desktop {
                        DemoSection::Normal
                    } else {
                        DemoSection::Options
                    };
                }),
            )
        });
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
            Some(|s: &mut GalleryState| {
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
            None::<fn(&mut GalleryState)>,
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
            None::<fn(&mut GalleryState)>,
        );

        let mut children = vec![back];
        // Upstream's action order: options (when there are any), info, code,
        // documentation, fullscreen.
        if let Some(tune) = tune {
            children.push(tune);
        }
        children.push(info);
        children.push(code);
        children.push(docs);
        if is_desktop {
            let has_options = self.has_options;
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
                // Leaving fullscreen lands on the page's default section:
                // options when the demo has any, info otherwise
                // (`_handleTap`'s desktop branch).
                Some(move |s: &mut GalleryState| {
                    s.demo_section = if s.demo_section == DemoSection::Fullscreen {
                        if has_options {
                            DemoSection::Options
                        } else {
                            DemoSection::Info
                        }
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

/// The section above (mobile) or beside (desktop) the demo: upstream's
/// `_DemoSectionInfo`, or `_DemoSectionOptions` for a demo with
/// configurations. The code section is batch M-H; see the module header.
fn section_widget(
    demo: &'static Demo,
    section: DemoSection,
    scheme: Scheme,
    is_desktop: bool,
    selected: usize,
    handle: StateHandle<GalleryState>,
) -> AnyWidget {
    debug_assert!(
        matches!(section, DemoSection::Info | DemoSection::Options),
        "only the info and options sections are reachable"
    );
    // Upstream's headline medium, with the desktop font delta on desktop.
    let mut title_style = text::HEADLINE_MEDIUM.styled(scheme.on_surface);
    if is_desktop {
        title_style.font_size += DESKTOP_DISPLAY1_FONT_DELTA;
    }
    if section == DemoSection::Options {
        return options_section(demo, scheme, title_style, selected, handle);
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

/// The per-configuration picker, upstream's `_DemoSectionOptions`: the
/// "Options" title, a divider, then one row per configuration -- the selected
/// one on `surface` in `primary`, the rest on the page in `onSurface`.
fn options_section(
    demo: &'static Demo,
    scheme: Scheme,
    title_style: rustflutter::engine::TextStyle,
    selected: usize,
    handle: StateHandle<GalleryState>,
) -> AnyWidget {
    let configurations = demo.configurations();
    let mut items: Vec<AnyWidget> = Vec::with_capacity(configurations.len());
    for (index, title) in configurations.iter().enumerate() {
        let is_selected = index == selected;
        // Upstream's `_DemoSectionOptionsItem`: a full-width row padded 24/8
        // whose tap selects the configuration (its `onConfigChanged`).
        let ink = if is_selected {
            scheme.primary
        } else {
            scheme.on_surface
        };
        let fill = if is_selected {
            scheme.surface
        } else {
            Color::TRANSPARENT
        };
        let item = leaf(move || {
            Container::new()
                .with_color(fill)
                .with_padding(EdgeInsets::symmetric(24.0, 8.0))
                .with_child(Text::new(*title).with_style(text::BODY_MEDIUM.styled(ink)))
        });
        let tap = handle.clone();
        items.push(single(item, move |rendered| {
            let tap = tap.clone();
            Box::new(
                Pointer::new(ids::DEMO_CHROME + 10 + index as u64, rendered).with_handlers(
                    PointerHandlers::new().with_tap(move |_| {
                        tap.set_state(move |state| state.demo_config = index);
                    }),
                ),
            )
        }));
    }

    // Upstream's `Divider(thickness: 1, height: 16, color: onSurface)`: the
    // line is one pixel centered in a sixteen-pixel band.
    let divider = leaf(move || {
        Container::new()
            .with_height(16.0)
            .with_padding(EdgeInsets::symmetric(0.0, 7.5))
            .with_child(
                Container::new()
                    .with_height(1.0)
                    .with_color(scheme.on_surface),
            )
    });

    many(
        vec![
            leaf(move || {
                Container::new()
                    .with_padding(EdgeInsets::only(24.0, 12.0, 24.0, 0.0))
                    .with_child(
                        Text::new(GalleryLocalizations::en().demo_options_tooltip())
                            .with_style(title_style.clone()),
                    )
            }),
            divider,
            many(items, |rendered| {
                let mut column = RenderFlex::column()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
                for item in rendered {
                    column = column.push(item);
                }
                Box::new(column)
            }),
            leaf(|| Container::new().with_height(12.0)),
        ],
        |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for child in rendered {
                column = column.push(child);
            }
            Box::new(column)
        },
    )
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
    // The card fills the demo area even when the demo is shorter: a demo's
    // own overlay (the bottom-sheet demo's sheet) lays out against the card,
    // and a content-sized card would put the sheet's bottom anchor halfway up
    // the window with the scrim entirely underneath the sheet -- unreachable,
    // so the sheet could never be dismissed. Upstream gets the height from
    // the Scaffold's body, which fills it.
    let content = component(DemoArea {
        child: std::cell::RefCell::new(Some(content)),
    });
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

/// Stretches the demo card to the demo area's height. See `demo_wrapper`.
struct DemoArea {
    child: std::cell::RefCell<Option<AnyWidget>>,
}

impl Component for DemoArea {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        // The bar floats over the body, which starts 56 below the top of the
        // page, and the wrapper's own bottom padding is 16. The SafeArea
        // inset is zero on the host.
        let min_height = (size_of(context).height - 56.0 - 16.0).max(0.0);
        let child = self
            .child
            .borrow_mut()
            .take()
            .unwrap_or_else(|| leaf(|| Empty));
        single(child, move |rendered| {
            Box::new(
                RenderConstrainedBox::new(BoxConstraints::new(
                    0.0,
                    f32::INFINITY,
                    min_height,
                    f32::INFINITY,
                ))
                .with_child(rendered),
            )
        })
    }
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
