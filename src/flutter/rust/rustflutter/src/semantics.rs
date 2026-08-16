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
//! A node needs a rectangle in root coordinates, and only the walk down the
//! render tree can work one out -- each parent knows where it put each child,
//! and nothing knows where it is itself. Upstream walks the render tree for
//! exactly this, through `visitChildrenForSemantics` and `applyPaintTransform`;
//! [`flush`] does the same walk through
//! [`crate::render::RenderBox::visit_children_for_semantics`], which carries
//! the offset because there is no `parentData` here to read it off the child.
//!
//! **It used to ride on the paint walk instead**, which had the offset already
//! and cost nothing extra. Two things were wrong with that and both were
//! structural. A repaint boundary that handed back the layer it kept did not
//! walk, so a subtree that had not been drawn again said nothing -- which was
//! patched by making the boundary redraw whenever a reader was listening,
//! throwing away every retained layer on the screen for as long as a screen
//! reader was open. And the offsets were wrong: a boundary paints its child at
//! the origin and puts the offset in the layer, so every node inside one
//! reported its position *within the boundary* as though it were the position
//! on the glass -- and [`crate::scrolling::LazyList`] puts a boundary around
//! every row. Both stop being possible once the walk is its own.
//!
//! # What the walk clips away
//!
//! The walk also carries the clips down. Every box that paints through one --
//! a viewport's window, a `ClipRect`'s bounds -- is asked for it as the walk
//! passes ([`crate::render::RenderBox::describe_approximate_paint_clip`] and
//! [`crate::render::RenderBox::describe_semantics_clip`], the pair upstream's
//! `_SemanticsGeometry.computeChildGeometry` accumulates), the answers are
//! intersected into the clips already carried, and each node's rectangle is
//! then cut by the result. A rectangle the clips empty does not reach the
//! platform at all: upstream keeps a paint-clipped node in the tree under a
//! `hidden` flag, and this bridge has no such flag to put in
//! [`SemanticsNode`], so -- of the choices that do not report a rectangle
//! outside the window -- the node is dropped.
//!
//! # The three gates
//!
//! A walk of its own is a walk somebody has to pay for, and the first version
//! of it paid on every frame a reader was listening. Upstream does not, and it
//! avoids the work at three separate places. All three are here:
//!
//! 1. **Nothing is marked when nobody is reading.** Upstream's
//!    `markNeedsSemanticsUpdate` returns at once while `_semanticsOwner` is
//!    null, and `flushSemantics` returns at once for the same reason. Here
//!    [`enabled`] is that gate.
//! 2. **A frame that changed nothing is not walked.** Upstream keeps
//!    `PipelineOwner._nodesNeedingSemanticsUpdate` and visits what is in it;
//!    [`mark_needs_update`] fills the same role, and [`flush`] returns without
//!    walking when it is empty. What marks is listed on [`mark_needs_update`],
//!    and each entry is a line upstream also has.
//! 3. **A walk that came out the same sends nothing.** Upstream's
//!    `SemanticsOwner.sendSemanticsUpdate` opens with
//!    `if (_dirtyNodes.isEmpty) return;` and puts only the dirty nodes on the
//!    wire. The tree the platform is holding is kept here (see [`tree`]) and
//!    compared, which answers the same question for a tree small enough that
//!    comparing it is cheaper than keeping a change log -- the same trade the
//!    Windows bridge already makes on the other side of the boundary.
//!
//! The one upstream gate that is *not* here is the fourth: upstream re-walks
//! only the subtree under the dirtied semantics boundary, because its dirty
//! list holds render objects and its node rectangles are relative to the parent
//! node, so a scrolled viewport moves one transform instead of every rectangle
//! under it. Here the rectangles are absolute -- both bridges below want "where
//! on the glass" -- so a subtree cannot be reused where it moved, and the walk
//! descends from the root. That is the same trade [`crate::render::RenderRef`]
//! makes for layout, for the same missing piece: there is no pipeline owner
//! holding a list of boundaries to resume a descent from.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::direction::TextDirection;
use crate::engine::Rect;
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
#[derive(Clone, Debug, Default)]
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
    /// The reading direction of everything said above: `label`, `value`,
    /// `hint`, and the two value forecasts.
    ///
    /// Upstream's `SemanticsConfiguration.textDirection`, which the
    /// `Semantics` widget defaults to the ambient `Directionality` and a
    /// paragraph takes from its own build (`paragraph.dart` sets it on the
    /// same line as the label), carried to the embedder as
    /// `SemanticsData.textDirection` and from there as
    /// `FlutterSemanticsNode2.text_direction`. `None` is that null: a node
    /// with nothing to read has no direction to read it in.
    pub text_direction: Option<TextDirection>,
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

/// Two of these are the same when a reader would be told the same thing.
///
/// Written out rather than derived because of the three scroll fields: they
/// hold `NaN` for "this does not scroll", which is what upstream's
/// `double? scrollPosition` becomes the moment it crosses to an embedder, and
/// two boxes that both do not scroll are saying the same thing. Derived
/// equality would call them different, and it is asked twice on every frame --
/// once by [`RenderSemantics::update_from`] to decide whether a label changed,
/// and once by [`flush`] to decide whether the platform needs telling. A
/// comparison that always answered "different" would defeat both gates while
/// looking like it worked.
impl PartialEq for SemanticsProperties {
    fn eq(&self, other: &SemanticsProperties) -> bool {
        /// Equal, or both of them "no answer".
        fn same(a: f32, b: f32) -> bool {
            a == b || (a.is_nan() && b.is_nan())
        }
        self.label == other.label
            && self.value == other.value
            && self.hint == other.hint
            && self.increased_value == other.increased_value
            && self.decreased_value == other.decreased_value
            && self.text_direction == other.text_direction
            && self.flags == other.flags
            && self.actions == other.actions
            && same(self.scroll_position, other.scroll_position)
            && same(self.scroll_extent_max, other.scroll_extent_max)
            && same(self.scroll_extent_min, other.scroll_extent_min)
    }
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

