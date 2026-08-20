// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Describing one widget so a tool can show it on its own.
//!
//! Upstream's `widget_previews/widget_previews.dart`. A `@Preview(...)`
//! annotation marks a function that returns a widget, and `flutter
//! widget-preview` finds them, builds a scaffold around each, and shows them
//! side by side -- a component gallery assembled from the source rather than
//! written by hand.
//!
//! # Nothing in this crate reads these yet, and that is expected
//!
//! Upstream's reader is an analyzer pass in `flutter_tools` that generates a
//! preview application; there is no analyzer and no code generator here. What
//! is ported is the *description* -- what a preview says about itself, and the
//! handful of decisions the description makes on its own.
//!
//! Those decisions are the reason this is a port rather than six inert structs:
//! [`PreviewBuilder::add_wrapper`] composes in a particular order,
//! [`PreviewBuilder::build`] applies the `"Default"` group, and
//! [`MultiPreview::transform`] flattens. Each is behaviour a caller can get
//! wrong, and each is pinned below.
//!
//! # The annotation half has no counterpart and would not be a type
//!
//! Dart's `@Preview()` is a const instance of a class used as metadata. Rust's
//! equivalent is an attribute macro -- `#[preview(...)]` -- which is a
//! proc-macro crate rather than a struct, and is only worth writing when
//! something exists to consume it. So [`Preview`] here is the value, and the
//! way to attach one is to write a function that returns it.
//!
//! Upstream marks the whole interface unstable in its own words -- "this
//! interface is not stable and **will change**" -- which is worth carrying
//! across with it.

use std::rc::Rc;

use crate::framework::AnyWidget;
use crate::platform::{Brightness, Locale};
use crate::render::Size;

/// Wraps the previewed widget in something else -- a `Scaffold`, a fixed-width
/// box, a mock provider. Upstream's `WidgetWrapper` typedef.
pub type WidgetWrapper = Rc<dyn Fn(AnyWidget) -> AnyWidget>;

/// Builds the theming a preview is shown under. Upstream's `PreviewTheme`
/// typedef -- a callback rather than a value, because a `ThemeData` is not
/// const-constructible and an annotation's arguments must be.
pub type PreviewTheme = Rc<dyn Fn() -> Rc<dyn PreviewThemeData>>;

/// Builds the localization data a preview is shown under. Upstream's
/// `PreviewLocalizations` typedef, a callback for the same reason.
pub type PreviewLocalizations = Rc<dyn Fn() -> PreviewLocalizationsData>;

/// Upstream `Preview`: everything one preview says about itself.
///
/// Every field is optional except the group, which has a default -- a preview
/// that names nothing is still a valid preview of whatever the annotated
/// function returns.
#[derive(Clone, Default)]
pub struct Preview {
    /// Which heading this appears under in the previewer. Upstream defaults it
    /// to `"Default"` rather than leaving it null, so that ungrouped previews
    /// collect under one heading instead of each becoming its own.
    pub group: String,
    /// What this preview is called. `None` leaves the tool to name it after
    /// the function.
    pub name: Option<String>,
    /// The size to lay the widget out at. `None` lets the previewer decide,
    /// which is how a widget that has its own idea of its size is shown at it.
    pub size: Option<Size>,
    pub text_scale_factor: Option<f64>,
    pub wrapper: Option<WidgetWrapper>,
    pub theme: Option<PreviewTheme>,
    pub brightness: Option<Brightness>,
    pub localizations: Option<PreviewLocalizations>,
}

/// Upstream's default group. Named because two places use it and they must
/// agree: the constructor's default and [`PreviewBuilder::build`]'s fallback.
pub const DEFAULT_GROUP: &str = "Default";

impl Preview {
    /// A preview in the default group with nothing else said about it.
    pub fn new() -> Preview {
        Preview {
            group: DEFAULT_GROUP.to_string(),
            ..Preview::default()
        }
    }

