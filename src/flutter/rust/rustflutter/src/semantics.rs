// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! What the screen says, for a reader who is not looking at it.
//!
//! Everything else in this framework describes pixels. This describes the same
//! interface again, in the only terms a screen reader can use: here is a
//! button, it says "Increment", tapping it does something, and it is at these
//! coordinates so a finger dragged across the glass can find it. Upstream this
//! is `semantics/semantics.dart` and the `Semantics` widget over it; the shape
//! here is the same and the vocabulary is the same, because the vocabulary is
//! not ours -- it is what `SemanticsNode` carries across to the platform, and
//! from there what TalkBack and Narrator understand.
//!
//! # Why it is off until it is asked for
//!
//! Nothing is built unless the platform says a screen reader is listening.
//! Upstream does the same (`PlatformView::SetSemanticsEnabled`, reaching
//! `SemanticsBinding.semanticsEnabled`), and the reason is not only cost: a
//! semantics tree that nothing consumes cannot be wrong in any way anybody
//! notices, so it would rot. Built only when it is read, it is read.
//!
//! # Where the rectangles come from
//!
//! A node needs a rectangle in root coordinates, and a render object only
//! learns where it is when it is painted -- paint is the walk that carries the
//! offset. So [`RenderSemantics`] collects during paint: it pushes itself on
//! the way down and pops on the way up, so the nesting of the resulting tree is
//! the nesting of the paint, which is the order a reader should meet things in.
//! Upstream compiles the tree in its own walk over the render tree, which it
//! can do because a `RenderObject` knows its parent; here nothing does, and
//! paint is the walk that has the answer anyway.

use std::cell::RefCell;
use std::rc::Rc;

use crate::framework::{AnyWidget, BuildContext, Component, component, single};
use crate::render::{BoxConstraints, BoxedRender, Offset, PaintContext, RenderBox, Size};

/// What a reader can be told to do with a node.
///
/// The discriminants are `flutter::SemanticsAction`, which is in turn
/// `SemanticsAction` in `semantics.dart` and in every embedder. Four copies of
/// one set of bits upstream; this is the fifth, and it has to match.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum SemanticsAction {
    Tap = 1 << 0,
    LongPress = 1 << 1,
    ScrollLeft = 1 << 2,
    ScrollRight = 1 << 3,
    ScrollUp = 1 << 4,
    ScrollDown = 1 << 5,
    Increase = 1 << 6,
    Decrease = 1 << 7,
    ShowOnScreen = 1 << 8,
    DidGainAccessibilityFocus = 1 << 15,
    DidLoseAccessibilityFocus = 1 << 16,
    Dismiss = 1 << 18,
    Focus = 1 << 22,
}

impl SemanticsAction {
    /// The action a bit stands for, or `None` for one this framework has no
    /// name for yet.
    pub fn from_bits(bits: i32) -> Option<SemanticsAction> {
        use SemanticsAction::*;
        Some(match bits {
            x if x == Tap as i32 => Tap,
            x if x == LongPress as i32 => LongPress,
            x if x == ScrollLeft as i32 => ScrollLeft,
            x if x == ScrollRight as i32 => ScrollRight,
            x if x == ScrollUp as i32 => ScrollUp,
            x if x == ScrollDown as i32 => ScrollDown,
            x if x == Increase as i32 => Increase,
            x if x == Decrease as i32 => Decrease,
            x if x == ShowOnScreen as i32 => ShowOnScreen,
            x if x == DidGainAccessibilityFocus as i32 => DidGainAccessibilityFocus,
            x if x == DidLoseAccessibilityFocus as i32 => DidLoseAccessibilityFocus,
            x if x == Dismiss as i32 => Dismiss,
            x if x == Focus as i32 => Focus,
            _ => return None,
        })
    }
}

