//! The About box and the licence list behind it -- a port of upstream's
//! `material/about.dart`.
//!
//! Every Flutter application ships with a page listing the licences of
//! everything it was built from, and the only interesting part of assembling
//! it is that **the relation between packages and licences runs both ways**: a
//! licence can cover several packages, and a package can be covered by
//! several licences. [`LicenseData`] keeps that as a list of licences plus an
//! index of which ones each package uses, which is what lets a shared licence
//! be stored once and listed under every package it covers.
//!
//! ## What is not here
//!
//! The page's own layout -- a master/detail flow that becomes two panes on a
//! wide screen -- is upstream's `_MasterDetailFlow`, several hundred lines of
//! navigator work this crate has no route machinery for. The breakpoint it
//! turns on is here, because that number is the decision.

use crate::licenses::LicenseEntry;
use std::rc::Rc;

/// Upstream's `_materialGutterThreshold`: the width at which the licence page
/// stops being a list and becomes two panes.
///
/// 720 logical pixels, which is where Material's own guidance puts the line
/// between a phone held in landscape and a tablet.
pub const MATERIAL_GUTTER_THRESHOLD: f32 = 720.0;

/// Upstream's `_wideGutterSize`.
pub const WIDE_GUTTER_SIZE: f32 = 24.0;

/// Upstream's `_narrowGutterSize`.
pub const NARROW_GUTTER_SIZE: f32 = 12.0;

/// Upstream's `_getGutterSize`.
pub fn gutter_size(width: f32) -> f32 {
    if width >= MATERIAL_GUTTER_THRESHOLD {
        WIDE_GUTTER_SIZE
    } else {
        NARROW_GUTTER_SIZE
    }
}

/// Upstream's `_defaultApplicationVersion`, which returns the empty string.
///
/// Upstream's comment is a `TODO(ianh)`: the version should come from the
/// embedder and there is no way to ask for it. Kept as it is rather than
/// invented, because a made-up version number on an About box is worse than
/// none.
pub fn default_application_version() -> String {
    String::new()
}

/// Upstream's `_defaultApplicationName`.
///
/// The heading of the page [`LicensePage`] shows, from upstream's
/// `Text(MaterialLocalizations.of(context).licensesPageTitle)`.
///
/// Upstream takes no override for it. The application's own name goes in the
/// body, above the licences, and the page's title says what the page is --
/// which is why it is a fixed word rather than something a caller composes:
/// two applications' licence pages should be the same page to a reader who
/// has seen one before.
pub fn licenses_page_title() -> &'static str {
    crate::material_app::DefaultMaterialLocalizations::LICENSES_PAGE_TITLE
}

/// The ancestor `Title` widget's title if there is one, and otherwise the
/// **name of the executable**. Upstream's comment explains what it does not
/// do: a title that changes while the application runs is not tracked, because
/// doing so would mean publishing the current title through an inherited
/// widget for a case that is better served by passing `applicationName`
/// explicitly.
pub fn default_application_name(ancestor_title: Option<&str>, executable_path: &str) -> String {
    if let Some(title) = ancestor_title {
        return title.to_string();
    }
    executable_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(executable_path)
        .to_string()
}

/// Upstream's `_LicenseData`: the licences, and which packages use which.
///
/// The binding is by **index into the licence list**, and upstream's comment
/// calls it a contract: the index is recorded before the licence is pushed, so
/// that `licenses.length` at that moment is where it is about to land. One
/// licence text covering twenty packages is stored once and listed under all
/// twenty.
#[derive(Default)]
pub struct LicenseData {
    licenses: Vec<Rc<dyn LicenseEntry>>,
    /// Package name to the indices of the licences that cover it, in the order
    /// they arrived.
    package_license_bindings: Vec<(String, Vec<usize>)>,
    packages: Vec<String>,
    /// Upstream's `firstPackage`, which gets special treatment below.
    first_package: Option<String>,
}

impl LicenseData {
    pub fn new() -> LicenseData {
        LicenseData::default()
    }

    pub fn licenses(&self) -> &[Rc<dyn LicenseEntry>] {
        &self.licenses
    }

