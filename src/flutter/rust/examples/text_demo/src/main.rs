// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! `flutter/textinput`, by hand.
//!
//! The fourth of the manual windows, and the one that needs a person most: a
//! keyboard is the piece of the platform that cannot be driven from inside the
//! process it is typing into. `platform_channels` drives one on Windows by
//! posting `WM_CHAR` at its own window, which Android will not let an
//! application do to itself -- see that example's `android.rs`. So this is
//! where typing gets checked on a touch screen: by typing.
//!
//! What to try:
//!
//! 1. **Tap the field.** The soft keyboard comes up, or on a desktop the caret
//!    starts blinking. That is `TextInput.setClient` followed by
//!    `TextInput.show`, and nothing in this file sends either.
//! 2. **Type.** Every keystroke crosses to the platform, edits the engine's own
//!    `TextInputModel` there, and comes back as `updateEditingState`. The
//!    counter below says how many times that round trip has happened.
//! 3. **Backspace, and move the caret.** Deleting is not "the text minus a
//!    character": it is `deleteSurroundingText` on Android and `VK_BACK` on
//!    Windows, and both land on the same model.
//! 4. **Type a character that is not ASCII** -- an emoji, or 中. It crosses as
//!    UTF-16, is held as UTF-8, and comes back with offsets counted in UTF-16
//!    code units. Three encodings, and the selection numbers below show whether
//!    the conversions agree.
//! 5. **Press done / enter.** `TextInputClient.performAction` arrives, and the
//!    submitted line is added to the list.
//! 6. **Tap the second field.** The first one's client is cleared and the
//!    second one's is set; the platform is told, which is what stops the
//!    keyboard editing a field that no longer has focus.
//!
//! There is no `TextInput`, no client id, no editing state and no IME anywhere
//! below. Adapting to those is the framework's job, and it is done once.

use std::cell::RefCell;
use std::os::raw::{c_char, c_int};
use std::rc::Rc;

use rustflutter::prelude::*;
use rustflutter::render::{
    BoxConstraints, CrossAxisAlignment, MainAxisSize, Offset, PaintContext, RenderBox,
    RenderConstrainedBox, RenderDecoratedBox, RenderFlex, RenderPadding, RenderStack, RenderWrap,
    Size, TextOverflow,
};

const WIDTH: i32 = 560;
const HEIGHT: i32 = 1080;

