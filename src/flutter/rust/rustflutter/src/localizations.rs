//! Localizations -- a port of upstream's `widgets/localizations.dart` and the
//! locale-matching algorithm from `widgets/app.dart` that feeds it.
//!
//! Two things are decided here, and they are separate on purpose.
//!
//! **Which locale.** The reader's device offers a *list* of preferred locales
//! in order, the application supports a different list, and something has to
//! pick. The algorithm is not "first match" -- it is deliberately reluctant
//! about language-only matches, because "some Portuguese" is a worse answer
//! than "Brazilian Portuguese from the reader's second choice" and the loop
//! has to see the second choice before it can know.
//!
//! **Which resources.** A delegate loads one bundle of strings for one locale,
//! and several delegates contribute different bundles. The rule that makes
//! this composable is that **only the first delegate for each type is
//! loaded**, so an application's own delegate placed ahead of the framework's
//! silently replaces it.
//!
//! ## What is here that upstream puts elsewhere
//!
//! Nothing, now. `scanTextButtonLabel` sat on this trait and belongs to
//! `MaterialLocalizations` -- the widgets layer's selection toolbar offers
//! copy, paste, look up, search web, share and select all, and reading text
//! with a camera is Material's.
//!
//! It is simply gone rather than moved. Nothing read it, here or anywhere,
//! which is how a string on the wrong trait went unnoticed in the first
//! place; parking it on the right one would have kept the same problem with a
//! better address. It arrives when the toolbar names its buttons, which this
//! crate does not yet do -- it models which buttons may appear
//! (`can_copy`, `can_paste` and the rest) and not what they say.
//!
//! ## What is not here
//!
//! The `Localizations` widget's element, its `_LocalizationsScope` and the
//! `Localizations.override` factory need an inherited-widget tree this crate
//! spells differently. What is ported is the delegate contract, the load rule
//! including its synchronous fast path, the default English resources, and the
//! resolution algorithm in full.

use crate::direction::TextDirection;
use crate::platform::Locale;
use std::collections::{HashMap, HashSet};

/// Upstream `LocalizationsDelegate`: one source of localized resources.
///
/// `resource_type` stands in for upstream's `Type get type => T`, which is how
/// a loaded bundle is looked up again. It is a `&'static str` here because
/// this crate has no runtime type identity to key on.
pub trait LocalizationsDelegate {
    /// Upstream's `type`.
    fn resource_type(&self) -> &'static str;

    /// Upstream's `isSupported`, and its documentation says **language**: a
    /// delegate answering for `en` is expected to cope with `en_GB` too.
    /// Refusing every locale it has no exact table for would leave a reader in
    /// Ireland with no strings at all.
    fn is_supported(&self, locale: &Locale) -> bool;

    /// Upstream's `load`. `is_synchronous` says whether the bundle was already
    /// in hand -- see [`load_all`].
    fn load(&self, locale: &Locale) -> LoadedResources;

    /// Upstream's `shouldReload`, consulted on every rebuild of the
    /// `Localizations` widget. It exists because a delegate is usually a
    /// `const` object that is rebuilt identical every frame, and reloading a
    /// string table per frame would be absurd; the default is therefore
    /// **false**, and a delegate that genuinely changes says so.
    fn should_reload(&self, _old: &dyn LocalizationsDelegate) -> bool {
        false
    }
}

/// What a delegate's `load` produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedResources {
    pub resource_type: &'static str,
    /// The bundle, as whatever the delegate wanted to hand over. A real
    /// delegate returns a struct of strings; the ruler only needs a value.
    pub value: String,
    /// Upstream's `SynchronousFuture`: the bundle was already in hand.
    pub is_synchronous: bool,
}

impl LoadedResources {
    pub fn synchronous(resource_type: &'static str, value: impl Into<String>) -> LoadedResources {
        LoadedResources {
            resource_type,
            value: value.into(),
            is_synchronous: true,
        }
    }

    pub fn asynchronous(resource_type: &'static str, value: impl Into<String>) -> LoadedResources {
        LoadedResources {
            resource_type,
            value: value.into(),
            is_synchronous: false,
        }
    }
}

/// What [`load_all`] worked out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoadPlan {
    /// Resources already in hand.
    pub ready: HashMap<&'static str, String>,
    /// Types still being awaited, in the order they were started.
    pub pending: Vec<&'static str>,
}

impl LoadPlan {
    /// Upstream's "all of the delegate.load() values were synchronous futures,
    /// we're done" -- it returns a `SynchronousFuture`, which resolves within
    /// the same frame.
    ///
    /// That is the whole point of the branch: a first frame that already has
    /// its strings, rather than one frame of nothing followed by the text
    /// appearing. Upstream goes to real trouble not to `Future.wait` on
    /// futures that had already completed.
    pub fn is_synchronous(&self) -> bool {
        self.pending.is_empty()
    }
}

/// Upstream's `_loadAll`.
///
/// **Only the first delegate of each type that supports the locale is
/// loaded.** That single line is what makes the delegate list overridable:
/// `MaterialApp` puts the application's delegates ahead of the framework's, so
/// an application supplying its own `MaterialLocalizations` shadows the
/// built-in one without removing anything.
///
/// Note the order of the two conditions -- the type is only claimed when the
/// delegate **also supports the locale**. A delegate that would shadow the
/// framework's but cannot handle this locale steps aside for it rather than
/// leaving the reader with nothing.
pub fn load_all(locale: &Locale, delegates: &[&dyn LocalizationsDelegate]) -> LoadPlan {
    let mut claimed: HashSet<&'static str> = HashSet::new();
    let mut chosen: Vec<&&dyn LocalizationsDelegate> = Vec::new();
    for delegate in delegates {
        if !claimed.contains(delegate.resource_type()) && delegate.is_supported(locale) {
            claimed.insert(delegate.resource_type());
            chosen.push(delegate);
        }
    }

    let mut plan = LoadPlan::default();
    for delegate in chosen {
        let loaded = delegate.load(locale);
        if loaded.is_synchronous {
            debug_assert!(
                !plan.ready.contains_key(loaded.resource_type),
                "two delegates claimed the same type"
            );
            plan.ready.insert(loaded.resource_type, loaded.value);
        } else {
            plan.pending.push(loaded.resource_type);
        }
    }
    plan
}