    pub fn packages(&self) -> &[String] {
        &self.packages
    }

    pub fn first_package(&self) -> Option<&str> {
        self.first_package.as_deref()
    }

    /// Which licences cover a package, by index.
    pub fn bindings_for(&self, package: &str) -> Option<&[usize]> {
        self.package_license_bindings
            .iter()
            .find(|(name, _)| name == package)
            .map(|(_, indices)| indices.as_slice())
    }

    fn add_package(&mut self, package: &str) {
        if self
            .package_license_bindings
            .iter()
            .any(|(name, _)| name == package)
        {
            return;
        }
        self.package_license_bindings
            .push((package.to_string(), Vec::new()));
        if self.first_package.is_none() {
            self.first_package = Some(package.to_string());
        }
        self.packages.push(package.to_string());
    }

    /// Upstream's `addLicense`.
    ///
    /// The packages are recorded **before** the licence is pushed, because the
    /// index each of them is given is where the licence is about to go. Doing
    /// it the other way round would bind every package to the licence after
    /// its own.
    pub fn add_license(&mut self, entry: Rc<dyn LicenseEntry>) {
        let at = self.licenses.len();
        for package in entry.packages() {
            self.add_package(&package);
            if let Some((_, indices)) = self
                .package_license_bindings
                .iter_mut()
                .find(|(name, _)| *name == package)
            {
                indices.push(at);
            }
        }
        self.licenses.push(entry);
    }

    /// Upstream's `sortPackages` with its default comparison.
    ///
    /// **The first package stays first whatever its name is.** Upstream's
    /// comment says why: the first package the registry returns is the
    /// application's own licence, and a reader opening the licence page is
    /// looking for *this* application before anything it was built from. Sorting
    /// it into the alphabet would bury it.
    ///
    /// Everything else is compared case-insensitively, so `Xml` does not sort
    /// before `archive`.
    pub fn sort_packages(&mut self) {
        let first = self.first_package.clone();
        self.packages.sort_by(|a, b| {
            if Some(a.as_str()) == first.as_deref() {
                return std::cmp::Ordering::Less;
            }
            if Some(b.as_str()) == first.as_deref() {
                return std::cmp::Ordering::Greater;
            }
            a.to_lowercase().cmp(&b.to_lowercase())
        });
    }
}

/// Upstream `AboutListTile`: a row that opens the About box.
///
/// Every field is optional and every one has the same shape of default: use
/// what the caller gave, otherwise ask the application. The tile itself is
/// only a way of reaching [`AboutDialog`], which is why the two carry the same
/// set.
pub struct AboutListTile {
    pub application_name: Option<String>,
    pub application_version: Option<String>,
    pub application_legalese: Option<String>,
    pub dense: Option<bool>,
}

impl Default for AboutListTile {
    fn default() -> AboutListTile {
        AboutListTile::new()
    }
}

impl AboutListTile {
    pub fn new() -> AboutListTile {
        AboutListTile {
            application_name: None,
            application_version: None,
            application_legalese: None,
            dense: None,
        }
    }

    pub fn with_application_name(mut self, name: impl Into<String>) -> Self {
        self.application_name = Some(name.into());
        self
    }

    pub fn with_application_version(mut self, version: impl Into<String>) -> Self {
        self.application_version = Some(version.into());
        self
    }

    pub fn with_application_legalese(mut self, legalese: impl Into<String>) -> Self {
        self.application_legalese = Some(legalese.into());
        self
    }

    pub fn with_dense(mut self, dense: bool) -> Self {
        self.dense = Some(dense);
        self
    }

    /// The name this tile shows, upstream's
    /// `applicationName ?? _defaultApplicationName(context)`.
    pub fn resolved_application_name(
        &self,
        ancestor_title: Option<&str>,
        executable_path: &str,
    ) -> String {
        self.application_name
            .clone()
            .unwrap_or_else(|| default_application_name(ancestor_title, executable_path))
    }