thread_local! {
    /// What each field holds, as the application hears it. Plain strings: the
    /// composing run, the selection and the client id are all below this.
    static TEXT: RefCell<[String; 2]> = RefCell::new([String::new(), String::new()]);
    /// How many times a field has reported a change. On screen because a round
    /// trip that produced the same text is still a round trip, and that is the
    /// half a person cannot otherwise tell from the half that is drawing.
    static CHANGES: RefCell<u32> = const { RefCell::new(0) };
    /// Everything that has been submitted, newest last.
    static SUBMITTED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

#[derive(Default)]
struct State {
    /// Bumped from the field callbacks. The cells above are the truth; this is
    /// only how the element tree hears about it.
    version: u32,
}

struct Page;

impl StatefulComponent for Page {
    type State = State;

    fn build(
        &self,
        _state: &State,
        handle: StateHandle<State>,
        _context: &mut rustflutter::framework::BuildContext,
    ) -> AnyWidget {
        let text = TEXT.with(|text| text.borrow().clone());
        let changes = CHANGES.with(|count| *count.borrow());
        let submitted = SUBMITTED.with(|list| list.borrow().clone());

        let mut children = vec![
            component(Label::title("flutter/textinput")),
            component(Label::muted(
                "tap a field and type. every keystroke goes to the platform and \
                 comes back.",
            )),
            gap(1.0),
            field(0, "your name", handle.clone()),
            component(Label::muted(format!(
                "   {} characters, {} UTF-16 units",
                text[0].chars().count(),
                text[0].encode_utf16().count()
            ))),
            gap(0.5),
            field(1, "and something with 中 or an emoji", handle),
            component(Label::muted(format!(
                "   {} characters, {} UTF-16 units",
                text[1].chars().count(),
                text[1].encode_utf16().count()
            ))),
            gap(1.0),
            component(Label::new(format!("changes reported: {changes}"))),
        ];

        if submitted.is_empty() {
            children.push(component(Label::muted(
                "nothing submitted yet -- press done or enter",
            )));
        } else {
            children.push(component(Label::new(format!(
                "submitted {} time(s):",
                submitted.len()
            ))));
            for line in submitted.iter().rev().take(4) {
                children.push(component(Label::muted(format!("   {line:?}"))));
            }
        }

        children.push(gap(1.0));
        children.push(component(Label::new(
            "and a line that does not fit, faded out at the edge rather than cut:",
        )));
        children.push(component(Faded(
            "this sentence runs on past the right edge of the window and \
             dissolves into it, rather than stopping at a hard clip",
        )));

        children.push(gap(1.0));
        children.push(component(Label::muted(
            "the character counts are the check worth watching: the text crosses \
             as UTF-16 and is held as UTF-8, so a wrong conversion shows up here \
             and nowhere else.",
        )));

        // -- Direction ------------------------------------------------------------
        //
        // The same start-aligned paragraph under each directionality. `start`
        // means the leading edge, so the two last lines land at opposite
        // edges of their boxes -- which is visible because a wrapped
        // paragraph's last line is the shorter one.
        children.push(gap(1.0));
        children.push(component(Label::new("text direction:")));
        children.push(component(Label::muted(
            "the same words, start-aligned, once under ltr directionality and \
             once under rtl:",
        )));
        children.push(directionality(
            TextDirection::Ltr,
            component(Directed(START_ALIGNED_TEXT, TextAlign::Start)),
        ));
        children.push(directionality(
            TextDirection::Rtl,
            component(Directed(START_ALIGNED_TEXT, TextAlign::Start)),
        ));
        // A genuinely right-to-left line of its own, under the same rtl
        // directionality.
        children.push(directionality(
            TextDirection::Rtl,
            component(Directed("مرحبا -- a right-to-left line of its own.", TextAlign::Start)),
        ));

        // -- Layout direction ----------------------------------------------------
        //
        // The same three layouts under each directionality, with the children
        // in the same order -- brightest swatch first -- so the check is where
        // each one lands: the row's and the wrap's first child at the right
        // under rtl, and the stack's overlay at the top right.
        children.push(gap(1.0));
        children.push(component(Label::new("layout direction:")));
        children.push(component(Label::muted(
            "a row, a wrap and a stack, laid out under ltr and then rtl. the \
             brightest swatch is the first child:",
        )));
        children.push(directionality(TextDirection::Ltr, component(DirectedRow)));
        children.push(directionality(TextDirection::Rtl, component(DirectedRow)));
        children.push(directionality(TextDirection::Ltr, component(DirectedWrap)));
        children.push(directionality(TextDirection::Rtl, component(DirectedWrap)));
        children.push(directionality(TextDirection::Ltr, component(DirectedStack)));
        children.push(directionality(TextDirection::Rtl, component(DirectedStack)));

        provide(Theme::dark(), page(children))
    }
}

/// The paragraph the direction comparison wraps, long enough to break across
/// two lines with the last one shorter -- alignment is only visible on a line
/// that does not fill its box.
const START_ALIGNED_TEXT: &str = "this paragraph is start-aligned: it begins at \
     the leading edge of its box, and which edge is leading is what the \
     directionality around it says";

/// One field. The whole of the application's side of this channel.
fn field(index: usize, placeholder: &str, handle: StateHandle<State>) -> AnyWidget {
    let changed = handle.clone();
    let submitted = handle;
    stateful(
        TextField::new(index as u64 + 1)
            .with_placeholder(placeholder)
            .with_on_changed(move |text| {
                let text = text.to_string();
                TEXT.with(|held| {
                    let mut held = held.borrow_mut();
                    if held[index] == text {
                        return;
                    }
                    held[index] = text;
                    CHANGES.with(|count| *count.borrow_mut() += 1);
                });
                changed.set_state(|state| state.version += 1);
            })
            .with_on_submitted(move |text| {
                SUBMITTED.with(|list| list.borrow_mut().push(text.to_string()));
                submitted.set_state(|state| state.version += 1);
            }),
    )
}

/// One line that is let run past its box and faded out at the edge, upstream's
/// `TextOverflow.fade`: the paragraph is drawn into an offscreen layer and a
/// transparent-to-opaque gradient is multiplied over the last little of it.
struct Faded(&'static str);

impl Component for Faded {
    fn build(&self, context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let style = theme_of(context).body();
        let text = self.0;
        // No wrapping, so the line overflows the width it is given and the
        // fade has something to fade. Upstream's marquee-and-fade pattern is
        // the same two settings.
        leaf(move || {
            Text::new(text).with_style(style.clone()).with_soft_wrap(false).with_overflow(
                TextOverflow::Fade,
            )
        })
    }
}

/// One paragraph of the direction comparison, in the body style, aligned as
/// asked. Which direction it is shaped in comes from the `directionality`
/// this is wrapped in -- see [`DirectedParagraph`].
struct Directed(&'static str, TextAlign);

impl Component for Directed {
    fn build(&self, context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let mut style = theme_of(context).body();
        style.align = self.1;
        let text = self.0;
        leaf(move || DirectedParagraph::new(text, style.clone()))
    }
}

/// A paragraph that takes its direction where it was built, and shapes with it.
///
/// The render-tree half of directionality is not in `RenderParagraph` yet --
/// it takes the text scale at construction exactly this way (a field read
/// from the walk's thread-local, used at layout) but not a direction -- so
/// the comparison page carries a leaf that does the taking itself. The leaf's
/// closure runs while the render walk is inside this paragraph's
/// `directionality`, which is what makes `current_direction()` there the
/// right answer. Once `RenderParagraph` grows the same field, this object
/// goes and the page uses ordinary `Text`s.
struct DirectedParagraph {
    text: String,
    style: TextStyle,
    direction: TextDirection,
    paragraph: Option<Rc<rustflutter::engine::Paragraph>>,
    size: Size,
}

impl DirectedParagraph {
    fn new(text: &str, style: TextStyle) -> DirectedParagraph {
        DirectedParagraph {
            direction: current_direction(),
            text: text.to_string(),
            style,
            paragraph: None,
            size: Size::ZERO,
        }
    }
}

impl RenderBox for DirectedParagraph {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        // Bounded means the caller's width is a width the wrapping respects;
        // unbounded would be one line, which alignment cannot show.
        let width = if constraints.has_bounded_width() {
            constraints.max_width
        } else {
            f32::MAX / 4.0
        };
        let paragraph = rustflutter::engine::Paragraph::new(
            &self.text,
            &self.style,
            None,
            false,
            width,
            self.direction,
        );
        // `Paragraph::new` re-lays out at the ink width, so `width()` is the
        // tight box around the glyphs -- the same convention
        // `RenderParagraph` relies on.
        self.size =
            constraints.constrain(Size::new(paragraph.width(), paragraph.height()));
        self.paragraph = Some(Rc::new(paragraph));
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        if let Some(paragraph) = &self.paragraph {
            context.canvas().draw_paragraph(paragraph, offset.dx, offset.dy);
        }
    }
}

// -- The layout direction comparison --------------------------------------

/// The three swatches every layout below lays out, brightest first. Which end
/// the bright one lands at is the whole of what the comparison shows.
const SHADES: [Color; 3] = [
    Color::rgb(0xE8, 0xB0, 0x4C),
    Color::rgb(0x8A, 0x6E, 0x3E),
    Color::rgb(0x4A, 0x3C, 0x26),
];

/// One rounded swatch of a fixed size.
fn swatch(color: Color) -> RenderConstrainedBox {
    RenderConstrainedBox::tight(44.0, 26.0)
        .with_child(RenderDecoratedBox::new().with_color(color).with_corner_radius(4.0))
}

/// A row of the swatches, laid out by a flex that takes its reading direction
/// at construction -- and the leaf's closure runs while the walk is inside the
/// `directionality`, so that construction is under the right one.
struct DirectedRow;

impl Component for DirectedRow {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        leaf(|| {
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.0);
            for shade in SHADES {
                row = row.push(swatch(shade));
            }
            row
        })
    }
}

