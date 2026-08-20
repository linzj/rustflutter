//! Port of `material/icons.dart`'s `Icons` and `PlatformAdaptiveIcons`.
//!
//! Upstream's file is 29,454 lines holding 8,825 `static const IconData`
//! declarations, all of it between `// BEGIN GENERATED ICONS` and its matching
//! end marker, with `// Generated code: do not hand-edit.` above. Copying
//! nine thousand codepoints across would add nothing a reader could not get
//! from the font itself, so [`Icons`] carries a representative handful and the
//! machinery around them, which is the part with anything to say.

/// One Material icon: upstream's `IconData`, reduced to what the lookup needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterialIcon {
    pub code_point: u32,
    pub name: &'static str,
    /// Upstream sets `matchTextDirection: true` on the icons that must flip in
    /// right-to-left text -- the arrows and the chevrons, not the letters.
    pub match_text_direction: bool,
}

/// Upstream `Icons`, declared `abstract final class` -- it cannot be extended
/// and it cannot be constructed. It is a namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Icons;

impl Icons {
    /// The count in upstream's generated block, recorded rather than reproduced.
    pub const UPSTREAM_ICON_COUNT: usize = 8825;

    pub const ARROW_BACK: MaterialIcon = MaterialIcon {
        code_point: 0xe093,
        name: "arrow_back",
        match_text_direction: true,
    };
    pub const ARROW_BACK_IOS: MaterialIcon = MaterialIcon {
        code_point: 0xe094,
        name: "arrow_back_ios",
        match_text_direction: true,
    };
    pub const ARROW_FORWARD: MaterialIcon = MaterialIcon {
        code_point: 0xe09f,
        name: "arrow_forward",
        match_text_direction: true,
    };
    pub const ARROW_FORWARD_IOS: MaterialIcon = MaterialIcon {
        code_point: 0xe0a0,
        name: "arrow_forward_ios",
        match_text_direction: true,
    };
    pub const SHARE: MaterialIcon = MaterialIcon {
        code_point: 0xe80d,
        name: "share",
        match_text_direction: false,
    };
    pub const IOS_SHARE: MaterialIcon = MaterialIcon {
        code_point: 0xe6b8,
        name: "ios_share",
        match_text_direction: false,
    };
    pub const MENU: MaterialIcon = MaterialIcon {
        code_point: 0xe5d2,
        name: "menu",
        match_text_direction: false,
    };
    pub const CLOSE: MaterialIcon = MaterialIcon {
        code_point: 0xe16a,
        name: "close",
        match_text_direction: false,
    };

    /// Upstream's `Icons.adaptive`, a getter returning
    /// `const PlatformAdaptiveIcons._()`.
    pub fn adaptive() -> PlatformAdaptiveIcons {
        PlatformAdaptiveIcons
    }

    /// The class doc's other requirement, which is not a Dart one at all:
    ///
    /// > To use this class, make sure you set `uses-material-design: true` in
    /// > your project's `pubspec.yaml` file in the `flutter` section. This
    /// > ensures that the Material Icons font is included in your application.
    ///
    /// **Every constant here is a codepoint in a font the build may not have
    /// shipped.** Nothing in the type system says so; the failure is a box of
    /// tofu at run time, and the only guard is a line in a YAML file.
    pub fn requires_material_design_font() -> bool {
        true
    }
}

/// The platforms `PlatformAdaptiveIcons` distinguishes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconPlatform {
    Android,
    Fuchsia,
    IOS,
    Linux,
    MacOS,
    Windows,
}

/// Upstream `PlatformAdaptiveIcons`.
///
/// Declared `final class PlatformAdaptiveIcons implements Icons`, which is a
/// curious thing to write: `Icons` has none but static members, and Dart does
/// not inherit statics, so **the `implements` promises nothing and delivers
/// nothing.** It is a name, put there so `Icons.adaptive` reads as a kind of
/// `Icons`.
///
/// The real difference is the one the shape forces. `Icons`' members are
/// `static const IconData`; every member here is an **instance getter**:
///
/// ```dart
/// IconData get arrow_back => !_isCupertino() ? Icons.arrow_back : Icons.arrow_back_ios;
/// ```
///
/// **A `const` cannot ask what platform it is running on.** So the adaptive set
/// cannot be a namespace of constants folded at compile time -- it has to be an
/// object whose members are evaluated at each access, which is what lets
/// `defaultTargetPlatform` be read when the icon is used. That is the whole
/// reason `adaptive` returns an instance rather than being another namespace,
/// and why you write `Icons.arrow_back` but `Icons.adaptive.arrow_back`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlatformAdaptiveIcons;

impl PlatformAdaptiveIcons {
    /// Upstream `_isCupertino`, written as an exhaustive switch so a new
    /// `TargetPlatform` cannot be forgotten.
    ///
    /// The line it draws is **Apple against everything else**: macOS goes with
    /// iOS, and Linux and Windows go with Android and Fuchsia.
    ///
    /// Worth setting beside `MaterialScrollBehavior` from the previous tick,
    /// which cuts the same six platforms in a different place -- there the three
    /// desktops stood together against the three touch ones. **Neither split is
    /// "the platform split"; each file draws the line its own question needs**,
    /// and an icon's question is which visual language a user expects, while a
    /// scrollbar's is whether there is a cursor to grab it with.
    pub fn is_cupertino(platform: IconPlatform) -> bool {
        match platform {
            IconPlatform::Android
            | IconPlatform::Fuchsia
            | IconPlatform::Linux
            | IconPlatform::Windows => false,
            IconPlatform::IOS | IconPlatform::MacOS => true,
        }
    }