/// What a node *is*, as opposed to what can be done to it.
///
/// A subset of upstream's `SemanticsFlags`: the ones that change what a screen
/// reader says out loud rather than how a particular platform arranges its
/// accessibility tree. Adding one is adding a field here, a bit in the C
/// struct, and a line in whichever bridge cares.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SemanticsFlags {
    pub is_button: bool,
    pub is_text_field: bool,
    pub is_header: bool,
    pub is_image: bool,
    pub is_link: bool,
    pub is_slider: bool,
    pub is_obscured: bool,
    pub is_read_only: bool,
    pub is_live_region: bool,
    /// Whether this node can be checked at all -- a switch or a checkbox has
    /// this even when it is off, which is what makes "off" sayable.
    pub has_checked_state: bool,
    pub is_checked: bool,
    pub has_enabled_state: bool,
    pub is_enabled: bool,
    pub is_selected: bool,
    /// Whether the keyboard is here. Separate from the framework's own focus
    /// because accessibility focus and keyboard focus are different things
    /// that happen to coincide most of the time.
    pub is_focused: bool,
}

/// Everything said about one thing on screen.
///
/// Upstream's `SemanticsProperties`, narrowed to what the bridges below
/// actually deliver.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SemanticsProperties {
    /// What it is called. The first thing read out.
    pub label: String,
    /// What it currently says -- a field's text, a slider's number.
    pub value: String,
    /// What it is for, read after the label when a reader asks for more.
    pub hint: String,
    /// What the value would become if increased or decreased.
    pub increased_value: String,
    pub decreased_value: String,
    pub flags: SemanticsFlags,
    /// The actions this node accepts, as a bit set.
    pub actions: i32,
    /// Where the reader is inside a scrollable, for the "row 3 of 40" a screen
    /// reader announces. `NaN` for a node that does not scroll, which is what
    /// upstream uses for the same "no answer".
    pub scroll_position: f32,
    pub scroll_extent_max: f32,
    pub scroll_extent_min: f32,
}

impl SemanticsProperties {
    pub fn label(text: impl Into<String>) -> SemanticsProperties {
        SemanticsProperties {
            label: text.into(),
            scroll_position: f32::NAN,
            scroll_extent_max: f32::NAN,
            scroll_extent_min: f32::NAN,
            ..SemanticsProperties::default()
        }
    }

    /// Whether this node accepts `action`.
    pub fn has(&self, action: SemanticsAction) -> bool {
        self.actions & action as i32 != 0
    }

    pub fn with_action(mut self, action: SemanticsAction) -> Self {
        self.actions |= action as i32;
        self
    }
}

/// One node of the tree that goes to the platform.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticsNode {
    /// Stable for as long as the widget that produced it keeps the same
    /// identifier. The platform keys its own accessibility nodes on this, and
    /// a node whose id changed is, to a screen reader, a different thing
    /// appearing where the old one was -- so it re-reads it.
    pub id: i32,
    pub properties: SemanticsProperties,
    /// In root coordinates, which is what every bridge below wants: the
    /// platform asks "what is at this point on the glass".
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    /// In paint order, which is reading order.
    pub children: Vec<i32>,
}

impl SemanticsNode {
    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }
}

// -- The frame's collection ---------------------------------------------------

#[derive(Default)]
struct Collector {
    /// Whether anything is listening. Nothing is collected otherwise.
    enabled: bool,
    /// Whether a collection is running right now, so `RenderSemantics::paint`
    /// knows to record rather than just paint.
    collecting: bool,
    nodes: Vec<SemanticsNode>,
    /// Indices into `nodes` for the annotations currently open, outermost
    /// last. This is what turns the paint recursion into a tree.
    open: Vec<usize>,
    /// Handlers for the actions the last collected tree offered, by node id.
    /// Kept between frames, because an action arrives from the platform
    /// whenever the reader gets round to it.
    handlers: Vec<(i32, ActionHandler)>,
    /// How many labelled annotations are open above the paint in progress.
    ///
    /// Text inside one of those is *what the annotation says*, and reading it
    /// again as its own node would make a screen reader say a button's name
    /// twice. Upstream reaches the same place with `excludeSemantics` and
    /// `MergeSemantics`; the rule here is the common case of both -- a node
    /// that gave itself a label speaks for everything under it.
    labelled_depth: usize,
    /// Where automatic ids for text nodes are handed out from.
    next_text_id: i32,
}

type ActionHandler = Rc<dyn Fn(SemanticsAction)>;

thread_local! {
    static COLLECTOR: RefCell<Collector> = RefCell::new(Collector::default());
}

