// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The gallery's message catalogue: the lookup surface.
//!
//! Ported from `lib/l10n/gallery_localizations.dart` (flutter/gallery @
//! d12640d), upstream's abstract `GalleryLocalizations` and its delegate.
//! English-only per PORTING.md: every member of the class is here, backed by
//! the English table in [`gallery_localizations_en`], and the other 76
//! locales upstream ships are placeholder modules next to it.
//!
//! The surface keeps upstream's shape through `Deref`: a
//! `GalleryLocalizations` dereferences to the table, so a call site reads
//! `localizations.crane_fly_stops(2)` exactly the way upstream's reads
//! `localizations.craneFlyStops(2)`. Restating all 802 members as forwarding
//! methods here would be a second, hand-maintained copy of the generated
//! table, which is precisely what the generator exists to avoid.
//!
//! What has no counterpart: `delegate` and `localizationsDelegates` (the
//! framework has no `LocalizationsDelegate` machinery to plug them into), and
//! the `of(BuildContext)` lookup (there is no `Localizations` widget; the
//! catalogue is constructed from a [`Locale`] directly). `localeName` is kept
//! as given rather than run through intl's canonicalization.

use std::ops::Deref;

use rustflutter::platform::Locale;

use super::gallery_localizations_en::GalleryLocalizationsEn;

/// Upstream's `GalleryLocalizations`.
pub struct GalleryLocalizations {
    /// Upstream's `localeName`.
    locale_name: String,
    table: GalleryLocalizationsEn,
}

impl GalleryLocalizations {
    /// The English catalogue.
    pub fn en() -> GalleryLocalizations {
        GalleryLocalizations {
            locale_name: "en".to_string(),
            table: GalleryLocalizationsEn,
        }
    }

    /// Upstream's `lookupGalleryLocalizations`: the catalogue for a locale.
    ///
    /// Every supported locale resolves to the English table, because English
    /// is the only language whose strings are ported. The locale's own tag is
    /// still recorded in `locale_name`, so the day a second table lands this
    /// function is the only place that changes.
    pub fn lookup(locale: &Locale) -> GalleryLocalizations {
        GalleryLocalizations {
            locale_name: locale.to_language_tag(),
            table: GalleryLocalizationsEn,
        }
    }

    /// Upstream's `localeName`.
    pub fn locale_name(&self) -> &str {
        &self.locale_name
    }

    /// Upstream's delegate's `isSupported`: the language codes the gallery
    /// ships a table for upstream.
    pub fn is_supported(locale: &Locale) -> bool {
        SUPPORTED_LANGUAGE_CODES.contains(&locale.language_code.as_str())
    }

    /// Upstream's `supportedLocales`.
    pub fn supported_locales() -> Vec<Locale> {
        SUPPORTED_LOCALES
            .iter()
            .map(|(language, country, script)| Locale {
                language_code: language.to_string(),
                country_code: country.map(str::to_string),
                script_code: script.map(str::to_string),
                variant_code: None,
            })
            .collect()
    }
}

impl Deref for GalleryLocalizations {
    type Target = GalleryLocalizationsEn;

    fn deref(&self) -> &GalleryLocalizationsEn {
        &self.table
    }
}

/// The language codes of upstream's delegate's `isSupported` list.
const SUPPORTED_LANGUAGE_CODES: &[&str] = &[
    "af", "am", "ar", "as", "az", "be", "bg", "bn", "bs", "ca", "cs", "cy", "da", "de", "el", "en",
    "es", "et", "eu", "fa", "fi", "fil", "fr", "gl", "gsw", "gu", "he", "hi", "hr", "hu", "hy",
    "id", "is", "it", "ja", "ka", "kk", "km", "kn", "ko", "ky", "lo", "lt", "lv", "mk", "ml", "mn",
    "mr", "ms", "my", "nb", "ne", "nl", "or", "pa", "pl", "pt", "ro", "ru", "si", "sk", "sl", "sq",
    "sr", "sv", "sw", "ta", "te", "th", "tl", "tr", "uk", "ur", "uz", "vi", "zh", "zu",
];