    pub fn with_group(mut self, group: impl Into<String>) -> Self {
        self.group = group.into();
        self
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn with_size(mut self, size: Size) -> Self {
        self.size = Some(size);
        self
    }

    pub fn with_text_scale_factor(mut self, factor: f64) -> Self {
        self.text_scale_factor = Some(factor);
        self
    }

    pub fn with_wrapper(mut self, wrapper: impl Fn(AnyWidget) -> AnyWidget + 'static) -> Self {
        self.wrapper = Some(Rc::new(wrapper));
        self
    }

    pub fn with_theme(mut self, theme: impl Fn() -> Rc<dyn PreviewThemeData> + 'static) -> Self {
        self.theme = Some(Rc::new(theme));
        self
    }

    pub fn with_brightness(mut self, brightness: Brightness) -> Self {
        self.brightness = Some(brightness);
        self
    }

    pub fn with_localizations(
        mut self,
        localizations: impl Fn() -> PreviewLocalizationsData + 'static,
    ) -> Self {
        self.localizations = Some(Rc::new(localizations));
        self
    }

    /// Upstream's `transform()`, which for a plain preview answers itself.
    ///
    /// It exists to be overridden: upstream marks it `@mustCallSuper` so a
    /// subclass that adds its own handling still yields a `Preview` the tool
    /// can read. Here the two shapes are separate types, so this is the
    /// identity and [`MultiPreview::transform`] is the interesting one.
    pub fn transform(&self) -> Preview {
        self.clone()
    }

    /// Upstream's `toBuilder()`.
    pub fn to_builder(&self) -> PreviewBuilder {
        PreviewBuilder {
            group: Some(self.group.clone()),
            name: self.name.clone(),
            size: self.size,
            text_scale_factor: self.text_scale_factor,
            wrapper: self.wrapper.clone(),
            theme: self.theme.clone(),
            brightness: self.brightness,
            localizations: self.localizations.clone(),
        }
    }
}

/// Upstream `MultiPreview`: one annotation standing for several previews.
///
/// What it is for is showing the same widget under several conditions at once
/// -- light and dark, three text scales, four locales -- without writing the
/// annotation out that many times.
///
/// Upstream is an abstract class whose subclass supplies `previews`; here it
/// holds them, because there is no annotation for a subclass to be.
#[derive(Clone, Default)]
pub struct MultiPreview {
    pub previews: Vec<Preview>,
}

impl MultiPreview {
    pub fn new(previews: Vec<Preview>) -> MultiPreview {
        MultiPreview { previews }
    }

    /// Upstream's `transform()`: the previews, each transformed in turn.
    ///
    /// **It is one level deep, not recursive.** Upstream maps `transform` over
    /// its own list and returns `List<Preview>` -- and since `Preview` is what
    /// comes back, a `MultiPreview` cannot be an element of another one. The
    /// flattening is the type, not the loop.
    pub fn transform(&self) -> Vec<Preview> {
        self.previews.iter().map(Preview::transform).collect()
    }
}

/// Upstream `PreviewBuilder`: a [`Preview`] assembled a field at a time.
///
/// The difference from building a [`Preview`] directly is [`add_wrapper`], and
/// that is the whole reason the type exists -- everything else here is a
/// mutable copy of the same fields.
///
/// [`add_wrapper`]: PreviewBuilder::add_wrapper
#[derive(Clone, Default)]
pub struct PreviewBuilder {
    /// `None` until set, which is how [`PreviewBuilder::build`] can tell "not
    /// said" from "said to be the default" and apply the fallback.
    pub group: Option<String>,
    pub name: Option<String>,
    pub size: Option<Size>,
    pub text_scale_factor: Option<f64>,
    pub wrapper: Option<WidgetWrapper>,
    pub theme: Option<PreviewTheme>,
    pub brightness: Option<Brightness>,
    pub localizations: Option<PreviewLocalizations>,
}

impl PreviewBuilder {
    pub fn new() -> PreviewBuilder {
        PreviewBuilder::default()
    }

    /// Upstream's `addWrapper`: puts `wrapper` **outside** whatever is already
    /// there.
    ///
    /// The order is the point, and upstream's line says it plainly --
    /// `wrapper = (widget) => newWrapper(wrapperLocal(widget))`. So wrappers
    /// added later are further out, and the first one added is nearest the
    /// widget. Adding a `Scaffold` and then a `Theme` gives a themed scaffold;
    /// the other order would put the theme inside the scaffold, where the
    /// scaffold's own chrome would not see it.
    pub fn add_wrapper(&mut self, wrapper: impl Fn(AnyWidget) -> AnyWidget + 'static) {
        let outer: WidgetWrapper = Rc::new(wrapper);
        self.wrapper = Some(match self.wrapper.take() {
            Some(inner) => Rc::new(move |widget| outer(inner(widget))),
            None => outer,
        });
    }

