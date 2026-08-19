//! How a framework object describes itself -- a port of the property half of
//! upstream's `foundation/diagnostics.dart`.
//!
//! Every widget, render object and element can be asked to list what it is
//! made of, and the list is what a debugger, a stack dump and the inspector
//! all print. The properties here are the vocabulary of that list.
//!
//! Two ideas run through the whole file:
//!
//! * **a property that is at its default is not interesting.** It does not
//!   vanish -- it drops to [`DiagnosticLevel::Fine`], which the ordinary
//!   printer hides and a caller asking for everything still sees. A dump that
//!   silently omitted things could not be trusted; one that shows a hundred
//!   defaults cannot be read. The level is how both are true at once.
//! * **a flag with nothing to say about its state shows its name instead.**
//!   `FlagProperty` and `ObjectFlagProperty` both do this, and both drop to
//!   hidden at the same time, so the fallback exists for the caller who asked
//!   to see hidden properties rather than for the ordinary reader.
//!
//! ## What is not here
//!
//! `TextTreeConfiguration` and `TextTreeRenderer` -- the box-drawing tables
//! and the renderer that walks them -- and the `Diagnosticable` mixins that
//! hang the properties off real objects. Those are the next step; this is the
//! vocabulary they print.

/// Upstream `DiagnosticLevel`: how much a reader wants to know.
///
/// The order is the whole meaning: a printer is given a minimum and shows
/// everything at or above it. `Off` is above `Error` so that nothing at all
/// clears it, and `Hidden` is below `Fine` so that a property can be present
/// and yet never printed by accident.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum DiagnosticLevel {
    /// Hidden unless asked for by name.
    Hidden,
    /// Present, uninteresting -- typically a value at its default.
    Fine,
    Debug,
    #[default]
    Info,
    Warning,
    Hint,
    /// Part of the one-line summary of an object.
    Summary,
    Error,
    /// Not shown at all, whatever the minimum.
    Off,
}

/// Upstream `DiagnosticsTreeStyle`: how a node and its children are laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DiagnosticsTreeStyle {
    /// Upstream's `none`: no style at all -- the node's children are rendered
    /// as though they were the parent's.
    None,
    /// The ordinary tree, with lines connecting parents to children.
    #[default]
    Sparse,
    /// For a child that is not being displayed, drawn with a dashed line.
    Offstage,
    /// Like sparse without the blank lines between children.
    Dense,
    /// For a child that stands between two others -- an animation's target,
    /// say -- drawn so it reads as belonging to neither.
    Transition,
    Error,
    /// Indentation only, no lines.
    Whitespace,
    /// No indentation either.
    Flat,
    /// Everything on one line.
    SingleLine,
    /// A property that is itself an error, on one line.
    ErrorProperty,
    /// The node and its immediate children only.
    Shallow,
    /// The node, with its children replaced by a count.
    TruncateChildren,
}

/// Upstream's `kNoDefaultValue` sentinel.
///
/// A property has to distinguish "the default is null" from "there is no
/// default", and `null` cannot say both. Upstream uses a private object; here
/// the two are different variants.
#[derive(Clone, Debug, PartialEq)]
pub enum DefaultValue {
    /// Upstream's `kNoDefaultValue`: nothing to compare against, so the
    /// property is always interesting.
    None,
    Some(String),
}

/// The value a property carries, as far as the printer needs to know.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum PropertyValue {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    Double(f64),
    Text(String),
    /// A list, kept as its rendered items.
    Items(Vec<String>),
}

impl PropertyValue {
    pub fn is_null(&self) -> bool {
        matches!(self, PropertyValue::Null)
    }

    /// Upstream's `debugFormatDouble`: **one decimal place, always**.
    ///
    /// A layout dump full of `23.999999999999996` is unreadable, and the
    /// difference that number hides from a reader is smaller than a pixel.
    ///
    /// **Rounded half away from zero, not half to even.** Upstream is
    /// `toStringAsFixed(1)`, which rounds `0.25` up to `0.3`; Rust's
    /// `{:.1}` rounds it down to `0.2`. Writing the obvious `format!` would
    /// have made a whole class of dumps disagree with upstream's by one in the
    /// last place, in a way nobody would think to look for. `f64::round` is
    /// half-away-from-zero, so the scaling below is what makes the two match.
    pub fn format_double(value: Option<f64>) -> String {
        match value {
            Some(value) => format!("{:.1}", (value * 10.0).round() / 10.0),
            None => "null".to_string(),
        }
    }

    /// How this value reads with no property-specific rule applied.
    pub fn to_description(&self) -> String {
        match self {
            PropertyValue::Null => "null".to_string(),
            PropertyValue::Bool(value) => value.to_string(),
            PropertyValue::Int(value) => value.to_string(),
            PropertyValue::Double(value) => Self::format_double(Some(*value)),
            PropertyValue::Text(value) => value.clone(),
            PropertyValue::Items(items) => format!("[{}]", items.join(", ")),
        }
    }
}

/// Upstream `DiagnosticsNode`: one line of an object's description.
///
/// Upstream it is abstract with several subclasses; what every one of them has
/// to answer is here.
pub trait DiagnosticsNode {
    /// Upstream's `name`.
    fn name(&self) -> Option<&str>;

    /// Upstream's `toDescription`.
    fn to_description(&self) -> String;

    /// Upstream's `level`.
    fn level(&self) -> DiagnosticLevel;

