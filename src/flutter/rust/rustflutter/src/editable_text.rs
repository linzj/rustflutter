//! The editable text field's controller and its state -- a port of the
//! decisions in upstream's `widgets/editable_text.dart`.
//!
//! Three things in here are worth arriving at before the code.
//!
//! **A text field is not the owner of its own value.** A
//! [`TextEditingController`] is, and the field merely listens. That is what
//! lets a form read the text without reaching into the widget, and it is why
//! the controller's setters carry rules of their own: setting a selection
//! clears the composing range it leaves, and setting the text resets the
//! cursor entirely.
//!
//! **Obscured input is briefly not obscured.** A password field on a phone
//! shows the character just typed for three cursor blinks and then hides it.
//! Nobody can type a password blind on a soft keyboard, and the compromise --
//! one character, briefly, on mobile only -- is what every platform settled on.
//!
//! **What the toolbar offers depends on more than the selection.** Cut is off
//! in a read-only field and off in an obscured one; copy is on in a read-only
//! field but still off in an obscured one; and several entries exist only on
//! one platform. The matrix is the interesting part of the file and it is
//! pinned entry by entry below.
//!
//! ## What is not here
//!
//! The render object, the input connection to the platform, the selection
//! overlay and the autofill plumbing are absent -- see [`crate::editable`] for
//! this crate's own editing widget. What is ported is the controller, the two
//! configuration objects, and the decisions `EditableTextState` makes that do
//! not need a tree.

use crate::services::text_input::TextEditingValue;
use crate::text_selection::{ClipboardStatus, LiveTextInputStatus};

/// Upstream `TargetPlatform`, from `foundation/platform.dart`.
///
/// Not counted by the coverage ruler, which reads `class` and `mixin`
/// declarations rather than enums, but several rules below turn on it and
/// spelling them with a bare string would make them unreadable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TargetPlatform {
    Android,
    Fuchsia,
    IOS,
    Linux,
    MacOS,
    #[default]
    Windows,
}

impl TargetPlatform {
    /// Every value, so a test can walk the table rather than sample it.
    ///
    /// Which matters more here than for most enums: nearly everything that
    /// switches on a platform has a "the rest do nothing" arm, and sampling
    /// two of the six leaves four platforms whose behaviour nothing has ever
    /// looked at.
    pub const ALL: [TargetPlatform; 6] = [
        TargetPlatform::Android,
        TargetPlatform::Fuchsia,
        TargetPlatform::IOS,
        TargetPlatform::Linux,
        TargetPlatform::MacOS,
        TargetPlatform::Windows,
    ];

    /// The three upstream calls "mobile platforms" when deciding whether to
    /// briefly reveal a password character. A desktop keyboard gives real
    /// feedback -- the reader can feel the keys -- so the reveal is a phone
    /// affordance and stays one.
    /// The platform this build is actually running on.
    ///
    /// Upstream's `defaultTargetPlatform`, which reads the host at run time;
    /// here it is decided at compile time, because a Rust binary is built for
    /// one host and cannot be asked to be another. A caller who wants another
    /// answer sets [`crate::theme::ThemeData::platform`], which is the same
    /// override upstream offers.
    pub const fn host() -> TargetPlatform {
        if cfg!(target_os = "windows") {
            TargetPlatform::Windows
        } else if cfg!(target_os = "macos") {
            TargetPlatform::MacOS
        } else if cfg!(target_os = "android") {
            TargetPlatform::Android
        } else if cfg!(target_os = "ios") {
            TargetPlatform::IOS
        } else {
            TargetPlatform::Linux
        }
    }

    pub fn is_mobile(self) -> bool {
        matches!(
            self,
            TargetPlatform::Android | TargetPlatform::Fuchsia | TargetPlatform::IOS
        )
    }
}

/// Upstream `TextRange.empty`, spelled as this crate spells composing ranges.
pub const NO_RANGE: (i32, i32) = (-1, -1);

/// Upstream's `TextSelection.collapsed(offset: -1)`.
///
/// It is not "the caret at the start" -- it is **no selection at all**. A field
/// built with a controller in this state fixes it on focus by putting the caret
/// at the end of the text, which is why a controller made from a plain string
/// does not drop the reader at position zero.
pub const NO_SELECTION: i32 = -1;

/// Upstream `TextEditingController`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextEditingController {
    value: TextEditingValue,
    notifications: usize,
}

impl TextEditingController {
    /// Upstream's default constructor, whose selection is
    /// [`NO_SELECTION`] rather than a caret at zero.
    pub fn new(text: Option<&str>) -> TextEditingController {
        let text = text.unwrap_or_default().to_string();
        TextEditingController {
            value: TextEditingValue {
                text,
                selection_base: NO_SELECTION,
                selection_extent: NO_SELECTION,
                composing_base: NO_RANGE.0,
                composing_extent: NO_RANGE.1,
            },
            notifications: 0,
        }
    }

    /// Upstream's `TextEditingController.fromValue`, which **asserts the
    /// composing range is valid for the text**. Upstream's note is that the
    /// check applies "even for readonly text fields": a composing range past
    /// the end of the text would throw when the field tried to underline it,
    /// and a read-only field is no less likely to be handed one.
    pub fn from_value(value: Option<TextEditingValue>) -> TextEditingController {
        let value = value.unwrap_or_default();
        debug_assert!(
            Self::composing_is_acceptable(&value),
            "TextEditingValue has an invalid non-empty composing range"
        );
        TextEditingController {
            value,
            notifications: 0,
        }
    }

    /// Upstream's `!composing.isValid || isComposingRangeValid`.
    pub fn composing_is_acceptable(value: &TextEditingValue) -> bool {
        if !value.is_composing() {
            return true;
        }
        let length = value.text.encode_utf16().count() as i32;
        value.composing_base >= 0
            && value.composing_extent >= 0
            && value.composing_base <= length
            && value.composing_extent <= length
    }

    pub fn value(&self) -> &TextEditingValue {
        &self.value
    }

    pub fn notifications(&self) -> usize {
        self.notifications
    }

    pub fn text(&self) -> &str {
        &self.value.text
    }

    pub fn selection(&self) -> (i32, i32) {
        (self.value.selection_base, self.value.selection_extent)
    }

    pub fn composing(&self) -> (i32, i32) {
        (self.value.composing_base, self.value.composing_extent)
    }

    /// Upstream's `value=`, which notifies whenever the value differs.
    pub fn set_value(&mut self, value: TextEditingValue) {
        debug_assert!(
            Self::composing_is_acceptable(&value),
            "TextEditingValue has an invalid non-empty composing range"
        );
        if self.value == value {
            return;
        }
        self.value = value;
        self.notifications += 1;
    }