/// Whether the platform has said a screen reader is listening.
pub fn enabled() -> bool {
    COLLECTOR.with(|collector| collector.borrow().enabled)
}

/// Turns semantics on or off. Called by the shell when an assistive technology
/// arrives or leaves.
pub fn set_enabled(on: bool) {
    COLLECTOR.with(|collector| {
        let mut collector = collector.borrow_mut();
        collector.enabled = on;
        if !on {
            collector.nodes.clear();
            collector.handlers.clear();
        }
    });
}

/// Runs `paint` with collection turned on, and returns the tree it produced.
///
/// Returns `None` when nothing is listening, which is the ordinary case and
/// costs one boolean.
pub fn collect(paint: impl FnOnce()) -> Option<Vec<SemanticsNode>> {
    if !enabled() {
        return None;
    }
    COLLECTOR.with(|collector| {
        let mut collector = collector.borrow_mut();
        collector.collecting = true;
        collector.nodes.clear();
        collector.open.clear();
        collector.handlers.clear();
        collector.labelled_depth = 0;
    });
    paint();
    COLLECTOR.with(|collector| {
        let mut collector = collector.borrow_mut();
        collector.collecting = false;
        collector.open.clear();
        collector.labelled_depth = 0;
        Some(collector.nodes.clone())
    })
}

/// Delivers an action the platform asked for.
///
/// Returns whether anything took it. Upstream this is
/// `SemanticsOwner.performAction`, and the same rule applies: an action for a
/// node that has since gone is not an error, it is a race with the reader.
pub fn perform_action(node_id: i32, action: SemanticsAction) -> bool {
    let handler = COLLECTOR.with(|collector| {
        collector
            .borrow()
            .handlers
            .iter()
            .find(|(id, _)| *id == node_id)
            .map(|(_, handler)| Rc::clone(handler))
    });
    match handler {
        Some(handler) => {
            handler(action);
            true
        }
        None => false,
    }
}

/// Whether a paint in progress is inside something that already has a label.
pub(crate) fn inside_labelled() -> bool {
    COLLECTOR.with(|collector| {
        let collector = collector.borrow();
        collector.collecting && collector.labelled_depth > 0
    })
}

/// Whether a collection is running right now.
pub(crate) fn collecting() -> bool {
    COLLECTOR.with(|collector| collector.borrow().collecting)
}

/// Hands out an identifier for a node that has none of its own.
///
/// Text is the case: a paragraph is a render object built inside a closure and
/// has no identifier anybody chose. Because render objects now outlive the
/// frame, an id taken once is stable for as long as the paragraph is -- which
/// is exactly as long as a screen reader should go on believing it is the same
/// thing.
pub(crate) fn take_text_id() -> i32 {
    COLLECTOR.with(|collector| {
        let mut collector = collector.borrow_mut();
        let id = TEXT_BASE.wrapping_add(collector.next_text_id);
        collector.next_text_id = collector.next_text_id.wrapping_add(1) & (TEXT_BASE - 1);
        id
    })
}

/// Where text node ids start. The third of the three ranges; see [`AUTO_BASE`].
const TEXT_BASE: i32 = 2 << 28;

/// Records a paragraph as a node of its own, for text nobody annotated.
///
/// Called from `RenderParagraph::paint`. Suppressed inside anything that gave
/// itself a label, because there the text *is* what the label says and reading
/// it twice is worse than not reading it at all.
pub(crate) fn describe_text(id: i32, text: &str, rect: (f32, f32, f32, f32)) {
    if text.trim().is_empty() || inside_labelled() {
        return;
    }
    if let Some(index) = open(id, SemanticsProperties::label(text), rect) {
        close(index);
    }
}

/// Opens a node during paint, returning its index.
fn open(id: i32, properties: SemanticsProperties, rect: (f32, f32, f32, f32)) -> Option<usize> {
    COLLECTOR.with(|collector| {
        let mut collector = collector.borrow_mut();
        if !collector.collecting {
            return None;
        }
        let index = collector.nodes.len();
        collector.nodes.push(SemanticsNode {
            id,
            properties,
            left: rect.0,
            top: rect.1,
            right: rect.2,
            bottom: rect.3,
            children: Vec::new(),
        });
        if let Some(parent) = collector.open.last().copied() {
            collector.nodes[parent].children.push(id);
        }
        collector.open.push(index);
        if !collector.nodes[index].properties.label.is_empty() {
            collector.labelled_depth += 1;
        }
        Some(index)
    })
}