/// Upstream `WidgetsLocalizations`: the strings the framework itself needs.
///
/// It is a short list, and that is the point -- these are the ones no
/// application supplies because no application wrote the widget that says
/// them. A reorderable list has to be able to tell a screen reader "move to
/// the start", and nothing above the widgets layer knows it exists.
pub trait WidgetsLocalizations {
    /// The reading direction, which upstream puts **here** rather than beside
    /// the locale: it is a property of the language, and a `Directionality`
    /// above the localizations would have to be kept in step by hand.
    fn text_direction(&self) -> TextDirection;

    fn reorder_item_to_start(&self) -> &str;
    fn reorder_item_to_end(&self) -> &str;
    fn reorder_item_up(&self) -> &str;
    fn reorder_item_down(&self) -> &str;
    fn reorder_item_left(&self) -> &str;
    fn reorder_item_right(&self) -> &str;
    fn copy_button_label(&self) -> &str;
    fn cut_button_label(&self) -> &str;
    fn paste_button_label(&self) -> &str;
    fn select_all_button_label(&self) -> &str;
    fn look_up_button_label(&self) -> &str;
    fn search_web_button_label(&self) -> &str;
    fn share_button_label(&self) -> &str;

    /// Upstream's `searchResultsFound`, announced when `RawAutocomplete`'s
    /// options list goes from **empty to non-empty**.
    ///
    /// It is a transition and not a state. A reader typing into an
    /// autocomplete cannot see the list appear, so the announcement is the
    /// appearing -- which is why there is a matching one for the list
    /// emptying, and why neither says how many.
    fn search_results_found(&self) -> &str;

    /// Upstream's `noResultsFound`, for the same list going the other way.
    fn no_results_found(&self) -> &str;

    /// Upstream's `radioButtonUnselectedLabel`: the accessibility hint for a
    /// radio button that is **off**.
    ///
    /// Only the unselected one has a string. A selected radio is announced as
    /// selected by the platform's own vocabulary for a control that is on;
    /// an unselected one in a group of unselected ones needs saying, because
    /// silence there is indistinguishable from the reader having missed it.
    fn radio_button_unselected_label(&self) -> &str;
}

/// Upstream `DefaultWidgetsLocalizations`: US English, and nothing else.
///
/// Upstream ships exactly one locale here, and the delegate below claims to
/// support **every** locale. That reads wrong until the alternative is
/// considered: a framework that refused unknown locales would leave a reader
/// with an application whose own strings are translated and whose "Paste"
/// button has no label at all. English is a poor answer and no answer is a
/// worse one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DefaultWidgetsLocalizations;

impl WidgetsLocalizations for DefaultWidgetsLocalizations {
    /// Upstream returns `TextDirection.ltr` unconditionally, for the same
    /// reason: it is the only direction it has strings for.
    fn text_direction(&self) -> TextDirection {
        TextDirection::Ltr
    }

    fn reorder_item_to_start(&self) -> &str {
        "Move to the start"
    }
    fn reorder_item_to_end(&self) -> &str {
        "Move to the end"
    }
    fn reorder_item_up(&self) -> &str {
        "Move up"
    }
    fn reorder_item_down(&self) -> &str {
        "Move down"
    }
    fn reorder_item_left(&self) -> &str {
        "Move left"
    }
    fn reorder_item_right(&self) -> &str {
        "Move right"
    }
    fn copy_button_label(&self) -> &str {
        "Copy"
    }
    fn cut_button_label(&self) -> &str {
        "Cut"
    }
    fn paste_button_label(&self) -> &str {
        "Paste"
    }
    fn select_all_button_label(&self) -> &str {
        "Select all"
    }
    fn look_up_button_label(&self) -> &str {
        "Look Up"
    }
    fn search_web_button_label(&self) -> &str {
        "Search Web"
    }
    fn share_button_label(&self) -> &str {
        "Share"
    }

    fn search_results_found(&self) -> &str {
        "Search results found"
    }

    fn no_results_found(&self) -> &str {
        "No results found"
    }

    fn radio_button_unselected_label(&self) -> &str {
        "Not selected"
    }
}

/// Upstream's `_WidgetsLocalizationsDelegate`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DefaultWidgetsLocalizationsDelegate;

impl LocalizationsDelegate for DefaultWidgetsLocalizationsDelegate {
    fn resource_type(&self) -> &'static str {
        "WidgetsLocalizations"
    }

    /// Upstream: `bool isSupported(Locale locale) => true`.
    fn is_supported(&self, _locale: &Locale) -> bool {
        true
    }

    /// Upstream returns a `SynchronousFuture`, so the framework's own strings
    /// never cost a frame.
    fn load(&self, _locale: &Locale) -> LoadedResources {
        LoadedResources::synchronous("WidgetsLocalizations", "en_US")
    }
}

/// Upstream `Localizations`: the widget that holds the loaded resources.
///
/// # It is a widget now
///
/// This was a value with no way into a tree, which meant the reader's language
/// stopped at [`crate::platform::locale`] and nothing below could ask for it.
/// [`provide_localizations`] publishes one and [`locale_of`] reads it back,
/// which is upstream's `Localizations.localeOf(context)`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Localizations {
    pub locale: Option<Locale>,
    resources: HashMap<&'static str, String>,
}

impl Localizations {
    pub fn new(locale: Locale) -> Localizations {
        Localizations {
            locale: Some(locale),
            resources: HashMap::new(),
        }
    }

    /// Upstream's `Localizations.of`, which returns **null** rather than
    /// asserting when the type is not there. Nullable is right: a widget that
    /// works without a particular localization -- and several do, falling back
    /// to an unlabelled control -- should be able to ask.
    pub fn of(&self, resource_type: &str) -> Option<&str> {
        self.resources.get(resource_type).map(String::as_str)
    }

    /// The locale this bundle was loaded for, if it has one.
    ///
    /// Not upstream's `maybeLocaleOf`, which takes a context -- that is
    /// [`maybe_locale_of`], and this is what it reads.
    pub fn maybe_locale(&self) -> Option<&Locale> {
        self.locale.as_ref()
    }

    pub fn apply(&mut self, plan: &LoadPlan) {
        for (resource_type, value) in plan.ready.iter() {
            self.resources.insert(resource_type, value.clone());
        }
    }

