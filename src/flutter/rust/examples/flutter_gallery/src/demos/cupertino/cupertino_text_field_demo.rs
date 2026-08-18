// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/cupertino/cupertino_text_field_demo.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! Upstream's `CupertinoTextFieldDemo` is a `CupertinoPageScaffold` whose
//! body is a `SafeArea` around a `ListView` (padding 16) of four
//! `CupertinoTextField`s: email, password, a disabled password, and a PIN
//! field with a padlock prefix and a bottom-border-only decoration. The demo
//! itself is stateless upstream; the fields carry their own text.
//!
//! Divergences, each commented at its site as well:
//!
//! * **The framework tier has no plain `CupertinoTextField`** (only
//!   `CupertinoSearchTextField`, a different look), so the field's face is
//!   built here on `editable::TextField`: the default decoration
//!   (text_field.dart's `_kDefaultRoundedBorderDecoration`, border radius 4
//!   at hairline width in `_kBorderColor`), the default padding
//!   (`_kDefaultPadding`, 7 on every side), and the `clearButtonMode:
//!   editing` clear button, drawn and wired the way the tier's search field
//!   does it.
//! * **`keyboardType`, `textInputAction` and `autocorrect` are dropped.** The
//!   framework's `TextField` has no setters for them (the action defaults to
//!   Done, which is what the PIN field asks for anyway).
//! * **The disabled field takes no input and shows its placeholder**, drawn
//!   in the same decoration box. Upstream's `enabled: false` keeps the
//!   decoration and the placeholder and drops the input, which is exactly
//!   this; the material text-field demo does the same.
//! * **The placeholder's color** is the field's own muted color rather than
//!   `CupertinoColors.placeholderText` -- the same difference the tier's
//!   `CupertinoSearchTextField` notes, because `editable::TextField` derives
//!   the placeholder style from the ambient material theme. Here that theme
//!   is the demo chrome's `MaterialDemoThemeData` (`pages/demo.rs`), which is
//!   light-schemed, so under a dark appearance the placeholder reads dark on
//!   the dark field; in the light appearance it matches upstream.
//! * Restoration (`restorationId`s on the list and the fields) is not
//!   carried: nothing here restores.

use std::cell::RefCell;
use std::rc::Rc;

use rustflutter::framework::{BuildContext, Key};
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{
    BoxConstraints, BoxedRender, CrossAxisAlignment, FlexChild, MainAxisSize, Offset, PaintContext,
    RenderBox, RenderFlex, RenderRef, Size,
};
use rustflutter::widgets::{ListView, Pointer};

use crate::app::{ids, GalleryState};

/// Hit-test ids, from the demo-local block (PORTING.md: fixed bases, no
/// counters). A clear button shares its field's id, as the tier's search
/// field does. The disabled field has none: it takes no input.
const EMAIL_FIELD: u64 = ids::DEMO_LOCAL;
const PASSWORD_FIELD: u64 = ids::DEMO_LOCAL + 1;
const PIN_FIELD: u64 = ids::DEMO_LOCAL + 2;
const LIST_SCROLL: u64 = ids::DEMO_LOCAL + 3;

/// The demo body for the `cupertino-text-field` slug.
///
/// `state` is read for the resolved brightness only: upstream's demo runs
/// under the app's `CupertinoTheme`, which the gallery derives from the
/// options' brightness, so the same theme is provided over the stage here.
pub(super) fn stage(state: &GalleryState) -> AnyWidget {
    let theme = match state.options.resolved_brightness() {
        Brightness::Light => CupertinoTheme::light(),
        Brightness::Dark => CupertinoTheme::dark(),
    };
    provide(theme, stateful(CupertinoTextFieldDemo))
}

/// text_field.dart's `_kBorderColor`.
const FIELD_BORDER_COLOR: CupertinoDynamicColor =
    CupertinoDynamicColor::with_brightness(Color(0x3300_0000), Color(0x33FF_FFFF));

