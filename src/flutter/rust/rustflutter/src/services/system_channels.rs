// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The channels the engine listens on (upstream
//! `services/system_channels.dart`), and the three small services that are
//! nothing but a call on one of them (`services/scribe.dart`,
//! `services/sensitive_content.dart`, `services/deferred_component.dart`).
//!
//! # Recorded divergences
//!
//! * Upstream's `SystemChannels` holds built channel objects, each with its
//!   codec. Here it holds the names: a channel in this crate is cheap to
//!   build and the codec is chosen where the call is made, so a table of
//!   pre-built channels would be a table of things nobody could use without
//!   knowing which codec each carried anyway. The names are what has to be
//!   exact, and they are what is generated.
//! * Every call upstream is a `Future`; they are callbacks here, for the
//!   reason recorded on [`spell_check`](crate::services::spell_check).

use crate::services::channel::MethodChannel;
use crate::services::codec::{JsonMethodCodec, StandardMethodCodec, Value};

/// Upstream `SystemChannels`: the names the engine answers on.
///
/// Generated from upstream, because a typo in one of these is a channel
/// nobody is listening on -- nothing errors, the call simply never arrives.
pub struct SystemChannels;

impl SystemChannels {
    /// Upstream `SystemChannels.navigation`, a `MethodChannel`.
    pub const NAVIGATION: &'static str = "flutter/navigation";
    /// Upstream `SystemChannels.backGesture`, a `MethodChannel`.
    pub const BACK_GESTURE: &'static str = "flutter/backgesture";
    /// Upstream `SystemChannels.platform`, a `MethodChannel`.
    pub const PLATFORM: &'static str = "flutter/platform";
    /// Upstream `SystemChannels.statusBar`, a `OptionalMethodChannel`.
    pub const STATUS_BAR: &'static str = "flutter/status_bar";
    /// Upstream `SystemChannels.processText`, a `MethodChannel`.
    pub const PROCESS_TEXT: &'static str = "flutter/processtext";
    /// Upstream `SystemChannels.textInput`, a `MethodChannel`.
    pub const TEXT_INPUT: &'static str = "flutter/textinput";
    /// Upstream `SystemChannels.scribe`, a `MethodChannel`.
    pub const SCRIBE: &'static str = "flutter/scribe";
    /// Upstream `SystemChannels.spellCheck`, a `MethodChannel`.
    pub const SPELL_CHECK: &'static str = "flutter/spellcheck";
    /// Upstream `SystemChannels.undoManager`, a `MethodChannel`.
    pub const UNDO_MANAGER: &'static str = "flutter/undomanager";
    /// Upstream `SystemChannels.keyEvent`, a `BasicMessageChannel`.
    pub const KEY_EVENT: &'static str = "flutter/keyevent";
    /// Upstream `SystemChannels.lifecycle`, a `BasicMessageChannel`.
    pub const LIFECYCLE: &'static str = "flutter/lifecycle";
    /// Upstream `SystemChannels.system`, a `BasicMessageChannel`.
    pub const SYSTEM: &'static str = "flutter/system";
    /// Upstream `SystemChannels.accessibility`, a `BasicMessageChannel`.
    pub const ACCESSIBILITY: &'static str = "flutter/accessibility";
    /// Upstream `SystemChannels.platform_views`, a `MethodChannel`.
    pub const PLATFORM_VIEWS: &'static str = "flutter/platform_views";
    /// Upstream `SystemChannels.platform_views_2`, a `MethodChannel`.
    pub const PLATFORM_VIEWS_2: &'static str = "flutter/platform_views_2";
    /// Upstream `SystemChannels.skia`, a `MethodChannel`.
    pub const SKIA: &'static str = "flutter/skia";
    /// Upstream `SystemChannels.mouseCursor`, a `MethodChannel`.
    pub const MOUSE_CURSOR: &'static str = "flutter/mousecursor";
    /// Upstream `SystemChannels.restoration`, a `MethodChannel`.
    pub const RESTORATION: &'static str = "flutter/restoration";
    /// Upstream `SystemChannels.deferredComponent`, a `MethodChannel`.
    pub const DEFERRED_COMPONENT: &'static str = "flutter/deferredcomponent";
    /// Upstream `SystemChannels.localization`, a `MethodChannel`.
    pub const LOCALIZATION: &'static str = "flutter/localization";
    /// Upstream `SystemChannels.menu`, a `MethodChannel`.
    pub const MENU: &'static str = "flutter/menu";
    /// Upstream `SystemChannels.contextMenu`, a `MethodChannel`.
    pub const CONTEXT_MENU: &'static str = "flutter/contextmenu";
    /// Upstream `SystemChannels.keyboard`, a `MethodChannel`.
    pub const KEYBOARD: &'static str = "flutter/keyboard";
    /// Upstream `SystemChannels.sensitiveContent`, a `MethodChannel`.
    pub const SENSITIVE_CONTENT: &'static str = "flutter/sensitivecontent";