fn close(index: usize) {
    COLLECTOR.with(|collector| {
        let mut collector = collector.borrow_mut();
        if !collector.nodes[index].properties.label.is_empty() {
            collector.labelled_depth = collector.labelled_depth.saturating_sub(1);
        }
        // Closes down to and including `index`. A child that opened and never
        // closed -- a paint that unwound -- would otherwise leave the stack
        // wrong for everything after it.
        while let Some(top) = collector.open.pop() {
            if top == index {
                break;
            }
        }
    });
}

fn remember_handler(id: i32, handler: ActionHandler) {
    COLLECTOR.with(|collector| {
        let mut collector = collector.borrow_mut();
        if !collector.collecting {
            return;
        }
        collector.handlers.push((id, handler));
    });
}

// -- The render object --------------------------------------------------------

/// Annotates its child, and reports where the child ended up.
///
/// Upstream's `RenderSemanticsAnnotations`. It draws nothing and changes no
/// layout: it is the same box as its child, with something said about it.
pub struct RenderSemantics {
    id: i32,
    properties: SemanticsProperties,
    on_action: Option<ActionHandler>,
    child: BoxedRender,
    size: Size,
}

impl RenderSemantics {
    pub fn new(
        id: i32,
        properties: SemanticsProperties,
        child: impl RenderBox + 'static,
    ) -> RenderSemantics {
        RenderSemantics {
            id,
            properties,
            on_action: None,
            child: crate::render::RenderRef::new(child),
            size: Size::ZERO,
        }
    }

    pub fn with_on_action(mut self, handler: impl Fn(SemanticsAction) + 'static) -> Self {
        self.on_action = Some(Rc::new(handler));
        self
    }
}

impl RenderBox for RenderSemantics {
    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.child.layout(constraints);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        let opened = open(
            self.id,
            self.properties.clone(),
            (
                offset.dx,
                offset.dy,
                offset.dx + self.size.width,
                offset.dy + self.size.height,
            ),
        );
        if opened.is_some() {
            if let Some(handler) = &self.on_action {
                remember_handler(self.id, Rc::clone(handler));
            }
        }
        context.paint_child(&self.child, offset);
        if let Some(index) = opened {
            close(index);
        }
    }

    fn hit_test(&self, position: Offset, result: &mut crate::render::HitTestResult) -> bool {
        self.child.hit_test(position, result)
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.child.min_intrinsic_width(height)
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.child.max_intrinsic_width(height)
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.child.min_intrinsic_height(width)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.child.max_intrinsic_height(width)
    }

    fn distance_to_baseline(&self) -> Option<f32> {
        self.child.distance_to_baseline()
    }
}

// -- The widget ---------------------------------------------------------------

/// Says something about a subtree, for a reader who cannot see it.
///
/// ```ignore
/// semantics(ID_INCREMENT, SemanticsProperties::button("Increment"), button)
///     .on_action(|_| increment())
/// ```
pub struct Semantics {
    id: i32,
    properties: SemanticsProperties,
    on_action: Option<ActionHandler>,
    child: RefCell<Option<AnyWidget>>,
}

impl Semantics {
    pub fn new(id: i32, properties: SemanticsProperties, child: AnyWidget) -> Semantics {
        Semantics { id, properties, on_action: None, child: RefCell::new(Some(child)) }
    }

    /// What to do when the reader activates this node.
    pub fn with_on_action(mut self, handler: impl Fn(SemanticsAction) + 'static) -> Self {
        self.on_action = Some(Rc::new(handler));
        self
    }

    /// Builds the widget. Not a `Component`, because there is nothing to build
    /// -- the annotation is the render object.
    pub fn build(self) -> AnyWidget {
        let child = self
            .child
            .borrow_mut()
            .take()
            .unwrap_or_else(|| crate::framework::leaf(|| crate::widgets::Empty));
        let id = self.id;
        let properties = self.properties;
        let handler = self.on_action;
        single(child, move |child| {
            let mut render = RenderSemantics::new(id, properties.clone(), child);
            if let Some(handler) = &handler {
                let handler = Rc::clone(handler);
                render = render.with_on_action(move |action| handler(action));
            }
            render
        })
    }
}