    pub fn arrow_back(platform: IconPlatform) -> MaterialIcon {
        if PlatformAdaptiveIcons::is_cupertino(platform) {
            Icons::ARROW_BACK_IOS
        } else {
            Icons::ARROW_BACK
        }
    }

    pub fn arrow_forward(platform: IconPlatform) -> MaterialIcon {
        if PlatformAdaptiveIcons::is_cupertino(platform) {
            Icons::ARROW_FORWARD_IOS
        } else {
            Icons::ARROW_FORWARD
        }
    }

    pub fn share(platform: IconPlatform) -> MaterialIcon {
        if PlatformAdaptiveIcons::is_cupertino(platform) {
            Icons::IOS_SHARE
        } else {
            Icons::SHARE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [IconPlatform; 6] = [
        IconPlatform::Android,
        IconPlatform::Fuchsia,
        IconPlatform::IOS,
        IconPlatform::Linux,
        IconPlatform::MacOS,
        IconPlatform::Windows,
    ];

    #[test]
    fn the_line_is_apple_against_everything_else_not_desktop_against_touch() {
        // macOS goes with iOS; Linux and Windows go with Android.
        assert!(PlatformAdaptiveIcons::is_cupertino(IconPlatform::MacOS));
        assert!(PlatformAdaptiveIcons::is_cupertino(IconPlatform::IOS));
        assert!(!PlatformAdaptiveIcons::is_cupertino(IconPlatform::Linux));
        assert!(!PlatformAdaptiveIcons::is_cupertino(IconPlatform::Windows));
        assert!(!PlatformAdaptiveIcons::is_cupertino(IconPlatform::Android));
        assert!(!PlatformAdaptiveIcons::is_cupertino(IconPlatform::Fuchsia));
    }

    #[test]
    fn the_previous_ticks_platform_split_cuts_the_same_six_in_a_different_place() {
        use crate::material_app::ScrollPlatform;
        // macOS is Apple here and desktop there; Android is neither.
        assert!(PlatformAdaptiveIcons::is_cupertino(IconPlatform::MacOS));
        assert!(ScrollPlatform::MacOS.is_desktop());

        assert!(PlatformAdaptiveIcons::is_cupertino(IconPlatform::IOS));
        assert!(
            !ScrollPlatform::IOS.is_desktop(),
            "and iOS is on opposite sides of the two lines"
        );

        assert!(!PlatformAdaptiveIcons::is_cupertino(IconPlatform::Windows));
        assert!(ScrollPlatform::Windows.is_desktop());
    }

    #[test]
    fn every_platform_gets_a_definite_answer() {
        // Upstream writes it as an exhaustive switch rather than a default.
        for platform in ALL {
            let back = PlatformAdaptiveIcons::arrow_back(platform);
            assert!(
                back == Icons::ARROW_BACK || back == Icons::ARROW_BACK_IOS,
                "{platform:?}"
            );
        }
    }

    #[test]
    fn the_adaptive_icons_differ_from_their_material_namesakes() {
        assert_eq!(
            PlatformAdaptiveIcons::share(IconPlatform::IOS),
            Icons::IOS_SHARE
        );
        assert_eq!(
            PlatformAdaptiveIcons::share(IconPlatform::Android),
            Icons::SHARE
        );
        assert_ne!(Icons::SHARE, Icons::IOS_SHARE, "two different codepoints");
    }

    #[test]
    fn the_arrows_flip_with_the_text_direction_and_the_rest_do_not() {
        assert!(Icons::ARROW_BACK.match_text_direction);
        assert!(Icons::ARROW_FORWARD.match_text_direction);
        assert!(!Icons::SHARE.match_text_direction);
        assert!(!Icons::MENU.match_text_direction);
    }

    #[test]
    fn every_icon_here_has_its_own_codepoint() {
        let icons = [
            Icons::ARROW_BACK,
            Icons::ARROW_BACK_IOS,
            Icons::ARROW_FORWARD,
            Icons::ARROW_FORWARD_IOS,
            Icons::SHARE,
            Icons::IOS_SHARE,
            Icons::MENU,
            Icons::CLOSE,
        ];
        for (index, icon) in icons.iter().enumerate() {
            for other in &icons[index + 1..] {
                assert_ne!(
                    icon.code_point, other.code_point,
                    "{} vs {}",
                    icon.name, other.name
                );
            }
        }
    }

    #[test]
    fn the_font_is_a_build_setting_rather_than_a_dependency_the_types_can_see() {
        assert!(Icons::requires_material_design_font());
        assert_eq!(Icons::UPSTREAM_ICON_COUNT, 8825);
    }

    #[test]
    fn adaptive_hands_back_an_object_because_a_const_cannot_ask_the_platform() {
        let adaptive = Icons::adaptive();
        assert_eq!(
            adaptive, PlatformAdaptiveIcons,
            "an instance, where Icons itself is a namespace"
        );
    }
}