/// The field's decoration: upstream's default rounded border, or the PIN
/// field's `BoxDecoration` with a bottom border only.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FieldDecoration {
    /// text_field.dart's `_kDefaultRoundedBorderDecoration`.
    Rounded,
    /// The PIN field's `Border(bottom: BorderSide(width: 0, color:
    /// CupertinoColors.inactiveGray))`.
    BottomBorder,
}

/// Upstream's `CupertinoTextFieldDemo`: the scaffold and the list.
struct CupertinoTextFieldDemo;

/// The list's scroll offset. Upstream's `ListView` carries it in its own
/// `Scrollable`; here it is the demo component's state, as in the material
/// list demo.
#[derive(Default)]
struct TextFieldDemoState {
    scroll: Scroll,
}

impl StatefulComponent for CupertinoTextFieldDemo {
    type State = TextFieldDemoState;

    fn advance(&self, state: &mut TextFieldDemoState, frame_time_micros: i64) -> bool {
        state.scroll.advance(frame_time_micros)
    }

    fn build(
        &self,
        state: &TextFieldDemoState,
        handle: StateHandle<TextFieldDemoState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let offset = state.scroll.offset;
        let extent = state.scroll.extent.clone();

        // The same handlers `app::scroll_handlers` gives the page
        // scrollables, against this list's own `Scroll` (see the material
        // list demo): a finger down stops a fling, a drag moves the content,
        // letting go throws it, and the wheel walks it.
        let down_handle = handle.clone();
        let drag_handle = handle.clone();
        let end_handle = handle.clone();
        let wheel_handle = handle;
        let handlers = PointerHandlers::new()
            .with_pointer_down(move |_| {
                down_handle.set_state(|state| state.scroll.stop());
            })
            .with_drag_update(move |drag| {
                let delta = drag.delta.dy;
                drag_handle.set_state(move |state| state.scroll.scroll_by(-delta));
            })
            .with_drag_end(move |end| {
                let velocity = end.velocity.dy;
                end_handle.set_state(move |state| state.scroll.fling(-velocity));
            })
            .with_scroll(move |scroll| {
                let delta = scroll.delta.dy;
                wheel_handle.set_state(move |state| state.scroll.scroll_by(delta));
            });

        // The four fields, in upstream's order. The first three are wrapped
        // in `Padding(padding: const EdgeInsets.symmetric(vertical: 8))`.
        let fields: Vec<AnyWidget> = vec![
            // Email. Upstream's `textInputAction: next`, `keyboardType:
            // emailAddress` and `autocorrect: false` are dropped (see the
            // module header).
            padded(stateful(
                DemoTextField::new(EMAIL_FIELD, "Email"), // demoTextFieldEmail
            )),
            // Password. Same drops as the email field.
            padded(stateful(
                DemoTextField::new(PASSWORD_FIELD, "Password").obscured(), // rallyLoginPassword
            )),
            // The disabled password field (upstream's `enabled: false`).
            padded(stateful(
                DemoTextField::new(PASSWORD_FIELD + 10, "Password")
                    .obscured()
                    .with_enabled(false),
            )),
            // PIN: padlock prefix, 6/12 padding, bottom-border decoration.
            stateful(
                DemoTextField::new(PIN_FIELD, "PIN") // demoCupertinoTextFieldPIN
                    .with_padlock_prefix()
                    .with_padding(EdgeInsets::symmetric(6.0, 12.0))
                    .with_decoration(FieldDecoration::BottomBorder),
            ),
        ];

        let list = many(fields, move |rendered| {
            let mut flex = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for field in rendered {
                flex = flex.push(field);
            }
            // Upstream's `ListView(padding: const EdgeInsets.all(16))`;
            // `widgets::ListView` has no padding slot, so the padding wraps
            // the column (the material list demo notes the same).
            let content = Container::new()
                .with_padding(EdgeInsets::all(16.0))
                .with_child(flex);
            let list = ListView::new()
                .with_offset(offset)
                .with_extent_sink(extent.clone())
                .push(content);
            Box::new(Pointer::new(LIST_SCROLL, list).with_handlers(handlers.clone()))
        });

        // Upstream's `SafeArea(child: ListView(...))`.
        let body = safe_area(list);
        component(
            CupertinoPageScaffold::new(body).with_navigation_bar(component(
                // `automaticallyImplyLeading: false` is the framework's
                // default: no back button unless asked for.
                CupertinoNavigationBar::new().with_middle("Text fields"), // demoCupertinoTextFieldTitle
            )),
        )
    }
}

