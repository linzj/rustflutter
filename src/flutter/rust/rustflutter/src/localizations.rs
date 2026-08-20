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
    fn scan_text_button_label(&self) -> &str;
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
    fn scan_text_button_label(&self) -> &str {
        "Scan text"
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

    /// Upstream's `maybeLocaleOf`.
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
            resolved_locale: resolved,
            notifications: 0,
        }
    }

    pub fn notifications(&self) -> usize {
        self.notifications
    }

    /// Upstream's `locale` getter.
    pub fn resolved(&self) -> Option<Locale> {
        match &self.locale {
            Some(forced) => {
                basic_locale_list_resolution(std::slice::from_ref(forced), &self.supported_locales)
            }
            None => self.resolved_locale.clone(),
        }
    }

    /// Upstream's `didChangeLocales`, arriving from the binding when the
    /// reader changes their system language.
    pub fn did_change_locales(&mut self, platform_locales: &[Locale]) {
        let next = basic_locale_list_resolution(platform_locales, &self.supported_locales);
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
    pub fn update(
        &mut self,
        locale: Option<Locale>,
        supported_locales: Vec<Locale>,
        platform_locales: &[Locale],
    ) {
        self.locale = locale;
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

        resolver.update(None, vec![with_country("en", "US")], &platform);
        assert_eq!(resolver.notifications(), 0, "nothing changed");

        resolver.update(
            None,
            vec![with_country("en", "US"), locale("fr")],
            &platform,
        );
        assert_eq!(resolver.notifications(), 1);
        assert_eq!(resolver.resolved(), Some(locale("fr")));
    }
}