    /// Upstream's `text=`, and its documentation calls it a test-only
    /// convenience for a reason: it **throws the cursor away**, resetting the
    /// selection to [`NO_SELECTION`] and clearing the composing range. Setting
    /// it while the reader is typing would drop them back to nowhere in
    /// particular; production code sets `value` with a selection it chose.
    pub fn set_text(&mut self, text: impl Into<String>) {
        let mut next = self.value.clone();
        next.text = text.into();
        next.selection_base = NO_SELECTION;
        next.selection_extent = NO_SELECTION;
        next.composing_base = NO_RANGE.0;
        next.composing_extent = NO_RANGE.1;
        self.set_value(next);
    }

    /// Upstream's `selection=`.
    ///
    /// Two rules. It **throws** for a selection past the end of the text --
    /// silently clamping would leave the caller believing a selection they
    /// never got. And it **clears the composing range when the new selection
    /// leaves it**: the reader has moved out of the word the IME was building,
    /// so that word is finished whether or not they meant to finish it.
    pub fn set_selection(&mut self, base: i32, extent: i32) -> Result<(), String> {
        let length = self.value.text.encode_utf16().count() as i32;
        let start = base.min(extent);
        let end = base.max(extent);
        if length < end || length < start {
            return Err(format!("invalid text selection: {base}..{extent}"));
        }
        let within = self.selection_is_within_composing_range(start, end);
        let mut next = self.value.clone();
        next.selection_base = base;
        next.selection_extent = extent;
        if !within {
            next.composing_base = NO_RANGE.0;
            next.composing_extent = NO_RANGE.1;
        }
        self.set_value(next);
        Ok(())
    }

    fn selection_is_within_composing_range(&self, start: i32, end: i32) -> bool {
        let composing_start = self.value.composing_base.min(self.value.composing_extent);
        let composing_end = self.value.composing_base.max(self.value.composing_extent);
        start >= composing_start && end <= composing_end
    }

    /// Upstream's `clear`, whose selection is **collapsed at zero** rather
    /// than [`NO_SELECTION`]. An emptied field has a caret; a freshly
    /// constructed one has not been focused yet and so has nowhere to put one.
    pub fn clear(&mut self) {
        self.set_value(TextEditingValue {
            text: String::new(),
            selection_base: 0,
            selection_extent: 0,
            composing_base: NO_RANGE.0,
            composing_extent: NO_RANGE.1,
        });
    }

    /// Upstream's `clearComposing`: the reader is done with that word.
    pub fn clear_composing(&mut self) {
        let mut next = self.value.clone();
        next.composing_base = NO_RANGE.0;
        next.composing_extent = NO_RANGE.1;
        self.set_value(next);
    }

    /// Upstream's `buildTextSpan`, reduced to the runs it produces.
    pub fn build_text_runs(&self, with_composing: bool) -> Vec<(String, bool)> {
        Self::text_runs_for(&self.value, with_composing)
    }

    /// The same, for a value that did not come through a constructor.
    ///
    /// The composing run is underlined and the rest is not, so the answer is
    /// three pieces. **A composing range that is out of range for the text is
    /// ignored rather than clamped**, and upstream says why: throwing in
    /// release would build the field with a broken subtree, and a missing
    /// underline is a far smaller failure than a missing field.
    ///
    /// That check is a release-mode safety net rather than dead code beside
    /// the assertions above. The assertions only see values that pass through
    /// [`TextEditingController::from_value`] or
    /// [`TextEditingController::set_value`], and in a release build they are
    /// not there at all -- so this is the only thing standing between a
    /// malformed range and a field that will not build.
    pub fn text_runs_for(value: &TextEditingValue, with_composing: bool) -> Vec<(String, bool)> {
        let out_of_range =
            !Self::composing_is_acceptable(value) || !value.is_composing() || !with_composing;
        if out_of_range {
            return vec![(value.text.clone(), false)];
        }
        let Some(range) = value.composing_bytes() else {
            return vec![(value.text.clone(), false)];
        };
        vec![
            (value.text[..range.start].to_string(), false),
            (value.text[range.clone()].to_string(), true),
            (value.text[range.end..].to_string(), false),
        ]
    }
}

/// Upstream `ToolbarOptions`, deprecated in favour of `contextMenuBuilder`.
///
/// Every option defaults to **false**, which reads oddly for a toolbar until
/// you see how it is used: a widget that supplies one is overriding the
/// default set entirely, so anything it does not name is something it did not
/// want.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ToolbarOptions {
    pub copy: bool,
    pub cut: bool,
    pub paste: bool,
    pub select_all: bool,
}

impl ToolbarOptions {
    /// Upstream's `ToolbarOptions.empty`.
    pub const EMPTY: ToolbarOptions = ToolbarOptions {
        copy: false,
        cut: false,
        paste: false,
        select_all: false,
    };

    pub fn new() -> ToolbarOptions {
        ToolbarOptions::EMPTY
    }
}

/// Upstream's `kDefaultContentInsertionMimeTypes`.
pub const DEFAULT_CONTENT_INSERTION_MIME_TYPES: &[&str] = &[
    "image/png",
    "image/bmp",
    "image/jpg",
    "image/tiff",
    "image/gif",
    "image/jpeg",
    "image/webp",
];

/// Upstream `ContentInsertionConfiguration`: the soft keyboard handing the
/// field an image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentInsertionConfiguration {
    /// Upstream asserts this is **not empty**, and the assertion is the right
    /// way round: an empty list reads as "allow nothing", which no caller
    /// means -- they would simply not have supplied a configuration. An empty
    /// list is a mistake, so it is refused rather than obeyed.
    pub allowed_mime_types: Vec<String>,
}

impl Default for ContentInsertionConfiguration {
    fn default() -> ContentInsertionConfiguration {
        ContentInsertionConfiguration::new()
    }
}

impl ContentInsertionConfiguration {
    pub fn new() -> ContentInsertionConfiguration {
        ContentInsertionConfiguration {
            allowed_mime_types: DEFAULT_CONTENT_INSERTION_MIME_TYPES
                .iter()
                .map(|mime| (*mime).to_string())
                .collect(),
        }
    }

    pub fn with_allowed_mime_types(types: &[&str]) -> Option<ContentInsertionConfiguration> {
        if types.is_empty() {
            return None;
        }
        Some(ContentInsertionConfiguration {
            allowed_mime_types: types.iter().map(|mime| (*mime).to_string()).collect(),
        })
    }