    /// Upstream's `Localizations.override` factory, whose whole job is to
    /// **inherit the delegates from above and change only the locale**. A
    /// caller wanting one subtree in another language would otherwise have to
    /// name every delegate the application configured.
    pub fn override_locale(
        parent_delegates: &[&'static str],
        added: &[&'static str],
    ) -> Vec<&'static str> {
        let mut combined: Vec<&'static str> = added.to_vec();
        combined.extend_from_slice(parent_delegates);
        combined
    }
}

/// Upstream `basicLocaleListResolution`, the default matching algorithm.
///
/// The subtlety is in how **reluctant** it is about a language-only match,
/// and in where that reluctance does *not* apply.
///
/// From the second preference onward, a language-only hit is remembered rather
/// than returned: "some Chinese, script unknown" is a worse answer than an
/// exact match one place further down the reader's list, and the loop has to
/// look at that next entry before it can know.
///
/// The **first** preference is the exception, and is returned at once --
/// upstream's reason is that the first locale is usually strongly held, so
/// a language match there is likely to be what the reader wants even if
/// something further down would match more precisely. The one case that still
/// waits is when the *next* preference names the same language, where one more
/// round is at worst the same answer and at best a better one.
// -- Reaching it from a build ------------------------------------------------

/// Publishes a [`Localizations`] to a subtree, which is upstream's
/// `Localizations` widget.
///
/// The root of an application is wrapped in one automatically -- see
/// `WidgetHost` -- so this is only needed to *change* what a subtree sees,
/// which is upstream's `Localizations.override`.
pub fn provide_localizations(
    localizations: Localizations,
    child: crate::framework::AnyWidget,
) -> crate::framework::AnyWidget {
    crate::framework::provide(localizations, child)
}

/// Upstream's `Localizations.maybeLocaleOf`: the reader's language, or `None`
/// where nothing published one.
pub fn maybe_locale_of(context: &crate::framework::BuildContext) -> Option<Locale> {
    context
        .inherited::<Localizations>()
        .and_then(|bundle| bundle.locale.clone())
}

/// Upstream's `Localizations.localeOf`.
///
/// Upstream asserts when there is no `Localizations` above; here the root
/// always publishes one, so the fallback is only reachable from a test that
/// mounted a widget on its own -- the same arrangement, and the same reason,
/// as [`crate::media_query::media_query_of`].
///
/// The fallback is [`crate::platform::locale`], which has a documented one of
/// its own: a platform that has said nothing is treated as `en`, because a
/// framework with no locale at all cannot format a date.
pub fn locale_of(context: &crate::framework::BuildContext) -> Locale {
    maybe_locale_of(context).unwrap_or_else(crate::platform::locale)
}

/// Upstream's `Localizations.of<T>`, which answers **null** rather than
/// asserting when the type is not there.
///
/// Nullable is right: a widget that works without a particular localization --
/// and several do, falling back to an unlabelled control -- should be able to
/// ask.
pub fn resource_of(
    context: &crate::framework::BuildContext,
    resource_type: &str,
) -> Option<String> {
    context
        .inherited::<Localizations>()
        .and_then(|bundle| bundle.of(resource_type).map(str::to_string))
}

/// Upstream `LocaleListResolutionCallback`: given every preferred locale and
/// every supported one, choose. `None` means "I have no opinion".
pub type LocaleListResolution = fn(&[Locale], &[Locale]) -> Option<Locale>;

/// Upstream `LocaleResolutionCallback`, which is handed **one** locale: the
/// reader's first preference, or `None` where the platform named none.
pub type LocaleResolution = fn(Option<&Locale>, &[Locale]) -> Option<Locale>;

/// Upstream's `_resolveLocales`: two chances to override, then the algorithm.
///
/// ```dart
/// if (localeListResolutionCallback != null) { ... if (locale != null) return locale; }
/// if (localeResolutionCallback != null) { ... if (locale != null) return locale; }
/// return basicLocaleListResolution(preferredLocales, supportedLocales);
/// ```
///
/// Three things about the order, none of which is arbitrary.
///
/// * The **list** callback comes first, because it is the one that can see
///   what the reader actually asked for. A reader whose preferences are
///   `[fr_CA, fr_FR, en]` is telling you something a single locale cannot.
/// * The **single** callback is handed only the first preference -- upstream
///   passes `preferredLocales.first`, and `None` when the list is empty. An
///   application that wrote the simpler callback gets the simpler question.
/// * Returning `None` from either is **not** the same as returning a locale
///   that happens to be unsupported: `None` means "carry on", so a callback
///   that only wants to intervene sometimes says nothing the rest of the time.
pub fn resolve_locales(
    preferred: &[Locale],
    supported: &[Locale],
    list_callback: Option<LocaleListResolution>,
    single_callback: Option<LocaleResolution>,
) -> Option<Locale> {
    if let Some(callback) = list_callback {
        if let Some(locale) = callback(preferred, supported) {
            return Some(locale);
        }
    }
    if let Some(callback) = single_callback {
        if let Some(locale) = callback(preferred.first(), supported) {
            return Some(locale);
        }
    }
    basic_locale_list_resolution(preferred, supported)
}

pub fn basic_locale_list_resolution(
    preferred_locales: &[Locale],
    supported_locales: &[Locale],
) -> Option<Locale> {
    let first_supported = supported_locales.first()?;
    if preferred_locales.is_empty() {
        return Some(first_supported.clone());
    }

    let key = |locale: &Locale| {
        format!(
            "{}_{}_{}",
            locale.language_code,
            locale.script_code.as_deref().unwrap_or("null"),
            locale.country_code.as_deref().unwrap_or("null")
        )
    };

    // Upstream builds four indexes in one reverse pass, so that the **first**
    // supported locale wins each key rather than the last. The application's
    // own ordering is a preference, and iterating forwards would silently
    // invert it.
    let mut all: HashMap<String, Locale> = HashMap::new();
    let mut language_and_country: HashMap<String, Locale> = HashMap::new();
    let mut language_and_script: HashMap<String, Locale> = HashMap::new();
    let mut language_only: HashMap<String, Locale> = HashMap::new();
    let mut country_only: HashMap<String, Locale> = HashMap::new();
    for locale in supported_locales.iter().rev() {
        all.insert(key(locale), locale.clone());
        if let Some(script) = &locale.script_code {
            language_and_script.insert(
                format!("{}_{}", locale.language_code, script),
                locale.clone(),
            );
        }
        if let Some(country) = &locale.country_code {
            language_and_country.insert(
                format!("{}_{}", locale.language_code, country),
                locale.clone(),
            );
            country_only.insert(country.clone(), locale.clone());
        }
        language_only.insert(locale.language_code.clone(), locale.clone());
    }

    let mut matches_language: Option<Locale> = None;
    let mut matches_country: Option<Locale> = None;

    for (index, user_locale) in preferred_locales.iter().enumerate() {
        if all.contains_key(&key(user_locale)) {
            return Some(user_locale.clone());
        }
        if let Some(script) = &user_locale.script_code {
            if let Some(found) =
                language_and_script.get(&format!("{}_{}", user_locale.language_code, script))
            {
                return Some(found.clone());
            }
        }
        if let Some(country) = &user_locale.country_code {
            if let Some(found) =
                language_and_country.get(&format!("{}_{}", user_locale.language_code, country))
            {
                return Some(found.clone());
            }
        }
        // A language-only hit from a higher-ranked preference, cashed in now
        // that this one has produced nothing better.
        if let Some(found) = matches_language.clone() {
            return Some(found);
        }
        if let Some(found) = language_only.get(&user_locale.language_code) {
            matches_language = Some(found.clone());
            // The first preference is usually strongly held, so a
            // language-only match there is taken at once -- unless the *next*
            // preference names the same language, where waiting one round is
            // at worst the same answer and at best a better one.
            let next_is_same_language = preferred_locales
                .get(index + 1)
                .is_some_and(|next| next.language_code == user_locale.language_code);
            if index == 0 && !next_is_same_language {
                return matches_language;
            }
        }
        if matches_country.is_none() {
            if let Some(country) = &user_locale.country_code {
                if let Some(found) = country_only.get(country) {
                    matches_country = Some(found.clone());
                }
            }
        }
    }

    // Upstream's comment: a country-only match is worth trying because a
    // reader is likely to be familiar with a language from their listed
    // country. Failing everything, the application's first supported locale.
    Some(
        matches_language
            .or(matches_country)
            .unwrap_or_else(|| first_supported.clone()),
    )
}

/// Upstream `LocalizationsResolver`: watches the platform and holds the
/// answer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LocalizationsResolver {
    /// Upstream's `locale`, the application's override. When set, resolution
    /// runs against **only this locale** rather than the platform's list --
    /// an application that says "be in French" means it.
    pub locale: Option<Locale>,
    pub supported_locales: Vec<Locale>,
    /// Upstream's `localeListResolutionCallback` and
    /// `localeResolutionCallback`, which **both** paths consult: the resolver
    /// runs `_resolveLocales` for the platform's list and again for an
    /// application's explicit locale, and that function asks the callbacks
    /// before the algorithm. A resolver that held the locales but not the
    /// callbacks could never let an application intervene at all.
    pub list_callback: Option<LocaleListResolution>,
    pub single_callback: Option<LocaleResolution>,
    resolved_locale: Option<Locale>,
    notifications: usize,
}