/// Upstream's `Padding(padding: const EdgeInsets.symmetric(vertical: 8))`.
fn padded(field: AnyWidget) -> AnyWidget {
    single(field, |inner| {
        Box::new(
            Container::new()
                .with_padding(EdgeInsets::symmetric(0.0, 8.0))
                .with_child(inner),
        )
    })
}

/// The `CupertinoTextField` face this demo needs, built on the framework's
/// `editable::TextField` (see the module header): placeholder, optional
/// obscuring, an optional padlock prefix, the clear button of
/// `clearButtonMode: editing`, and one of the two decorations upstream uses.
struct DemoTextField {
    id: u64,
    placeholder: &'static str,
    obscure: bool,
    enabled: bool,
    padlock_prefix: bool,
    /// Upstream's `padding`; text_field.dart's `_kDefaultPadding` unless
    /// overridden (the PIN field's `EdgeInsets.symmetric(horizontal: 6,
    /// vertical: 12)`).
    padding: EdgeInsets,
    decoration: FieldDecoration,
    /// Where the inner field publishes its handle, so the clear button can
    /// reach the field's text -- the search field's arrangement in the tier.
    field_sink: Rc<RefCell<Option<StateHandle<TextFieldState>>>>,
}

impl DemoTextField {
    fn new(id: u64, placeholder: &'static str) -> DemoTextField {
        DemoTextField {
            id,
            placeholder,
            obscure: false,
            enabled: true,
            padlock_prefix: false,
            padding: EdgeInsets::all(7.0),
            decoration: FieldDecoration::Rounded,
            field_sink: Rc::new(RefCell::new(None)),
        }
    }

    /// Upstream's `obscureText: true`.
    fn obscured(mut self) -> Self {
        self.obscure = true;
        self
    }

    fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The PIN field's `prefix: const Icon(CupertinoIcons.padlock_solid,
    /// size: 28)`.
    fn with_padlock_prefix(mut self) -> Self {
        self.padlock_prefix = true;
        self
    }

    fn with_padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = padding;
        self
    }

    fn with_decoration(mut self, decoration: FieldDecoration) -> Self {
        self.decoration = decoration;
        self
    }
}

/// What a [`DemoTextField`] remembers: the text, mirrored from the inner
/// field so the clear button's visibility rebuilds with it (the tier's
/// search field keeps the same mirror).
#[derive(Default)]
struct DemoTextFieldState {
    text: String,
}

impl StatefulComponent for DemoTextField {
    type State = DemoTextFieldState;

    fn key(&self) -> Key {
        Some(self.id)
    }