    /// Upstream's `build()`.
    ///
    /// The only thing it decides is the group: an unset one becomes
    /// [`DEFAULT_GROUP`], matching what [`Preview::new`] does, so a preview
    /// assembled either way lands under the same heading.
    pub fn build(&self) -> Preview {
        Preview {
            group: self
                .group
                .clone()
                .unwrap_or_else(|| DEFAULT_GROUP.to_string()),
            name: self.name.clone(),
            size: self.size,
            text_scale_factor: self.text_scale_factor,
            wrapper: self.wrapper.clone(),
            theme: self.theme.clone(),
            brightness: self.brightness,
            localizations: self.localizations.clone(),
        }
    }
}

/// Upstream `PreviewLocalizationsData`: the localisation setup a preview is
/// shown under.
///
/// The two resolution callbacks are upstream's `localeListResolutionCallback`
/// and `localeResolutionCallback` -- the same pair `WidgetsApp` takes, and they
/// are here for the same reason: a preview of a screen that behaves differently
/// per locale is only useful if it can be shown in one.
#[derive(Clone, Default)]
pub struct PreviewLocalizationsData {
    pub locale: Option<Locale>,
    /// Upstream defaults this to `[Locale('en', 'US')]` rather than to an empty
    /// list: a preview with no supported locales would resolve to nothing.
    pub supported_locales: Vec<Locale>,
    /// Upstream's `localizationsDelegates`. `None` rather than an empty list,
    /// which is upstream's own distinction: absent means the previewer supplies
    /// its defaults, empty means this preview wants none of them.
    pub localizations_delegates: Option<Vec<Rc<dyn crate::localizations::LocalizationsDelegate>>>,
    pub locale_list_resolution: Option<LocaleListResolution>,
    pub locale_resolution: Option<LocaleResolution>,
}

/// Upstream's `LocaleListResolutionCallback`: picks from the platform's whole
/// preference list.
pub type LocaleListResolution = Rc<dyn Fn(Option<&[Locale]>, &[Locale]) -> Option<Locale>>;

/// Upstream's `LocaleResolutionCallback`: picks from the platform's first
/// preference alone.
pub type LocaleResolution = Rc<dyn Fn(Option<&Locale>, &[Locale]) -> Option<Locale>>;

impl PreviewLocalizationsData {
    /// Upstream's defaults, including the `en_US` supported list.
    pub fn new() -> PreviewLocalizationsData {
        PreviewLocalizationsData {
            supported_locales: vec![Locale {
                language_code: "en".to_string(),
                country_code: Some("US".to_string()),
                ..Locale::default()
            }],
            ..PreviewLocalizationsData::default()
        }
    }
}

/// Upstream `PreviewThemeData`: theming a preview is shown under, as the one
/// thing theming does -- wrap the widget.
///
/// Upstream is an `abstract base class` with a single member,
/// `Widget apply(BuildContext, Widget child)`, so this is a trait. What a
/// concrete one puts around the child is its own business; `MultiPreviewThemeData`
/// is the only implementation upstream ships.
pub trait PreviewThemeData {
    /// Upstream's `apply`.
    fn apply(&self, child: AnyWidget) -> AnyWidget;
}

/// Upstream `MultiPreviewThemeData`: several themes applied to one preview.
///
/// # The first theme in the list ends up outermost
///
/// Upstream folds over `themes.reversed`, wrapping the result each time -- so
/// the last theme is applied first and finishes innermost, and the first one is
/// applied last and finishes outside everything.
///
/// **Note this is the opposite way round from
/// [`PreviewBuilder::add_wrapper`]**, where each wrapper added goes outside the
/// ones before it. Both are upstream's, and they differ because one is a list
/// read in order and the other is a sequence of calls: a list reads
/// outside-in, and calls accumulate inside-out. Worth having the two facts next
/// to each other, because a caller who learns one will assume the other.
pub struct MultiPreviewThemeData {
    pub themes: Vec<Rc<dyn PreviewThemeData>>,
}

impl MultiPreviewThemeData {
    pub fn new(themes: Vec<Rc<dyn PreviewThemeData>>) -> MultiPreviewThemeData {
        MultiPreviewThemeData { themes }
    }
}

impl PreviewThemeData for MultiPreviewThemeData {
    fn apply(&self, child: AnyWidget) -> AnyWidget {
        let mut result = child;
        for theme in self.themes.iter().rev() {
            result = theme.apply(result);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::leaf;
    use crate::render::RenderConstrainedBox;
    use std::cell::RefCell;

    /// A wrapper that writes its own name into a shared log when it runs, so
    /// the *order* wrappers are applied in is observable rather than inferred.
    fn recording(
        name: &'static str,
        log: Rc<RefCell<Vec<&'static str>>>,
    ) -> impl Fn(AnyWidget) -> AnyWidget {
        move |widget| {
            log.borrow_mut().push(name);
            widget
        }
    }

    fn a_widget() -> AnyWidget {
        leaf(|| RenderConstrainedBox::tight(1.0, 1.0))
    }

    // -- The group default --------------------------------------------------------

    #[test]
    fn a_preview_that_names_no_group_lands_in_the_default_one() {
        // Upstream defaults it to a string rather than leaving it null, so
        // ungrouped previews collect under one heading instead of each becoming
        // its own.
        assert_eq!(Preview::new().group, DEFAULT_GROUP);
        assert_eq!(DEFAULT_GROUP, "Default");
    }

    #[test]
    fn a_builder_that_was_never_told_a_group_builds_the_same_default() {
        // The two constructions have to agree, or the same preview lands under
        // two headings depending on how it was made.
        assert_eq!(PreviewBuilder::new().build().group, Preview::new().group);
    }

    #[test]
    fn a_group_that_was_set_survives_the_round_trip_through_a_builder() {
        let preview = Preview::new().with_group("Buttons").with_name("Filled");
        let rebuilt = preview.to_builder().build();
        assert_eq!(rebuilt.group, "Buttons");
        assert_eq!(rebuilt.name.as_deref(), Some("Filled"));
    }

    // -- add_wrapper: later goes outside ------------------------------------------

    #[test]
    fn each_wrapper_added_goes_outside_the_ones_before_it() {
        // Upstream's line is `newWrapper(wrapperLocal(widget))`, so the first
        // wrapper added is nearest the widget and runs first.
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut builder = PreviewBuilder::new();
        builder.add_wrapper(recording("inner", Rc::clone(&log)));
        builder.add_wrapper(recording("middle", Rc::clone(&log)));
        builder.add_wrapper(recording("outer", Rc::clone(&log)));

        let wrapper = builder.build().wrapper.expect("a wrapper");
        wrapper(a_widget());

        assert_eq!(
            *log.borrow(),
            vec!["inner", "middle", "outer"],
            "the first added runs first, which is to say it is innermost"
        );
    }

    #[test]
    fn the_first_wrapper_added_is_used_as_is() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut builder = PreviewBuilder::new();
        builder.add_wrapper(recording("only", Rc::clone(&log)));
        builder.build().wrapper.expect("a wrapper")(a_widget());
        assert_eq!(*log.borrow(), vec!["only"]);
    }

    #[test]
    fn a_builder_with_no_wrapper_builds_a_preview_with_none() {
        assert!(PreviewBuilder::new().build().wrapper.is_none());
    }

    // -- MultiPreviewThemeData: the first in the list is outermost ----------------

    /// A theme that records when it is applied, for the same reason the wrapper
    /// above does.
    struct Recording(&'static str, Rc<RefCell<Vec<&'static str>>>);

    impl PreviewThemeData for Recording {
        fn apply(&self, child: AnyWidget) -> AnyWidget {
            self.1.borrow_mut().push(self.0);
            child
        }
    }

    #[test]
    fn the_first_theme_in_the_list_ends_up_outermost() {
        // Upstream folds over `themes.reversed`, so the last theme is applied
        // first and finishes innermost.
        let log = Rc::new(RefCell::new(Vec::new()));
        let themes = MultiPreviewThemeData::new(vec![
            Rc::new(Recording("first", Rc::clone(&log))),
            Rc::new(Recording("second", Rc::clone(&log))),
            Rc::new(Recording("third", Rc::clone(&log))),
        ]);
        themes.apply(a_widget());

        assert_eq!(
            *log.borrow(),
            vec!["third", "second", "first"],
            "applied back to front, so the first one wraps everything"
        );
    }

    #[test]
    fn the_two_orderings_are_opposite_and_both_are_upstreams() {
        // A caller who learns one will assume the other, so this is the test
        // that says they differ. `add_wrapper` accumulates inside-out;
        // `MultiPreviewThemeData` reads a list outside-in.
        let wrapper_log = Rc::new(RefCell::new(Vec::new()));
        let mut builder = PreviewBuilder::new();
        builder.add_wrapper(recording("a", Rc::clone(&wrapper_log)));
        builder.add_wrapper(recording("b", Rc::clone(&wrapper_log)));
        builder.build().wrapper.expect("a wrapper")(a_widget());

        let theme_log = Rc::new(RefCell::new(Vec::new()));
        MultiPreviewThemeData::new(vec![
            Rc::new(Recording("a", Rc::clone(&theme_log))),
            Rc::new(Recording("b", Rc::clone(&theme_log))),
        ])
        .apply(a_widget());

        assert_eq!(*wrapper_log.borrow(), vec!["a", "b"]);
        assert_eq!(*theme_log.borrow(), vec!["b", "a"]);
        assert_ne!(*wrapper_log.borrow(), *theme_log.borrow());
    }

    #[test]
    fn an_empty_theme_list_leaves_the_child_alone() {
        let log: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
        MultiPreviewThemeData::new(Vec::new()).apply(a_widget());
        assert!(log.borrow().is_empty(), "nothing was applied");
    }

    // -- MultiPreview -------------------------------------------------------------

    #[test]
    fn a_multi_preview_transforms_into_its_members() {
        let multi = MultiPreview::new(vec![
            Preview::new().with_name("light"),
            Preview::new().with_name("dark"),
        ]);
        let names: Vec<Option<String>> = multi.transform().into_iter().map(|p| p.name).collect();
        assert_eq!(
            names,
            vec![Some("light".to_string()), Some("dark".to_string())]
        );
    }

    #[test]
    fn transforming_a_plain_preview_answers_the_same_preview() {
        let preview = Preview::new().with_name("one").with_text_scale_factor(2.0);
        let transformed = preview.transform();
        assert_eq!(transformed.name.as_deref(), Some("one"));
        assert_eq!(transformed.text_scale_factor, Some(2.0));
    }

    #[test]
    fn an_empty_multi_preview_transforms_into_nothing() {
        assert!(MultiPreview::default().transform().is_empty());
    }

    // -- Localizations ------------------------------------------------------------

    #[test]
    fn the_default_supported_locale_is_en_us_and_not_an_empty_list() {
        // Upstream's default is `[Locale('en', 'US')]`. An empty list would
        // resolve to nothing, so a preview that said nothing about locales
        // would show nothing.
        let data = PreviewLocalizationsData::new();
        assert_eq!(data.supported_locales.len(), 1);
        assert_eq!(data.supported_locales[0].language_code, "en");
        assert_eq!(
            data.supported_locales[0].country_code.as_deref(),
            Some("US")
        );
        assert!(data.locale.is_none(), "no locale forced");
    }

    #[test]
    fn absent_delegates_are_not_the_same_as_none_wanted() {
        // Upstream's field is nullable *and* a list: absent means the previewer
        // supplies its defaults, empty means this preview wants none of them.
        let quiet = PreviewLocalizationsData::new();
        assert!(quiet.localizations_delegates.is_none());

        let none_wanted = PreviewLocalizationsData {
            localizations_delegates: Some(Vec::new()),
            ..PreviewLocalizationsData::new()
        };
        assert_eq!(
            none_wanted.localizations_delegates.map(|d| d.len()),
            Some(0)
        );
    }
}