impl LocalizationsResolver {
    pub fn new(
        supported_locales: Vec<Locale>,
        platform_locales: &[Locale],
    ) -> LocalizationsResolver {
        let resolved = basic_locale_list_resolution(platform_locales, &supported_locales);
        LocalizationsResolver {
            locale: None,
            supported_locales,
            list_callback: None,
            single_callback: None,
            resolved_locale: resolved,
            notifications: 0,
        }
    }

    pub fn with_callbacks(
        mut self,
        list_callback: Option<LocaleListResolution>,
        single_callback: Option<LocaleResolution>,
    ) -> LocalizationsResolver {
        self.list_callback = list_callback;
        self.single_callback = single_callback;
        self
    }

    pub fn notifications(&self) -> usize {
        self.notifications
    }

    /// Upstream's `locale` getter.
    pub fn resolved(&self) -> Option<Locale> {
        match &self.locale {
            // `_resolveLocales(<Locale>[_locale!], supportedLocales)`: the
            // application's own locale is a **preference list of one**, put
            // through the same resolution as the platform's. So an application
            // that asks for a locale it does not support gets the same
            // fallback a reader asking for it would -- and its own callbacks
            // get to see the request first.
            Some(forced) => resolve_locales(
                std::slice::from_ref(forced),
                &self.supported_locales,
                self.list_callback,
                self.single_callback,
            ),
            None => self.resolved_locale.clone(),
        }
    }

    /// Upstream's `didChangeLocales`, arriving from the binding when the
    /// reader changes their system language.
    pub fn did_change_locales(&mut self, platform_locales: &[Locale]) {
        let next = resolve_locales(
            platform_locales,
            &self.supported_locales,
            self.list_callback,
            self.single_callback,
        );
        if next != self.resolved_locale {
            self.resolved_locale = next;
            self.notifications += 1;
        }
    }

    /// Upstream's `update`, and it re-resolves **only when the supported set
    /// changed**.
    ///
    /// That is deliberate rather than an oversight: the other four fields are
    /// read on demand by the `locale` getter, so changing them needs no work
    /// here, while the supported set is what the cached platform resolution
    /// was computed against.
    ///
    /// It has a consequence worth stating, because it looks like a bug from
    /// either side alone: **a new callback changes an explicit locale's answer
    /// at once and the platform's not at all**, until something else
    /// re-resolves. The explicit path runs the callbacks on every read; the
    /// platform path ran them when the locales last arrived.
    pub fn update(
        &mut self,
        locale: Option<Locale>,
        supported_locales: Vec<Locale>,
        list_callback: Option<LocaleListResolution>,
        single_callback: Option<LocaleResolution>,
        platform_locales: &[Locale],
    ) {
        self.locale = locale;
        self.list_callback = list_callback;
        self.single_callback = single_callback;
        if self.supported_locales != supported_locales {
            self.supported_locales = supported_locales;
            self.did_change_locales(platform_locales);
        }
    }