    pub fn accepts(&self, mime_type: &str) -> bool {
        self.allowed_mime_types.iter().any(|held| held == mime_type)
    }
}

/// Upstream `SmartDashesType`: whether the platform rewrites `--` as an
/// em dash while the reader types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmartDashesType {
    Disabled,
    Enabled,
}

/// Upstream `SmartQuotesType`: whether the platform rewrites `"` as typographic
/// quotes while the reader types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmartQuotesType {
    Disabled,
    Enabled,
}

impl SmartDashesType {
    /// Upstream sends `smartDashesType.index.toString()`, so the wire carries
    /// "0" and "1". The declaration order above is part of that format.
    pub fn index_string(self) -> &'static str {
        match self {
            SmartDashesType::Disabled => "0",
            SmartDashesType::Enabled => "1",
        }
    }

    /// Upstream's default, written the same way in `EditableText`,
    /// `TextField` and `CupertinoTextField`:
    ///
    /// ```dart
    /// smartDashesType ?? (obscureText ? SmartDashesType.disabled : SmartDashesType.enabled)
    /// ```
    ///
    /// **An obscured field turns it off**, and the reason is not cosmetic:
    /// smart substitution rewrites what was typed. Two hyphens become an em
    /// dash and a straight quote becomes a curly one -- harmless in prose, and
    /// in a password field it silently changes the characters the reader
    /// believes they entered.
    ///
    /// The parameter is nullable upstream and stays `Option` here, because
    /// **unset is not the same as either value**: unset means "decide from
    /// `obscureText`", and a field that wanted smart dashes in a password box
    /// can still say `Enabled` and get them.
    pub fn resolve(given: Option<SmartDashesType>, obscure_text: bool) -> SmartDashesType {
        given.unwrap_or(if obscure_text {
            SmartDashesType::Disabled
        } else {
            SmartDashesType::Enabled
        })
    }
}

impl SmartQuotesType {
    /// The same index-as-a-string encoding as [`SmartDashesType`].
    pub fn index_string(self) -> &'static str {
        match self {
            SmartQuotesType::Disabled => "0",
            SmartQuotesType::Enabled => "1",
        }
    }

    /// The same rule as [`SmartDashesType::resolve`], written separately
    /// upstream and separately here, because they are separate parameters: a
    /// field may take one and refuse the other.
    pub fn resolve(given: Option<SmartQuotesType>, obscure_text: bool) -> SmartQuotesType {
        given.unwrap_or(if obscure_text {
            SmartQuotesType::Disabled
        } else {
            SmartQuotesType::Enabled
        })
    }
}

/// Upstream `EditableText`: the field's configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct EditableText {
    pub read_only: bool,
    pub obscure_text: bool,
    /// Upstream's `smartDashesType`, nullable so that unset can mean "follow
    /// `obscure_text`". See [`SmartDashesType::resolve`].
    pub smart_dashes_type: Option<SmartDashesType>,
    /// Upstream's `smartQuotesType`.
    pub smart_quotes_type: Option<SmartQuotesType>,
    /// Upstream's `obscuringCharacter`, asserted to be exactly one character.
    /// The default is a bullet.
    pub obscuring_character: char,
    pub enable_interactive_selection: bool,
    pub toolbar_options: ToolbarOptions,
    /// Whether the field was given the newer `TextSelectionHandleControls`.
    /// Upstream branches every toolbar rule on this, because the deprecated
    /// controls carry a `ToolbarOptions` and the new ones do not.
    pub uses_handle_controls: bool,
    pub content_insertion: Option<ContentInsertionConfiguration>,
    pub max_length: Option<usize>,
}

impl Default for EditableText {
    fn default() -> EditableText {
        EditableText::new()
    }
}

impl EditableText {
    pub fn new() -> EditableText {
        EditableText {
            read_only: false,
            obscure_text: false,
            smart_dashes_type: None,
            smart_quotes_type: None,
            obscuring_character: '\u{2022}',
            enable_interactive_selection: true,
            toolbar_options: ToolbarOptions::EMPTY,
            uses_handle_controls: true,
            content_insertion: None,
            max_length: None,
        }
    }

    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub fn with_obscure_text(mut self, obscure: bool) -> Self {
        self.obscure_text = obscure;
        self
    }

    pub fn with_smart_dashes(mut self, smart: SmartDashesType) -> Self {
        self.smart_dashes_type = Some(smart);
        self
    }

    pub fn with_smart_quotes(mut self, smart: SmartQuotesType) -> Self {
        self.smart_quotes_type = Some(smart);
        self
    }

    /// What the platform is actually told, once the default has been resolved
    /// against `obscure_text`.
    pub fn smart_dashes(&self) -> SmartDashesType {
        SmartDashesType::resolve(self.smart_dashes_type, self.obscure_text)
    }

    pub fn smart_quotes(&self) -> SmartQuotesType {
        SmartQuotesType::resolve(self.smart_quotes_type, self.obscure_text)
    }

    pub fn with_toolbar_options(mut self, options: ToolbarOptions) -> Self {
        self.toolbar_options = options;
        self.uses_handle_controls = false;
        self
    }

    pub fn with_interactive_selection(mut self, enable: bool) -> Self {
        self.enable_interactive_selection = enable;
        self
    }
}

/// Upstream `EditableTextState`, reduced to the decisions it makes.
#[derive(Clone, Debug, PartialEq)]
pub struct EditableTextState {
    pub widget: EditableText,
    pub value: TextEditingValue,
    pub platform: TargetPlatform,
    /// Upstream reads this from the platform dispatcher. Android turns it off
    /// when the reader has asked the system not to reveal passwords.
    pub briefly_show_password: bool,
    pub clipboard_status: ClipboardStatus,
    pub live_text_input_status: Option<LiveTextInputStatus>,
    pub is_web: bool,
    pub has_input_connection: bool,
    obscure_show_char_ticks_pending: u32,
    obscure_latest_char_index: Option<i32>,
    batch_edit_depth: i32,
}

impl EditableTextState {
    /// Upstream's `_kObscureShowLatestCharCursorTicks`: three cursor blinks,
    /// not a duration. Tying it to the cursor means the reveal lasts as long
    /// as the reader's own sense of the field's rhythm rather than a number
    /// somebody picked.
    pub const OBSCURE_SHOW_LATEST_CHAR_TICKS: u32 = 3;