    /// Every channel upstream names.
    pub const ALL: [&'static str; 24] = [
        SystemChannels::NAVIGATION,
        SystemChannels::BACK_GESTURE,
        SystemChannels::PLATFORM,
        SystemChannels::STATUS_BAR,
        SystemChannels::PROCESS_TEXT,
        SystemChannels::TEXT_INPUT,
        SystemChannels::SCRIBE,
        SystemChannels::SPELL_CHECK,
        SystemChannels::UNDO_MANAGER,
        SystemChannels::KEY_EVENT,
        SystemChannels::LIFECYCLE,
        SystemChannels::SYSTEM,
        SystemChannels::ACCESSIBILITY,
        SystemChannels::PLATFORM_VIEWS,
        SystemChannels::PLATFORM_VIEWS_2,
        SystemChannels::SKIA,
        SystemChannels::MOUSE_CURSOR,
        SystemChannels::RESTORATION,
        SystemChannels::DEFERRED_COMPONENT,
        SystemChannels::LOCALIZATION,
        SystemChannels::MENU,
        SystemChannels::CONTEXT_MENU,
        SystemChannels::KEYBOARD,
        SystemChannels::SENSITIVE_CONTENT,
    ];
}

/// Upstream `Scribe`: stylus handwriting straight into a text field.
///
/// Android's Scribe and the iPad's Scribble are the same idea: the reader
/// writes with a pen over the field and the platform turns it into text. The
/// framework's whole part is asking whether it is there and saying when to
/// start.
pub struct Scribe;

impl Scribe {
    pub const IS_FEATURE_AVAILABLE: &'static str = "Scribe.isFeatureAvailable";
    pub const IS_STYLUS_HANDWRITING_AVAILABLE: &'static str = "Scribe.isStylusHandwritingAvailable";
    pub const START_STYLUS_HANDWRITING: &'static str = "Scribe.startStylusHandwriting";

    fn channel() -> MethodChannel<StandardMethodCodec> {
        MethodChannel::named(SystemChannels::SCRIBE, StandardMethodCodec)
    }

    /// Upstream `isFeatureAvailable`: whether the platform has the feature at
    /// all, which is a question about the device and not about the pen.
    ///
    /// Upstream throws when the platform answers null. Here that is `false`:
    /// a platform that could not say whether it has the feature does not have
    /// it, and there is nothing to throw to from a callback.
    pub fn is_feature_available(callback: impl FnOnce(bool) + 'static) {
        Scribe::ask(Scribe::IS_FEATURE_AVAILABLE, callback);
    }

    /// Upstream `isStylusHandwritingAvailable`: whether it is available
    /// *now*, which is the narrower question -- a device with the feature
    /// still says no when no pen has been near it.
    pub fn is_stylus_handwriting_available(callback: impl FnOnce(bool) + 'static) {
        Scribe::ask(Scribe::IS_STYLUS_HANDWRITING_AVAILABLE, callback);
    }

    /// Upstream `startStylusHandwriting`.
    pub fn start_stylus_handwriting() {
        Scribe::channel().invoke(Scribe::START_STYLUS_HANDWRITING, Value::Null);
    }

    fn ask(method: &str, callback: impl FnOnce(bool) + 'static) {
        Scribe::channel().invoke_with_reply(method, Value::Null, move |reply| {
            callback(matches!(reply, Ok(Some(Value::Bool(true)))));
        });
    }
}

/// Upstream `ContentSensitivity`: how careful the platform should be with
/// what is on screen.
///
/// A screen showing a password or a bank balance should not appear in the
/// task switcher's thumbnail or in a screen recording, and this is how the
/// application says so.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ContentSensitivity {
    /// Android's `CONTENT_SENSITIVITY_AUTO`: let the platform decide from the
    /// autofill hints. Upstream has this named and not yet implemented; it is
    /// here for the same reason, so that the indices line up.
    #[default]
    AutoSensitive,
    Sensitive,
    NotSensitive,
}

impl ContentSensitivity {
    /// The wire value, which is upstream's enum index -- so the order of the
    /// variants above is part of the protocol and not a matter of taste.
    pub fn index(self) -> i32 {
        match self {
            ContentSensitivity::AutoSensitive => 0,
            ContentSensitivity::Sensitive => 1,
            ContentSensitivity::NotSensitive => 2,
        }
    }