    /// Upstream's `showName`.
    fn show_name(&self) -> bool {
        true
    }

    /// Upstream's `showSeparator`, the `:` between a name and its value.
    fn show_separator(&self) -> bool {
        true
    }

    /// Upstream's `style`.
    fn style(&self) -> DiagnosticsTreeStyle {
        DiagnosticsTreeStyle::Sparse
    }

    /// Upstream's `toString`, which is the name, the separator and the
    /// description -- and only the description when the name is not shown.
    fn to_line(&self) -> String {
        match (self.show_name(), self.name()) {
            (true, Some(name)) if self.show_separator() => {
                format!("{name}: {}", self.to_description())
            }
            (true, Some(name)) => format!("{name}{}", self.to_description()),
            _ => self.to_description(),
        }
    }
}

/// Upstream `DiagnosticsProperty`: a named value on an object.
#[derive(Clone, Debug, PartialEq)]
pub struct DiagnosticsProperty {
    pub name: Option<String>,
    pub value: PropertyValue,
    /// Upstream's `description`, which overrides the value's own rendering.
    pub description: Option<String>,
    /// Upstream's `ifNull`: what to print instead of `null`.
    pub if_null: Option<String>,
    /// Upstream's `ifEmpty`.
    pub if_empty: Option<String>,
    pub tooltip: Option<String>,
    pub default_value: DefaultValue,
    /// Upstream's `missingIfNull`: whether a null value is a *problem* rather
    /// than merely a value.
    pub missing_if_null: bool,
    pub show_name: bool,
    pub show_separator: bool,
    pub style: DiagnosticsTreeStyle,
    /// Upstream's `_defaultLevel`, before the rules below adjust it.
    pub default_level: DiagnosticLevel,
    /// Upstream's `exception`: set when computing the value threw.
    pub exception: Option<String>,
}

impl Default for DiagnosticsProperty {
    fn default() -> DiagnosticsProperty {
        DiagnosticsProperty::new::<&str>(None, PropertyValue::Null)
    }
}

impl DiagnosticsProperty {
    pub fn new<S: Into<String>>(name: Option<S>, value: PropertyValue) -> DiagnosticsProperty {
        DiagnosticsProperty {
            name: name.map(Into::into),
            value,
            description: None,
            if_null: None,
            if_empty: None,
            tooltip: None,
            default_value: DefaultValue::None,
            missing_if_null: false,
            show_name: true,
            show_separator: true,
            style: DiagnosticsTreeStyle::SingleLine,
            default_level: DiagnosticLevel::Info,
            exception: None,
        }
    }

    pub fn with_default_value(mut self, default_value: impl Into<String>) -> Self {
        self.default_value = DefaultValue::Some(default_value.into());
        self
    }

    pub fn with_if_null(mut self, if_null: impl Into<String>) -> Self {
        self.if_null = Some(if_null.into());
        self
    }

    pub fn with_if_empty(mut self, if_empty: impl Into<String>) -> Self {
        self.if_empty = Some(if_empty.into());
        self
    }

    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn with_missing_if_null(mut self, missing: bool) -> Self {
        self.missing_if_null = missing;
        self
    }

    pub fn with_show_name(mut self, show: bool) -> Self {
        self.show_name = show;
        self
    }

    pub fn with_level(mut self, level: DiagnosticLevel) -> Self {
        self.default_level = level;
        self
    }

    pub fn with_exception(mut self, exception: impl Into<String>) -> Self {
        self.exception = Some(exception.into());
        self
    }

    /// Upstream's `isInteresting`: **anything without a default is always
    /// interesting**, and anything at its default is not.
    pub fn is_interesting(&self) -> bool {
        match &self.default_value {
            DefaultValue::None => true,
            DefaultValue::Some(default) => &self.value.to_description() != default,
        }
    }

    /// Upstream's `level`, in upstream's order, which is the order of how much
    /// each condition matters.
    ///
    /// A `Hidden` default wins outright -- a caller who said "never show this"
    /// meant it, even for an error. Then an exception, because a property that
    /// could not be computed is the most important thing on the line. Then a
    /// null that was declared missing. Only then does being at the default
    /// demote it.
    pub fn level(&self) -> DiagnosticLevel {
        if self.default_level == DiagnosticLevel::Hidden {
            return self.default_level;
        }
        if self.exception.is_some() {
            return DiagnosticLevel::Error;
        }
        if self.value.is_null() && self.missing_if_null {
            return DiagnosticLevel::Warning;
        }
        if !self.is_interesting() {
            return DiagnosticLevel::Fine;
        }
        self.default_level
    }

    /// Upstream's `valueToString`, plus the `ifNull` and description
    /// overrides that `toDescription` applies around it.
    pub fn to_description(&self) -> String {
        if let Some(exception) = &self.exception {
            return exception.clone();
        }
        if let Some(description) = &self.description {
            return description.clone();
        }
        if self.value.is_null() {
            if let Some(if_null) = &self.if_null {
                return if_null.clone();
            }
        }
        self.value.to_description()
    }
}

impl DiagnosticsNode for DiagnosticsProperty {
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    fn to_description(&self) -> String {
        DiagnosticsProperty::to_description(self)
    }

    fn level(&self) -> DiagnosticLevel {
        DiagnosticsProperty::level(self)
    }

    fn show_name(&self) -> bool {
        self.show_name
    }

    fn show_separator(&self) -> bool {
        self.show_separator
    }

