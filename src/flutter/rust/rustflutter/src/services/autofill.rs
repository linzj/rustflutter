// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Telling the operating system what a field is for (upstream
//! `services/autofill.dart`).
//!
//! An operating system that remembers an address can only offer it to a field
//! that says it wants an address. That is the whole of autofill: a field
//! names what it holds, gives itself an identifier stable across restarts,
//! and the platform does the rest.
//!
//! # Recorded divergences
//!
//! * Upstream's `AutofillScopeMixin.attach` wraps the triggering field's
//!   configuration in a private `_AutofillScopeTextInputConfiguration` that
//!   adds a `fields` list. That private class is not a public class of
//!   upstream's; the wrapping is
//!   [`AutofillScope::configuration_with_fields`] here, which is the same
//!   JSON by another route.
//! * `AutofillClient` and `AutofillScope` are traits rather than abstract
//!   classes, and `AutofillScopeMixin` is a blanket implementation of the
//!   part upstream puts in the mixin -- which is what a Dart mixin over an
//!   interface is.

use crate::services::codec::Value;
use crate::services::text_input::{TextEditingValue, TextInputConfiguration};

/// Upstream `AutofillHints`: the names a field can give for what it holds.
///
/// Every one of these is a string the platform matches on, so a typo is a
/// field the operating system silently declines to fill -- which is why the
/// table is generated from upstream rather than typed out.
pub struct AutofillHints;