    /// Whether a reader would be told any words, which is whether a direction
    /// is worth carrying for them.
    ///
    /// Upstream's `SemanticsData` insists on a `textDirection` for exactly
    /// these fields and none other -- `label == '' || textDirection != null`,
    /// and the same assert for `value`, `increasedValue`, `decreasedValue`,
    /// and `hint` -- so those are the ones asked about here.
    fn reads_aloud(&self) -> bool {
        !(self.label.is_empty()
            && self.value.is_empty()
            && self.hint.is_empty()
            && self.increased_value.is_empty()
            && self.decreased_value.is_empty())
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

// -- What a render object says about itself -----------------------------------

/// One render object's answer to "what are you, for a reader".
///
/// Upstream's `SemanticsConfiguration`, filled in by
/// `describeSemanticsConfiguration`. Narrower here because there is no
/// merging step to configure: what an object says is what its node says.
pub struct SemanticsAnnotation {
    pub id: i32,
    pub properties: SemanticsProperties,
    pub on_action: Option<ActionHandler>,
    /// Whether an enclosing label already speaks for this.
    ///
    /// Set on text and nothing else. A button says "Save" and the text inside
    /// it says "Save"; read as two nodes a reader hears it twice, which is
    /// worse than hearing it once in the wrong voice. Upstream reaches the same
    /// place with `excludeSemantics` and `MergeSemantics` -- this is the common
    /// case of both, and it is the text that yields because the label is the
    /// one somebody chose.
    pub yields_to_a_label: bool,
}

impl SemanticsAnnotation {
    /// What [`crate::render::RenderBox::describe_semantics`] hands back for an
    /// annotation somebody wrote.
    pub fn new(
        id: i32,
        properties: SemanticsProperties,
        on_action: Option<ActionHandler>,
    ) -> SemanticsAnnotation {
        SemanticsAnnotation { id, properties, on_action, yields_to_a_label: false }
    }

    /// What a paragraph hands back for text nobody annotated.
    ///
    /// The direction is taken with the words, because text is the thing a
    /// direction is *of*: upstream's `RenderParagraph` sets
    /// `config.textDirection = textDirection` on the same line as the label.
    /// A paragraph here does not carry a direction of its own yet -- the
    /// render-tree half of `directionality` is still landing -- so the
    /// ambient direction stands in for it, the same stand-in the shaper
    /// takes for the same reason, and
    /// [`SemanticsAnnotation::with_text_direction`] is the way in once the
    /// paragraph has one to give.
    pub fn text(id: i32, said: &str) -> SemanticsAnnotation {
        SemanticsAnnotation {
            id,
            properties: SemanticsProperties {
                text_direction: Some(crate::direction::current_direction()),
                ..SemanticsProperties::label(said)
            },
            on_action: None,
            yields_to_a_label: true,
        }
    }

    /// Says which way this node's words run, for a render object that knows.
    ///
    /// [`crate::render::RenderParagraph`] is the case: it will capture the
    /// ambient direction where it was built, the way it already captures the
    /// text scale, and hand it back here so the node says which way its text
    /// runs however late the walk asks. Everything else takes the ambient
    /// direction by default instead, which is upstream's `Semantics` widget
    /// defaulting the configuration with `Directionality.maybeOf`.
    pub fn with_text_direction(mut self, direction: TextDirection) -> Self {
        self.properties.text_direction = Some(direction);
        self
    }
}

// -- The frame's collection ---------------------------------------------------

#[derive(Default)]
struct Collector {
    /// Whether anything is listening. Nothing is collected otherwise.
    enabled: bool,
    nodes: Vec<SemanticsNode>,
    /// Indices into `nodes` for the annotations currently open, outermost
    /// last. This is what turns the paint recursion into a tree.
    open: Vec<usize>,
    /// The tree the platform is holding -- what the last [`flush`] handed
    /// over, or nothing if it has never been handed one.
    ///
    /// Upstream keeps the same thing, as the live `SemanticsNode` tree under
    /// `SemanticsOwner`, and for the same two reasons: a walk that comes out
    /// the same as this sends nothing, and a frame that is never walked leaves
    /// this standing as the answer -- which is right, because nothing that
    /// would have changed it happened.
    sent: Vec<SemanticsNode>,
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

pub type ActionHandler = Rc<dyn Fn(SemanticsAction)>;

thread_local! {
    static COLLECTOR: RefCell<Collector> = RefCell::new(Collector::default());

    /// Whether anything has happened that the semantics tree would notice.
    ///
    /// This is upstream's `PipelineOwner._nodesNeedingSemanticsUpdate`, which
    /// is a set of the render objects to revisit. It is one boolean here for
    /// the reason [`crate::render::RenderRef::mark_needs_layout`] walks all
    /// the way to the root instead of stopping at a boundary: a set is only
    /// worth keeping if a descent can be *started* from what is in it, and
    /// there is no pipeline owner here to start one. So the answer this holds
    /// is "walk" or "do not walk", and the saving is the frames where it says
    /// not to -- which, for a screen that is being read rather than animated,
    /// is nearly all of them.
    ///
    /// Starts true: a tree nobody has walked yet has told nobody anything.
    static NEEDS_UPDATE: Cell<bool> = const { Cell::new(true) };
}

/// Whether the platform has said a screen reader is listening.
pub fn enabled() -> bool {
    COLLECTOR.with(|collector| collector.borrow().enabled)
}

/// Says the semantics tree is no longer what the platform is holding.
///
/// Upstream's `RenderObject.markNeedsSemanticsUpdate`, and like it this is
/// called from exactly the places that can change what a reader would hear:
///
/// * [`crate::render::RenderRef::layout`], on the path that actually lays out
///   -- upstream calls it on the line after `performLayout` for the same
///   reason, that a box which was just measured may have moved, resized, or
///   stopped existing, and a rectangle is made of all three.
/// * [`RenderSemantics::update_from`], when the annotation itself changed --
///   upstream's `RenderSemanticsAnnotations.set properties`.
/// * `RenderOpacity::update_from`, when the opacity crossed zero in either
///   direction -- upstream's `set opacity` marks on exactly that condition,
///   because a subtree that stopped being drawn stopped being describable
///   while a fade between two visible values changes nothing anybody hears.
/// * [`set_enabled`], when a reader arrives -- upstream's
///   `scheduleInitialSemantics`.
///
/// Cheap enough to call unconditionally: it is one thread-local boolean, where
/// upstream has to reach the owner to find out whether to bother.
///
/// Public for the reason upstream's is: a `RenderBox` written outside this
/// crate whose [`crate::render::RenderBox::describe_semantics`] answer changed
/// has the same thing to say, and no other way to say it.
pub fn mark_needs_update() {
    NEEDS_UPDATE.with(|needs| needs.set(true));
}

/// Turns semantics on or off. Called by the shell when an assistive technology
/// arrives or leaves.
pub fn set_enabled(on: bool) {
    COLLECTOR.with(|collector| {
        let mut collector = collector.borrow_mut();
        collector.enabled = on;
        if !on {
            collector.nodes.clear();
            collector.sent.clear();
        }
    });
    // A reader that has just arrived has been told nothing, so everything is
    // news; a reader that has just left leaves an empty tree behind, so the
    // next one to arrive is not compared against a stale one.
    mark_needs_update();
}

/// The tree the platform is holding.
///
/// Upstream's `SemanticsOwner.rootSemanticsNode` and what hangs from it. This
/// is the answer between frames as well as during them: a frame that found
/// nothing marked did not walk, and what it did not walk is still true.
pub fn tree() -> Vec<SemanticsNode> {
    COLLECTOR.with(|collector| collector.borrow().sent.clone())
}

/// The view's own node, which everything painted into it hangs from.
///
/// Upstream this is `RenderView`'s semantics node, and it is always zero. It
/// exists here for the reason it exists there -- a screen reader is handed one
/// tree, not a heap of unrelated ones -- and for one more that upstream never
/// had to think about: the order the nodes reach a platform is lost on the way
/// (`SemanticsNodeUpdates` is a map, on this branch and upstream both), so the
/// order a reader meets them in has to be carried by a parent's child list.
/// Without a parent above them, the top-level nodes have nowhere to carry it.
pub const ROOT_ID: i32 = 0;

/// Brings the semantics tree up to date, and returns it if the platform needs
/// telling.
///
/// This is upstream's `PipelineOwner.flushSemantics` followed by
/// `SemanticsOwner.sendSemanticsUpdate`, and it declines to do the work at all
/// of the same three places they do -- see "The three gates" in the module
/// documentation. `None` means there is nothing to send, and it is the answer
/// on nearly every frame: nobody is reading, or nothing marked itself, or the
/// walk found the tree the platform already has. Only the last of those costs
/// a walk.
///
/// `size` is the view, and becomes [`ROOT_ID`]'s rectangle. The tree must be
/// laid out already: every offset this reads was written during layout.
pub fn flush(size: Size, root: &dyn RenderBox) -> Option<Vec<SemanticsNode>> {
    // Gate one: nobody is reading. Upstream's `if (_semanticsOwner == null)`.
    if !enabled() {
        return None;
    }
    // Gate two: nothing that a reader would notice has happened since the last
    // walk. Upstream takes the render objects out of
    // `_nodesNeedingSemanticsUpdate` here and revisits those; this has one
    // boolean rather than a list, so it either walks or it does not.
    if !NEEDS_UPDATE.with(|needs| needs.replace(false)) {
        return None;
    }
    COLLECTOR.with(|collector| {
        let mut collector = collector.borrow_mut();
        collector.nodes.clear();
        collector.open.clear();
        collector.labelled_depth = 0;
    });
    // Opened before the walk and closed after it, so that everything the walk
    // finds lands inside it -- in paint order, which is reading order.
    let opened = open(
        ROOT_ID,
        SemanticsProperties::label(""),
        (0.0, 0.0, size.width, size.height),
    );
    describe_subtree(root, Offset::ZERO, Clips::UNCLIPPED);
    if let Some(index) = opened {
        close(index);
    }
    COLLECTOR.with(|collector| {
        let mut collector = collector.borrow_mut();
        collector.open.clear();
        collector.labelled_depth = 0;
        // Gate three: the walk happened and came out the same. Upstream's
        // `if (_dirtyNodes.isEmpty) return;`. A frame that relaid out anything
        // at all arrives here -- a growing ripple, a settling scroll that has
        // stopped moving, a rebuild that changed only a colour -- and most of
        // them have nothing to say that was not said last time.
        if collector.nodes == collector.sent {
            return None;
        }
        collector.sent = std::mem::take(&mut collector.nodes);
        Some(collector.sent.clone())
    })
}

/// One render object and everything under it, at `offset` from the root and
/// inside `clips`.
///
/// Upstream's `_RenderObjectSemantics` walk. The recursion is the tree: what a
/// node opens stays open until its children have been described, so the nesting
/// of the render tree becomes the nesting a reader is handed.
fn describe_subtree(render: &dyn RenderBox, offset: Offset, clips: Clips) {
    let opened = match render.describe_semantics() {
        // Text that something above already speaks for. Its children are still
        // walked -- suppressing what a node says is not suppressing what is
        // under it -- though a paragraph has none.
        Some(annotation) if annotation.yields_to_a_label && inside_labelled() => None,
        Some(annotation) => {
            let size = render.size();
            let bounds = Rect::xywh(offset.dx, offset.dy, size.width, size.height);
            match clips.applied_to(bounds) {
                // Nothing of it is inside the clips, so nothing of it is on the
                // glass. Upstream drops the node out of its parent's children
                // (`children.removeWhere(shouldDrop)`), and everything under it
                // leaves with the node -- so the subtree is not walked.
                None => return,
                Some(rect) => open(
                    annotation.id,
                    annotation.properties,
                    (rect.left, rect.top, rect.right, rect.bottom),
                ),
            }
        }
        None => None,
    };
    render.visit_children_for_semantics(&mut |child, child_offset| {
        describe_subtree(
            child,
            offset.plus(child_offset),
            clips.refined_by(render, child, offset),
        );
    });
    if let Some(index) = opened {
        close(index);
    }
}

// -- The clips the walk carries ------------------------------------------------

/// The clip rectangles that apply below a render object, in root coordinates.
///
/// Upstream's `_SemanticsGeometry`, which holds one `paintClipRect` and one
/// `semanticsClipRect` per semantics node and rebuilds both by walking the
/// render chain between two nodes, transforming as it goes. Here the walk is
/// already in root coordinates -- that is what the offsets it carries are --
/// so the same two rectangles are simply carried down it, each contributor
/// translating its own answer from its own coordinates.
#[derive(Clone, Copy, Debug, Default)]
struct Clips {
    paint: Option<Rect>,
    semantics: Option<Rect>,
}

impl Clips {
    /// No clip at all, which is what the walk starts with. Upstream's
    /// `_SemanticsGeometry.root`: the view's own node is clipped by nothing.
    const UNCLIPPED: Clips = Clips { paint: None, semantics: None };

    /// What `parent`'s answers about `child` leave of these clips.
    ///
    /// One link of the accumulation loop in
    /// `_SemanticsGeometry.computeChildGeometry`, with the translation that
    /// upstream needs a `Matrix4` for done by the walk instead. Paint clips
    /// intersect all the way down. A semantics clip *replaces* what was
    /// carried -- upstream's `localSemanticsClipInParent ??
    /// semanticsClipRect?.intersect(...)`, where the clip nearest the node
    /// wins -- and the paint clips below it narrow the replacement further.
    fn refined_by(
        self,
        parent: &dyn RenderBox,
        child: &dyn RenderBox,
        parent_offset: Offset,
    ) -> Clips {
        let at = |clip: Rect| {
            Rect::ltrb(
                clip.left + parent_offset.dx,
                clip.top + parent_offset.dy,
                clip.right + parent_offset.dx,
                clip.bottom + parent_offset.dy,
            )
        };
        let paint = parent.describe_approximate_paint_clip(child).map(at);
        let semantics = parent.describe_semantics_clip(child).map(at);
        Clips {
            paint: match (self.paint, paint) {
                (Some(carried), Some(local)) => Some(intersect(carried, local)),
                (carried, local) => carried.or(local),
            },
            semantics: semantics.or_else(|| {
                self.semantics.map(|carried| match paint {
                    Some(local) => intersect(carried, local),
                    None => carried,
                })
            }),
        }
    }

    /// `bounds`, cut down to what the clips leave of it, or `None` when
    /// nothing a reader could touch survives.
    ///
    /// The tail of `_SemanticsGeometry.computeChildGeometry`: the rect is cut
    /// by the semantics clip first (`semanticsClipRect?.intersect(semanticBounds)`)
    /// and by the paint clip second. Empty after the semantics clip is
    /// upstream's `isInvisible`, dropped from the tree. Empty after the paint
    /// clip but not before it is upstream's `hidden` -- kept in the tree there,
    /// for the readers that scroll to a node they have been told about. This
    /// bridge has no hidden flag to carry, and reporting the uncut rectangle
    /// puts coordinates outside the window onto the glass, so it is dropped
    /// too.
    ///
    /// With neither clip present the rect is reported as it lies, empty or
    /// not. Upstream drops an empty rect as `isInvisible` wherever it came
    /// from; here an empty rect usually means the test engine shaped no text,
    /// and a paragraph that says something is still worth reading.
    fn applied_to(&self, bounds: Rect) -> Option<Rect> {
        if self.paint.is_none() && self.semantics.is_none() {
            return Some(bounds);
        }
        let mut rect = self.semantics.map_or(bounds, |clip| intersect(bounds, clip));
        if let Some(clip) = self.paint {
            let painted = intersect(rect, clip);
            if is_empty(painted) && !is_empty(rect) {
                return None; // `hidden`, upstream; dropped here.
            }
            rect = painted;
        }
        (!is_empty(rect)).then_some(rect)
    }
}

/// Upstream's `Rect.intersect`: the overlap, or an inside-out rectangle where
/// the two do not meet -- which is empty, and left that way there too.
fn intersect(a: Rect, b: Rect) -> Rect {
    Rect::ltrb(a.left.max(b.left), a.top.max(b.top), a.right.min(b.right), a.bottom.min(b.bottom))
}

/// Upstream's `Rect.isEmpty`.
fn is_empty(rect: Rect) -> bool {
    rect.width() <= 0.0 || rect.height() <= 0.0
}

/// Delivers an action the platform asked for.
///
/// Returns whether anything took it. Upstream this is
/// `SemanticsOwner.performAction`, and the same rule applies: an action for a
/// node that has since gone is not an error, it is a race with the reader.
///
/// The handler is fetched from the render tree rather than from a list kept by
/// the last walk, and that is not a detail. A rebuild that changes only a
/// closure changes nothing measured and nothing drawn, so nothing marks itself
/// and no walk happens -- which is the whole point of
/// [`mark_needs_update`] -- and a remembered handler would then be last
/// build's. The live object always has this build's, because
/// [`RenderSemantics::update_from`] took it. Upstream never has to choose:
/// its `SemanticsNode` holds the render object, so reaching one reaches the
/// other.
pub fn perform_action(root: &dyn RenderBox, node_id: i32, action: SemanticsAction) -> bool {
    match find_handler(root, node_id) {
        Some(handler) => {
            handler(action);
            true
        }
        None => false,
    }
}

/// The handler the node with this id offered, if it is still on screen.
///
/// Walks the same children [`flush`] walks and under the same clips, so a node
/// a reader cannot have been told about -- one under a fully transparent
/// subtree, or one the clips cut away entirely -- cannot be activated either.
fn find_handler(render: &dyn RenderBox, node_id: i32) -> Option<ActionHandler> {
    find_handler_in(render, node_id, Offset::ZERO, Clips::UNCLIPPED)
}

/// The walk behind [`find_handler`], which is [`describe_subtree`] again with a
/// different thing collected: the same clips, the same dropping of what they
/// empty, because a node that was never in the tree is not a node a reader can
/// name.
fn find_handler_in(
    render: &dyn RenderBox,
    node_id: i32,
    offset: Offset,
    clips: Clips,
) -> Option<ActionHandler> {
    if let Some(annotation) = render.describe_semantics() {
        let size = render.size();
        clips.applied_to(Rect::xywh(offset.dx, offset.dy, size.width, size.height))?;
        if annotation.id == node_id {
            return annotation.on_action;
        }
    }
    let mut found = None;
    render.visit_children_for_semantics(&mut |child, child_offset| {
        if found.is_none() {
            found = find_handler_in(
                child,
                node_id,
                offset.plus(child_offset),
                clips.refined_by(render, child, offset),
            );
        }
    });
    found
}

/// Whether the walk is inside something that already has a label.
fn inside_labelled() -> bool {
    COLLECTOR.with(|collector| collector.borrow().labelled_depth > 0)
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

/// Opens a node during the walk, returning its index.
fn open(id: i32, properties: SemanticsProperties, rect: (f32, f32, f32, f32)) -> Option<usize> {
    COLLECTOR.with(|collector| {
        let mut collector = collector.borrow_mut();
        if !collector.enabled {
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
        // The direction is taken here rather than at describe time because
        // construction is the one moment the ambient direction is this
        // annotation's: the render walk pushes it around the subtree while
        // the object is being built, and the semantics walk that asks what
        // this says runs long after it has popped. Upstream's `Semantics`
        // widget does the same defaulting in its own build
        // (`textDirection ?? Directionality.maybeOf(context)`), and its
        // `SemanticsData` insists on the result -- a node that says anything
        // says which way it runs. A node with nothing to read keeps `None`,
        // which crosses as "unknown".
        let properties = if properties.reads_aloud() {
            SemanticsProperties {
                text_direction: Some(crate::direction::current_direction()),
                ..properties
            }
        } else {
            properties
        };
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
    fn update_from(
        &mut self,
        fresh: &mut dyn RenderBox,
    ) -> Option<crate::render::UpdateEffect> {
        use crate::render::UpdateEffect;
        let fresh = fresh.as_any_mut().downcast_mut::<RenderSemantics>()?;
        // Nothing here is measured and nothing is drawn, so the effect this
        // reports is about the child alone -- which is why a changed label has
        // to say so itself. Upstream's `RenderSemanticsAnnotations.set
        // properties` ends in `markNeedsSemanticsUpdate()` for the same
        // reason, and this is the only kind of change in the whole framework
        // that neither layout nor paint would have noticed on its behalf.
        //
        // The handler is deliberately not part of that comparison. Two
        // closures cannot be told apart -- every build makes a fresh `Rc` --
        // so comparing them would mark every frame, and not comparing them
        // would be wrong if anything remembered the old one. Nothing does:
        // `perform_action` reads the handler off this object at the moment the
        // reader asks, and `self.on_action` below is always this build's.
        let changed = self.id != fresh.id || self.properties != fresh.properties;
        self.id = fresh.id;
        self.properties = fresh.properties.clone();
        self.on_action = fresh.on_action.take();
        if changed {
            mark_needs_update();
        }
        let effect = UpdateEffect::relayout_if(!self.child.is(&fresh.child));
        self.child = fresh.child.clone();
        Some(effect)
    }

    fn layout(&mut self, constraints: BoxConstraints) -> Size {
        self.size = self.child.layout(constraints);
        self.size
    }

    fn size(&self) -> Size {
        self.size
    }

    fn paint(&self, context: &mut PaintContext, offset: Offset) {
        context.paint_child(&self.child, offset);
    }

    fn describe_semantics(&self) -> Option<SemanticsAnnotation> {
        Some(SemanticsAnnotation::new(
            self.id,
            self.properties.clone(),
            self.on_action.as_ref().map(Rc::clone),
        ))
    }

    fn visit_children(&self, visit: &mut dyn FnMut(&dyn RenderBox, Offset)) {
        visit(&self.child, Offset::ZERO);
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
        // Tap because a finger reaches a field by touching it, and Focus
        // because a keyboard-shaped reader reaches it by moving to it --
        // upstream's `EditableText` offers both for the same two ways in.
        .with_action(SemanticsAction::Tap)
        .with_action(SemanticsAction::Focus)
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
/// somewhere that cannot be mistaken for an automatic one. Never zero, which
/// belongs to [`ROOT_ID`].
pub fn node_id_for(caller: u64) -> i32 {
    1 + (caller % (AUTO_BASE as u64 - 1)) as i32
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

    /// Lays out a tree, paints it, and returns what it says about itself.
    ///
    /// The paint is here because a real frame paints -- and because a walk that
    /// still worked when the drawing had been skipped is the whole point.
    fn describe_tree(widget: AnyWidget, size: Size) -> Vec<SemanticsNode> {
        describe_tree_keeping_root(widget, size).0
    }

    /// The same, handing back the render tree as well, for the tests that ask
    /// it something after the frame -- an action arrives long after the frame
    /// that drew the thing it names.
    fn describe_tree_keeping_root(
        widget: AnyWidget,
        size: Size,
    ) -> (Vec<SemanticsNode>, crate::render::BoxedRender) {
        let mut tree = ElementTree::new();
        tree.rebuild(widget);
        let mut root = tree.build_render_tree().expect("a tree was mounted");
        // Loose, so a child that asked for a size keeps it: a tight box
        // would stretch the very thing whose rectangle is under test.
        root.layout(BoxConstraints::loose(size.width, size.height));
        let mut layers =
            crate::engine::LayerTree::new(size.width as i32, size.height as i32);
        {
            let mut context = PaintContext::new(&mut layers, size);
            root.paint(&mut context, Offset::ZERO);
        }
        flush(size, &root);
        (tree_or_fail(), root)
    }

    /// What the platform is holding, which had better be something.
    fn tree_or_fail() -> Vec<SemanticsNode> {
        let nodes = tree();
        assert!(!nodes.is_empty(), "semantics are on but nothing was collected");
        nodes
    }

    #[test]
    fn nothing_is_collected_until_something_asks() {
        set_enabled(false);
        let mut tree = ElementTree::new();
        tree.rebuild(leaf(|| crate::widgets::Text::new("unread")));
        let root = tree.build_render_tree().expect("mounted");
        let collected = flush(Size::new(200.0, 100.0), &root);
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

        // The view's own node, and the annotation inside it.
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].id, ROOT_ID);
        let node = &nodes[1];
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
    fn a_node_under_a_boundary_still_says_where_it_is() {
        set_enabled(true);
        let nodes = describe_tree(
            single(
                crate::widgets::repaint_boundary(semantics(
                    7,
                    SemanticsProperties::button("Increment"),
                    leaf(|| SizedBox::new(80.0, 40.0)),
                )),
                |child| RenderPadding::new(EdgeInsets::all(10.0), child),
            ),
            Size::new(200.0, 100.0),
        );
        set_enabled(false);
        let node = nodes.iter().find(|n| n.id == 7).expect("the button is read");
        assert_eq!((node.left, node.top), (10.0, 10.0), "reported somewhere else");
    }

    #[test]
    fn a_node_under_a_layer_still_says_where_it_is() {
        // A repaint boundary is not the only thing that puts the offset in a
        // layer and paints its child at the origin -- opacity and transform do
        // it too, because a layer carries its own position. Anything reading a
        // node's rectangle out of the paint walk read the position *inside* the
        // layer, and a partly faded subtree is an ordinary thing to have.
        use crate::render::{RenderOpacity, RenderTransform};

        set_enabled(true);
        for (what, wrap) in [
            ("opacity", 0),
            ("transform", 1),
        ] {
            let inner = semantics(
                8,
                SemanticsProperties::button("Increment"),
                leaf(|| SizedBox::new(80.0, 40.0)),
            );
            let wrapped = match wrap {
                0 => single(inner, |child| RenderOpacity::new(0.5, child)),
                _ => single(inner, |child| {
                    RenderTransform::new([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], child)
                }),
            };
            let nodes = describe_tree(
                single(wrapped, |child| RenderPadding::new(EdgeInsets::all(10.0), child)),
                Size::new(200.0, 100.0),
            );
            let node = nodes.iter().find(|n| n.id == 8).expect("the button is read");
            assert_eq!((node.left, node.top), (10.0, 10.0), "under {what}");
        }
        set_enabled(false);
    }

    #[test]
    fn a_viewport_clips_its_rows_to_the_window() {
        // The walk used to report a scrolled-out row at its place in the
        // content -- the gallery's home had "Tooltips" at y=3651 under a window
        // a tenth that tall, and the bridges passed it on. Upstream cuts every
        // node's rect against the clips its ancestors describe
        // (`_SemanticsGeometry.computeChildGeometry`); this is that, with a
        // viewport contributing both of its rects.
        //
        // Four rows of 100 in a 200-tall window scrolled 150: row one is wholly
        // above the window, rows two and four straddle its edges, row three is
        // entirely inside.
        use crate::render::{Axis, MainAxisSize, RenderFlex, RenderViewport};

        set_enabled(true);
        let row = |id: i32, said: &str| {
            semantics_with_action(
                id,
                SemanticsProperties::label(said),
                leaf(|| SizedBox::new(200.0, 100.0)),
                |_| {},
            )
        };
        let (nodes, root) = describe_tree_keeping_root(
            single(
                many(
                    vec![
                        row(1, "first"),
                        row(2, "second"),
                        row(3, "third"),
                        row(4, "fourth"),
                    ],
                    |children| {
                        let mut column = RenderFlex::column().with_main_axis_size(MainAxisSize::Min);
                        for child in children {
                            column = column.push(child);
                        }
                        column
                    },
                ),
                |column| RenderViewport::new(Axis::Vertical, column).with_offset(150.0),
            ),
            Size::new(200.0, 200.0),
        );
        set_enabled(false);

        // Wholly above the window: not in the tree at all. Upstream drops it
        // as `isInvisible`, and its subtree goes with it.
        assert!(nodes.iter().all(|node| node.id != 1), "a row off the window was reported");

        let by_id = |id: i32| nodes.iter().find(|node| node.id == id).unwrap();
        // The second row is content 100..200, absolute -50..50: the window
        // keeps 0..50 of it.
        assert_eq!((by_id(2).top, by_id(2).bottom), (0.0, 50.0), "cut to the part that shows");
        // The third is content 200..300, absolute 50..150: inside entire.
        assert_eq!((by_id(3).top, by_id(3).bottom), (50.0, 150.0), "a held row keeps its rect");
        // The fourth straddles the far edge: 150..250 becomes 150..200.
        assert_eq!((by_id(4).top, by_id(4).bottom), (150.0, 200.0), "cut at the far edge");
        // Nothing reports outside the window, which is the whole complaint.
        for node in &nodes {
            assert!(
                node.left >= 0.0 && node.top >= 0.0 && node.right <= 200.0 && node.bottom <= 200.0,
                "{node:?} is outside the window"
            );
        }

        // And the dropping reaches actions: a row the reader was never told
        // about is not a row a reader can name, so nothing takes an action for
        // it either.
        assert!(!perform_action(&root, 1, SemanticsAction::Tap), "an off-window row acted");
        assert!(perform_action(&root, 3, SemanticsAction::Tap));
    }

    #[test]
    fn a_carousel_card_beyond_the_window_is_not_reported() {
        // The other half of the same complaint: the gallery's carousel in a
        // window 690 wide reported a card at x=1318. Three cards of 690 in a
        // window of 690, scrolled 62, puts the third card's left edge exactly
        // there -- past the window and past the cache band, so it is gone
        // rather than clipped.
        use crate::render::{Axis, MainAxisSize, RenderFlex, RenderViewport};

        set_enabled(true);
        let nodes = describe_tree(
            single(
                many(
                    vec![
                        semantics(1, SemanticsProperties::label("Rally"), leaf(|| SizedBox::new(690.0, 100.0))),
                        semantics(2, SemanticsProperties::label("Shrine"), leaf(|| SizedBox::new(690.0, 100.0))),
                        semantics(3, SemanticsProperties::label("Fortnightly"), leaf(|| SizedBox::new(690.0, 100.0))),
                    ],
                    |children| {
                        let mut row = RenderFlex::row().with_main_axis_size(MainAxisSize::Min);
                        for child in children {
                            row = row.push(child);
                        }
                        row
                    },
                ),
                |row| RenderViewport::new(Axis::Horizontal, row).with_offset(62.0),
            ),
            Size::new(690.0, 100.0),
        );
        set_enabled(false);

        assert!(nodes.iter().all(|node| node.id != 3), "the card at x=1318 was reported");
        let by_id = |id: i32| nodes.iter().find(|node| node.id == id).unwrap();
        // The first card has scrolled 62 off the leading edge, the second runs
        // off the trailing one, and neither reports past the window.
        assert_eq!((by_id(1).left, by_id(1).right), (0.0, 628.0), "cut at the leading edge");
        assert_eq!((by_id(2).left, by_id(2).right), (628.0, 690.0), "cut at the trailing edge");
    }

    #[test]
    fn a_node_overhanging_a_clip_is_held_to_it() {
        // A `ClipRect` paints through its own bounds, and a reader is told
        // about the part of a child that is inside them -- upstream's
        // `RenderClipRect.describeApproximatePaintClip`. The stack is the
        // window's size with the badge pinned 10 before its bottom edge, so 10
        // of the badge hang past it; the second badge is wholly outside.
        use crate::render::{RenderClipRect, RenderStack, StackPosition};

        set_enabled(true);
        let nodes = describe_tree(
            single(
                many(
                    vec![
                        semantics(6, SemanticsProperties::label("badge"), leaf(|| SizedBox::new(40.0, 20.0))),
                        semantics(7, SemanticsProperties::label("gone"), leaf(|| SizedBox::new(40.0, 20.0))),
                    ],
                    |children| {
                        let positions = [
                            StackPosition { left: Some(10.0), top: Some(90.0), ..Default::default() },
                            StackPosition { left: Some(10.0), top: Some(120.0), ..Default::default() },
                        ];
                        let mut stack = RenderStack::new();
                        for (child, position) in children.into_iter().zip(positions) {
                            stack = stack.push_positioned(child, position);
                        }
                        stack
                    },
                ),
                |stack| RenderClipRect::new(stack),
            ),
            Size::new(200.0, 100.0),
        );
        set_enabled(false);

        let badge = nodes.iter().find(|node| node.id == 6).expect("the badge is read");
        assert_eq!(
            (badge.left, badge.top, badge.right, badge.bottom),
            (10.0, 90.0, 50.0, 100.0),
            "held to the clip it is painted through"
        );
        // Wholly past the clip, and with no viewport above to give it a cache
        // band: nothing of it is on the glass, so it is not in the tree.
        assert!(nodes.iter().all(|node| node.id != 7), "a node past the clip was reported");
    }

    #[test]
    fn a_reader_no_longer_costs_the_screen_its_retained_layers() {
        // What collecting on the paint walk used to cost. A boundary could not
        // hand back the layer it kept, because the subtree behind that layer
        // was where the semantics came from -- so opening a screen reader threw
        // away every retained layer on the screen, for as long as it stayed
        // open. The walk is its own now, so the layer and the reader are no
        // longer in each other's way.
        use crate::engine_test_stubs::{layer_calls, reset_layer_calls};
        use crate::widgets::repaint_boundary;

        set_enabled(true);
        let size = Size::new(200.0, 100.0);
        let mut tree = ElementTree::new();
        tree.rebuild(repaint_boundary(semantics(
            11,
            SemanticsProperties::button("Increment"),
            leaf(|| SizedBox::new(80.0, 40.0)),
        )));
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::loose(size.width, size.height));

        let frame = |root: &mut crate::render::BoxedRender| {
            reset_layer_calls();
            let mut layers = crate::engine::LayerTree::new(200, 100);
            {
                let mut context = PaintContext::new(&mut layers, size);
                root.paint(&mut context, Offset::ZERO);
            }
            flush(size, root);
            (layer_calls(), tree_or_fail())
        };

        let (first, said) = frame(&mut root);
        assert_eq!((first.retainable, first.retained), (1, 0), "the first frame draws");
        assert!(said.iter().any(|node| node.id == 11), "and is read");

        let (quiet, said) = frame(&mut root);
        assert_eq!((quiet.retainable, quiet.retained), (0, 1), "drawn again for a reader");
        assert!(said.iter().any(|node| node.id == 11), "the node stopped being reported");
        set_enabled(false);
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

        assert_eq!(nodes.len(), 4);
        assert_eq!(nodes[0].children, vec![1], "the view's node holds the tree");
        assert_eq!(nodes[1].id, 1);
        assert_eq!(nodes[1].children, vec![2, 3], "reading order is paint order");
        assert!(nodes[2].children.is_empty());
        // The second row is below the first, which is the whole reason a
        // rectangle is worth carrying: a finger dragged down the glass meets
        // them in this order.
        assert!(nodes[3].top >= nodes[2].bottom);
    }

    #[test]
    fn an_action_reaches_the_widget_that_offered_it() {
        set_enabled(true);
        let taps = Rc::new(Cell::new(0));
        let counted = Rc::clone(&taps);
        let (_, root) = describe_tree_keeping_root(
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

        assert!(perform_action(&root, 4, SemanticsAction::Tap));
        assert_eq!(taps.get(), 1);
        assert!(!perform_action(&root, 99, SemanticsAction::Tap), "no such node");
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
        let (_, root) = describe_tree_keeping_root(
            component(Button::new(9, "Save").with_handlers(
                PointerHandlers::new().with_tap(move |_| counted.set(counted.get() + 1)),
            )),
            Size::new(300.0, 100.0),
        );

        assert!(perform_action(&root, node_id_for(9), SemanticsAction::Tap));
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

        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[1].properties.label, "Rendered by Rust");
    }

    #[test]
    fn a_label_under_an_rtl_directionality_says_which_way_it_runs() {
        // Upstream's `Semantics` widget defaults the configuration's
        // textDirection to `Directionality.maybeOf(context)`, and the node
        // carries the result to the embedder -- a reader that is not told
        // which way "حفظ" runs reads it back to front. The direction is
        // taken where the annotation is built, inside the render walk that
        // pushes the ambient direction around the subtree.
        use crate::direction::{TextDirection, directionality};

        set_enabled(true);
        let nodes = describe_tree(
            directionality(
                TextDirection::Rtl,
                semantics(
                    7,
                    SemanticsProperties::button("حفظ"),
                    leaf(|| SizedBox::new(80.0, 40.0)),
                ),
            ),
            Size::new(200.0, 100.0),
        );
        set_enabled(false);

        let node = nodes.iter().find(|n| n.id == 7).expect("the button is read");
        assert_eq!(node.properties.text_direction, Some(TextDirection::Rtl));
        // The view's own node says nothing, so it carries no direction: only
        // words have one.
        assert_eq!(nodes[0].properties.text_direction, None);
    }

    #[test]
    fn a_label_without_a_directionality_runs_left_to_right() {
        // Left to right is what a tree with no `directionality` in it gets,
        // from the ambient direction's own fallback; a label still carries
        // it rather than nothing, because upstream's `SemanticsData` demands
        // a direction of everything it can read aloud.
        use crate::direction::TextDirection;

        set_enabled(true);
        let nodes = describe_tree(
            semantics(
                3,
                SemanticsProperties::label("plain"),
                leaf(|| SizedBox::new(50.0, 20.0)),
            ),
            Size::new(200.0, 100.0),
        );
        set_enabled(false);

        let node = nodes.iter().find(|n| n.id == 3).expect("the label is read");
        assert_eq!(node.properties.text_direction, Some(TextDirection::Ltr));
    }

    #[test]
    fn a_node_with_nothing_to_read_carries_no_direction() {
        // A node that offers an action but says nothing: upstream's assert
        // demands a textDirection of the read-aloud fields and none other,
        // so this one crosses as "unknown" rather than guessing.
        set_enabled(true);
        let nodes = describe_tree(
            semantics_with_action(
                5,
                SemanticsProperties::default(),
                leaf(|| SizedBox::new(50.0, 20.0)),
                |_| {},
            ),
            Size::new(200.0, 100.0),
        );
        set_enabled(false);

        let node = nodes.iter().find(|n| n.id == 5).expect("the node is read");
        assert_eq!(node.properties.label, "");
        assert_eq!(node.properties.text_direction, None, "no words, no direction");
    }

    #[test]
    fn text_takes_the_direction_of_its_context() {
        // A paragraph's annotation takes the ambient direction with the
        // words, standing in for the paragraph's own until the render side
        // captures one; `with_text_direction` is where that lands, and a
        // render object that already knows can say so through it today.
        use crate::direction::{TextDirection, with_direction};

        let plain = SemanticsAnnotation::text(1, "plain");
        assert_eq!(plain.properties.text_direction, Some(TextDirection::Ltr));

        let rtl = with_direction(TextDirection::Rtl, || SemanticsAnnotation::text(2, "مرحبا"));
        assert_eq!(rtl.properties.text_direction, Some(TextDirection::Rtl));

        let known = SemanticsAnnotation::text(3, "mixed").with_text_direction(TextDirection::Rtl);
        assert_eq!(known.properties.text_direction, Some(TextDirection::Rtl));
    }

    #[test]
    fn a_changed_direction_is_news_to_a_reader() {
        // A direction participates in the sameness the walk and the update
        // compare -- upstream's `_isDifferentFromCurrentSemanticAnnotation`
        // compares `textDirection` beside the label -- so a subtree whose
        // directionality flipped is re-sent rather than read in the old one.
        use crate::direction::TextDirection;

        let ltr = SemanticsProperties {
            text_direction: Some(TextDirection::Ltr),
            ..SemanticsProperties::label("same words")
        };
        let rtl = SemanticsProperties {
            text_direction: Some(TextDirection::Rtl),
            ..SemanticsProperties::label("same words")
        };
        assert_eq!(ltr, ltr.clone());
        assert_ne!(ltr, rtl, "the same words run differently");
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

        // Forced, because the point under test is what the walk finds, and a
        // rebuild that changed nothing is exactly the frame the walk is now
        // allowed to skip.
        let describe_once = |root: &mut crate::render::BoxedRender| {
            let mut layers = crate::engine::LayerTree::new(200, 100);
            {
                let mut context = PaintContext::new(&mut layers, Size::new(200.0, 100.0));
                root.paint(&mut context, Offset::ZERO);
            }
            mark_needs_update();
            flush(Size::new(200.0, 100.0), root);
            tree_or_fail()
        };

        let first = describe_once(&mut root);
        tree.rebuild_dirty();
        let mut again = tree.build_render_tree().expect("still mounted");
        let second = describe_once(&mut again);
        set_enabled(false);

        assert_eq!(first[1].id, second[1].id, "the same text became a new node");
    }

    #[test]
    fn the_view_is_one_node_with_everything_under_it() {
        // A reader is handed a tree, not a heap. Upstream's `RenderView` node
        // is what makes that true there, and it is what makes reading *order*
        // survive the crossing here: the platform is handed a map, so the only
        // place a sequence can live is a parent's list of children.
        use crate::components::Label;
        use crate::framework::component;
        use crate::components::stack_column;

        set_enabled(true);
        let nodes = describe_tree(
            stack_column(
                vec![
                    component(Label::new("first")),
                    component(Label::new("second")),
                    component(Label::new("third")),
                ],
                4.0,
            ),
            Size::new(300.0, 400.0),
        );
        set_enabled(false);

        assert_eq!(nodes[0].id, ROOT_ID);
        assert_eq!((nodes[0].width(), nodes[0].height()), (300.0, 400.0));
        // Everything else is somebody's child, so nothing is loose.
        let claimed: Vec<i32> = nodes.iter().flat_map(|n| n.children.clone()).collect();
        for node in &nodes[1..] {
            assert!(claimed.contains(&node.id), "{node:?} hangs from nothing");
        }
        let said: Vec<&str> = nodes[0]
            .children
            .iter()
            .map(|id| {
                nodes
                    .iter()
                    .find(|n| n.id == *id)
                    .map(|n| n.properties.label.as_str())
                    .unwrap_or("")
            })
            .collect();
        assert_eq!(said, vec!["first", "second", "third"], "top to bottom");
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

    #[test]
    fn a_new_label_under_a_boundary_is_still_read_out() {
        // Written when semantics rode on the paint walk, where a repaint
        // boundary handing back a kept layer meant a subtree that said nothing
        // about itself -- a reader would lose every row of a list after the
        // first frame. The walk is its own now, so that is no longer how this
        // passes; it passes because `RenderSemantics::update_from` marks when
        // its label changed, and because the two frames in the middle -- which
        // change nothing -- are allowed to skip the walk entirely and leave
        // last frame's answer standing, which is still the right answer.
        //
        // Three things at once, then: a label that changed is read, a label
        // that did not is not re-derived, and neither depends on the drawing.
        use crate::framework::{StateHandle, StatefulComponent, BuildContext, stateful};
        use crate::widgets::repaint_boundary;
        use std::cell::RefCell;
        use std::rc::Rc;

        #[derive(Default)]
        struct Which {
            second: bool,
        }

        struct Label {
            sink: Rc<RefCell<Option<StateHandle<Which>>>>,
        }

        impl StatefulComponent for Label {
            type State = Which;

            fn build(
                &self,
                state: &Which,
                handle: StateHandle<Which>,
                _context: &mut BuildContext,
            ) -> AnyWidget {
                *self.sink.borrow_mut() = Some(handle);
                let said = if state.second { "after" } else { "before" };
                repaint_boundary(semantics(
                    9,
                    SemanticsProperties::label(said),
                    leaf(|| SizedBox::new(80.0, 40.0)),
                ))
            }
        }

        set_enabled(true);
        let sink: Rc<RefCell<Option<StateHandle<Which>>>> = Rc::new(RefCell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(Label { sink: sink.clone() }));

        let size = Size::new(200.0, 100.0);
        let frame = |tree: &mut ElementTree| {
            let mut root = tree.build_render_tree().expect("mounted");
            root.layout(BoxConstraints::loose(size.width, size.height));
            let mut layers = crate::engine::LayerTree::new(size.width as i32, size.height as i32);
            {
                let mut context = PaintContext::new(&mut layers, size);
                root.paint(&mut context, Offset::ZERO);
            }
            flush(size, &root);
            tree_or_fail()
        };

        let said = |nodes: &[SemanticsNode]| {
            nodes
                .iter()
                .find(|n| n.id == 9)
                .map(|n| n.properties.label.clone())
                .unwrap_or_default()
        };

        assert_eq!(said(&frame(&mut tree)), "before");
        // Painted once already, so the boundary is holding a layer.
        assert_eq!(said(&frame(&mut tree)), "before", "the node stopped being reported");

        sink.borrow().as_ref().expect("built").set_state(|state| state.second = true);
        tree.rebuild_dirty();
        assert_eq!(said(&frame(&mut tree)), "after", "a reader was told last frame's label");
        set_enabled(false);
    }

    /// A box that counts how often it has been asked what it says, and can be
    /// made to answer differently every time it is asked.
    ///
    /// Counting the question rather than the answer is the only way to tell
    /// the second gate from the third: both end in `flush` returning `None`,
    /// and they differ in whether the walk happened at all.
    struct Counted {
        asked: Rc<Cell<u32>>,
        /// Whether the answer changes with the count. A box that says
        /// something new every time makes a walk visible in what is sent; one
        /// that says the same thing makes the *absence* of a send visible even
        /// though the walk ran.
        chatty: bool,
        size: Size,
    }

    impl RenderBox for Counted {
        fn layout(&mut self, constraints: BoxConstraints) -> Size {
            self.size = Size::new(constraints.max_width, constraints.max_height);
            self.size
        }
        fn size(&self) -> Size {
            self.size
        }
        fn paint(&self, _context: &mut PaintContext, _offset: Offset) {}
        fn describe_semantics(&self) -> Option<SemanticsAnnotation> {
            self.asked.set(self.asked.get() + 1);
            let label = if self.chatty {
                format!("asked {} times", self.asked.get())
            } else {
                "the same as ever".to_string()
            };
            Some(SemanticsAnnotation::new(21, SemanticsProperties::label(label), None))
        }
    }

    fn counting(asked: &Rc<Cell<u32>>, chatty: bool) -> AnyWidget {
        let asked = Rc::clone(asked);
        leaf(move || Counted { asked: Rc::clone(&asked), chatty, size: Size::ZERO })
    }

    #[test]
    fn a_frame_that_changed_nothing_is_not_walked() {
        // The second gate, and the reason this work was done. Upstream's
        // `flushSemantics` visits what is in
        // `PipelineOwner._nodesNeedingSemanticsUpdate` and nothing else; on a
        // frame where nothing put anything there, no render object is asked
        // what it says. The box below would answer differently every time it
        // were asked, so if the walk ran the tree would change and something
        // would be sent -- which makes "the walk did not run" a thing a test
        // can see rather than a thing a comment claims.
        set_enabled(true);
        let asked = Rc::new(Cell::new(0));
        let size = Size::new(200.0, 100.0);
        let mut tree = ElementTree::new();
        tree.rebuild(counting(&asked, true));
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::loose(size.width, size.height));

        assert!(flush(size, &root).is_some(), "the first frame has everything to say");
        assert_eq!(asked.get(), 1);

        // Frames two and three change nothing: no rebuild, no layout, nothing
        // marked. Upstream would visit an empty dirty set; this returns before
        // the walk.
        assert!(flush(size, &root).is_none(), "a quiet frame sent something");
        assert!(flush(size, &root).is_none());
        assert_eq!(asked.get(), 1, "a quiet frame asked the tree what it says");

        // And it is not stuck: whatever marks, walks.
        mark_needs_update();
        assert!(flush(size, &root).is_some(), "a marked frame said nothing");
        assert_eq!(asked.get(), 2);
        set_enabled(false);
    }

    #[test]
    fn a_walk_that_came_out_the_same_sends_nothing() {
        // The third gate. Upstream's `sendSemanticsUpdate` opens with
        // `if (_dirtyNodes.isEmpty) return;` and puts only changed nodes on
        // the wire; here the walk ran -- `asked` proves it -- and produced the
        // tree the platform is already holding, so nothing crosses.
        //
        // This is the ordinary case for anything that animates without
        // speaking: a ripple, a colour tween, a scroll that has come to rest.
        set_enabled(true);
        let asked = Rc::new(Cell::new(0));
        let size = Size::new(200.0, 100.0);
        let mut tree = ElementTree::new();
        tree.rebuild(counting(&asked, false));
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::loose(size.width, size.height));

        assert!(flush(size, &root).is_some(), "the first frame has everything to say");
        assert_eq!(asked.get(), 1);

        mark_needs_update();
        assert!(flush(size, &root).is_none(), "the same tree was sent twice");
        assert_eq!(asked.get(), 2, "the walk was supposed to run");
        // What the platform holds is unchanged, not cleared.
        assert!(tree_or_fail().iter().any(|node| node.id == 21));
        set_enabled(false);
    }

    #[test]
    fn laying_out_is_what_marks_the_ordinary_frame() {
        // Upstream calls `markNeedsSemanticsUpdate` from inside
        // `RenderObject.layout`, on the line after `performLayout`, and that
        // single call is what covers nearly everything: a scroll, a rebuild
        // that changed a size, a row that appeared. Here it is
        // `RenderRef::layout`, past the early return -- so a re-layout at the
        // same constraints on a clean tree marks nothing, and a real one does.
        set_enabled(true);
        let asked = Rc::new(Cell::new(0));
        let size = Size::new(200.0, 100.0);
        let mut tree = ElementTree::new();
        tree.rebuild(counting(&asked, true));
        let mut root = tree.build_render_tree().expect("mounted");

        root.layout(BoxConstraints::loose(size.width, size.height));
        assert!(flush(size, &root).is_some());

        // Same constraints, clean tree: the early return, and nothing marked.
        root.layout(BoxConstraints::loose(size.width, size.height));
        assert!(flush(size, &root).is_none(), "an unchanged layout marked semantics");

        // A different size is a real layout, and everything that moved has
        // something new to say about where it is.
        root.layout(BoxConstraints::loose(180.0, 90.0));
        assert!(flush(Size::new(180.0, 90.0), &root).is_some(), "a re-layout said nothing");
        set_enabled(false);
    }

    #[test]
    fn an_action_reaches_the_handler_no_frame_carried() {
        // The cost of skipping walks, paid where it cannot be felt. A rebuild
        // that replaces only a closure changes nothing measured and nothing
        // drawn, so nothing marks itself and no walk happens -- and a handler
        // remembered by the last walk would be the wrong one by exactly one
        // build. `perform_action` asks the live object instead.
        use crate::framework::{BuildContext, StateHandle, StatefulComponent, stateful};

        #[derive(Default)]
        struct Round {
            second: bool,
        }

        struct Chooser {
            sink: Rc<RefCell<Option<StateHandle<Round>>>>,
            called: Rc<Cell<&'static str>>,
        }

        impl StatefulComponent for Chooser {
            type State = Round;

            fn build(
                &self,
                state: &Round,
                handle: StateHandle<Round>,
                _context: &mut BuildContext,
            ) -> AnyWidget {
                *self.sink.borrow_mut() = Some(handle);
                let which = if state.second { "second" } else { "first" };
                let called = Rc::clone(&self.called);
                semantics_with_action(
                    12,
                    // Deliberately the same label both times: if the
                    // annotation itself changed, `update_from` would mark and
                    // a walk would happen, and the point is the frame where
                    // one does not.
                    SemanticsProperties::button("Act"),
                    leaf(|| SizedBox::new(50.0, 20.0)),
                    move |_| called.set(which),
                )
            }
        }

        set_enabled(true);
        let size = Size::new(200.0, 100.0);
        let sink: Rc<RefCell<Option<StateHandle<Round>>>> = Rc::new(RefCell::new(None));
        let called = Rc::new(Cell::new("nobody"));
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(Chooser { sink: sink.clone(), called: Rc::clone(&called) }));
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::loose(size.width, size.height));
        assert!(flush(size, &root).is_some());
        assert!(perform_action(&root, 12, SemanticsAction::Tap));
        assert_eq!(called.get(), "first");

        sink.borrow().as_ref().expect("built").set_state(|state| state.second = true);
        tree.rebuild_dirty();
        let root = tree.build_render_tree().expect("still mounted");
        // No layout and no walk: nothing about this frame is worth either.
        assert!(flush(size, &root).is_none(), "a new closure should not be news");
        assert!(perform_action(&root, 12, SemanticsAction::Tap));
        assert_eq!(called.get(), "second", "the reader called last build's closure");
        set_enabled(false);
    }

    #[test]
    fn a_reader_arriving_is_told_everything() {
        // Upstream's `scheduleInitialSemantics`: the tree a reader has never
        // been shown is entirely news, however quiet the frame is otherwise.
        set_enabled(true);
        let asked = Rc::new(Cell::new(0));
        let size = Size::new(200.0, 100.0);
        let mut tree = ElementTree::new();
        tree.rebuild(counting(&asked, false));
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::loose(size.width, size.height));
        assert!(flush(size, &root).is_some());
        assert!(flush(size, &root).is_none(), "quiet");

        // The reader leaves and another arrives. Nothing on screen moved.
        set_enabled(false);
        assert!(super::tree().is_empty(), "a tree nobody holds should not be kept");
        set_enabled(true);
        let sent = flush(size, &root).expect("the new reader was told nothing");
        assert!(sent.iter().any(|node| node.id == 21));
        set_enabled(false);
    }

    #[test]
    fn a_fade_to_nothing_marks_but_a_fade_between_two_visible_values_does_not() {
        // Upstream's `RenderOpacity.set opacity` marks semantics on
        // `wasVisible != isVisible` and on nothing else. It is the one place
        // in this framework where a repaint alone changes what a reader would
        // hear, because a fully transparent subtree is one
        // `visit_children_for_semantics` refuses to enter.
        use crate::render::{RenderOpacity, UpdateEffect};

        set_enabled(true);
        let mut faded = RenderOpacity::new(1.0, SizedBox::new(10.0, 10.0));
        let step = |faded: &mut RenderOpacity, to: f32| {
            let mut fresh = RenderOpacity::new(to, SizedBox::new(10.0, 10.0));
            NEEDS_UPDATE.with(|needs| needs.set(false));
            let effect = faded.update_from(&mut fresh);
            (effect, NEEDS_UPDATE.with(|needs| needs.get()))
        };

        let (effect, marked) = step(&mut faded, 0.5);
        assert_eq!(effect, Some(UpdateEffect::Relayout), "the child is a new object");
        assert!(!marked, "half way is still visible, and says the same thing");

        let (_, marked) = step(&mut faded, 0.0);
        assert!(marked, "a subtree that stopped being drawn stopped being described");

        let (_, marked) = step(&mut faded, 0.25);
        assert!(marked, "and it is describable again");
        set_enabled(false);
    }
}