    /// Upstream's `onTap`: the tile's whole purpose is to show the dialog,
    /// handing on every field it was given.
    pub fn to_dialog(&self) -> AboutDialog {
        AboutDialog {
            application_name: self.application_name.clone(),
            application_version: self.application_version.clone(),
            application_legalese: self.application_legalese.clone(),
        }
    }
}

/// Upstream `AboutDialog`: the box itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AboutDialog {
    pub application_name: Option<String>,
    pub application_version: Option<String>,
    pub application_legalese: Option<String>,
}

impl AboutDialog {
    pub fn new() -> AboutDialog {
        AboutDialog::default()
    }

    pub fn with_application_name(mut self, name: impl Into<String>) -> Self {
        self.application_name = Some(name.into());
        self
    }

    pub fn with_application_version(mut self, version: impl Into<String>) -> Self {
        self.application_version = Some(version.into());
        self
    }

    pub fn with_application_legalese(mut self, legalese: impl Into<String>) -> Self {
        self.application_legalese = Some(legalese.into());
        self
    }

    pub fn resolved_application_name(
        &self,
        ancestor_title: Option<&str>,
        executable_path: &str,
    ) -> String {
        self.application_name
            .clone()
            .unwrap_or_else(|| default_application_name(ancestor_title, executable_path))
    }

    /// Upstream's `applicationVersion ?? _defaultApplicationVersion(context)`,
    /// which is the empty string when nobody said.
    pub fn resolved_application_version(&self) -> String {
        self.application_version
            .clone()
            .unwrap_or_else(default_application_version)
    }
}

/// Upstream `LicensePage`: the list of everything the application was built
/// from.
pub struct LicensePage {
    pub application_name: Option<String>,
    pub application_version: Option<String>,
    pub application_legalese: Option<String>,
}

impl Default for LicensePage {
    fn default() -> LicensePage {
        LicensePage::new()
    }
}

impl LicensePage {
    pub fn new() -> LicensePage {
        LicensePage {
            application_name: None,
            application_version: None,
            application_legalese: None,
        }
    }

    pub fn with_application_name(mut self, name: impl Into<String>) -> Self {
        self.application_name = Some(name.into());
        self
    }

    pub fn with_application_version(mut self, version: impl Into<String>) -> Self {
        self.application_version = Some(version.into());
        self
    }

    pub fn with_application_legalese(mut self, legalese: impl Into<String>) -> Self {
        self.application_legalese = Some(legalese.into());
        self
    }

    /// Whether this width gets the two-pane layout, upstream's `isLateral`.
    pub fn is_lateral(width: f32) -> bool {
        width >= MATERIAL_GUTTER_THRESHOLD
    }