impl SemanticsProperties {
    /// A thing that can be pressed.
    pub fn button(label: impl Into<String>) -> SemanticsProperties {
        SemanticsProperties {
            flags: SemanticsFlags {
                is_button: true,
                has_enabled_state: true,
                is_enabled: true,
                ..SemanticsFlags::default()
            },
            ..SemanticsProperties::label(label)
        }
        .with_action(SemanticsAction::Tap)
    }

    /// A thing that can be pressed but currently cannot.
    pub fn disabled_button(label: impl Into<String>) -> SemanticsProperties {
        SemanticsProperties {
            flags: SemanticsFlags {
                is_button: true,
                has_enabled_state: true,
                is_enabled: false,
                ..SemanticsFlags::default()
            },
            ..SemanticsProperties::label(label)
        }
    }

    /// A thing with two states, like a switch or a checkbox.
    pub fn toggle(label: impl Into<String>, on: bool) -> SemanticsProperties {
        SemanticsProperties {
            flags: SemanticsFlags {
                has_checked_state: true,
                is_checked: on,
                has_enabled_state: true,
                is_enabled: true,
                ..SemanticsFlags::default()
            },
            ..SemanticsProperties::label(label)
        }
        .with_action(SemanticsAction::Tap)
    }

    /// A place text is typed.
    pub fn text_field(label: impl Into<String>, text: impl Into<String>) -> SemanticsProperties {
        SemanticsProperties {
            value: text.into(),
            flags: SemanticsFlags { is_text_field: true, ..SemanticsFlags::default() },
            ..SemanticsProperties::label(label)
        }
        .with_action(SemanticsAction::Tap)
    }

    /// A heading. Screen readers let a reader jump between these.
    pub fn header(label: impl Into<String>) -> SemanticsProperties {
        SemanticsProperties {
            flags: SemanticsFlags { is_header: true, ..SemanticsFlags::default() },
            ..SemanticsProperties::label(label)
        }
    }

    /// Something that scrolls, and how far down it is.
    pub fn scrollable(offset: f32, min: f32, max: f32, vertical: bool) -> SemanticsProperties {
        let actions = if vertical {
            SemanticsAction::ScrollUp as i32 | SemanticsAction::ScrollDown as i32
        } else {
            SemanticsAction::ScrollLeft as i32 | SemanticsAction::ScrollRight as i32
        };
        SemanticsProperties {
            actions,
            scroll_position: offset,
            scroll_extent_min: min,
            scroll_extent_max: max,
            ..SemanticsProperties::default()
        }
    }
}

/// Where automatically-allocated node ids start.
///
/// There are two sources of identity here and they must not meet: an
/// identifier the caller already had (a hit-test id, so that the two answers
/// to "which control is this" agree), and one invented for a widget that has
/// none. Upstream has only the second, because there a semantics node is
/// always allocated by the framework. Splitting the range is what makes both
/// possible: below the base is the caller's, at or above it is ours.
const AUTO_BASE: i32 = 1 << 28;

/// A node id for a caller's identifier.
///
/// Folded into the low range rather than truncated, so a caller who chose a
/// large id -- the examples hand out blocks at `1 << 40` -- still lands
/// somewhere that cannot be mistaken for an automatic one.
pub fn node_id_for(caller: u64) -> i32 {
    (caller % AUTO_BASE as u64) as i32
}

/// [`Semantics`] as a widget.
pub fn semantics(id: i32, properties: SemanticsProperties, child: AnyWidget) -> AnyWidget {
    Semantics::new(id, properties, child).build()
}

/// Annotates a subtree without the caller having to invent an identifier.
///
/// The element's own id is used, which is stable for exactly as long as the
/// widget keeps its place in the tree -- and that is the right lifetime: a
/// node that moved somewhere else in the tree *is* a different thing to a
/// screen reader, and one that merely rebuilt is not. Upstream reaches the
/// same stability from its persistent semantics tree.
///
/// Prefer [`semantics`] where an identifier already exists -- every component
/// that can be tapped already has one for hit testing, and reusing it keeps
/// the two answers to "which thing is this" in agreement.
pub struct AutoSemantics {
    properties: SemanticsProperties,
    on_action: Option<ActionHandler>,
    child: RefCell<Option<AnyWidget>>,
}