    /// Upstream's `localizationsDelegates` getter, which appends the
    /// framework's default **last**. Last is what makes it a default: the
    /// first delegate per type wins, so anything the application supplied is
    /// found first.
    pub fn delegate_order(application_delegates: &[&'static str]) -> Vec<&'static str> {
        let mut order = application_delegates.to_vec();
        order.push("WidgetsLocalizations");
        order
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn locale(language: &str) -> Locale {
        Locale::new(language)
    }

    // -- Reaching it from a build ---------------------------------------------

    use crate::framework::{
        AnyWidget, BuildContext, Component, ElementTree, component, leaf, many,
    };
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    /// Reads the ambient locale and counts its builds, the way a real consumer
    /// would.
    struct LocaleProbe {
        seen: Rc<RefCell<Vec<String>>>,
        builds: Rc<Cell<u32>>,
    }

    impl Component for LocaleProbe {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            self.builds.set(self.builds.get() + 1);
            self.seen
                .borrow_mut()
                .push(locale_of(context).language_code.clone());
            leaf(|| crate::widgets::Empty)
        }
    }

    #[allow(clippy::type_complexity)]
    fn locale_probe() -> (AnyWidget, Rc<RefCell<Vec<String>>>, Rc<Cell<u32>>) {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let builds = Rc::new(Cell::new(0));
        (
            component(LocaleProbe {
                seen: Rc::clone(&seen),
                builds: Rc::clone(&builds),
            }),
            seen,
            builds,
        )
    }

    #[test]
    fn a_widget_can_ask_what_language_it_is_in() {
        // Before this the reader's language stopped at `platform::locale`:
        // nothing in the framework read it, and there was no way into a tree
        // for it to be read from.
        let (probe, seen, _) = locale_probe();
        let mut tree = ElementTree::new();
        tree.rebuild(provide_localizations(
            Localizations::new(locale("fr")),
            probe,
        ));
        assert_eq!(*seen.borrow(), vec!["fr"]);
    }

    #[test]
    fn a_subtree_can_be_in_another_language() {
        // Upstream's `Localizations.override`, which exists so one part of a
        // page can be in a language the rest is not -- a preview of a
        // translation, a name that should not be transliterated.
        let (outer, outer_seen, _) = locale_probe();
        let (inner, inner_seen, _) = locale_probe();
        let mut tree = ElementTree::new();
        tree.rebuild(provide_localizations(
            Localizations::new(locale("fr")),
            many(
                vec![
                    outer,
                    provide_localizations(Localizations::new(locale("ja")), inner),
                ],
                |children| {
                    let mut flex = crate::render::RenderFlex::column();
                    for child in children {
                        flex = flex.push(child);
                    }
                    Box::new(flex)
                },
            ),
        ));
        assert_eq!(*outer_seen.borrow(), vec!["fr"]);
        assert_eq!(*inner_seen.borrow(), vec!["ja"], "the nearer one wins");
    }

    #[test]
    fn changing_the_language_rebuilds_whoever_asked_and_nobody_else() {
        // What `publish` buys over remounting: the reader changes their system
        // language and only the widgets that asked about it are built again.
        let (asker, seen, asker_builds) = locale_probe();
        let quiet_builds = Rc::new(Cell::new(0));

        struct Quiet(Rc<Cell<u32>>);
        impl Component for Quiet {
            fn build(&self, _context: &mut BuildContext) -> AnyWidget {
                self.0.set(self.0.get() + 1);
                leaf(|| crate::widgets::Empty)
            }
        }

        let mut tree = ElementTree::new();
        tree.rebuild(provide_localizations(
            Localizations::new(locale("fr")),
            many(
                vec![asker, component(Quiet(Rc::clone(&quiet_builds)))],
                |children| {
                    let mut flex = crate::render::RenderFlex::column();
                    for child in children {
                        flex = flex.push(child);
                    }
                    Box::new(flex)
                },
            ),
        ));
        assert_eq!((asker_builds.get(), quiet_builds.get()), (1, 1));

        assert!(tree.publish(Localizations::new(locale("ja"))));
        tree.rebuild_dirty();
        assert_eq!(*seen.borrow(), vec!["fr", "ja"]);
        assert_eq!(
            (asker_builds.get(), quiet_builds.get()),
            (2, 1),
            "the one that never asked is not rebuilt"
        );
    }

    #[test]
    fn a_widget_with_nothing_above_it_falls_back_to_the_platform() {
        // Upstream asserts here. The root of an application always publishes
        // one, so this is only reachable from a test that mounted a widget on
        // its own -- and answering with the platform's is better than
        // answering with an empty locale nothing could match.
        crate::platform::set_locales(vec![with_country("pt", "BR")]);
        let (probe, seen, _) = locale_probe();
        let mut tree = ElementTree::new();
        tree.rebuild(probe);
        assert_eq!(*seen.borrow(), vec!["pt"]);
        crate::platform::reset();
    }

    fn with_country(language: &str, country: &str) -> Locale {
        Locale {
            language_code: language.to_string(),
            country_code: Some(country.to_string()),
            ..Locale::default()
        }
    }

    fn with_script(language: &str, script: &str) -> Locale {
        Locale {
            language_code: language.to_string(),
            script_code: Some(script.to_string()),
            ..Locale::default()
        }
    }

    fn full(language: &str, script: &str, country: &str) -> Locale {
        Locale {
            language_code: language.to_string(),
            script_code: Some(script.to_string()),
            country_code: Some(country.to_string()),
            ..Locale::default()
        }
    }

    // -- Locale resolution -------------------------------------------------

    #[test]
    fn an_exact_match_wins_outright() {
        let resolved = basic_locale_list_resolution(
            &[with_country("en", "GB")],
            &[with_country("en", "US"), with_country("en", "GB")],
        );
        assert_eq!(resolved, Some(with_country("en", "GB")));
    }

    #[test]
    fn a_language_and_country_match_beats_the_bare_language() {
        let resolved = basic_locale_list_resolution(
            &[with_country("pt", "BR")],
            &[locale("pt"), with_country("pt", "BR")],
        );
        assert_eq!(resolved, Some(with_country("pt", "BR")));
    }

    #[test]
    fn a_language_and_script_match_is_tried_before_the_country() {
        let resolved = basic_locale_list_resolution(
            &[full("zh", "Hant", "TW")],
            &[with_script("zh", "Hant"), with_country("zh", "CN")],
        );
        assert_eq!(resolved, Some(with_script("zh", "Hant")));
    }

    #[test]
    fn a_language_only_hit_from_a_later_preference_waits_for_something_better() {
        // "Some Chinese, script unknown" is a worse answer than an exact match
        // one place further down the reader's own list. The first preference
        // here matches nothing at all, so the reluctance is visible.
        let resolved = basic_locale_list_resolution(
            &[
                locale("sv"),
                full("zh", "Hant", "TW"),
                with_country("en", "US"),
            ],
            &[locale("zh"), with_country("en", "US")],
        );
        assert_eq!(
            resolved,
            Some(with_country("en", "US")),
            "the exact match two places down beat the remembered Chinese"
        );
    }

    #[test]
    fn the_first_preference_is_not_reluctant_at_all() {
        // Upstream takes a language-only match on the first locale at once,
        // because that locale is usually strongly held -- even though en_US
        // further down would have matched exactly.
        let resolved = basic_locale_list_resolution(
            &[full("zh", "Hant", "TW"), with_country("en", "US")],
            &[locale("zh"), with_country("en", "US")],
        );
        assert_eq!(resolved, Some(locale("zh")));
    }

    #[test]
    fn a_language_only_hit_is_cashed_in_when_nothing_better_turns_up() {
        let resolved = basic_locale_list_resolution(
            &[full("zh", "Hant", "TW"), with_country("de", "DE")],
            &[locale("zh"), with_country("en", "US")],
        );
        assert_eq!(
            resolved,
            Some(locale("zh")),
            "German was no help, so the remembered Chinese stands"
        );
    }

    #[test]
    fn a_language_only_hit_on_the_first_preference_is_taken_at_once() {
        // The first preference is usually strongly held.
        let resolved = basic_locale_list_resolution(
            &[with_country("zh", "TW"), with_country("en", "US")],
            &[locale("zh"), with_country("en", "US")],
        );
        assert_eq!(resolved, Some(locale("zh")));
    }

    #[test]
    fn unless_the_next_preference_names_the_same_language() {
        // Waiting one more round is at worst the same answer and at best a
        // better one.
        let resolved = basic_locale_list_resolution(
            &[with_country("zh", "TW"), with_country("zh", "HK")],
            &[locale("zh"), with_country("zh", "HK")],
        );
        assert_eq!(
            resolved,
            Some(with_country("zh", "HK")),
            "the second preference matched exactly, which is better"
        );
    }

    #[test]
    fn a_country_only_match_is_the_last_thing_tried_before_giving_up() {
        // Upstream's reason: a reader is likely to be familiar with a language
        // from their listed country.
        let resolved = basic_locale_list_resolution(
            &[with_country("sv", "CH")],
            &[with_country("de", "CH"), with_country("en", "US")],
        );
        assert_eq!(resolved, Some(with_country("de", "CH")));
    }

    #[test]
    fn the_applications_first_supported_locale_is_the_last_resort() {
        let resolved = basic_locale_list_resolution(
            &[locale("sv")],
            &[with_country("en", "US"), locale("de")],
        );
        assert_eq!(resolved, Some(with_country("en", "US")));
    }

    #[test]
    fn no_preferences_at_all_gives_the_first_supported_locale() {
        let resolved = basic_locale_list_resolution(&[], &[with_country("en", "US"), locale("de")]);
        assert_eq!(resolved, Some(with_country("en", "US")));
    }

    #[test]
    fn an_application_supporting_nothing_resolves_to_nothing() {
        assert_eq!(basic_locale_list_resolution(&[locale("en")], &[]), None);
    }

    #[test]
    fn the_earliest_supported_locale_wins_a_tie_rather_than_the_latest() {
        // Upstream indexes in reverse so that the application's own ordering
        // is honoured; iterating forwards would silently invert it.
        let resolved = basic_locale_list_resolution(
            &[locale("en")],
            &[with_country("en", "GB"), with_country("en", "US")],
        );
        assert_eq!(resolved, Some(with_country("en", "GB")));
    }

    // -- Delegates ---------------------------------------------------------

    struct Delegate {
        resource_type: &'static str,
        value: &'static str,
        supports: Option<&'static str>,
        synchronous: bool,
    }

    impl Delegate {
        fn any(resource_type: &'static str, value: &'static str) -> Delegate {
            Delegate {
                resource_type,
                value,
                supports: None,
                synchronous: true,
            }
        }

        fn only(
            resource_type: &'static str,
            value: &'static str,
            language: &'static str,
        ) -> Delegate {
            Delegate {
                resource_type,
                value,
                supports: Some(language),
                synchronous: true,
            }
        }

        fn slow(resource_type: &'static str, value: &'static str) -> Delegate {
            Delegate {
                resource_type,
                value,
                supports: None,
                synchronous: false,
            }
        }
    }

    impl LocalizationsDelegate for Delegate {
        fn resource_type(&self) -> &'static str {
            self.resource_type
        }

        fn is_supported(&self, locale: &Locale) -> bool {
            match self.supports {
                Some(language) => locale.language_code == language,
                None => true,
            }
        }

        fn load(&self, _locale: &Locale) -> LoadedResources {
            if self.synchronous {
                LoadedResources::synchronous(self.resource_type, self.value)
            } else {
                LoadedResources::asynchronous(self.resource_type, self.value)
            }
        }
    }

    #[test]
    fn the_first_delegate_of_each_type_is_the_one_that_loads() {
        // Which is what lets an application shadow the framework's without
        // removing anything.
        let mine = Delegate::any("WidgetsLocalizations", "mine");
        let theirs = DefaultWidgetsLocalizationsDelegate;
        let plan = load_all(&locale("en"), &[&mine, &theirs]);
        assert_eq!(
            plan.ready.get("WidgetsLocalizations"),
            Some(&"mine".to_string())
        );
        assert_eq!(plan.ready.len(), 1);
    }

    #[test]
    fn a_shadowing_delegate_that_cannot_handle_this_locale_steps_aside() {
        // The type is claimed only when the delegate *also* supports the
        // locale, so the reader is not left with nothing.
        let mine = Delegate::only("WidgetsLocalizations", "mine", "fr");
        let theirs = DefaultWidgetsLocalizationsDelegate;

        let french = load_all(&locale("fr"), &[&mine, &theirs]);
        assert_eq!(
            french.ready.get("WidgetsLocalizations"),
            Some(&"mine".to_string())
        );

        let german = load_all(&locale("de"), &[&mine, &theirs]);
        assert_eq!(
            german.ready.get("WidgetsLocalizations"),
            Some(&"en_US".to_string()),
            "the framework's, because mine could not answer"
        );
    }

    #[test]
    fn delegates_of_different_types_all_load() {
        let widgets = Delegate::any("WidgetsLocalizations", "w");
        let material = Delegate::any("MaterialLocalizations", "m");
        let plan = load_all(&locale("en"), &[&widgets, &material]);
        assert_eq!(plan.ready.len(), 2);
    }

    #[test]
    fn everything_already_in_hand_resolves_within_the_same_frame() {
        // A first frame that already has its strings, rather than one frame of
        // nothing followed by the text appearing.
        let widgets = DefaultWidgetsLocalizationsDelegate;
        let plan = load_all(&locale("en"), &[&widgets]);
        assert!(plan.is_synchronous());
        assert!(plan.pending.is_empty());
    }

    #[test]
    fn one_slow_delegate_makes_the_whole_load_wait() {
        let fast = Delegate::any("WidgetsLocalizations", "w");
        let slow = Delegate::slow("MaterialLocalizations", "m");
        let plan = load_all(&locale("en"), &[&fast, &slow]);
        assert!(!plan.is_synchronous());
        assert_eq!(plan.pending, vec!["MaterialLocalizations"]);
        assert_eq!(plan.ready.len(), 1, "and the fast one is ready regardless");
    }

    #[test]
    fn the_framework_default_goes_last_which_is_what_makes_it_a_default() {
        assert_eq!(
            LocalizationsResolver::delegate_order(&["MaterialLocalizations"]),
            vec!["MaterialLocalizations", "WidgetsLocalizations"]
        );
    }

    #[test]
    fn the_framework_delegate_claims_every_locale_on_purpose() {
        // English is a poor answer for a Greek reader; no label on the Paste
        // button is a worse one.
        let delegate = DefaultWidgetsLocalizationsDelegate;
        assert!(delegate.is_supported(&locale("el")));
        assert!(delegate.is_supported(&locale("ar")));
        assert_eq!(
            DefaultWidgetsLocalizations.text_direction(),
            TextDirection::Ltr
        );
        assert_eq!(DefaultWidgetsLocalizations.paste_button_label(), "Paste");
    }

    #[test]
    fn a_delegate_does_not_reload_on_every_rebuild() {
        // A const delegate is rebuilt identical every frame, and reloading a
        // string table per frame would be absurd.
        let one = Delegate::any("A", "a");
        let two = Delegate::any("A", "a");
        assert!(!one.should_reload(&two));
    }

    // -- The widget and the resolver ---------------------------------------

    #[test]
    fn asking_for_a_type_that_is_not_there_gives_nothing_rather_than_failing() {
        // Several widgets work without a particular localization, falling back
        // to an unlabelled control, and should be able to ask.
        let mut localizations = Localizations::new(locale("en"));
        let plan = load_all(&locale("en"), &[&DefaultWidgetsLocalizationsDelegate]);
        localizations.apply(&plan);

        assert_eq!(localizations.of("WidgetsLocalizations"), Some("en_US"));
        assert_eq!(localizations.of("MaterialLocalizations"), None);
        assert_eq!(localizations.maybe_locale(), Some(&locale("en")));
    }

    #[test]
    fn an_override_inherits_the_delegates_from_above_and_adds_to_the_front() {
        // Or a caller wanting one subtree in another language would have to
        // name every delegate the application configured.
        assert_eq!(
            Localizations::override_locale(
                &["MaterialLocalizations", "WidgetsLocalizations"],
                &["Mine"]
            ),
            vec!["Mine", "MaterialLocalizations", "WidgetsLocalizations"]
        );
    }

    #[test]
    fn an_application_that_says_be_in_french_means_it() {
        // The override resolves against only itself, not against the
        // platform's list.
        let mut resolver = LocalizationsResolver::new(
            vec![with_country("en", "US"), locale("fr")],
            &[with_country("en", "US")],
        );
        assert_eq!(resolver.resolved(), Some(with_country("en", "US")));

        resolver.locale = Some(locale("fr"));
        assert_eq!(resolver.resolved(), Some(locale("fr")));
    }

    #[test]
    fn changing_the_system_language_notifies_only_when_the_answer_changed() {
        let mut resolver = LocalizationsResolver::new(
            vec![with_country("en", "US"), locale("fr")],
            &[with_country("en", "US")],
        );
        assert_eq!(resolver.notifications(), 0);

        resolver.did_change_locales(&[with_country("en", "GB")]);
        assert_eq!(
            resolver.notifications(),
            0,
            "still resolves to en_US, so nobody needs telling"
        );

        resolver.did_change_locales(&[locale("fr")]);
        assert_eq!(resolver.notifications(), 1);
        assert_eq!(resolver.resolved(), Some(locale("fr")));
    }

    #[test]
    fn update_re_resolves_only_when_the_supported_set_changed() {
        // The other fields are read on demand, so changing them needs no work
        // here; the supported set is what the cached resolution was computed
        // against.
        let platform = [locale("fr")];
        let mut resolver = LocalizationsResolver::new(vec![with_country("en", "US")], &platform);
        assert_eq!(resolver.resolved(), Some(with_country("en", "US")));

        resolver.update(None, vec![with_country("en", "US")], None, None, &platform);
        assert_eq!(resolver.notifications(), 0, "nothing changed");

        resolver.update(
            None,
            vec![with_country("en", "US"), locale("fr")],
            None,
            None,
            &platform,
        );
        assert_eq!(resolver.notifications(), 1);
        assert_eq!(resolver.resolved(), Some(locale("fr")));
    }
}

#[cfg(test)]
mod resolver_callback_tests {
    use super::*;