/// The swatches in a wrap 132 wide, so they break 2-and-1 into two lines:
/// under rtl each line reads right to left, and the lines still stack
/// downwards.
struct DirectedWrap;

impl Component for DirectedWrap {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        leaf(|| {
            let mut wrap = RenderWrap::horizontal().with_spacing(8.0);
            for shade in SHADES {
                wrap = wrap.push(swatch(shade));
            }
            // Two swatches and the gap come to 96; the third would make 148,
            // so this width is what makes the wrap wrap.
            RenderConstrainedBox::new(BoxConstraints::new(0.0, 132.0, 0.0, f32::INFINITY))
                .with_child(wrap)
        })
    }
}

/// A dim backdrop with one bright swatch over it, unpositioned, so it sits
/// where the stack's default `topStart` alignment says -- top left in ltr, top
/// right in rtl.
struct DirectedStack;

impl Component for DirectedStack {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        leaf(|| {
            RenderStack::new()
                .push(RenderConstrainedBox::tight(132.0, 40.0).with_child(
                    RenderDecoratedBox::new()
                        .with_color(Color::rgb(0x1C, 0x24, 0x33))
                        .with_corner_radius(4.0),
                ))
                .push(swatch(SHADES[0]))
        })
    }
}

/// A padded, left-aligned column.
fn page(children: Vec<AnyWidget>) -> AnyWidget {
    many(children, |rendered| {
        let mut column = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(8.0);
        for child in rendered {
            column = column.push(child);
        }
        Box::new(RenderPadding::new(EdgeInsets::all(24.0), column))
    })
}