    pub fn new(widget: EditableText) -> EditableTextState {
        EditableTextState {
            widget,
            value: TextEditingValue::default(),
            platform: TargetPlatform::Android,
            briefly_show_password: true,
            clipboard_status: ClipboardStatus::Unknown,
            live_text_input_status: None,
            is_web: false,
            has_input_connection: true,
            obscure_show_char_ticks_pending: 0,
            obscure_latest_char_index: None,
            batch_edit_depth: 0,
        }
    }

    pub fn with_value(mut self, value: TextEditingValue) -> Self {
        self.value = value;
        self
    }

    pub fn with_platform(mut self, platform: TargetPlatform) -> Self {
        self.platform = platform;
        self
    }

    fn has_selection(&self) -> bool {
        self.value.has_selection()
    }

    fn selected_text(&self) -> &str {
        match self.value.selection_bytes() {
            Some(range) => &self.value.text[range],
            None => "",
        }
    }

    /// Upstream's `_shouldCreateInputConnection`.
    ///
    /// A read-only field normally needs no connection to the keyboard, with
    /// two exceptions that are both about **where the selection lives**. On
    /// the web and on macOS the platform owns the selection, so cutting the
    /// connection would cut the reader's ability to select the text at all --
    /// and selecting read-only text to copy it is the whole point of a
    /// read-only field.
    pub fn should_create_input_connection(&self) -> bool {
        self.is_web || self.platform == TargetPlatform::MacOS || !self.widget.read_only
    }

    /// Upstream's `cutEnabled`.
    ///
    /// Off in a read-only field, for the obvious reason, and off in an
    /// obscured one, for a less obvious one: cutting a password would put it
    /// on the clipboard in plain text, where every other application can read
    /// it.
    pub fn cut_enabled(&self) -> bool {
        if !self.widget.uses_handle_controls {
            return self.widget.toolbar_options.cut
                && !self.widget.read_only
                && !self.widget.obscure_text;
        }
        !self.widget.read_only && !self.widget.obscure_text && self.has_selection()
    }

    /// Upstream's `copyEnabled`, which differs from cut in exactly one place:
    /// **a read-only field can be copied from.** Reading and copying is what a
    /// read-only field is for.
    pub fn copy_enabled(&self) -> bool {
        if !self.widget.uses_handle_controls {
            return self.widget.toolbar_options.copy && !self.widget.obscure_text;
        }
        !self.widget.obscure_text && self.has_selection()
    }

    /// Upstream's `pasteEnabled`, and note what it does **not** check:
    /// `obscureText`. Pasting into a password field is fine -- the secret is
    /// going in, not coming out. Only the clipboard's own state gates it, and
    /// [`ClipboardStatus::Unknown`] does not count as pasteable.
    pub fn paste_enabled(&self) -> bool {
        if !self.widget.uses_handle_controls {
            return self.widget.toolbar_options.paste && !self.widget.read_only;
        }
        !self.widget.read_only && self.clipboard_status == ClipboardStatus::Pasteable
    }

    /// Upstream's `selectAllEnabled`, the most platform-dependent of the four.
    ///
    /// The shared part is `readOnly && obscureText` being refused -- a field
    /// whose contents can be neither seen nor changed has nothing to select
    /// for. After that the platforms disagree: **macOS never offers it**, iOS
    /// offers it only when nothing is selected yet, and the rest offer it
    /// unless everything is already selected. The last two are the same idea
    /// spelled with different strictness: an entry that would not change
    /// anything should not be in the menu.
    pub fn select_all_enabled(&self) -> bool {
        if !self.widget.uses_handle_controls {
            return self.widget.toolbar_options.select_all
                && (!self.widget.read_only || !self.widget.obscure_text)
                && self.widget.enable_interactive_selection;
        }
        if !self.widget.enable_interactive_selection
            || (self.widget.read_only && self.widget.obscure_text)
        {
            return false;
        }
        let length = self.value.text.encode_utf16().count() as i32;
        match self.platform {
            TargetPlatform::MacOS => false,
            TargetPlatform::IOS => !self.value.text.is_empty() && !self.has_selection(),
            _ => {
                let start = self.value.selection_base.min(self.value.selection_extent);
                let end = self.value.selection_base.max(self.value.selection_extent);
                !self.value.text.is_empty() && !(start == 0 && end == length)
            }
        }
    }

    /// Upstream's `lookUpEnabled` and `searchWebEnabled`, which are the same
    /// expression: iOS only, something selected, and the selection is not
    /// **only whitespace**. Looking up a run of spaces would open a dictionary
    /// on nothing.
    pub fn look_up_enabled(&self) -> bool {
        self.platform == TargetPlatform::IOS
            && !self.widget.obscure_text
            && self.has_selection()
            && !self.selected_text().trim().is_empty()
    }

    pub fn search_web_enabled(&self) -> bool {
        self.look_up_enabled()
    }

    /// Upstream's `shareEnabled`: the same test, on Android **and** iOS.
    /// Sharing is a system affordance the desktops do not have.
    pub fn share_enabled(&self) -> bool {
        matches!(self.platform, TargetPlatform::Android | TargetPlatform::IOS)
            && !self.widget.obscure_text
            && self.has_selection()
            && !self.selected_text().trim().is_empty()
    }

    /// Upstream's `liveTextInputEnabled`: scanning text out of the camera.
    ///
    /// It requires a **collapsed** selection, unlike everything else above,
    /// and that is the right way round -- Live Text inserts, so it needs a
    /// caret to insert at rather than a range to act on.
    pub fn live_text_input_enabled(&self) -> bool {
        self.live_text_input_status == Some(LiveTextInputStatus::Enabled)
            && !self.widget.obscure_text
            && !self.widget.read_only
            && !self.has_selection()
    }

    /// Upstream's reveal condition inside `updateEditingValue`.
    ///
    /// The `length + 1` test is what makes this a *typing* affordance and
    /// nothing else: a paste, a deletion, or an IME committing three
    /// characters at once all fail it, and none of them is a keystroke the
    /// reader needs confirmed.
    pub fn should_reveal_obscured_input(&self, next: &TextEditingValue) -> bool {
        self.has_input_connection
            && self.widget.obscure_text
            && self.briefly_show_password
            && next.text.encode_utf16().count() == self.value.text.encode_utf16().count() + 1
    }

    /// Applies an incoming value, arming the brief reveal if it qualifies.
    pub fn update_editing_value(&mut self, next: TextEditingValue) {
        if self.should_reveal_obscured_input(&next) {
            self.obscure_show_char_ticks_pending = Self::OBSCURE_SHOW_LATEST_CHAR_TICKS;
            self.obscure_latest_char_index = Some(self.value.selection_base);
        } else {
            self.obscure_show_char_ticks_pending = 0;
            self.obscure_latest_char_index = None;
        }
        self.value = next;
    }