    fn style(&self) -> DiagnosticsTreeStyle {
        self.style
    }
}

/// Upstream `MessageProperty`: a whole message on one line, with no value.
///
/// It exists so that a message can sit in a property list at all -- the name
/// and the text are both given, and there is nothing to compare against a
/// default.
#[derive(Clone, Debug, PartialEq)]
pub struct MessageProperty(pub DiagnosticsProperty);

impl MessageProperty {
    pub fn new(name: impl Into<String>, message: impl Into<String>) -> MessageProperty {
        let mut property = DiagnosticsProperty::new(Some(name), PropertyValue::Null);
        property.description = Some(message.into());
        MessageProperty(property)
    }
}

/// Upstream `StringProperty`.
#[derive(Clone, Debug, PartialEq)]
pub struct StringProperty {
    pub base: DiagnosticsProperty,
    /// Upstream's `quoted`, **true by default**.
    pub quoted: bool,
}

impl StringProperty {
    pub fn new(name: impl Into<String>, value: Option<String>) -> StringProperty {
        StringProperty {
            base: DiagnosticsProperty::new(
                Some(name),
                match value {
                    Some(value) => PropertyValue::Text(value),
                    None => PropertyValue::Null,
                },
            ),
            quoted: true,
        }
    }

    pub fn with_quoted(mut self, quoted: bool) -> Self {
        self.quoted = quoted;
        self
    }

    pub fn with_if_empty(mut self, if_empty: impl Into<String>) -> Self {
        self.base = self.base.with_if_empty(if_empty);
        self
    }

    /// Upstream's `valueToString`.
    ///
    /// **An empty string does not look empty once it is in quotes**, which is
    /// upstream's own comment and the reason `ifEmpty` is checked *inside* the
    /// quoting branch rather than before it. `""` reads as a value; `<none>`
    /// reads as an absence.
    ///
    /// `line_break_properties` is upstream's `parentConfiguration`: when the
    /// parent is putting everything on one line, newlines are escaped rather
    /// than printed, or the one line becomes several.
    pub fn value_to_string(&self, line_break_properties: bool) -> String {
        let text = match (&self.base.description, &self.base.value) {
            (Some(description), _) => Some(description.clone()),
            (None, PropertyValue::Text(value)) => Some(value.clone()),
            _ => None,
        };
        let text = text.map(|text| {
            if line_break_properties {
                text
            } else {
                text.replace('\n', "\\n")
            }
        });
        match (self.quoted, text) {
            (true, Some(text)) => {
                if text.is_empty() {
                    if let Some(if_empty) = &self.base.if_empty {
                        return if_empty.clone();
                    }
                }
                format!("\"{text}\"")
            }
            (_, Some(text)) => text,
            (_, None) => "null".to_string(),
        }
    }
}

/// Upstream `DoubleProperty`.
#[derive(Clone, Debug, PartialEq)]
pub struct DoubleProperty {
    pub base: DiagnosticsProperty,
    /// Upstream's `unit`, appended with no space.
    pub unit: Option<String>,
}

impl DoubleProperty {
    pub fn new(name: impl Into<String>, value: Option<f64>) -> DoubleProperty {
        DoubleProperty {
            base: DiagnosticsProperty::new(
                Some(name),
                match value {
                    Some(value) => PropertyValue::Double(value),
                    None => PropertyValue::Null,
                },
            ),
            unit: None,
        }
    }

    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    pub fn number_to_string(&self) -> String {
        match self.base.value {
            PropertyValue::Double(value) => PropertyValue::format_double(Some(value)),
            _ => "null".to_string(),
        }
    }

    /// Upstream's `_NumProperty.valueToString`: the unit is appended with **no
    /// separator**, so `16.0` with a unit of `px` is `16.0px`.
    pub fn value_to_string(&self) -> String {
        if self.base.value.is_null() {
            return "null".to_string();
        }
        match &self.unit {
            Some(unit) => format!("{}{unit}", self.number_to_string()),
            None => self.number_to_string(),
        }
    }
}

/// Upstream `IntProperty`.
#[derive(Clone, Debug, PartialEq)]
pub struct IntProperty {
    pub base: DiagnosticsProperty,
    pub unit: Option<String>,
}

impl IntProperty {
    pub fn new(name: impl Into<String>, value: Option<i64>) -> IntProperty {
        IntProperty {
            base: DiagnosticsProperty::new(
                Some(name),
                match value {
                    Some(value) => PropertyValue::Int(value),
                    None => PropertyValue::Null,
                },
            ),
            unit: None,
        }
    }

    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    pub fn value_to_string(&self) -> String {
        match (&self.base.value, &self.unit) {
            (PropertyValue::Null, _) => "null".to_string(),
            (value, Some(unit)) => format!("{}{unit}", value.to_description()),
            (value, None) => value.to_description(),
        }
    }
}

/// Upstream `PercentProperty`: a fraction shown as a percentage.
#[derive(Clone, Debug, PartialEq)]
pub struct PercentProperty {
    pub base: DiagnosticsProperty,
    pub unit: Option<String>,
}