    /// Collects the registry into a [`LicenseData`], sorted -- upstream's
    /// `_licenses` future, which folds every entry the registry yields.
    pub fn collect(entries: Vec<Rc<dyn LicenseEntry>>) -> LicenseData {
        let mut data = LicenseData::new();
        for entry in entries {
            data.add_license(entry);
        }
        data.sort_packages();
        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::licenses::LicenseParagraph;

    struct Entry {
        packages: Vec<String>,
        text: String,
    }

    impl Entry {
        fn new(packages: &[&str], text: &str) -> Rc<dyn LicenseEntry> {
            Rc::new(Entry {
                packages: packages.iter().map(|name| name.to_string()).collect(),
                text: text.to_string(),
            })
        }
    }

    impl LicenseEntry for Entry {
        fn packages(&self) -> Vec<String> {
            self.packages.clone()
        }

        fn paragraphs(&self) -> Vec<LicenseParagraph> {
            vec![LicenseParagraph::new(self.text.clone(), 0)]
        }
    }

    #[test]
    fn one_licence_covering_many_packages_is_stored_once_and_listed_under_each() {
        // The relation runs both ways, and this is the direction that would be
        // expensive to get wrong: the BSD text shipped with forty packages
        // would be forty copies.
        let mut data = LicenseData::new();
        data.add_license(Entry::new(&["archive", "crypto", "xml"], "BSD"));
        assert_eq!(data.licenses().len(), 1);
        assert_eq!(data.packages().len(), 3);
        for package in ["archive", "crypto", "xml"] {
            assert_eq!(data.bindings_for(package), Some(&[0usize][..]));
        }
    }

    #[test]
    fn a_package_covered_by_several_licences_gets_them_all_in_order() {
        let mut data = LicenseData::new();
        data.add_license(Entry::new(&["archive"], "BSD"));
        data.add_license(Entry::new(&["archive", "xml"], "MIT"));
        data.add_license(Entry::new(&["archive"], "Apache"));
        assert_eq!(data.bindings_for("archive"), Some(&[0usize, 1, 2][..]));
        assert_eq!(data.bindings_for("xml"), Some(&[1usize][..]));
        assert_eq!(data.licenses().len(), 3);
    }

    #[test]
    fn the_index_is_recorded_before_the_licence_is_pushed() {
        // Upstream calls this a contract. Binding after pushing would tie
        // every package to the licence *after* its own, which would show up as
        // an entire licence page listing the wrong texts -- and only once
        // there were two licences.
        let mut data = LicenseData::new();
        data.add_license(Entry::new(&["first"], "one"));
        data.add_license(Entry::new(&["second"], "two"));
        let first = data.bindings_for("first").expect("bound")[0];
        let second = data.bindings_for("second").expect("bound")[0];
        assert_eq!(
            data.licenses()[first].paragraphs()[0].text,
            "one",
            "the package points at its own licence"
        );
        assert_eq!(data.licenses()[second].paragraphs()[0].text, "two");
    }

    #[test]
    fn the_applications_own_package_stays_first_whatever_it_is_called() {
        // The registry hands back the application's licence first, and a
        // reader opening the licence page is looking for *this* application
        // before anything it was built from. Sorting it into the alphabet
        // would bury it.
        let mut data = LicenseData::new();
        data.add_license(Entry::new(&["zebra_app"], "app"));
        data.add_license(Entry::new(&["archive"], "BSD"));
        data.add_license(Entry::new(&["material"], "MIT"));
        assert_eq!(data.first_package(), Some("zebra_app"));

        data.sort_packages();
        assert_eq!(
            data.packages(),
            &[
                "zebra_app".to_string(),
                "archive".to_string(),
                "material".to_string()
            ]
        );
    }

    #[test]
    fn everything_else_sorts_case_insensitively() {
        // So Xml does not come before archive, which a plain byte comparison
        // would do and which reads as the list being unsorted.
        let mut data = LicenseData::new();
        data.add_license(Entry::new(&["app"], "app"));
        data.add_license(Entry::new(&["Xml"], "x"));
        data.add_license(Entry::new(&["archive"], "a"));
        data.add_license(Entry::new(&["Zip"], "z"));
        data.sort_packages();
        assert_eq!(
            data.packages(),
            &[
                "app".to_string(),
                "archive".to_string(),
                "Xml".to_string(),
                "Zip".to_string()
            ]
        );
    }

    #[test]
    fn a_package_named_twice_is_still_one_package() {
        let mut data = LicenseData::new();
        data.add_license(Entry::new(&["archive"], "BSD"));
        data.add_license(Entry::new(&["archive"], "MIT"));
        assert_eq!(data.packages().len(), 1);
        assert_eq!(data.bindings_for("archive"), Some(&[0usize, 1][..]));
        assert_eq!(data.first_package(), Some("archive"));
    }

    #[test]
    fn collecting_the_registry_binds_and_sorts_in_one_go() {
        let data = LicensePage::collect(vec![
            Entry::new(&["my_app"], "app"),
            Entry::new(&["archive", "xml"], "BSD"),
            Entry::new(&["Zip"], "MIT"),
        ]);
        assert_eq!(data.first_package(), Some("my_app"));
        assert_eq!(
            data.packages(),
            &[
                "my_app".to_string(),
                "archive".to_string(),
                "xml".to_string(),
                "Zip".to_string()
            ]
        );
        assert_eq!(data.licenses().len(), 3);
        assert_eq!(data.bindings_for("xml"), Some(&[1usize][..]));
    }

    #[test]
    fn an_empty_registry_is_an_empty_page_rather_than_an_error() {
        let data = LicensePage::collect(Vec::new());
        assert!(data.packages().is_empty());
        assert_eq!(data.first_package(), None);
        assert_eq!(data.bindings_for("anything"), None);
    }

    #[test]
    fn the_application_name_falls_back_to_the_executables_own_name() {
        // Upstream's comment says what it deliberately does not do: a title
        // that changes while the application runs is not tracked, because a
        // caller who needs that should pass applicationName instead.
        assert_eq!(
            default_application_name(Some("My Notes"), "/usr/bin/notes"),
            "My Notes"
        );
        assert_eq!(default_application_name(None, "/usr/bin/notes"), "notes");
        assert_eq!(
            default_application_name(None, r"C:\Program Files\Notes\notes.exe"),
            "notes.exe",
            "and it knows both kinds of separator"
        );
        assert_eq!(default_application_name(None, "notes"), "notes");
    }

    #[test]
    fn an_unknown_version_is_blank_rather_than_invented() {
        // Upstream's TODO: the version should come from the embedder and there
        // is no way to ask. A made-up version number on an About box is worse
        // than none.
        assert_eq!(default_application_version(), "");
        assert_eq!(AboutDialog::new().resolved_application_version(), "");
        assert_eq!(
            AboutDialog::new()
                .with_application_version("2.1")
                .resolved_application_version(),
            "2.1"
        );
    }

    #[test]
    fn the_tile_exists_to_open_the_dialog_and_hands_on_what_it_was_given() {
        let tile = AboutListTile::new()
            .with_application_name("Notes")
            .with_application_version("2.1")
            .with_application_legalese("(c) 2026")
            .with_dense(true);
        assert_eq!(tile.dense, Some(true));
        assert_eq!(
            tile.resolved_application_name(None, "/usr/bin/whatever"),
            "Notes"
        );

        let dialog = tile.to_dialog();
        assert_eq!(dialog.application_name.as_deref(), Some("Notes"));
        assert_eq!(dialog.application_version.as_deref(), Some("2.1"));
        assert_eq!(dialog.application_legalese.as_deref(), Some("(c) 2026"));

        // A tile that was told nothing asks the application, and so does the
        // dialog it opens.
        let plain = AboutListTile::new();
        assert_eq!(
            plain.resolved_application_name(None, "/usr/bin/notes"),
            "notes"
        );
        assert_eq!(
            plain
                .to_dialog()
                .resolved_application_name(None, "/usr/bin/notes"),
            "notes"
        );
    }

    #[test]
    fn the_page_becomes_two_panes_at_seven_hundred_and_twenty() {
        // Material's own line between a phone held in landscape and a tablet.
        assert!(!LicensePage::is_lateral(719.0));
        assert!(LicensePage::is_lateral(720.0), "the threshold is inclusive");
        assert!(LicensePage::is_lateral(1024.0));

        // And the gutter widens at the same width, not at a different one.
        assert_eq!(gutter_size(719.0), NARROW_GUTTER_SIZE);
        assert_eq!(gutter_size(720.0), WIDE_GUTTER_SIZE);
        assert_eq!(MATERIAL_GUTTER_THRESHOLD, 720.0);
    }

    #[test]
    fn the_page_carries_the_same_three_fields_the_dialog_does() {
        let page = LicensePage::new()
            .with_application_name("Notes")
            .with_application_version("2.1")
            .with_application_legalese("(c) 2026");
        assert_eq!(page.application_name.as_deref(), Some("Notes"));
        assert_eq!(page.application_version.as_deref(), Some("2.1"));
        assert_eq!(page.application_legalese.as_deref(), Some("(c) 2026"));
    }
}

#[cfg(test)]
mod licenses_page_title_tests {
    use super::{default_application_name, licenses_page_title};

    #[test]
    fn the_page_is_called_licenses_and_not_the_applications_name() {
        // The application's name goes in the body, above the licences. The
        // title says what the page is, so that two applications' licence pages
        // are the same page to a reader who has seen one before.
        assert_eq!(licenses_page_title(), "Licenses");
        assert_ne!(
            licenses_page_title(),
            default_application_name(Some("Gallery"), "/bin/gallery")
        );
    }
}