struct TextApp;

impl WidgetApplication for TextApp {
    fn background(&self) -> Color {
        Theme::dark().background
    }

    fn build(&mut self, _context: &BuildContext) -> AnyWidget {
        component(SafeArea::new(stateful(Page)))
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rustflutter_app_main(argc: c_int, argv: *const *const c_char) -> c_int {
    // `--png <path>` renders one frame headlessly, the way the gallery's own
    // `--png` does, so the fade line below can be checked without a person
    // typing into a window.
    let args = collect_args(argc, argv);
    let png = args
        .iter()
        .position(|a| a == "--png")
        .and_then(|index| args.get(index + 1).cloned());
    if let Some(path) = png {
        return render_png(&path);
    }

    register_application(|| Box::new(WidgetHost::new(TextApp)));
    let options = RunOptions {
        width: WIDTH,
        height: HEIGHT,
        title: String::from("Text input - rustflutter"),
        ..RunOptions::default()
    };
    match run(&options) {
        Ok(()) => 0,
        Err(code) => code,
    }
}

/// Renders the first frame to `path`, through the same composition the
/// windowed path uses.
fn render_png(path: &str) -> c_int {
    rustflutter::engine::initialize();
    let screen = || provide(Theme::dark(), component(SafeArea::new(stateful(Page))));
    let mut tree = rustflutter::framework::ElementTree::new();
    tree.rebuild(screen());
    let mut root = tree.build_render_tree().expect("the tree has a root");
    root.layout(BoxConstraints::tight(WIDTH as f32, HEIGHT as f32));
    let mut layers = rustflutter::app::compose_frame(
        WIDTH,
        HEIGHT,
        1.0,
        rustflutter::render::Size::new(WIDTH as f32, HEIGHT as f32),
        Theme::dark().background,
        |context| root.paint(context, rustflutter::render::Offset::ZERO),
    );
    match layers.write_png(std::path::Path::new(path)) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("text_demo: render failed: {err}");
            1
        }
    }
}

/// The command line as strings, for the flags above.
fn collect_args(argc: c_int, argv: *const *const c_char) -> Vec<String> {
    if argv.is_null() {
        return Vec::new();
    }
    (0..argc as usize)
        .map(|index| unsafe {
            std::ffi::CStr::from_ptr(*argv.add(index)).to_string_lossy().into_owned()
        })
        .collect()
}