impl Component for AutoSemantics {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let child = self
            .child
            .borrow_mut()
            .take()
            .unwrap_or_else(|| crate::framework::leaf(|| crate::widgets::Empty));
        let mut annotation = Semantics::new(
            AUTO_BASE.wrapping_add(context.element().index() as i32),
            self.properties.clone(),
            child,
        );
        annotation.on_action = self.on_action.clone();
        annotation.build()
    }
}

/// [`AutoSemantics`] as a widget.
pub fn describe(properties: SemanticsProperties, child: AnyWidget) -> AnyWidget {
    component(AutoSemantics { properties, on_action: None, child: RefCell::new(Some(child)) })
}

/// [`AutoSemantics`] with an action handler.
pub fn describe_with_action(
    properties: SemanticsProperties,
    child: AnyWidget,
    handler: impl Fn(SemanticsAction) + 'static,
) -> AnyWidget {
    component(AutoSemantics {
        properties,
        on_action: Some(Rc::new(handler)),
        child: RefCell::new(Some(child)),
    })
}

/// [`Semantics`] with an action handler.
pub fn semantics_with_action(
    id: i32,
    properties: SemanticsProperties,
    child: AnyWidget,
    handler: impl Fn(SemanticsAction) + 'static,
) -> AnyWidget {
    Semantics::new(id, properties, child).with_on_action(handler).build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{ElementTree, leaf, many};
    use crate::render::{RenderFlex, RenderPadding, EdgeInsets};
    use crate::widgets::SizedBox;
    use std::cell::Cell;

    /// Lays out and paints a tree, and returns what the paint said about it.
    fn describe_tree(widget: AnyWidget, size: Size) -> Vec<SemanticsNode> {
        let mut tree = ElementTree::new();
        tree.rebuild(widget);
        let mut root = tree.build_render_tree().expect("a tree was mounted");
        // Loose, so a child that asked for a size keeps it: a tight box
        // would stretch the very thing whose rectangle is under test.
        root.layout(BoxConstraints::loose(size.width, size.height));
        let mut layers =
            crate::engine::LayerTree::new(size.width as i32, size.height as i32);
        collect(|| {
            let mut context = PaintContext::new(&mut layers, size);
            root.paint(&mut context, Offset::ZERO);
        })
        .expect("semantics are on")
    }

    #[test]
    fn nothing_is_collected_until_something_asks() {
        set_enabled(false);
        let collected = collect(|| unreachable!("paint should not even run"));
        assert!(collected.is_none(), "a tree nobody reads should not be built");
    }

    #[test]
    fn an_annotation_reports_where_its_child_ended_up() {
        set_enabled(true);
        let nodes = describe_tree(
            single(
                semantics(
                    7,
                    SemanticsProperties::button("Increment"),
                    leaf(|| SizedBox::new(80.0, 40.0)),
                ),
                |child| RenderPadding::new(EdgeInsets::all(10.0), child),
            ),
            Size::new(200.0, 100.0),
        );
        set_enabled(false);

        assert_eq!(nodes.len(), 1);
        let node = &nodes[0];
        assert_eq!(node.id, 7);
        assert_eq!(node.properties.label, "Increment");
        assert!(node.properties.flags.is_button);
        assert!(node.properties.has(SemanticsAction::Tap));
        // In root coordinates, past the padding: what the platform asks for is
        // where on the glass this is, not where inside its parent.
        assert_eq!((node.left, node.top), (10.0, 10.0));
        assert_eq!((node.width(), node.height()), (80.0, 40.0));
    }

    #[test]
    fn nesting_follows_the_paint() {
        set_enabled(true);
        let nodes = describe_tree(
            semantics(
                1,
                SemanticsProperties::label("a list"),
                many(
                    vec![
                        semantics(2, SemanticsProperties::label("first"), leaf(|| SizedBox::new(50.0, 20.0))),
                        semantics(3, SemanticsProperties::label("second"), leaf(|| SizedBox::new(50.0, 20.0))),
                    ],
                    |children| {
                        let mut flex = RenderFlex::column();
                        for child in children {
                            flex = flex.push(child);
                        }
                        flex
                    },
                ),
            ),
            Size::new(200.0, 100.0),
        );
        set_enabled(false);

        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].id, 1);
        assert_eq!(nodes[0].children, vec![2, 3], "reading order is paint order");
        assert!(nodes[1].children.is_empty());
        // The second row is below the first, which is the whole reason a
        // rectangle is worth carrying: a finger dragged down the glass meets
        // them in this order.
        assert!(nodes[2].top >= nodes[1].bottom);
    }

    #[test]
    fn an_action_reaches_the_widget_that_offered_it() {
        set_enabled(true);
        let taps = Rc::new(Cell::new(0));
        let counted = Rc::clone(&taps);
        let _ = describe_tree(
            semantics_with_action(
                4,
                SemanticsProperties::button("Increment"),
                leaf(|| SizedBox::new(50.0, 20.0)),
                move |action| {
                    if action == SemanticsAction::Tap {
                        counted.set(counted.get() + 1);
                    }
                },
            ),
            Size::new(200.0, 100.0),
        );

        assert!(perform_action(4, SemanticsAction::Tap));
        assert_eq!(taps.get(), 1);
        assert!(!perform_action(99, SemanticsAction::Tap), "no such node");
        set_enabled(false);
    }

    #[test]
    fn a_toggle_says_which_way_it_is() {
        let off = SemanticsProperties::toggle("Notifications", false);
        let on = SemanticsProperties::toggle("Notifications", true);
        // Having the state is what makes "off" sayable at all; a node without
        // it is just a label, and a reader is told nothing.
        assert!(off.flags.has_checked_state && !off.flags.is_checked);
        assert!(on.flags.has_checked_state && on.flags.is_checked);
        assert!(off.has(SemanticsAction::Tap));
    }

    #[test]
    fn a_scrollable_says_how_far_down_it_is() {
        let scroller = SemanticsProperties::scrollable(120.0, 0.0, 900.0, true);
        assert!(scroller.has(SemanticsAction::ScrollUp));
        assert!(scroller.has(SemanticsAction::ScrollDown));
        assert!(!scroller.has(SemanticsAction::ScrollLeft));
        assert_eq!(scroller.scroll_position, 120.0);
    }

    #[test]
    fn a_screen_of_ordinary_components_describes_itself() {
        // Nothing below asks for semantics. The point of wiring the built-in
        // components is that an application gets this without knowing it did.
        use crate::components::{Button, Label, Switch, stack_column};
        use crate::framework::component;

        set_enabled(true);
        let nodes = describe_tree(
            stack_column(
                vec![
                    component(Label::title("Settings")),
                    component(Label::new("Notifications are on")),
                    component(Switch::new(2, true)),
                    component(Button::new(3, "Save")),
                    component(Button::new(4, "Delete").with_enabled(false)),
                ],
                8.0,
            ),
            Size::new(300.0, 400.0),
        );
        set_enabled(false);

        let says = |text: &str| nodes.iter().find(|n| n.properties.label == text);

        let title = says("Settings").expect("the title is read");
        assert!(title.properties.flags.is_header, "a title is a heading to jump to");

        assert!(says("Notifications are on").is_some(), "body text is read");

        let switch = nodes.iter().find(|n| n.id == node_id_for(2)).expect("the switch is there");
        assert!(switch.properties.flags.has_checked_state);
        assert!(switch.properties.flags.is_checked, "and it is on");

        let save = says("Save").expect("the button is read");
        assert_eq!(save.id, node_id_for(3), "its semantics id is its hit-test id");
        assert!(save.properties.flags.is_button);
        assert!(save.properties.has(SemanticsAction::Tap));
        assert!(save.properties.flags.is_enabled);

        let delete = says("Delete").expect("a disabled button is still read");
        assert!(delete.properties.flags.has_enabled_state && !delete.properties.flags.is_enabled);
        assert!(
            !delete.properties.has(SemanticsAction::Tap),
            "and offers nothing a reader could do with it"
        );

        // Every node has somewhere to be, and the ones whose size does not
        // depend on shaping have real area. A rectangle is what makes touch
        // exploration possible at all -- a node with none is, to a finger
        // dragged across the glass, not there. (The text nodes measure zero
        // here because the engine every unit test shapes against reports zero
        // for every metric; on a device they are the size of their glyphs.)
        for node in &nodes {
            assert!(node.left.is_finite() && node.top.is_finite(), "{node:?}");
            assert!(node.width() >= 0.0 && node.height() >= 0.0, "{node:?}");
        }
        assert!(switch.width() > 0.0 && switch.height() > 0.0, "the switch is a target");
        assert!(save.width() > 0.0 && save.height() > 0.0, "so is the button");
    }

    #[test]
    fn tapping_a_button_through_semantics_does_what_the_finger_does() {
        use crate::components::Button;
        use crate::framework::component;
        use crate::gestures::PointerHandlers;

        set_enabled(true);
        let saves = Rc::new(Cell::new(0));
        let counted = Rc::clone(&saves);
        let _ = describe_tree(
            component(Button::new(9, "Save").with_handlers(
                PointerHandlers::new().with_tap(move |_| counted.set(counted.get() + 1)),
            )),
            Size::new(300.0, 100.0),
        );

        assert!(perform_action(node_id_for(9), SemanticsAction::Tap));
        assert_eq!(saves.get(), 1, "the same closure a finger would have called");
        set_enabled(false);
    }

    #[test]
    fn text_nobody_annotated_is_still_read() {
        // A raw `Text`, with nothing asking for accessibility anywhere. It is
        // the most common thing on a screen and the most important thing to
        // read; upstream `Text` describes itself for the same reason.
        set_enabled(true);
        let nodes = describe_tree(
            leaf(|| crate::widgets::Text::new("Rendered by Rust")),
            Size::new(200.0, 100.0),
        );
        set_enabled(false);

        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].properties.label, "Rendered by Rust");
    }

    #[test]
    fn a_label_speaks_for_the_text_inside_it() {
        // The button says "Save" and its child text says "Save". Read as two
        // nodes a reader hears it twice, which is worse than hearing it once
        // in the wrong voice. Upstream's `excludeSemantics` is the same rule.
        use crate::components::Button;
        use crate::framework::component;

        set_enabled(true);
        let nodes = describe_tree(component(Button::new(5, "Save")), Size::new(200.0, 100.0));
        set_enabled(false);

        let said: Vec<&str> =
            nodes.iter().map(|n| n.properties.label.as_str()).filter(|l| !l.is_empty()).collect();
        assert_eq!(said, vec!["Save"], "heard once, from the button");
    }

    #[test]
    fn a_paragraph_keeps_its_node_across_a_frame() {
        // The identity a screen reader keys on. It is stable because the
        // render object is: see the persistence work in section eighteen.
        set_enabled(true);
        let mut tree = ElementTree::new();
        tree.rebuild(leaf(|| crate::widgets::Text::new("unchanging")));
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::loose(200.0, 100.0));

        let describe_once = |root: &mut crate::render::BoxedRender| {
            let mut layers = crate::engine::LayerTree::new(200, 100);
            collect(|| {
                let mut context = PaintContext::new(&mut layers, Size::new(200.0, 100.0));
                root.paint(&mut context, Offset::ZERO);
            })
            .expect("on")
        };

        let first = describe_once(&mut root);
        tree.rebuild_dirty();
        let mut again = tree.build_render_tree().expect("still mounted");
        let second = describe_once(&mut again);
        set_enabled(false);

        assert_eq!(first[0].id, second[0].id, "the same text became a new node");
    }

    #[test]
    fn every_action_bit_round_trips() {
        for action in [
            SemanticsAction::Tap,
            SemanticsAction::LongPress,
            SemanticsAction::ScrollUp,
            SemanticsAction::Increase,
            SemanticsAction::Dismiss,
            SemanticsAction::Focus,
        ] {
            assert_eq!(SemanticsAction::from_bits(action as i32), Some(action));
        }
        assert_eq!(SemanticsAction::from_bits(1 << 30), None, "a bit we have no name for");
    }
}