impl AutofillHints {
    pub const ADDRESS_CITY: &'static str = "addressCity";
    pub const ADDRESS_CITY_AND_STATE: &'static str = "addressCityAndState";
    pub const ADDRESS_STATE: &'static str = "addressState";
    pub const BIRTHDAY: &'static str = "birthday";
    pub const BIRTHDAY_DAY: &'static str = "birthdayDay";
    pub const BIRTHDAY_MONTH: &'static str = "birthdayMonth";
    pub const BIRTHDAY_YEAR: &'static str = "birthdayYear";
    pub const COUNTRY_CODE: &'static str = "countryCode";
    pub const COUNTRY_NAME: &'static str = "countryName";
    pub const CREDIT_CARD_EXPIRATION_DATE: &'static str = "creditCardExpirationDate";
    pub const CREDIT_CARD_EXPIRATION_DAY: &'static str = "creditCardExpirationDay";
    pub const CREDIT_CARD_EXPIRATION_MONTH: &'static str = "creditCardExpirationMonth";
    pub const CREDIT_CARD_EXPIRATION_YEAR: &'static str = "creditCardExpirationYear";
    pub const CREDIT_CARD_FAMILY_NAME: &'static str = "creditCardFamilyName";
    pub const CREDIT_CARD_GIVEN_NAME: &'static str = "creditCardGivenName";
    pub const CREDIT_CARD_MIDDLE_NAME: &'static str = "creditCardMiddleName";
    pub const CREDIT_CARD_NAME: &'static str = "creditCardName";
    pub const CREDIT_CARD_NUMBER: &'static str = "creditCardNumber";
    pub const CREDIT_CARD_SECURITY_CODE: &'static str = "creditCardSecurityCode";
    pub const CREDIT_CARD_TYPE: &'static str = "creditCardType";
    pub const EMAIL: &'static str = "email";
    pub const FAMILY_NAME: &'static str = "familyName";
    pub const FULL_STREET_ADDRESS: &'static str = "fullStreetAddress";
    pub const GENDER: &'static str = "gender";
    pub const GIVEN_NAME: &'static str = "givenName";
    pub const IMPP: &'static str = "impp";
    pub const JOB_TITLE: &'static str = "jobTitle";
    pub const LANGUAGE: &'static str = "language";
    pub const LOCATION: &'static str = "location";
    pub const MIDDLE_INITIAL: &'static str = "middleInitial";
    pub const MIDDLE_NAME: &'static str = "middleName";
    pub const NAME: &'static str = "name";
    pub const NAME_PREFIX: &'static str = "namePrefix";
    pub const NAME_SUFFIX: &'static str = "nameSuffix";
    pub const NEW_PASSWORD: &'static str = "newPassword";
    pub const NEW_USERNAME: &'static str = "newUsername";
    pub const NICKNAME: &'static str = "nickname";
    pub const ONE_TIME_CODE: &'static str = "oneTimeCode";
    pub const EMAIL_O_T_P_CODE: &'static str = "emailOTPCode";
    pub const ORGANIZATION_NAME: &'static str = "organizationName";
    pub const PASSWORD: &'static str = "password";
    pub const PHOTO: &'static str = "photo";
    pub const POSTAL_ADDRESS: &'static str = "postalAddress";
    pub const POSTAL_ADDRESS_EXTENDED: &'static str = "postalAddressExtended";
    pub const POSTAL_ADDRESS_EXTENDED_POSTAL_CODE: &'static str = "postalAddressExtendedPostalCode";
    pub const POSTAL_CODE: &'static str = "postalCode";
    pub const STREET_ADDRESS_LEVEL1: &'static str = "streetAddressLevel1";
    pub const STREET_ADDRESS_LEVEL2: &'static str = "streetAddressLevel2";
    pub const STREET_ADDRESS_LEVEL3: &'static str = "streetAddressLevel3";
    pub const STREET_ADDRESS_LEVEL4: &'static str = "streetAddressLevel4";
    pub const STREET_ADDRESS_LINE1: &'static str = "streetAddressLine1";
    pub const STREET_ADDRESS_LINE2: &'static str = "streetAddressLine2";
    pub const STREET_ADDRESS_LINE3: &'static str = "streetAddressLine3";
    pub const SUBLOCALITY: &'static str = "sublocality";
    pub const TELEPHONE_NUMBER: &'static str = "telephoneNumber";
    pub const TELEPHONE_NUMBER_AREA_CODE: &'static str = "telephoneNumberAreaCode";
    pub const TELEPHONE_NUMBER_COUNTRY_CODE: &'static str = "telephoneNumberCountryCode";
    pub const TELEPHONE_NUMBER_DEVICE: &'static str = "telephoneNumberDevice";
    pub const TELEPHONE_NUMBER_EXTENSION: &'static str = "telephoneNumberExtension";
    pub const TELEPHONE_NUMBER_LOCAL: &'static str = "telephoneNumberLocal";
    pub const TELEPHONE_NUMBER_LOCAL_PREFIX: &'static str = "telephoneNumberLocalPrefix";
    pub const TELEPHONE_NUMBER_LOCAL_SUFFIX: &'static str = "telephoneNumberLocalSuffix";
    pub const TELEPHONE_NUMBER_NATIONAL: &'static str = "telephoneNumberNational";
    pub const TRANSACTION_AMOUNT: &'static str = "transactionAmount";
    pub const TRANSACTION_CURRENCY: &'static str = "transactionCurrency";
    pub const URL: &'static str = "url";
    pub const USERNAME: &'static str = "username";