    fn build(
        &self,
        state: &DemoTextFieldState,
        handle: StateHandle<DemoTextFieldState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let label = theme.resolve(CupertinoColors::LABEL);
        let placeholder_color = theme.resolve(CupertinoColors::PLACEHOLDER_TEXT);
        let item_color = theme.resolve(CupertinoColors::SECONDARY_LABEL);
        let background = theme.scaffold_background_color;
        let id = self.id;
        let padlock_prefix = self.padlock_prefix;

        // The row's contents: the optional padlock prefix, the field (or, for
        // the disabled field, its placeholder), and the clear button.
        let mut children: Vec<AnyWidget> = Vec::new();
        if padlock_prefix {
            children.push(leaf(move || PadlockGlyph {
                color: label,
                laid_out: Size::ZERO,
            }));
        }
        if self.enabled {
            let mut field = TextField::new(id)
                .with_placeholder(self.placeholder)
                // text_field.dart's default `style`: the Cupertino text
                // style. The placeholder's color is the field's own muted
                // color instead (see the module header).
                .with_style(theme.text_style())
                .with_state_sink(self.field_sink.clone())
                .with_on_changed({
                    let handle = handle.clone();
                    move |text| {
                        let mirrored = text.to_string();
                        handle.set_state(move |state| state.text = mirrored);
                    }
                });
            if self.obscure {
                field = field.obscured();
            }
            children.push(stateful(field));
        } else {
            // `enabled: false`: the decoration and the placeholder stay, the
            // input goes (see the module header).
            let placeholder = self.placeholder;
            let mut placeholder_style = theme.text_style();
            placeholder_style.color = placeholder_color;
            children.push(leaf(move || {
                Text::new(placeholder).with_style(placeholder_style.clone())
            }));
        }
        // `clearButtonMode: OverlayVisibilityMode.editing`: the clear button
        // once there is text. The tier's search field shows it on the same
        // condition.
        let show_clear = !state.text.is_empty() && self.enabled;
        if show_clear {
            children.push(leaf(move || ClearGlyph {
                color: item_color,
                background,
                laid_out: Size::ZERO,
            }));
        }

        let sink = self.field_sink.clone();
        let padding = self.padding;
        let decoration = self.decoration;
        let border_color = theme.resolve(FIELD_BORDER_COLOR);
        let inactive_gray = theme.resolve(CupertinoColors::INACTIVE_GRAY);

        many(children, move |rendered| {
            let mut rendered = rendered.into_iter();
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            if padlock_prefix {
                // The prefix butts against the text's start; a 6px gap keeps
                // the glyph off the first character the way the upstream
                // screenshot reads.
                if let Some(prefix) = rendered.next() {
                    row = row.push(prefix);
                    row = row.push(Container::new().with_size(6.0, 1.0));
                }
            }
            let field = rendered.next().expect("the field");
            row = row.push_flex(FlexChild::expanded(field, 1));
            if let Some(clear_mark) = rendered.next() {
                let sink = sink.clone();
                let clear_handle = handle.clone();
                // `_clearText`: empty the field through its own handle, which
                // also tells the IME, and empty the mirror.
                let clear = Pointer::new(id, clear_mark).with_handlers(
                    PointerHandlers::new().with_tap(move |_| {
                        if let Some(field_handle) = &*sink.borrow() {
                            field_handle.set_state(|state| state.clear());
                        }
                        clear_handle.set_state(|state| state.text.clear());
                    }),
                );
                row = row.push(clear);
            }
            let content = Container::new().with_padding(padding).with_child(row);
            let decorated: BoxedRender = match decoration {
                // `_kDefaultRoundedBorderDecoration`: hairline border
                // (upstream's width 0, one logical pixel here -- the
                // tier's hairline convention), radius 4.
                FieldDecoration::Rounded => RenderRef::new(
                    Container::new()
                        .with_border(1.0, border_color)
                        .with_corner_radius(4.0)
                        .with_child(content),
                ),
                FieldDecoration::BottomBorder => RenderRef::new(
                    RenderFlex::column()
                        .with_main_axis_size(MainAxisSize::Min)
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .push(content)
                        // The bottom `BorderSide(width: 0)` in inactiveGray,
                        // one logical pixel here.
                        .push(Container::new().with_height(1.0).with_color(inactive_gray)),
                ),
            };
            decorated
        })
    }
}

/// The PIN field's prefix, `CupertinoIcons.padlock_solid` at 28, drawn: a
/// filled body with a stroked shackle. With no icon font here (cupertino.rs's
/// module docs) the glyphs this demo needs are drawn, the way the tier draws
/// its marks.
struct PadlockGlyph {
    color: Color,
    laid_out: Size,
}