    fn locale(language: &str) -> Locale {
        Locale::new(language)
    }

    fn always_swedish(_preferred: &[Locale], _supported: &[Locale]) -> Option<Locale> {
        Some(Locale::new("sv"))
    }

    fn never(_preferred: &[Locale], _supported: &[Locale]) -> Option<Locale> {
        None
    }

    fn single_always_swedish(_first: Option<&Locale>, _supported: &[Locale]) -> Option<Locale> {
        Some(Locale::new("sv"))
    }

    #[test]
    fn the_callbacks_are_asked_about_the_platforms_locales() {
        // The resolver held the locales and not the callbacks, so an
        // application that wrote one could never be asked.
        let mut resolver =
            LocalizationsResolver::new(vec![locale("en"), locale("fr")], &[locale("fr")])
                .with_callbacks(Some(always_swedish), None);
        resolver.did_change_locales(&[locale("en")]);
        assert_eq!(
            resolver.resolved(),
            Some(locale("sv")),
            "the callback has the last word, even over a supported locale"
        );
    }

    #[test]
    fn the_callbacks_are_asked_about_an_explicit_locale_too() {
        // `_resolveLocales(<Locale>[_locale!], supportedLocales)`: the
        // application's own locale goes through the same two chances.
        let mut resolver = LocalizationsResolver::new(vec![locale("en"), locale("fr")], &[])
            .with_callbacks(None, Some(single_always_swedish));
        resolver.locale = Some(locale("fr"));
        assert_eq!(resolver.resolved(), Some(locale("sv")));
    }