    /// Every hint upstream defines, for a caller that wants to check one
    /// it was handed.
    pub const ALL: [&'static str; 67] = [
        AutofillHints::ADDRESS_CITY,
        AutofillHints::ADDRESS_CITY_AND_STATE,
        AutofillHints::ADDRESS_STATE,
        AutofillHints::BIRTHDAY,
        AutofillHints::BIRTHDAY_DAY,
        AutofillHints::BIRTHDAY_MONTH,
        AutofillHints::BIRTHDAY_YEAR,
        AutofillHints::COUNTRY_CODE,
        AutofillHints::COUNTRY_NAME,
        AutofillHints::CREDIT_CARD_EXPIRATION_DATE,
        AutofillHints::CREDIT_CARD_EXPIRATION_DAY,
        AutofillHints::CREDIT_CARD_EXPIRATION_MONTH,
        AutofillHints::CREDIT_CARD_EXPIRATION_YEAR,
        AutofillHints::CREDIT_CARD_FAMILY_NAME,
        AutofillHints::CREDIT_CARD_GIVEN_NAME,
        AutofillHints::CREDIT_CARD_MIDDLE_NAME,
        AutofillHints::CREDIT_CARD_NAME,
        AutofillHints::CREDIT_CARD_NUMBER,
        AutofillHints::CREDIT_CARD_SECURITY_CODE,
        AutofillHints::CREDIT_CARD_TYPE,
        AutofillHints::EMAIL,
        AutofillHints::FAMILY_NAME,
        AutofillHints::FULL_STREET_ADDRESS,
        AutofillHints::GENDER,
        AutofillHints::GIVEN_NAME,
        AutofillHints::IMPP,
        AutofillHints::JOB_TITLE,
        AutofillHints::LANGUAGE,
        AutofillHints::LOCATION,
        AutofillHints::MIDDLE_INITIAL,
        AutofillHints::MIDDLE_NAME,
        AutofillHints::NAME,
        AutofillHints::NAME_PREFIX,
        AutofillHints::NAME_SUFFIX,
        AutofillHints::NEW_PASSWORD,
        AutofillHints::NEW_USERNAME,
        AutofillHints::NICKNAME,
        AutofillHints::ONE_TIME_CODE,
        AutofillHints::EMAIL_O_T_P_CODE,
        AutofillHints::ORGANIZATION_NAME,
        AutofillHints::PASSWORD,
        AutofillHints::PHOTO,
        AutofillHints::POSTAL_ADDRESS,
        AutofillHints::POSTAL_ADDRESS_EXTENDED,
        AutofillHints::POSTAL_ADDRESS_EXTENDED_POSTAL_CODE,
        AutofillHints::POSTAL_CODE,
        AutofillHints::STREET_ADDRESS_LEVEL1,
        AutofillHints::STREET_ADDRESS_LEVEL2,
        AutofillHints::STREET_ADDRESS_LEVEL3,
        AutofillHints::STREET_ADDRESS_LEVEL4,
        AutofillHints::STREET_ADDRESS_LINE1,
        AutofillHints::STREET_ADDRESS_LINE2,
        AutofillHints::STREET_ADDRESS_LINE3,
        AutofillHints::SUBLOCALITY,
        AutofillHints::TELEPHONE_NUMBER,
        AutofillHints::TELEPHONE_NUMBER_AREA_CODE,
        AutofillHints::TELEPHONE_NUMBER_COUNTRY_CODE,
        AutofillHints::TELEPHONE_NUMBER_DEVICE,
        AutofillHints::TELEPHONE_NUMBER_EXTENSION,
        AutofillHints::TELEPHONE_NUMBER_LOCAL,
        AutofillHints::TELEPHONE_NUMBER_LOCAL_PREFIX,
        AutofillHints::TELEPHONE_NUMBER_LOCAL_SUFFIX,
        AutofillHints::TELEPHONE_NUMBER_NATIONAL,
        AutofillHints::TRANSACTION_AMOUNT,
        AutofillHints::TRANSACTION_CURRENCY,
        AutofillHints::URL,
        AutofillHints::USERNAME,
    ];
}

/// Upstream `AutofillConfiguration`: what one field tells the platform about
/// itself.
#[derive(Clone, Debug, PartialEq)]
pub struct AutofillConfiguration {
    /// Upstream's `enabled`. A disabled configuration is
    /// [`AutofillConfiguration::DISABLED`], and the only thing that reads it
    /// is `to_value`, which answers nothing at all for one.
    pub enabled: bool,
    /// What identifies this field to the platform across restarts. Upstream
    /// is emphatic that it has to be stable: an identifier that changes is a
    /// field the platform has never seen before, every time.
    pub unique_identifier: String,
    /// What the field holds, from [`AutofillHints`].
    pub autofill_hints: Vec<String>,
    pub current_editing_value: TextEditingValue,
    /// A hint for the platform's own UI, where it has one.
    pub hint_text: Option<String>,
}