impl RenderBox for PadlockGlyph {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.laid_out = constraints.constrain(Size::new(28.0, 28.0));
        self.laid_out
    }

    fn size(&self) -> Size {
        self.laid_out
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let s = self.laid_out.width.min(self.laid_out.height);
        let x = |f: f32| offset.dx + f * s;
        let y = |f: f32| offset.dy + f * s;
        let canvas = context.canvas();
        // The shackle: the top half of a ring above the body.
        let shackle = Paint::new(self.color).with_style(Style::Stroke { width: s / 10.0 });
        canvas.draw_circle(x(0.5), y(0.36), 0.2 * s, &shackle);
        // The body covers the shackle's lower half, leaving the ring showing.
        canvas.draw_rounded_rect(
            Rect::ltrb(x(0.18), y(0.4), x(0.82), y(0.92)),
            0.12 * s,
            &Paint::new(self.color),
        );
    }

    fn hit_test_self(&self, _position: Offset) -> bool {
        true
    }
}

/// The clear mark, `CupertinoIcons.xmark_circle_fill`, drawn the way the
/// tier's search field draws it: a filled circle in the item color with the
/// cross knocked out in the field's background color.
struct ClearGlyph {
    color: Color,
    background: Color,
    laid_out: Size,
}

impl ClearGlyph {
    /// The mark's drawn size, the tier's `SEARCH_FIELD_ITEM_SIZE`.
    const SIZE: f32 = 20.0;
}

impl RenderBox for ClearGlyph {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.laid_out = constraints.constrain(Size::new(Self::SIZE, Self::SIZE));
        self.laid_out
    }

    fn size(&self) -> Size {
        self.laid_out
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let center = offset.dx + Self::SIZE / 2.0;
        let middle = offset.dy + Self::SIZE / 2.0;
        let canvas = context.canvas();
        canvas.draw_circle(center, middle, Self::SIZE / 2.0, &Paint::new(self.color));
        let cross = Paint::new(self.background)
            .with_style(Style::Stroke { width: 1.6 })
            .with_stroke_cap(StrokeCap::Round);
        let arm = 3.5;
        canvas.draw_line(
            (center - arm, middle - arm),
            (center + arm, middle + arm),
            &cross,
        );
        canvas.draw_line(
            (center - arm, middle + arm),
            (center + arm, middle - arm),
            &cross,
        );
    }

    fn hit_test_self(&self, _position: Offset) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fields_defaults_are_upstreams() {
        // `_kDefaultRoundedBorderDecoration` over `_kDefaultPadding` (7 on
        // every side), enabled and unobscured unless the demo says otherwise.
        let field = DemoTextField::new(EMAIL_FIELD, "Email");
        assert!(field.enabled);
        assert!(!field.obscure);
        assert!(!field.padlock_prefix);
        assert_eq!(field.decoration, FieldDecoration::Rounded);
        assert_eq!(field.padding, EdgeInsets::all(7.0));
    }

    #[test]
    fn the_pin_field_is_upstreams_configuration() {
        let field = DemoTextField::new(PIN_FIELD, "PIN")
            .with_padlock_prefix()
            .with_padding(EdgeInsets::symmetric(6.0, 12.0))
            .with_decoration(FieldDecoration::BottomBorder);
        assert!(field.padlock_prefix);
        assert_eq!(field.padding, EdgeInsets::symmetric(6.0, 12.0));
        assert_eq!(field.decoration, FieldDecoration::BottomBorder);
    }

    #[test]
    fn the_placeholders_are_upstreams_strings() {
        // `demoTextFieldEmail`, `rallyLoginPassword` and
        // `demoCupertinoTextFieldPIN` resolve to these in English.
        assert_eq!(
            DemoTextField::new(EMAIL_FIELD, "Email").placeholder,
            "Email"
        );
        assert_eq!(
            DemoTextField::new(PASSWORD_FIELD, "Password").placeholder,
            "Password"
        );
        assert_eq!(DemoTextField::new(PIN_FIELD, "PIN").placeholder, "PIN");
    }

    #[test]
    fn the_border_color_is_text_field_darts() {
        // `_kBorderColor`: 20% black in the light appearance, 20% white in
        // the dark one.
        assert_eq!(
            FIELD_BORDER_COLOR.resolve(Brightness::Light),
            Color(0x3300_0000)
        );
        assert_eq!(
            FIELD_BORDER_COLOR.resolve(Brightness::Dark),
            Color(0x33FF_FFFF)
        );
    }
}