    /// Upstream's `_onCursorTick`, which counts the reveal down -- and
    /// **collapses it to zero outright** if the platform setting went away
    /// mid-reveal, rather than letting the remaining ticks play out.
    pub fn on_cursor_tick(&mut self) {
        if self.obscure_show_char_ticks_pending == 0 {
            return;
        }
        self.obscure_show_char_ticks_pending = if self.briefly_show_password {
            self.obscure_show_char_ticks_pending - 1
        } else {
            0
        };
    }

    pub fn obscure_ticks_pending(&self) -> u32 {
        self.obscure_show_char_ticks_pending
    }

    /// Upstream's `buildTextSpan` for an obscured field: every character
    /// replaced, except possibly the one just typed.
    ///
    /// The reveal is **mobile only**, and checked here rather than at the
    /// point the ticks are armed -- so a field that moves between platforms
    /// stops revealing immediately rather than after its countdown.
    pub fn obscured_text(&self) -> String {
        let mut text: String = std::iter::repeat_n(
            self.widget.obscuring_character,
            self.value.text.chars().count(),
        )
        .collect();
        if !(self.briefly_show_password && self.platform.is_mobile()) {
            return text;
        }
        if self.obscure_show_char_ticks_pending == 0 {
            return text;
        }
        let Some(at) = self.obscure_latest_char_index else {
            return text;
        };
        let characters: Vec<char> = self.value.text.chars().collect();
        if at < 0 || at as usize >= characters.len() {
            return text;
        }
        let mut shown: Vec<char> = text.chars().collect();
        shown[at as usize] = characters[at as usize];
        text = shown.into_iter().collect();
        text
    }

    /// Upstream's `beginBatchEdit`. Batches **nest**, and the count is what
    /// makes that work: a formatter that opens one inside another must not
    /// have its inner close send a half-finished value to the platform.
    pub fn begin_batch_edit(&mut self) {
        self.batch_edit_depth += 1;
    }

    /// Upstream's `endBatchEdit`, which reports whether the outermost batch
    /// just closed -- that is when the value goes to the platform.
    pub fn end_batch_edit(&mut self) -> bool {
        self.batch_edit_depth -= 1;
        debug_assert!(
            self.batch_edit_depth >= 0,
            "unbalanced call to endBatchEdit: beginBatchEdit must be called first"
        );
        self.batch_edit_depth == 0
    }

    pub fn batch_edit_depth(&self) -> i32 {
        self.batch_edit_depth
    }

    /// Upstream's dispose-time assertion: a field must not be torn down with a
    /// batch still open, because the value inside it would never be sent.
    pub fn batch_edits_are_balanced(&self) -> bool {
        self.batch_edit_depth <= 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str, base: i32, extent: i32) -> TextEditingValue {
        TextEditingValue {
            text: text.to_string(),
            selection_base: base,
            selection_extent: extent,
            composing_base: -1,
            composing_extent: -1,
        }
    }

    fn state(widget: EditableText) -> EditableTextState {
        EditableTextState::new(widget).with_value(value("hello world", 0, 5))
    }

    // -- The controller ----------------------------------------------------

    #[test]
    fn a_fresh_controller_has_no_selection_rather_than_a_caret_at_zero() {
        // The field fixes it on focus by putting the caret at the end, which
        // is why a controller made from a string does not drop the reader at
        // position zero.
        let controller = TextEditingController::new(Some("hello"));
        assert_eq!(controller.text(), "hello");
        assert_eq!(controller.selection(), (NO_SELECTION, NO_SELECTION));

        let empty = TextEditingController::new(None);
        assert_eq!(empty.text(), "");
    }

    #[test]
    fn clearing_leaves_a_caret_where_a_fresh_controller_has_none() {
        // An emptied field has been focused; a freshly constructed one has
        // not, so it has nowhere to put a caret.
        let mut controller = TextEditingController::new(Some("hello"));
        controller.clear();
        assert_eq!(controller.text(), "");
        assert_eq!(controller.selection(), (0, 0));
    }

    #[test]
    fn setting_the_text_throws_the_cursor_away() {
        // Which is why upstream calls it a test-only convenience: doing it
        // while the reader is typing drops them back to nowhere in particular.
        let mut controller = TextEditingController::new(Some("hello"));
        controller.set_selection(1, 3).unwrap();
        assert_eq!(controller.selection(), (1, 3));

        controller.set_text("goodbye");
        assert_eq!(controller.text(), "goodbye");
        assert_eq!(
            controller.selection(),
            (NO_SELECTION, NO_SELECTION),
            "and the selection went with it"
        );
    }

    #[test]
    fn a_selection_past_the_end_of_the_text_is_refused_and_not_clamped() {
        // Clamping would leave the caller believing a selection they never
        // got.
        let mut controller = TextEditingController::new(Some("hello"));
        assert!(controller.set_selection(0, 6).is_err());
        assert!(controller.set_selection(9, 9).is_err());
        assert!(controller.set_selection(0, 5).is_ok(), "the end is allowed");
    }

    #[test]
    fn moving_out_of_the_composing_word_finishes_it() {
        // The reader left the word the IME was building, so that word is done
        // whether or not they meant to finish it.
        let mut controller = TextEditingController::from_value(Some(TextEditingValue {
            text: "hello world".to_string(),
            selection_base: 7,
            selection_extent: 7,
            composing_base: 6,
            composing_extent: 11,
        }));
        assert_eq!(controller.composing(), (6, 11));

        controller.set_selection(8, 8).unwrap();
        assert_eq!(
            controller.composing(),
            (6, 11),
            "still inside, so still composing"
        );

        controller.set_selection(2, 2).unwrap();
        assert_eq!(controller.composing(), NO_RANGE, "and now it is finished");
    }

    #[test]
    fn setting_the_same_value_notifies_nobody() {
        let mut controller = TextEditingController::new(Some("hello"));
        controller.set_selection(1, 3).unwrap();
        let before = controller.notifications();
        controller.set_selection(1, 3).unwrap();
        assert_eq!(controller.notifications(), before);
    }

    #[test]
    fn a_composing_range_past_the_end_of_the_text_is_ignored_rather_than_clamped() {
        // Throwing in release would build the field with a broken subtree, and
        // a missing underline is a far smaller failure than a missing field.
        // The constructors assert against this, so the net is reached only by
        // a value that did not come through one -- which in a release build is
        // every value, since the assertions are not there.
        let malformed = TextEditingValue {
            text: "hi".to_string(),
            selection_base: 0,
            selection_extent: 0,
            composing_base: 0,
            composing_extent: 9,
        };
        assert!(!TextEditingController::composing_is_acceptable(&malformed));
        assert_eq!(
            TextEditingController::text_runs_for(&malformed, true),
            vec![("hi".to_string(), false)],
            "one plain run, and the field still builds"
        );
    }