impl PercentProperty {
    pub fn new(name: impl Into<String>, fraction: Option<f64>) -> PercentProperty {
        PercentProperty {
            base: DiagnosticsProperty::new(
                Some(name),
                match fraction {
                    Some(value) => PropertyValue::Double(value),
                    None => PropertyValue::Null,
                },
            ),
            unit: None,
        }
    }

    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// Upstream's `numberToString`, which **clamps to 0..1 before scaling**.
    ///
    /// An animation slightly overshooting its bounds should read as 100%
    /// rather than 103%: the reader is being told how far along something is,
    /// and there is no such thing as further along than finished.
    pub fn number_to_string(&self) -> String {
        match self.base.value {
            PropertyValue::Double(value) => format!(
                "{}%",
                PropertyValue::format_double(Some(value.clamp(0.0, 1.0) * 100.0))
            ),
            _ => "null".to_string(),
        }
    }

    /// Upstream's `valueToString`, whose unit is separated **by a space**
    /// where `DoubleProperty`'s is not. A percentage followed directly by a
    /// unit would read as one token.
    pub fn value_to_string(&self) -> String {
        if self.base.value.is_null() {
            return "null".to_string();
        }
        match &self.unit {
            Some(unit) => format!("{} {unit}", self.number_to_string()),
            None => self.number_to_string(),
        }
    }
}

/// Upstream `FlagProperty`: a boolean described by what it means rather than
/// by `true` and `false`.
#[derive(Clone, Debug, PartialEq)]
pub struct FlagProperty {
    pub base: DiagnosticsProperty,
    pub if_true: Option<String>,
    pub if_false: Option<String>,
}

impl FlagProperty {
    /// Upstream asserts that at least one of the two is given -- a flag with
    /// neither has nothing to contribute.
    pub fn new(name: impl Into<String>, value: Option<bool>) -> FlagProperty {
        let mut base = DiagnosticsProperty::new(
            Some(name),
            match value {
                Some(value) => PropertyValue::Bool(value),
                None => PropertyValue::Null,
            },
        );
        base.show_name = false;
        FlagProperty {
            base,
            if_true: None,
            if_false: None,
        }
    }

    pub fn with_if_true(mut self, if_true: impl Into<String>) -> Self {
        self.if_true = Some(if_true.into());
        self
    }

    pub fn with_if_false(mut self, if_false: impl Into<String>) -> Self {
        self.if_false = Some(if_false.into());
        self
    }

    pub fn is_valid(&self) -> bool {
        self.if_true.is_some() || self.if_false.is_some()
    }

    /// Whether this flag's own state has a description.
    fn has_description(&self) -> bool {
        match self.base.value {
            PropertyValue::Bool(true) => self.if_true.is_some(),
            PropertyValue::Bool(false) => self.if_false.is_some(),
            _ => false,
        }
    }

    pub fn value_to_string(&self) -> String {
        match (&self.base.value, &self.if_true, &self.if_false) {
            (PropertyValue::Bool(true), Some(if_true), _) => if_true.clone(),
            (PropertyValue::Bool(false), _, Some(if_false)) => if_false.clone(),
            (value, _, _) => value.to_description(),
        }
    }

    /// Upstream's `showName` override.
    ///
    /// **With nothing to say about this state, the name is shown instead** --
    /// otherwise the line would be a bare `true` with no clue what it was
    /// about. It pairs with the level below: the property is hidden in the
    /// same case, so the name is a fallback for the caller who asked for
    /// hidden properties, not something an ordinary dump shows.
    pub fn show_name(&self) -> bool {
        if !self.has_description() {
            return true;
        }
        self.base.show_name
    }

    /// Upstream's `level` override.
    pub fn level(&self) -> DiagnosticLevel {
        if !self.has_description() {
            return DiagnosticLevel::Hidden;
        }
        self.base.level()
    }
}

/// Upstream `ObjectFlagProperty`: present or absent, described in words.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectFlagProperty {
    pub base: DiagnosticsProperty,
    pub if_present: Option<String>,
}

impl ObjectFlagProperty {
    pub fn new(name: impl Into<String>, present: bool) -> ObjectFlagProperty {
        let mut base = DiagnosticsProperty::new(
            Some(name),
            if present {
                PropertyValue::Bool(true)
            } else {
                PropertyValue::Null
            },
        );
        base.show_name = false;
        ObjectFlagProperty {
            base,
            if_present: None,
        }
    }

    /// Upstream's `ObjectFlagProperty.has`, whose description is the name with
    /// `has ` in front -- so a callback field called `onTap` prints as
    /// `has onTap`, which is what a reader wants to know about a callback.
    pub fn has(name: impl Into<String>, present: bool) -> ObjectFlagProperty {
        let name = name.into();
        let if_present = format!("has {name}");
        ObjectFlagProperty::new(name, present).with_if_present(if_present)
    }

    pub fn with_if_present(mut self, if_present: impl Into<String>) -> Self {
        self.if_present = Some(if_present.into());
        self
    }

    pub fn with_if_null(mut self, if_null: impl Into<String>) -> Self {
        self.base = self.base.with_if_null(if_null);
        self
    }

    fn has_description(&self) -> bool {
        if self.base.value.is_null() {
            self.base.if_null.is_some()
        } else {
            self.if_present.is_some()
        }
    }

    pub fn value_to_string(&self) -> String {
        if !self.base.value.is_null() {
            if let Some(if_present) = &self.if_present {
                return if_present.clone();
            }
        } else if let Some(if_null) = &self.base.if_null {
            return if_null.clone();
        }
        self.base.value.to_description()
    }

    /// The same pair of overrides as [`FlagProperty`], for the same reason.
    pub fn show_name(&self) -> bool {
        if !self.has_description() {
            return true;
        }
        self.base.show_name
    }