/// Upstream's `supportedLocales`, as (language, country, script).
const SUPPORTED_LOCALES: &[(&str, Option<&str>, Option<&str>)] = &[
    ("en", None, None),
    ("af", None, None),
    ("am", None, None),
    ("ar", None, None),
    ("ar", Some("EG"), None),
    ("ar", Some("JO"), None),
    ("ar", Some("MA"), None),
    ("ar", Some("SA"), None),
    ("as", None, None),
    ("az", None, None),
    ("be", None, None),
    ("bg", None, None),
    ("bn", None, None),
    ("bs", None, None),
    ("ca", None, None),
    ("cs", None, None),
    ("cy", None, None),
    ("da", None, None),
    ("de", None, None),
    ("de", Some("AT"), None),
    ("de", Some("CH"), None),
    ("el", None, None),
    ("en", Some("AU"), None),
    ("en", Some("CA"), None),
    ("en", Some("GB"), None),
    ("en", Some("IE"), None),
    ("en", Some("IN"), None),
    ("en", Some("NZ"), None),
    ("en", Some("SG"), None),
    ("en", Some("ZA"), None),
    ("es", None, None),
    ("es", Some("419"), None),
    ("es", Some("AR"), None),
    ("es", Some("BO"), None),
    ("es", Some("CL"), None),
    ("es", Some("CO"), None),
    ("es", Some("CR"), None),
    ("es", Some("DO"), None),
    ("es", Some("EC"), None),
    ("es", Some("GT"), None),
    ("es", Some("HN"), None),
    ("es", Some("MX"), None),
    ("es", Some("NI"), None),
    ("es", Some("PA"), None),
    ("es", Some("PE"), None),
    ("es", Some("PR"), None),
    ("es", Some("PY"), None),
    ("es", Some("SV"), None),
    ("es", Some("US"), None),
    ("es", Some("UY"), None),
    ("es", Some("VE"), None),
    ("et", None, None),
    ("eu", None, None),
    ("fa", None, None),
    ("fi", None, None),
    ("fil", None, None),
    ("fr", None, None),
    ("fr", Some("CA"), None),
    ("fr", Some("CH"), None),
    ("gl", None, None),
    ("gsw", None, None),
    ("gu", None, None),
    ("he", None, None),
    ("hi", None, None),
    ("hr", None, None),
    ("hu", None, None),
    ("hy", None, None),
    ("id", None, None),
    ("is", None, None),
    ("it", None, None),
    ("ja", None, None),
    ("ka", None, None),
    ("kk", None, None),
    ("km", None, None),
    ("kn", None, None),
    ("ko", None, None),
    ("ky", None, None),
    ("lo", None, None),
    ("lt", None, None),
    ("lv", None, None),
    ("mk", None, None),
    ("ml", None, None),
    ("mn", None, None),
    ("mr", None, None),
    ("ms", None, None),
    ("my", None, None),
    ("nb", None, None),
    ("ne", None, None),
    ("nl", None, None),
    ("or", None, None),
    ("pa", None, None),
    ("pl", None, None),
    ("pt", None, None),
    ("pt", Some("BR"), None),
    ("pt", Some("PT"), None),
    ("ro", None, None),
    ("ru", None, None),
    ("si", None, None),
    ("sk", None, None),
    ("sl", None, None),
    ("sq", None, None),
    ("sr", None, None),
    ("sr", None, Some("Latn")),
    ("sv", None, None),
    ("sw", None, None),
    ("ta", None, None),
    ("te", None, None),
    ("th", None, None),
    ("tl", None, None),
    ("tr", None, None),
    ("uk", None, None),
    ("ur", None, None),
    ("uz", None, None),
    ("vi", None, None),
    ("zh", None, None),
    ("zh", Some("CN"), None),
    ("zh", Some("HK"), None),
    ("zh", Some("TW"), None),
    ("zu", None, None),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_supported_lists_match_upstream() {
        assert_eq!(GalleryLocalizations::supported_locales().len(), 119);
        assert_eq!(SUPPORTED_LANGUAGE_CODES.len(), 77);
        assert!(GalleryLocalizations::is_supported(&Locale::new("fr")));
        assert!(!GalleryLocalizations::is_supported(&Locale::new("xx")));
    }

    #[test]
    fn lookup_resolves_to_english_and_keeps_the_tag() {
        let localizations = GalleryLocalizations::lookup(&Locale {
            language_code: "zh".to_string(),
            country_code: Some("CN".to_string()),
            ..Locale::default()
        });
        assert_eq!(localizations.locale_name(), "zh-CN");
        // The Deref surface: call sites read the way upstream's do.
        assert_eq!(localizations.back_to_gallery(), "Back to Gallery");
    }

    #[test]
    fn parameterized_messages_interpolate() {
        let localizations = GalleryLocalizations::en();
        assert_eq!(
            localizations.github_repo("flutter/gallery"),
            "flutter/gallery GitHub repository"
        );
        assert_eq!(localizations.crane_fly_stops(1), "1 stop");
        assert_eq!(localizations.crane_fly_stops(3), "3 stops");
        assert_eq!(localizations.shrine_cart_item_count(0), "0 ITEMS");
    }
}