    #[test]
    fn the_composing_word_is_the_only_underlined_run() {
        let controller = TextEditingController::from_value(Some(TextEditingValue {
            text: "hello world".to_string(),
            selection_base: 11,
            selection_extent: 11,
            composing_base: 6,
            composing_extent: 11,
        }));
        assert_eq!(
            controller.build_text_runs(true),
            vec![
                ("hello ".to_string(), false),
                ("world".to_string(), true),
                (String::new(), false),
            ]
        );
        assert_eq!(
            controller.build_text_runs(false),
            vec![("hello world".to_string(), false)],
            "and a caller that does not want it gets one plain run"
        );
    }

    #[test]
    fn clearing_the_composing_range_says_the_word_is_done() {
        let mut controller = TextEditingController::from_value(Some(TextEditingValue {
            text: "hello".to_string(),
            selection_base: 5,
            selection_extent: 5,
            composing_base: 0,
            composing_extent: 5,
        }));
        controller.clear_composing();
        assert_eq!(controller.composing(), NO_RANGE);
        assert_eq!(controller.text(), "hello", "and the text stayed");
    }

    // -- The two configuration objects -------------------------------------

    #[test]
    fn every_toolbar_option_is_off_until_asked_for() {
        // A widget supplying one is overriding the default set entirely, so
        // anything it does not name is something it did not want.
        let options = ToolbarOptions::new();
        assert!(!options.copy && !options.cut && !options.paste && !options.select_all);
        assert_eq!(ToolbarOptions::EMPTY, ToolbarOptions::default());
    }

    #[test]
    fn an_empty_mime_list_is_a_mistake_rather_than_an_instruction() {
        // Nobody means "allow nothing" -- they would simply not have supplied
        // a configuration.
        assert!(ContentInsertionConfiguration::with_allowed_mime_types(&[]).is_none());

        let png_only =
            ContentInsertionConfiguration::with_allowed_mime_types(&["image/png"]).unwrap();
        assert!(png_only.accepts("image/png"));
        assert!(!png_only.accepts("image/gif"));
    }

    #[test]
    fn the_default_mime_types_are_what_a_keyboard_can_actually_send() {
        let config = ContentInsertionConfiguration::new();
        assert!(config.accepts("image/png"));
        assert!(config.accepts("image/gif"));
        assert!(!config.accepts("video/mp4"));
    }

    // -- The input connection ----------------------------------------------

    #[test]
    fn a_read_only_field_still_needs_the_keyboard_where_the_platform_owns_the_selection() {
        // Cutting the connection on the web or macOS would cut the reader's
        // ability to select the text at all -- and selecting read-only text to
        // copy it is the whole point.
        let read_only = state(EditableText::new().with_read_only(true));
        assert!(!read_only.should_create_input_connection());

        let mut on_web = read_only.clone();
        on_web.is_web = true;
        assert!(on_web.should_create_input_connection());

        let on_mac = read_only.clone().with_platform(TargetPlatform::MacOS);
        assert!(on_mac.should_create_input_connection());

        let editable = state(EditableText::new());
        assert!(editable.should_create_input_connection());
    }

    // -- The toolbar matrix ------------------------------------------------

    #[test]
    fn a_read_only_field_can_be_copied_from_but_not_cut_from() {
        // Reading and copying is what a read-only field is for.
        let field = state(EditableText::new().with_read_only(true));
        assert!(!field.cut_enabled());
        assert!(field.copy_enabled());
    }

    #[test]
    fn a_password_is_neither_cut_nor_copied() {
        // Either would put it on the clipboard in plain text, where every
        // other application can read it.
        let field = state(EditableText::new().with_obscure_text(true));
        assert!(!field.cut_enabled());
        assert!(!field.copy_enabled());
    }

    #[test]
    fn pasting_into_a_password_field_is_fine_because_the_secret_goes_in() {
        // Note what pasteEnabled does not check: obscureText.
        let mut field = state(EditableText::new().with_obscure_text(true));
        field.clipboard_status = ClipboardStatus::Pasteable;
        assert!(field.paste_enabled());

        field.widget.read_only = true;
        assert!(!field.paste_enabled(), "but not into a read-only one");
    }

    #[test]
    fn an_unknown_clipboard_does_not_count_as_pasteable() {
        // Offering paste before the answer is back gives a button that might
        // do nothing.
        let mut field = state(EditableText::new());
        field.clipboard_status = ClipboardStatus::Unknown;
        assert!(!field.paste_enabled());

        field.clipboard_status = ClipboardStatus::NotPasteable;
        assert!(!field.paste_enabled());

        field.clipboard_status = ClipboardStatus::Pasteable;
        assert!(field.paste_enabled());
    }

    #[test]
    fn nothing_is_offered_for_a_field_that_can_be_neither_seen_nor_changed() {
        let field = state(
            EditableText::new()
                .with_read_only(true)
                .with_obscure_text(true),
        );
        assert!(!field.select_all_enabled());
        assert!(!field.cut_enabled());
        assert!(!field.copy_enabled());
    }

    #[test]
    fn select_all_is_offered_only_where_it_would_change_something() {
        // An entry that does nothing should not be in the menu -- and the
        // platforms spell that with different strictness.
        let all_selected = EditableTextState::new(EditableText::new())
            .with_value(value("hello", 0, 5))
            .with_platform(TargetPlatform::Android);
        assert!(!all_selected.select_all_enabled(), "already all of it");

        let some_selected = EditableTextState::new(EditableText::new())
            .with_value(value("hello", 0, 2))
            .with_platform(TargetPlatform::Android);
        assert!(some_selected.select_all_enabled());

        let ios_some = some_selected.clone().with_platform(TargetPlatform::IOS);
        assert!(
            !ios_some.select_all_enabled(),
            "iOS is stricter: anything selected at all is enough to drop it"
        );

        let ios_none = EditableTextState::new(EditableText::new())
            .with_value(value("hello", 2, 2))
            .with_platform(TargetPlatform::IOS);
        assert!(ios_none.select_all_enabled());
    }

    #[test]
    fn macos_never_offers_select_all_at_all() {
        let field = EditableTextState::new(EditableText::new())
            .with_value(value("hello", 2, 2))
            .with_platform(TargetPlatform::MacOS);
        assert!(!field.select_all_enabled());
    }