    pub fn level(&self) -> DiagnosticLevel {
        if !self.has_description() {
            return DiagnosticLevel::Hidden;
        }
        self.base.level()
    }
}

/// Upstream `EnumProperty`: an enum shown by its name.
#[derive(Clone, Debug, PartialEq)]
pub struct EnumProperty {
    pub base: DiagnosticsProperty,
}

impl EnumProperty {
    pub fn new(name: impl Into<String>, value: Option<String>) -> EnumProperty {
        EnumProperty {
            base: DiagnosticsProperty::new(
                Some(name),
                match value {
                    Some(value) => PropertyValue::Text(value),
                    None => PropertyValue::Null,
                },
            ),
        }
    }
}

/// Upstream `IterableProperty`: a list of values.
#[derive(Clone, Debug, PartialEq)]
pub struct IterableProperty {
    pub base: DiagnosticsProperty,
}

impl IterableProperty {
    pub fn new(name: impl Into<String>, items: Option<Vec<String>>) -> IterableProperty {
        IterableProperty {
            base: DiagnosticsProperty::new(
                Some(name),
                match items {
                    Some(items) => PropertyValue::Items(items),
                    None => PropertyValue::Null,
                },
            ),
        }
    }

    pub fn with_if_empty(mut self, if_empty: impl Into<String>) -> Self {
        self.base = self.base.with_if_empty(if_empty);
        self
    }

    /// Upstream's `valueToString`, and the `ifEmpty` case.
    ///
    /// An empty list is **not** the same as a missing one, so the two get
    /// different text: `[]` for empty and whatever `ifNull` says for absent.
    pub fn value_to_string(&self) -> String {
        match &self.base.value {
            PropertyValue::Items(items) if items.is_empty() => match &self.base.if_empty {
                Some(if_empty) => if_empty.clone(),
                None => "[]".to_string(),
            },
            PropertyValue::Items(items) => items.join(", "),
            PropertyValue::Null => match &self.base.if_null {
                Some(if_null) => if_null.clone(),
                None => "null".to_string(),
            },
            value => value.to_description(),
        }
    }

    /// Upstream's `level`: an empty list is uninteresting **unless** the
    /// caller gave it an `ifEmpty` -- which is a caller saying that the
    /// emptiness is itself worth reporting.
    pub fn level(&self) -> DiagnosticLevel {
        if let PropertyValue::Items(items) = &self.base.value {
            if items.is_empty() && self.base.if_empty.is_none() {
                return DiagnosticLevel::Fine;
            }
        }
        self.base.level()
    }
}

/// Upstream `FlagsSummary`: several flags on one line.
///
/// It exists because a widget with a dozen boolean states would otherwise
/// contribute a dozen lines, most of them off. Only the flags that are set are
/// named.
#[derive(Clone, Debug, PartialEq)]
pub struct FlagsSummary {
    pub base: DiagnosticsProperty,
    pub flags: Vec<(String, bool)>,
}

impl FlagsSummary {
    pub fn new(name: impl Into<String>, flags: Vec<(String, bool)>) -> FlagsSummary {
        FlagsSummary {
            base: DiagnosticsProperty::new(Some(name), PropertyValue::Null),
            flags,
        }
    }

    pub fn with_if_empty(mut self, if_empty: impl Into<String>) -> Self {
        self.base = self.base.with_if_empty(if_empty);
        self
    }

    fn set_flags(&self) -> Vec<&str> {
        self.flags
            .iter()
            .filter(|(_, set)| *set)
            .map(|(name, _)| name.as_str())
            .collect()
    }

    pub fn value_to_string(&self) -> String {
        let set = self.set_flags();
        if set.is_empty() {
            return self
                .base
                .if_empty
                .clone()
                .unwrap_or_else(|| "[]".to_string());
        }
        set.join(", ")
    }

    /// Upstream's `level`: uninteresting when no flag is set.
    pub fn level(&self) -> DiagnosticLevel {
        if self.set_flags().is_empty() {
            return DiagnosticLevel::Fine;
        }
        self.base.level()
    }
}

/// Upstream `DiagnosticPropertiesBuilder`: what an object fills in when asked
/// to describe itself.
#[derive(Debug, Default)]
pub struct DiagnosticPropertiesBuilder {
    properties: Vec<DiagnosticsProperty>,
    /// Upstream's `defaultDiagnosticsTreeStyle`.
    pub default_diagnostics_tree_style: DiagnosticsTreeStyle,
    /// Upstream's `emptyBodyDescription`, shown when an object contributed
    /// nothing.
    pub empty_body_description: Option<String>,
}

impl DiagnosticPropertiesBuilder {
    pub fn new() -> DiagnosticPropertiesBuilder {
        DiagnosticPropertiesBuilder {
            properties: Vec::new(),
            default_diagnostics_tree_style: DiagnosticsTreeStyle::Sparse,
            empty_body_description: None,
        }
    }

    /// Upstream's `add`.
    pub fn add(&mut self, property: DiagnosticsProperty) {
        self.properties.push(property);
    }

    pub fn properties(&self) -> &[DiagnosticsProperty] {
        &self.properties
    }