    /// The mode an index names, or nothing for upstream's `_unknown` -- which
    /// is the platform reporting a mode this version of the framework has
    /// never heard of. Upstream throws an `UnsupportedError` and says to file
    /// an issue; there is nothing to throw to here, and nothing is the same
    /// answer.
    pub fn from_index(index: i32) -> Option<ContentSensitivity> {
        match index {
            0 => Some(ContentSensitivity::AutoSensitive),
            1 => Some(ContentSensitivity::Sensitive),
            2 => Some(ContentSensitivity::NotSensitive),
            _ => None,
        }
    }
}

/// Upstream `SensitiveContentService`.
pub struct SensitiveContentService {
    channel: MethodChannel<StandardMethodCodec>,
}

impl Default for SensitiveContentService {
    fn default() -> SensitiveContentService {
        SensitiveContentService::new()
    }
}

impl SensitiveContentService {
    pub const SET_METHOD: &'static str = "SensitiveContent.setContentSensitivity";
    pub const GET_METHOD: &'static str = "SensitiveContent.getContentSensitivity";
    pub const IS_SUPPORTED_METHOD: &'static str = "SensitiveContent.isSupported";

    pub fn new() -> SensitiveContentService {
        SensitiveContentService {
            channel: MethodChannel::named(SystemChannels::SENSITIVE_CONTENT, StandardMethodCodec),
        }
    }

    /// Upstream `setContentSensitivity`.
    pub fn set_content_sensitivity(&self, sensitivity: ContentSensitivity) {
        self.channel.invoke(
            SensitiveContentService::SET_METHOD,
            Value::I32(sensitivity.index()),
        );
    }

    /// Upstream `getContentSensitivity`.
    pub fn get_content_sensitivity(
        &self,
        callback: impl FnOnce(Option<ContentSensitivity>) + 'static,
    ) {
        self.channel.invoke_with_reply(
            SensitiveContentService::GET_METHOD,
            Value::Null,
            move |reply| {
                callback(match reply {
                    Ok(Some(Value::I32(index))) => ContentSensitivity::from_index(index),
                    Ok(Some(Value::I64(index))) => ContentSensitivity::from_index(index as i32),
                    _ => None,
                });
            },
        );
    }

    /// Upstream `isSupported`.
    ///
    /// Upstream answers false off Android without asking anyone. There is no
    /// `TargetPlatform` in this crate to ask, so the question goes to the
    /// platform every time -- which is the authoritative answer in either
    /// case, and is what upstream is short-circuiting rather than replacing.
    /// A platform with no handler for the method answers nothing, which is
    /// false here.
    pub fn is_supported(&self, callback: impl FnOnce(bool) + 'static) {
        self.channel.invoke_with_reply(
            SensitiveContentService::IS_SUPPORTED_METHOD,
            Value::Null,
            move |reply| {
                callback(matches!(reply, Ok(Some(Value::Bool(true)))));
            },
        );
    }
}

/// Upstream `DeferredComponent`: asking Android to fetch part of the
/// application that was not installed with it.
pub struct DeferredComponent;

impl DeferredComponent {
    pub const INSTALL_METHOD: &'static str = "installDeferredComponent";
    pub const UNINSTALL_METHOD: &'static str = "uninstallDeferredComponent";

    fn channel() -> MethodChannel<JsonMethodCodec> {
        MethodChannel::named(SystemChannels::DEFERRED_COMPONENT, JsonMethodCodec)
    }

    /// Upstream `installDeferredComponent`.
    ///
    /// The `loadingUnitId` upstream sends is always `-1` with a comment
    /// explaining why: the Dart side cannot see loading unit ids, so the
    /// component is named instead, and the field is kept so that the API can
    /// take one later without a protocol change.
    pub fn install(component_name: &str) {
        DeferredComponent::channel().invoke(
            DeferredComponent::INSTALL_METHOD,
            DeferredComponent::arguments(component_name),
        );
    }

    /// Upstream `uninstallDeferredComponent`.
    pub fn uninstall(component_name: &str) {
        DeferredComponent::channel().invoke(
            DeferredComponent::UNINSTALL_METHOD,
            DeferredComponent::arguments(component_name),
        );
    }