impl AutofillConfiguration {
    /// Upstream's public constructor, which is `enabled: true` and nothing
    /// else -- there is no way to build a disabled one but
    /// [`AutofillConfiguration::DISABLED`], because a field that has
    /// something to say says it.
    pub fn new(
        unique_identifier: impl Into<String>,
        autofill_hints: Vec<String>,
        current_editing_value: TextEditingValue,
    ) -> AutofillConfiguration {
        AutofillConfiguration {
            enabled: true,
            unique_identifier: unique_identifier.into(),
            autofill_hints,
            current_editing_value,
            hint_text: None,
        }
    }

    /// Upstream's `AutofillConfiguration.disabled`.
    pub fn disabled() -> AutofillConfiguration {
        AutofillConfiguration {
            enabled: false,
            unique_identifier: String::new(),
            autofill_hints: Vec::new(),
            current_editing_value: TextEditingValue::new(""),
            hint_text: None,
        }
    }

    pub fn with_hint_text(mut self, hint_text: impl Into<String>) -> Self {
        self.hint_text = Some(hint_text.into());
        self
    }

    /// Upstream `toJson`, which answers null for a disabled configuration --
    /// the field is left out of the message rather than sent as switched off.
    pub fn to_value(&self) -> Option<Value> {
        if !self.enabled {
            return None;
        }
        let mut fields = vec![
            (
                Value::String("uniqueIdentifier".to_string()),
                Value::String(self.unique_identifier.clone()),
            ),
            (
                Value::String("hints".to_string()),
                Value::List(
                    self.autofill_hints
                        .iter()
                        .map(|hint| Value::String(hint.clone()))
                        .collect(),
                ),
            ),
            (
                Value::String("editingValue".to_string()),
                self.current_editing_value.to_state(),
            ),
        ];
        // Upstream's `'hintText': ?hintText` leaves the key out entirely when
        // there is none, rather than sending a null.
        if let Some(hint_text) = &self.hint_text {
            fields.push((
                Value::String("hintText".to_string()),
                Value::String(hint_text.clone()),
            ));
        }
        Some(Value::Map(fields))
    }
}

impl Default for AutofillConfiguration {
    fn default() -> AutofillConfiguration {
        AutofillConfiguration::disabled()
    }
}

/// Upstream `AutofillClient`: one field the platform can fill.
pub trait AutofillClient {
    /// Upstream `autofillId`.
    fn autofill_id(&self) -> String;

    /// Upstream `textInputConfiguration`.
    fn text_input_configuration(&self) -> TextInputConfiguration;

    /// Upstream `autofill`: the platform filled this field in.
    fn autofill(&self, new_editing_value: TextEditingValue);
}

/// Upstream `AutofillScope`: a group of fields the platform fills together.
///
/// A login form is one scope and two fields, and that is the point: the
/// platform is told about both at once, so that picking a saved account fills
/// the username *and* the password rather than offering them separately.
pub trait AutofillScope {
    /// Upstream `getAutofillClient`.
    fn autofill_client(&self, autofill_id: &str) -> Option<&dyn AutofillClient>;

    /// Upstream `autofillClients`.
    fn autofill_clients(&self) -> Vec<&dyn AutofillClient>;

    /// Upstream's private `_AutofillScopeTextInputConfiguration`: the
    /// triggering field's own configuration, plus every field in the scope
    /// under a `fields` key.
    ///
    /// This is what makes a scope a scope on the wire. Sending only the
    /// field that was tapped gets that field filled and leaves the rest of
    /// the form empty.
    fn configuration_with_fields(&self, current: &TextInputConfiguration) -> Value {
        let mut value = current.to_value();
        let fields = Value::List(
            self.autofill_clients()
                .iter()
                .map(|client| client.text_input_configuration().to_value())
                .collect(),
        );
        if let Value::Map(pairs) = &mut value {
            pairs.push((Value::String("fields".to_string()), fields));
        }
        value
    }
}