    /// The properties a printer would show at a given minimum level.
    ///
    /// This is the payoff of the whole level scheme: one list serves both the
    /// ordinary dump and the caller who wants everything.
    pub fn visible(&self, minimum: DiagnosticLevel) -> Vec<&DiagnosticsProperty> {
        self.properties
            .iter()
            .filter(|property| {
                property.level() >= minimum && property.level() != DiagnosticLevel::Off
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_property_at_its_default_is_demoted_rather_than_dropped() {
        // The whole scheme in one line: a dump that silently omitted things
        // could not be trusted, and one showing a hundred defaults cannot be
        // read. The level makes both true at once.
        let boring = DiagnosticsProperty::new(Some("padding"), PropertyValue::Double(0.0))
            .with_default_value("0.0");
        assert!(!boring.is_interesting());
        assert_eq!(boring.level(), DiagnosticLevel::Fine);

        let changed = DiagnosticsProperty::new(Some("padding"), PropertyValue::Double(8.0))
            .with_default_value("0.0");
        assert!(changed.is_interesting());
        assert_eq!(changed.level(), DiagnosticLevel::Info);
    }

    #[test]
    fn a_property_with_no_default_is_always_interesting() {
        // "The default is null" and "there is no default" are different
        // things, and null cannot say both.
        let no_default = DiagnosticsProperty::new(Some("child"), PropertyValue::Null);
        assert_eq!(no_default.default_value, DefaultValue::None);
        assert!(no_default.is_interesting());
        assert_eq!(no_default.level(), DiagnosticLevel::Info);

        let default_is_null =
            DiagnosticsProperty::new(Some("child"), PropertyValue::Null).with_default_value("null");
        assert!(!default_is_null.is_interesting());
        assert_eq!(default_is_null.level(), DiagnosticLevel::Fine);
    }

    #[test]
    fn the_level_rules_are_tried_in_upstreams_order() {
        // A hidden default wins outright -- a caller who said "never show
        // this" meant it, even for an error.
        let hidden = DiagnosticsProperty::new(Some("x"), PropertyValue::Null)
            .with_level(DiagnosticLevel::Hidden)
            .with_exception("boom")
            .with_missing_if_null(true);
        assert_eq!(hidden.level(), DiagnosticLevel::Hidden);

        // Then the exception, because a property that could not be computed is
        // the most important thing on the line.
        let threw = DiagnosticsProperty::new(Some("x"), PropertyValue::Null)
            .with_exception("boom")
            .with_missing_if_null(true)
            .with_default_value("null");
        assert_eq!(threw.level(), DiagnosticLevel::Error);
        assert_eq!(threw.to_description(), "boom");

        // Then a null that was declared missing, even though it is also at its
        // default.
        let missing = DiagnosticsProperty::new(Some("x"), PropertyValue::Null)
            .with_missing_if_null(true)
            .with_default_value("null");
        assert_eq!(missing.level(), DiagnosticLevel::Warning);
    }

    #[test]
    fn the_level_order_is_what_a_printer_filters_by() {
        // Off is above Error so nothing clears it; Hidden is below Fine so a
        // property can be present and never printed by accident.
        assert!(DiagnosticLevel::Hidden < DiagnosticLevel::Fine);
        assert!(DiagnosticLevel::Fine < DiagnosticLevel::Info);
        assert!(DiagnosticLevel::Info < DiagnosticLevel::Warning);
        assert!(DiagnosticLevel::Summary < DiagnosticLevel::Error);
        assert!(DiagnosticLevel::Error < DiagnosticLevel::Off);
        assert_eq!(DiagnosticLevel::default(), DiagnosticLevel::Info);
    }

    #[test]
    fn one_list_serves_both_the_ordinary_dump_and_the_caller_who_wants_everything() {
        let mut builder = DiagnosticPropertiesBuilder::new();
        builder.add(
            DiagnosticsProperty::new(Some("width"), PropertyValue::Double(100.0))
                .with_default_value("0.0"),
        );
        builder.add(
            DiagnosticsProperty::new(Some("height"), PropertyValue::Double(0.0))
                .with_default_value("0.0"),
        );
        builder.add(
            DiagnosticsProperty::new(Some("secret"), PropertyValue::Text("x".to_string()))
                .with_level(DiagnosticLevel::Hidden),
        );

        let names = |level| -> Vec<String> {
            builder
                .visible(level)
                .iter()
                .map(|property| property.name.clone().unwrap_or_default())
                .collect()
        };
        assert_eq!(names(DiagnosticLevel::Info), vec!["width"]);
        assert_eq!(names(DiagnosticLevel::Fine), vec!["width", "height"]);
        assert_eq!(
            names(DiagnosticLevel::Hidden),
            vec!["width", "height", "secret"]
        );
        assert_eq!(builder.properties().len(), 3, "all three are still there");
    }

    #[test]
    fn a_double_is_shown_to_one_decimal_place_always() {
        // A layout dump full of 23.999999999999996 is unreadable, and what
        // that number hides from a reader is smaller than a pixel.
        assert_eq!(
            PropertyValue::format_double(Some(23.999_999_999_999_996)),
            "24.0"
        );
        assert_eq!(PropertyValue::format_double(Some(8.0)), "8.0");
        // Half away from zero, as Dart's toStringAsFixed is -- not Rust's
        // `{:.1}`, which is half to even and would say "0.2" here. Writing the
        // obvious format! would have made a whole class of dumps disagree with
        // upstream by one in the last place.
        assert_eq!(PropertyValue::format_double(Some(0.25)), "0.3");
        assert_eq!(PropertyValue::format_double(Some(-0.25)), "-0.3");
        assert_eq!(PropertyValue::format_double(Some(0.35)), "0.4");
        assert_eq!(PropertyValue::format_double(None), "null");
    }

    #[test]
    fn a_doubles_unit_is_appended_with_no_space_and_a_percentages_with_one() {
        // A percentage followed directly by a unit would read as one token.
        let pixels = DoubleProperty::new("width", Some(16.0)).with_unit("px");
        assert_eq!(pixels.value_to_string(), "16.0px");
        assert_eq!(
            DoubleProperty::new("width", Some(16.0)).value_to_string(),
            "16.0"
        );
        assert_eq!(DoubleProperty::new("width", None).value_to_string(), "null");

        let percent = PercentProperty::new("progress", Some(0.5)).with_unit("done");
        assert_eq!(percent.value_to_string(), "50.0% done");
    }

    #[test]
    fn a_percentage_is_clamped_before_it_is_scaled() {
        // An animation slightly overshooting should read as 100%: the reader
        // is being told how far along something is, and there is no such thing
        // as further along than finished.
        assert_eq!(
            PercentProperty::new("t", Some(1.03)).number_to_string(),
            "100.0%"
        );
        assert_eq!(
            PercentProperty::new("t", Some(-0.2)).number_to_string(),
            "0.0%"
        );
        assert_eq!(
            PercentProperty::new("t", Some(0.333)).number_to_string(),
            "33.3%"
        );
        assert_eq!(PercentProperty::new("t", None).value_to_string(), "null");
    }

    #[test]
    fn an_int_carries_its_unit_too() {
        assert_eq!(IntProperty::new("count", Some(3)).value_to_string(), "3");
        assert_eq!(
            IntProperty::new("depth", Some(3))
                .with_unit(" levels")
                .value_to_string(),
            "3 levels"
        );
        assert_eq!(IntProperty::new("count", None).value_to_string(), "null");
    }

    #[test]
    fn an_empty_string_does_not_look_empty_once_it_is_in_quotes() {
        // Upstream's own comment, and the reason ifEmpty is checked inside the
        // quoting branch rather than before it. "" reads as a value; <none>
        // reads as an absence.
        let quoted = StringProperty::new("label", Some(String::new()));
        assert_eq!(quoted.value_to_string(true), "\"\"");

        let with_if_empty =
            StringProperty::new("label", Some(String::new())).with_if_empty("<none>");
        assert_eq!(with_if_empty.value_to_string(true), "<none>");

        // Unquoted, ifEmpty does not apply -- there is nothing to disguise.
        let unquoted = StringProperty::new("label", Some(String::new()))
            .with_quoted(false)
            .with_if_empty("<none>");
        assert_eq!(unquoted.value_to_string(true), "");
    }

    #[test]
    fn a_string_is_quoted_by_default() {
        let property = StringProperty::new("label", Some("hello".to_string()));
        assert!(property.quoted);
        assert_eq!(property.value_to_string(true), "\"hello\"");
        assert_eq!(
            StringProperty::new("label", Some("hello".to_string()))
                .with_quoted(false)
                .value_to_string(true),
            "hello"
        );
        assert_eq!(
            StringProperty::new("label", None).value_to_string(true),
            "null"
        );
    }

    #[test]
    fn newlines_are_escaped_when_the_parent_wants_one_line() {
        // Or the one line becomes several, and the tree drawing falls apart.
        let property = StringProperty::new("text", Some("two\nlines".to_string()));
        assert_eq!(property.value_to_string(true), "\"two\nlines\"");
        assert_eq!(
            property.value_to_string(false),
            "\"two\\nlines\"",
            "escaped for a single-line parent"
        );
    }

    #[test]
    fn a_flag_with_nothing_to_say_about_its_state_shows_its_name_and_hides() {
        // Otherwise the line would be a bare `true` with no clue what it was
        // about. The name is a fallback for the caller who asked for hidden
        // properties, not something an ordinary dump shows.
        let described = FlagProperty::new("dirty", Some(true)).with_if_true("dirty");
        assert_eq!(described.value_to_string(), "dirty");
        assert!(!described.show_name(), "the description says it all");
        assert_eq!(described.level(), DiagnosticLevel::Info);

        let undescribed = FlagProperty::new("dirty", Some(false)).with_if_true("dirty");
        assert_eq!(undescribed.value_to_string(), "false");
        assert!(undescribed.show_name(), "so the reader knows what is false");
        assert_eq!(
            undescribed.level(),
            DiagnosticLevel::Hidden,
            "and it is hidden in the same case"
        );
    }

    #[test]
    fn a_null_flag_is_hidden_whichever_descriptions_were_given() {
        let property = FlagProperty::new("dirty", None)
            .with_if_true("dirty")
            .with_if_false("clean");
        assert!(property.show_name());
        assert_eq!(property.level(), DiagnosticLevel::Hidden);
        assert!(property.is_valid());
    }

    #[test]
    fn a_flag_needs_at_least_one_description_to_contribute_anything() {
        assert!(!FlagProperty::new("dirty", Some(true)).is_valid());
        assert!(
            FlagProperty::new("dirty", Some(true))
                .with_if_false("clean")
                .is_valid()
        );
    }

    #[test]
    fn a_callback_reads_as_has_something_rather_than_true() {
        // Which is what a reader wants to know about a callback field.
        let present = ObjectFlagProperty::has("onTap", true);
        assert_eq!(present.value_to_string(), "has onTap");
        assert!(!present.show_name());
        assert_eq!(present.level(), DiagnosticLevel::Info);

        let absent = ObjectFlagProperty::has("onTap", false);
        assert_eq!(
            absent.level(),
            DiagnosticLevel::Hidden,
            "an absent callback is not worth a line"
        );
        assert!(absent.show_name());
    }

    #[test]
    fn an_object_flag_can_describe_its_absence_instead() {
        let property = ObjectFlagProperty::new("child", false).with_if_null("no child");
        assert_eq!(property.value_to_string(), "no child");
        assert_eq!(property.level(), DiagnosticLevel::Info);
        assert!(!property.show_name());
    }

    #[test]
    fn an_empty_list_is_not_a_missing_one() {
        // They get different text, because they are different facts.
        let empty = IterableProperty::new("children", Some(Vec::new()));
        assert_eq!(empty.value_to_string(), "[]");
        assert_eq!(
            empty.level(),
            DiagnosticLevel::Fine,
            "an empty list is uninteresting on its own"
        );

        let missing = IterableProperty::new("children", None);
        assert_eq!(missing.value_to_string(), "null");

        // Unless the caller says the emptiness is worth reporting.
        let notable =
            IterableProperty::new("children", Some(Vec::new())).with_if_empty("<no children>");
        assert_eq!(notable.value_to_string(), "<no children>");
        assert_eq!(notable.level(), DiagnosticLevel::Info);
    }

    #[test]
    fn a_list_with_something_in_it_is_joined() {
        let property = IterableProperty::new(
            "children",
            Some(vec!["Text".to_string(), "Icon".to_string()]),
        );
        assert_eq!(property.value_to_string(), "Text, Icon");
        assert_eq!(property.level(), DiagnosticLevel::Info);
    }

    #[test]
    fn a_flags_summary_names_only_the_flags_that_are_set() {
        // A widget with a dozen boolean states would otherwise contribute a
        // dozen lines, most of them off.
        let summary = FlagsSummary::new(
            "state",
            vec![
                ("focused".to_string(), true),
                ("hovered".to_string(), false),
                ("pressed".to_string(), true),
            ],
        );
        assert_eq!(summary.value_to_string(), "focused, pressed");
        assert_eq!(summary.level(), DiagnosticLevel::Info);

        let quiet = FlagsSummary::new("state", vec![("focused".to_string(), false)]);
        assert_eq!(quiet.value_to_string(), "[]");
        assert_eq!(
            quiet.level(),
            DiagnosticLevel::Fine,
            "nothing set is nothing to say"
        );
        assert_eq!(
            FlagsSummary::new("state", vec![("focused".to_string(), false)])
                .with_if_empty("<none set>")
                .value_to_string(),
            "<none set>"
        );
    }

    #[test]
    fn a_line_is_the_name_the_separator_and_the_description() {
        let named = DiagnosticsProperty::new(Some("width"), PropertyValue::Double(8.0));
        assert_eq!(named.to_line(), "width: 8.0");

        let anonymous = DiagnosticsProperty::new(Some("width"), PropertyValue::Double(8.0))
            .with_show_name(false);
        assert_eq!(anonymous.to_line(), "8.0");

        let unnamed = DiagnosticsProperty::new::<&str>(None, PropertyValue::Int(3));
        assert_eq!(unnamed.to_line(), "3");
    }

    #[test]
    fn if_null_stands_in_for_a_missing_value_and_a_description_beats_both() {
        let plain = DiagnosticsProperty::new(Some("child"), PropertyValue::Null);
        assert_eq!(plain.to_description(), "null");

        let with_if_null =
            DiagnosticsProperty::new(Some("child"), PropertyValue::Null).with_if_null("no child");
        assert_eq!(with_if_null.to_description(), "no child");

        let mut described = with_if_null.clone();
        described.description = Some("something else".to_string());
        assert_eq!(described.to_description(), "something else");
    }

    #[test]
    fn a_message_property_is_a_whole_message_with_no_value_to_compare() {
        let message = MessageProperty::new("note", "this widget was rebuilt 40 times");
        assert_eq!(
            message.0.to_description(),
            "this widget was rebuilt 40 times"
        );
        assert_eq!(message.0.level(), DiagnosticLevel::Info);
        assert_eq!(
            message.0.to_line(),
            "note: this widget was rebuilt 40 times"
        );
    }

    #[test]
    fn an_enum_property_shows_the_name_rather_than_the_ordinal() {
        let property = EnumProperty::new("direction", Some("Axis.vertical".to_string()));
        assert_eq!(property.base.to_description(), "Axis.vertical");
        assert_eq!(
            EnumProperty::new("direction", None).base.to_description(),
            "null"
        );
    }

    #[test]
    fn the_builder_carries_the_style_the_object_wants_its_children_drawn_in() {
        let mut builder = DiagnosticPropertiesBuilder::new();
        assert_eq!(
            builder.default_diagnostics_tree_style,
            DiagnosticsTreeStyle::Sparse
        );
        builder.default_diagnostics_tree_style = DiagnosticsTreeStyle::Dense;
        builder.empty_body_description = Some("<no properties>".to_string());
        assert!(builder.properties().is_empty());
        assert_eq!(
            builder.empty_body_description.as_deref(),
            Some("<no properties>")
        );
    }
}