    #[test]
    fn an_explicit_locale_is_still_resolved_against_what_is_supported() {
        // Not used as given: an application that asks for a locale it does not
        // support falls back exactly as a reader asking for it would.
        let mut resolver =
            LocalizationsResolver::new(vec![locale("en"), locale("fr")], &[locale("en")]);
        resolver.locale = Some(Locale::new("de"));
        assert_eq!(
            resolver.resolved(),
            Some(locale("en")),
            "the first supported locale, as `basicLocaleListResolution` ends"
        );
    }

    #[test]
    fn a_callback_that_says_nothing_leaves_the_algorithm_alone() {
        let mut resolver = LocalizationsResolver::new(vec![locale("en"), locale("fr")], &[])
            .with_callbacks(Some(never), None);
        resolver.did_change_locales(&[locale("fr")]);
        assert_eq!(resolver.resolved(), Some(locale("fr")));
    }

    #[test]
    fn a_new_callback_moves_an_explicit_locale_at_once_and_the_platform_later() {
        // The asymmetry `update` leaves behind, which looks like a bug from
        // either side alone: the explicit path runs the callbacks on every
        // read, the platform path ran them when the locales last arrived.
        let mut resolver =
            LocalizationsResolver::new(vec![locale("en"), locale("fr")], &[locale("fr")]);
        assert_eq!(resolver.resolved(), Some(locale("fr")));

        resolver.update(
            None,
            vec![locale("en"), locale("fr")],
            Some(always_swedish),
            None,
            &[locale("fr")],
        );
        assert_eq!(
            resolver.resolved(),
            Some(locale("fr")),
            "the platform's answer was settled before the callback existed"
        );

        resolver.locale = Some(locale("en"));
        assert_eq!(
            resolver.resolved(),
            Some(locale("sv")),
            "but an explicit locale asks it now"
        );

        // And the platform's answer follows the next time the locales arrive.
        resolver.locale = None;
        resolver.did_change_locales(&[locale("fr")]);
        assert_eq!(resolver.resolved(), Some(locale("sv")));
    }
}

// -- The two chances to override the resolution -------------------------------

#[cfg(test)]
mod resolve_locales_tests {
    //! Upstream's `_resolveLocales`, which this port had only the last step
    //! of: `basicLocaleListResolution` was reachable and the two callbacks
    //! above it were not, so an application could not intervene at all.