    #[test]
    fn an_empty_field_has_nothing_to_select() {
        for platform in [TargetPlatform::Android, TargetPlatform::IOS] {
            let field = EditableTextState::new(EditableText::new())
                .with_value(value("", 0, 0))
                .with_platform(platform);
            assert!(!field.select_all_enabled(), "{platform:?}");
        }
    }

    #[test]
    fn turning_interactive_selection_off_takes_select_all_with_it() {
        let field = EditableTextState::new(EditableText::new().with_interactive_selection(false))
            .with_value(value("hello", 2, 2))
            .with_platform(TargetPlatform::Android);
        assert!(!field.select_all_enabled());
    }

    #[test]
    fn the_deprecated_options_gate_everything_when_handle_controls_are_not_used() {
        // The old controls carry a ToolbarOptions and the new ones do not, so
        // every rule branches on which is in play.
        let nothing =
            EditableTextState::new(EditableText::new().with_toolbar_options(ToolbarOptions::EMPTY))
                .with_value(value("hello", 0, 3));
        assert!(!nothing.cut_enabled());
        assert!(!nothing.copy_enabled());

        let copy_only =
            EditableTextState::new(EditableText::new().with_toolbar_options(ToolbarOptions {
                copy: true,
                ..ToolbarOptions::EMPTY
            }))
            .with_value(value("hello", 0, 3));
        assert!(copy_only.copy_enabled());
        assert!(!copy_only.cut_enabled());
    }

    #[test]
    fn copy_needs_something_selected_only_under_the_new_controls() {
        // The deprecated path never consulted the selection, which is part of
        // why it was replaced.
        let collapsed_new =
            EditableTextState::new(EditableText::new()).with_value(value("hello", 2, 2));
        assert!(!collapsed_new.copy_enabled());

        let collapsed_old =
            EditableTextState::new(EditableText::new().with_toolbar_options(ToolbarOptions {
                copy: true,
                ..ToolbarOptions::EMPTY
            }))
            .with_value(value("hello", 2, 2));
        assert!(collapsed_old.copy_enabled());
    }

    #[test]
    fn looking_up_a_run_of_spaces_would_open_a_dictionary_on_nothing() {
        let words = EditableTextState::new(EditableText::new())
            .with_value(value("hello world", 0, 5))
            .with_platform(TargetPlatform::IOS);
        assert!(words.look_up_enabled());
        assert!(words.search_web_enabled());

        let spaces = EditableTextState::new(EditableText::new())
            .with_value(value("hello   world", 5, 8))
            .with_platform(TargetPlatform::IOS);
        assert!(!spaces.look_up_enabled());
    }

    #[test]
    fn look_up_is_ios_only_where_share_is_ios_and_android() {
        // Sharing is a system affordance the desktops do not have; looking up
        // is one only iOS has.
        let make = |platform| {
            EditableTextState::new(EditableText::new())
                .with_value(value("hello world", 0, 5))
                .with_platform(platform)
        };
        assert!(make(TargetPlatform::IOS).look_up_enabled());
        assert!(!make(TargetPlatform::Android).look_up_enabled());

        assert!(make(TargetPlatform::IOS).share_enabled());
        assert!(make(TargetPlatform::Android).share_enabled());
        assert!(!make(TargetPlatform::MacOS).share_enabled());
        assert!(!make(TargetPlatform::Windows).share_enabled());
    }

    #[test]
    fn live_text_needs_a_caret_where_everything_else_needs_a_selection() {
        // Live Text inserts, so it needs a place to insert at rather than a
        // range to act on.
        let mut field = EditableTextState::new(EditableText::new())
            .with_value(value("hello", 2, 2))
            .with_platform(TargetPlatform::IOS);
        field.live_text_input_status = Some(LiveTextInputStatus::Enabled);
        assert!(field.live_text_input_enabled());

        let mut selected = field.clone();
        selected.value = value("hello", 0, 3);
        assert!(!selected.live_text_input_enabled());

        let mut off = field.clone();
        off.live_text_input_status = Some(LiveTextInputStatus::Disabled);
        assert!(!off.live_text_input_enabled());

        let mut unknown = field.clone();
        unknown.live_text_input_status = None;
        assert!(!unknown.live_text_input_enabled());
    }

    // -- The brief reveal --------------------------------------------------

    #[test]
    fn only_a_single_typed_character_is_briefly_revealed() {
        // A paste, a deletion, or an IME committing three characters at once
        // all fail the length + 1 test, and none of them is a keystroke the
        // reader needs confirmed.
        let mut field = EditableTextState::new(EditableText::new().with_obscure_text(true))
            .with_value(value("abc", 3, 3));

        assert!(field.should_reveal_obscured_input(&value("abcd", 4, 4)));
        assert!(
            !field.should_reveal_obscured_input(&value("abcdef", 6, 6)),
            "a paste"
        );
        assert!(
            !field.should_reveal_obscured_input(&value("ab", 2, 2)),
            "a deletion"
        );

        field.briefly_show_password = false;
        assert!(
            !field.should_reveal_obscured_input(&value("abcd", 4, 4)),
            "and the reader asked the system not to"
        );
    }

    #[test]
    fn the_revealed_character_is_the_one_that_was_just_typed() {
        let mut field = EditableTextState::new(EditableText::new().with_obscure_text(true))
            .with_value(value("abc", 3, 3))
            .with_platform(TargetPlatform::Android);
        field.update_editing_value(value("abcd", 4, 4));

        assert_eq!(field.obscure_ticks_pending(), 3);
        assert_eq!(
            field.obscured_text(),
            "•••d",
            "everything hidden but the newest"
        );
    }

    #[test]
    fn the_reveal_lasts_three_cursor_blinks_and_then_stops() {
        let mut field = EditableTextState::new(EditableText::new().with_obscure_text(true))
            .with_value(value("abc", 3, 3))
            .with_platform(TargetPlatform::Android);
        field.update_editing_value(value("abcd", 4, 4));

        for expected in [2, 1, 0] {
            field.on_cursor_tick();
            assert_eq!(field.obscure_ticks_pending(), expected);
        }
        assert_eq!(field.obscured_text(), "••••");

        field.on_cursor_tick();
        assert_eq!(field.obscure_ticks_pending(), 0, "and stays there");
    }