    pub(crate) fn arguments(component_name: &str) -> Value {
        Value::Map(vec![
            (Value::String("loadingUnitId".to_string()), Value::I32(-1)),
            (
                Value::String("componentName".to_string()),
                Value::String(component_name.to_string()),
            ),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_channel_name_is_the_one_the_engine_answers_on() {
        // Generated from upstream because a typo here is a channel nobody is
        // listening on: nothing errors, the call simply never arrives.
        // A tripwire on the array. Whether each name is one the engine
        // actually answers on is `tools/wire_strings.py`'s question, and it
        // asks upstream rather than asking this file.
        assert_eq!(SystemChannels::ALL.len(), 24);
        assert_eq!(SystemChannels::TEXT_INPUT, "flutter/textinput");
        assert_eq!(SystemChannels::PLATFORM, "flutter/platform");
        assert_eq!(SystemChannels::SCRIBE, "flutter/scribe");
        assert_eq!(
            SystemChannels::SENSITIVE_CONTENT,
            "flutter/sensitivecontent"
        );
        assert_eq!(
            SystemChannels::DEFERRED_COMPONENT,
            "flutter/deferredcomponent"
        );
        // Every one is under the engine's prefix, and no two are the same --
        // two names alike would mean one was transcribed wrong.
        assert!(
            SystemChannels::ALL
                .iter()
                .all(|name| name.starts_with("flutter/"))
        );
        let mut sorted = SystemChannels::ALL.to_vec();
        sorted.sort_unstable();
        let count = sorted.len();
        sorted.dedup();
        assert_eq!(sorted.len(), count);
    }

    #[test]
    fn the_names_this_module_already_used_agree_with_the_generated_table() {
        // Three services were written against these names before the table
        // existed. If the two ever disagree, one of them is wrong and this is
        // where it shows.
        assert_eq!(
            crate::services::spell_check::DefaultSpellCheckService::CHANNEL,
            SystemChannels::SPELL_CHECK
        );
        assert_eq!(
            crate::services::process_text::DefaultProcessTextService::CHANNEL,
            SystemChannels::PROCESS_TEXT
        );
    }

    #[test]
    fn a_content_sensitivity_is_its_index_on_the_wire() {
        // The order of the variants is part of the protocol: the platform is
        // sent an integer and reads it as its own enum's index, so reordering
        // them here would quietly mark a password screen as safe to record.
        assert_eq!(ContentSensitivity::AutoSensitive.index(), 0);
        assert_eq!(ContentSensitivity::Sensitive.index(), 1);
        assert_eq!(ContentSensitivity::NotSensitive.index(), 2);
        for mode in [
            ContentSensitivity::AutoSensitive,
            ContentSensitivity::Sensitive,
            ContentSensitivity::NotSensitive,
        ] {
            assert_eq!(ContentSensitivity::from_index(mode.index()), Some(mode));
        }
    }

    #[test]
    fn a_mode_this_framework_has_never_heard_of_is_nothing_rather_than_a_guess() {
        // Upstream's `_unknown`: the platform has a mode this version does
        // not know, and upstream throws an `UnsupportedError` saying to file
        // an issue. Guessing the nearest known mode would be the one wrong
        // answer -- it could mark a sensitive screen as not sensitive.
        assert_eq!(ContentSensitivity::from_index(3), None);
        assert_eq!(ContentSensitivity::from_index(-1), None);
    }

    #[test]
    fn a_deferred_component_is_named_and_its_loading_unit_is_always_minus_one() {
        // Upstream's own comment: the Dart side cannot see loading unit ids,
        // so the component is named instead, and the field stays in the
        // message so a later API can carry one without a protocol change.
        let arguments = DeferredComponent::arguments("payments");
        let Value::Map(pairs) = &arguments else {
            panic!("the arguments are a map");
        };
        assert_eq!(pairs.len(), 2);
        assert_eq!(
            pairs[0],
            (Value::String("loadingUnitId".to_string()), Value::I32(-1))
        );
        assert_eq!(
            pairs[1],
            (
                Value::String("componentName".to_string()),
                Value::String("payments".to_string())
            )
        );
    }

    #[test]
    fn the_method_names_are_the_ones_the_platform_dispatches_on() {
        assert_eq!(Scribe::IS_FEATURE_AVAILABLE, "Scribe.isFeatureAvailable");
        assert_eq!(
            Scribe::IS_STYLUS_HANDWRITING_AVAILABLE,
            "Scribe.isStylusHandwritingAvailable"
        );
        assert_eq!(
            Scribe::START_STYLUS_HANDWRITING,
            "Scribe.startStylusHandwriting"
        );
        assert_eq!(
            SensitiveContentService::SET_METHOD,
            "SensitiveContent.setContentSensitivity"
        );
        assert_eq!(
            DeferredComponent::INSTALL_METHOD,
            "installDeferredComponent"
        );
    }
}