/// Upstream `AutofillScopeMixin`.
///
/// A Dart mixin over an interface is a default implementation of part of it,
/// which in Rust is a blanket implementation -- so every [`AutofillScope`]
/// has this, exactly as every class mixing upstream's in does.
pub trait AutofillScopeMixin: AutofillScope {
    /// Upstream's `attach`, less the connection itself: upstream asserts that
    /// every client in the scope has autofill enabled, and this is that
    /// check, returned rather than asserted.
    ///
    /// Upstream's assert is a debug-only guard, and the thing it guards
    /// against is real: one field in a form with autofill switched off makes
    /// the platform's saved account fill the others and skip that one, which
    /// looks like a bug in the form rather than in the field.
    fn every_client_is_enabled(&self) -> bool {
        self.autofill_clients().iter().all(|client| {
            client
                .text_input_configuration()
                .autofill_configuration
                .enabled
        })
    }
}

impl<T: AutofillScope + ?Sized> AutofillScopeMixin for T {}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(id: &str, hints: &[&str]) -> AutofillConfiguration {
        AutofillConfiguration::new(
            id,
            hints.iter().map(|hint| hint.to_string()).collect(),
            TextEditingValue::new(""),
        )
    }

    fn key<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
        let Value::Map(pairs) = value else {
            return None;
        };
        pairs
            .iter()
            .find(|(field, _)| matches!(field, Value::String(field) if field == name))
            .map(|(_, value)| value)
    }

    #[test]
    fn every_hint_is_the_string_the_platform_matches_on() {
        // The table is generated from upstream because a typo here is a field
        // the operating system silently declines to fill -- nothing errors,
        // the offer just never appears.
        // A tripwire on the array, not a check that the sixty-seven are the
        // right sixty-seven: that is `tools/wire_strings.py`, which looks each
        // value up in upstream's own sources.
        assert_eq!(AutofillHints::ALL.len(), 67);
        assert_eq!(AutofillHints::EMAIL, "email");
        assert_eq!(AutofillHints::PASSWORD, "password");
        assert_eq!(AutofillHints::ONE_TIME_CODE, "oneTimeCode");
        assert_eq!(AutofillHints::CREDIT_CARD_NUMBER, "creditCardNumber");
        // Screaming snake in Rust, camel on the wire: the constant's name is
        // this crate's convention and the value is the platform's.
        assert!(
            AutofillHints::ALL
                .iter()
                .all(|hint| !hint.contains('_') && !hint.is_empty())
        );
        // No duplicates: two constants with the same value would mean one of
        // them was transcribed wrong.
        let mut sorted = AutofillHints::ALL.to_vec();
        sorted.sort_unstable();
        let count = sorted.len();
        sorted.dedup();
        assert_eq!(sorted.len(), count);
    }

    #[test]
    fn a_disabled_configuration_is_left_out_of_the_message_entirely() {
        // Upstream's `toJson` answers null rather than sending `enabled:
        // false`, and the field is then absent from the configuration. A port
        // that sent it switched off would be telling the platform about a
        // field that does not want to be told about.
        assert_eq!(AutofillConfiguration::disabled().to_value(), None);
        assert!(field("id", &[AutofillHints::EMAIL]).to_value().is_some());
        // And that is the default, so a field that has not said anything says
        // nothing.
        assert_eq!(AutofillConfiguration::default().to_value(), None);
    }

    #[test]
    fn the_hint_text_key_is_absent_rather_than_null_when_there_is_none() {
        // Upstream's `'hintText': ?hintText` drops the key. Sending a null
        // and dropping the key are different messages, and the platform reads
        // them differently.
        let plain = field("id", &[AutofillHints::EMAIL])
            .to_value()
            .expect("json");
        assert_eq!(key(&plain, "hintText"), None);

        let hinted = field("id", &[AutofillHints::EMAIL])
            .with_hint_text("Work email")
            .to_value()
            .expect("json");
        assert_eq!(
            key(&hinted, "hintText"),
            Some(&Value::String("Work email".to_string()))
        );
    }

    #[test]
    fn the_configuration_carries_the_identifier_the_hints_and_the_value() {
        let value = field("username-field", &[AutofillHints::USERNAME])
            .to_value()
            .expect("json");
        assert_eq!(
            key(&value, "uniqueIdentifier"),
            Some(&Value::String("username-field".to_string()))
        );
        assert_eq!(
            key(&value, "hints"),
            Some(&Value::List(vec![Value::String("username".to_string())]))
        );
        assert!(key(&value, "editingValue").is_some());
    }

    /// A login form: two fields, one scope.
    struct Field {
        id: &'static str,
        hint: &'static str,
        enabled: bool,
    }

    impl AutofillClient for Field {
        fn autofill_id(&self) -> String {
            self.id.to_string()
        }

        fn text_input_configuration(&self) -> TextInputConfiguration {
            TextInputConfiguration {
                autofill_configuration: if self.enabled {
                    AutofillConfiguration::new(
                        self.id,
                        vec![self.hint.to_string()],
                        TextEditingValue::new(""),
                    )
                } else {
                    AutofillConfiguration::disabled()
                },
                ..TextInputConfiguration::default()
            }
        }

        fn autofill(&self, _new_editing_value: TextEditingValue) {}
    }

    struct LoginForm {
        fields: Vec<Field>,
    }

    impl AutofillScope for LoginForm {
        fn autofill_client(&self, autofill_id: &str) -> Option<&dyn AutofillClient> {
            self.fields
                .iter()
                .find(|field| field.id == autofill_id)
                .map(|field| field as &dyn AutofillClient)
        }

        fn autofill_clients(&self) -> Vec<&dyn AutofillClient> {
            self.fields
                .iter()
                .map(|field| field as &dyn AutofillClient)
                .collect()
        }
    }

    fn login_form(enabled: bool) -> LoginForm {
        LoginForm {
            fields: vec![
                Field {
                    id: "username",
                    hint: AutofillHints::USERNAME,
                    enabled: true,
                },
                Field {
                    id: "password",
                    hint: AutofillHints::PASSWORD,
                    enabled,
                },
            ],
        }
    }

    #[test]
    fn a_scope_sends_every_field_and_not_only_the_one_that_was_tapped() {
        // This is what makes a scope a scope on the wire. Sending only the
        // tapped field gets that field filled and leaves the rest of the form
        // empty, which is the failure the whole class exists to prevent.
        let form = login_form(true);
        let tapped = form
            .autofill_client("username")
            .expect("a field")
            .text_input_configuration();
        let value = form.configuration_with_fields(&tapped);
        let Some(Value::List(fields)) = key(&value, "fields") else {
            panic!("a scope's configuration carries the other fields");
        };
        assert_eq!(fields.len(), 2);
        // And the message is still the tapped field's own configuration
        // otherwise -- `fields` is added, not substituted.
        assert!(key(&value, "inputType").is_some());
    }

    #[test]
    fn a_field_with_autofill_switched_off_makes_the_whole_scope_wrong() {
        // Upstream asserts this in `attach`. The thing it guards against is
        // real: one disabled field in a form means the platform's saved
        // account fills the others and skips that one, which looks like a bug
        // in the form rather than in the field.
        assert!(login_form(true).every_client_is_enabled());
        assert!(!login_form(false).every_client_is_enabled());
    }

    #[test]
    fn a_scope_can_find_the_client_it_was_asked_for_and_no_other() {
        let form = login_form(true);
        assert_eq!(
            form.autofill_client("password").map(|c| c.autofill_id()),
            Some("password".to_string())
        );
        assert!(form.autofill_client("not-a-field").is_none());
        assert_eq!(form.autofill_clients().len(), 2);
    }
}