    use super::{Locale, resolve_locales};

    fn supported() -> Vec<Locale> {
        vec![Locale::new("en"), Locale::new("fr"), Locale::new("ja")]
    }

    fn preferred() -> Vec<Locale> {
        vec![Locale::new("de"), Locale::new("fr")]
    }

    #[test]
    fn the_list_callback_gets_the_first_word() {
        // It is the one that can see what the reader actually asked for, in
        // order, which is a thing a single locale cannot say.
        fn always_japanese(_preferred: &[Locale], _supported: &[Locale]) -> Option<Locale> {
            Some(Locale::new("ja"))
        }
        assert_eq!(
            resolve_locales(&preferred(), &supported(), Some(always_japanese), None),
            Some(Locale::new("ja"))
        );
    }

    #[test]
    fn and_when_both_speak_the_list_one_is_the_one_heard() {
        // The test that says the order is an order. With only one callback
        // answering at a time, swapping the two changes nothing -- which is
        // what the first draft of this module asserted, and a mutation that
        // swapped them stayed green.
        fn list_says_japanese(_preferred: &[Locale], _supported: &[Locale]) -> Option<Locale> {
            Some(Locale::new("ja"))
        }
        fn single_says_english(_first: Option<&Locale>, _supported: &[Locale]) -> Option<Locale> {
            Some(Locale::new("en"))
        }
        assert_eq!(
            resolve_locales(
                &preferred(),
                &supported(),
                Some(list_says_japanese),
                Some(single_says_english)
            ),
            Some(Locale::new("ja")),
            "the one that can see the whole list decides first"
        );
    }

    #[test]
    fn and_the_single_callback_the_second() {
        fn always_japanese(_first: Option<&Locale>, _supported: &[Locale]) -> Option<Locale> {
            Some(Locale::new("ja"))
        }
        assert_eq!(
            resolve_locales(&preferred(), &supported(), None, Some(always_japanese)),
            Some(Locale::new("ja"))
        );
    }

    #[test]
    fn a_callback_that_says_nothing_hands_on_rather_than_deciding() {
        // `None` is "carry on", not "no locale". A callback that only wants to
        // intervene sometimes has to be able to stay quiet the rest of the
        // time, and if `None` ended the search every such callback would blank
        // out the application it was written for.
        fn quiet_list(_preferred: &[Locale], _supported: &[Locale]) -> Option<Locale> {
            None
        }
        fn quiet_single(_first: Option<&Locale>, _supported: &[Locale]) -> Option<Locale> {
            None
        }
        // Both quiet: the basic algorithm answers, which for [de, fr] against
        // [en, fr, ja] is fr.
        assert_eq!(
            resolve_locales(
                &preferred(),
                &supported(),
                Some(quiet_list),
                Some(quiet_single)
            ),
            Some(Locale::new("fr"))
        );
        // And the quiet list callback still lets the single one speak.
        fn speaks(_first: Option<&Locale>, _supported: &[Locale]) -> Option<Locale> {
            Some(Locale::new("ja"))
        }
        assert_eq!(
            resolve_locales(&preferred(), &supported(), Some(quiet_list), Some(speaks)),
            Some(Locale::new("ja"))
        );
    }

    #[test]
    fn the_single_callback_is_handed_one_locale_and_not_the_list() {
        // Upstream passes `preferredLocales.first`. An application that wrote
        // the simpler callback gets the simpler question, and a callback that
        // read the whole list would be reading a list it was never given.
        fn echoes_first(first: Option<&Locale>, _supported: &[Locale]) -> Option<Locale> {
            first.cloned()
        }
        assert_eq!(
            resolve_locales(&preferred(), &supported(), None, Some(echoes_first)),
            Some(Locale::new("de")),
            "the first preference, not the one that matches"
        );
    }

    #[test]
    fn and_none_at_all_where_the_platform_named_none() {
        // Upstream passes null rather than a made-up locale, so a callback can
        // tell "the reader wants nothing in particular" from "the reader wants
        // English".
        fn refuses_nothing(first: Option<&Locale>, _supported: &[Locale]) -> Option<Locale> {
            match first {
                None => Some(Locale::new("ja")),
                Some(_) => None,
            }
        }
        assert_eq!(
            resolve_locales(&[], &supported(), None, Some(refuses_nothing)),
            Some(Locale::new("ja"))
        );
        assert_ne!(
            resolve_locales(&preferred(), &supported(), None, Some(refuses_nothing)),
            Some(Locale::new("ja"))
        );
    }
}