    #[test]
    fn the_setting_going_away_mid_reveal_ends_it_at_once() {
        // Rather than letting the remaining ticks play out with the reader's
        // password on screen.
        let mut field = EditableTextState::new(EditableText::new().with_obscure_text(true))
            .with_value(value("abc", 3, 3))
            .with_platform(TargetPlatform::Android);
        field.update_editing_value(value("abcd", 4, 4));
        assert_eq!(field.obscure_ticks_pending(), 3);

        field.briefly_show_password = false;
        field.on_cursor_tick();
        assert_eq!(field.obscure_ticks_pending(), 0, "not 2");
    }

    #[test]
    fn a_desktop_never_reveals_the_character_at_all() {
        // A desktop keyboard gives real feedback -- the reader can feel the
        // keys -- so the reveal is a phone affordance and stays one.
        let mut field = EditableTextState::new(EditableText::new().with_obscure_text(true))
            .with_value(value("abc", 3, 3))
            .with_platform(TargetPlatform::MacOS);
        field.update_editing_value(value("abcd", 4, 4));
        assert_eq!(
            field.obscure_ticks_pending(),
            3,
            "the ticks are armed either way"
        );
        assert_eq!(
            field.obscured_text(),
            "••••",
            "but the platform check happens when the text is built"
        );
    }

    #[test]
    fn the_next_keystroke_moves_the_reveal_to_it() {
        let mut field = EditableTextState::new(EditableText::new().with_obscure_text(true))
            .with_value(value("ab", 2, 2))
            .with_platform(TargetPlatform::IOS);
        field.update_editing_value(value("abc", 3, 3));
        assert_eq!(field.obscured_text(), "••c");

        field.update_editing_value(value("abcd", 4, 4));
        assert_eq!(field.obscured_text(), "•••d", "and the old one is hidden");
    }

    #[test]
    fn a_change_that_is_not_a_keystroke_hides_everything_again() {
        let mut field = EditableTextState::new(EditableText::new().with_obscure_text(true))
            .with_value(value("abc", 3, 3))
            .with_platform(TargetPlatform::Android);
        field.update_editing_value(value("abcd", 4, 4));
        assert_eq!(field.obscured_text(), "•••d");

        field.update_editing_value(value("abcdefg", 7, 7));
        assert_eq!(field.obscured_text(), "•••••••");
        assert_eq!(field.obscure_ticks_pending(), 0);
    }

    #[test]
    fn the_obscuring_character_is_whatever_the_field_chose() {
        let mut widget = EditableText::new().with_obscure_text(true);
        widget.obscuring_character = '*';
        let field = EditableTextState::new(widget).with_value(value("abc", 3, 3));
        assert_eq!(field.obscured_text(), "***");
    }

    // -- Batch edits -------------------------------------------------------

    #[test]
    fn only_the_outermost_batch_closing_sends_the_value() {
        // A formatter opening one inside another must not have its inner close
        // send a half-finished value to the platform.
        let mut field = state(EditableText::new());
        field.begin_batch_edit();
        field.begin_batch_edit();
        assert_eq!(field.batch_edit_depth(), 2);

        assert!(!field.end_batch_edit(), "the inner one");
        assert!(field.end_batch_edit(), "and now the outer");
        assert_eq!(field.batch_edit_depth(), 0);
    }

    #[test]
    fn a_field_torn_down_mid_batch_would_never_send_what_is_in_it() {
        let mut field = state(EditableText::new());
        assert!(field.batch_edits_are_balanced());
        field.begin_batch_edit();
        assert!(!field.batch_edits_are_balanced());
        field.end_batch_edit();
        assert!(field.batch_edits_are_balanced());
    }
}

#[cfg(test)]
mod smart_substitution_tests {
    use super::{EditableText, SmartDashesType, SmartQuotesType};

    #[test]
    fn an_obscured_field_turns_smart_substitution_off() {
        // Not cosmetic: smart substitution rewrites what was typed. Two
        // hyphens become an em dash and a straight quote becomes a curly one,
        // which in a password field silently changes the characters the reader
        // believes they entered.
        let password = EditableText::new().with_obscure_text(true);
        assert_eq!(password.smart_dashes(), SmartDashesType::Disabled);
        assert_eq!(password.smart_quotes(), SmartQuotesType::Disabled);
    }

    #[test]
    fn and_an_ordinary_one_leaves_it_on() {
        let prose = EditableText::new();
        assert!(!prose.obscure_text);
        assert_eq!(prose.smart_dashes(), SmartDashesType::Enabled);
        assert_eq!(prose.smart_quotes(), SmartQuotesType::Enabled);
    }

    #[test]
    fn unset_is_not_the_same_as_either_value() {
        // The parameter is nullable upstream, and the third state is what
        // makes the default possible: unset means decide from obscureText.
        // A field that wants smart dashes in a password box can still ask.
        let asked = EditableText::new()
            .with_obscure_text(true)
            .with_smart_dashes(SmartDashesType::Enabled);
        assert_eq!(asked.smart_dashes(), SmartDashesType::Enabled);

        let refused = EditableText::new().with_smart_dashes(SmartDashesType::Disabled);
        assert!(!refused.obscure_text);
        assert_eq!(refused.smart_dashes(), SmartDashesType::Disabled);
    }

    #[test]
    fn and_the_two_are_separate_parameters() {
        // Written separately upstream and separately here: a field may take
        // one and refuse the other, which a single "smart substitution" flag
        // could not express.
        let mixed = EditableText::new()
            .with_smart_dashes(SmartDashesType::Disabled)
            .with_smart_quotes(SmartQuotesType::Enabled);
        assert_eq!(mixed.smart_dashes(), SmartDashesType::Disabled);
        assert_eq!(mixed.smart_quotes(), SmartQuotesType::Enabled);
    }

    #[test]
    fn the_resolution_reads_both_of_its_arguments() {
        // Every combination, so neither argument can be quietly ignored.
        for obscure in [false, true] {
            assert_eq!(
                SmartDashesType::resolve(None, obscure),
                if obscure {
                    SmartDashesType::Disabled
                } else {
                    SmartDashesType::Enabled
                }
            );
            for given in [SmartDashesType::Disabled, SmartDashesType::Enabled] {
                assert_eq!(
                    SmartDashesType::resolve(Some(given), obscure),
                    given,
                    "a value given wins over the default, {obscure}"
                );
            }
            for given in [SmartQuotesType::Disabled, SmartQuotesType::Enabled] {
                assert_eq!(SmartQuotesType::resolve(Some(given), obscure), given);
            }
        }
        // And the two defaults really differ, or the obscure argument would be
        // doing nothing.
        assert_ne!(
            SmartDashesType::resolve(None, true),
            SmartDashesType::resolve(None, false)
        );
    }
}
