// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/rally/data.dart` (flutter/gallery @ d12640d),
//! upstream's hard-coded `AccountData`/`BillData`/`BudgetData` tables and
//! their sums.
//!
//! Upstream's `DummyDataService` methods take a `BuildContext` to reach the
//! localizations and the locale-aware formatters; the port is English-only
//! (PORTING.md), so they read the English catalogue directly and the
//! formatters in [`super::formatters`] are plain functions. `DateTime.utc`
//! values are the framework's [`Date`], which rolls over out-of-range days
//! the way Dart's constructor does (2019-02-29 becomes 2019-03-01).

use rustflutter::pickers::Date;

use crate::data::icons::IconData;
use crate::l10n::gallery_localizations::GalleryLocalizations;

use super::formatters;
use super::icons as rally_icons;

/// Upstream's `sumAccountDataPrimaryAmount`.
pub fn sum_account_data_primary_amount(items: &[AccountData]) -> f64 {
    sum_of(items, |item| item.primary_amount)
}

/// Upstream's `sumBillDataPrimaryAmount`.
pub fn sum_bill_data_primary_amount(items: &[BillData]) -> f64 {
    sum_of(items, |item| item.primary_amount)
}

/// Upstream's `sumBillDataPaidAmount`.
pub fn sum_bill_data_paid_amount(items: &[BillData]) -> f64 {
    let paid: Vec<&BillData> = items.iter().filter(|item| item.is_paid).collect();
    sum_of(&paid, |item| item.primary_amount)
}

/// Upstream's `sumBudgetDataPrimaryAmount`.
pub fn sum_budget_data_primary_amount(items: &[BudgetData]) -> f64 {
    sum_of(items, |item| item.primary_amount)
}

/// Upstream's `sumBudgetDataAmountUsed`.
pub fn sum_budget_data_amount_used(items: &[BudgetData]) -> f64 {
    sum_of(items, |item| item.amount_used)
}

/// Upstream's `sumOf`.
pub fn sum_of<T>(list: &[T], get_value: impl Fn(&T) -> f64) -> f64 {
    let mut sum = 0.0;
    for item in list {
        sum += get_value(item);
    }
    sum
}

/// Upstream's `AccountData`. The `primary_amount` is the balance in USD.
#[derive(Clone, Debug, PartialEq)]
pub struct AccountData {
    pub name: String,
    pub primary_amount: f64,
    pub account_number: String,
}

/// Upstream's `BillData`. The `primary_amount` is the amount due in USD.
#[derive(Clone, Debug, PartialEq)]
pub struct BillData {
    pub name: String,
    pub primary_amount: f64,
    /// The due date, already run through
    /// [`formatters::date_format_abbreviated_month_day`] the way upstream
    /// stores the formatted string.
    pub due_date: String,
    pub is_paid: bool,
}

/// Upstream's `BudgetData`. The `primary_amount` is the budget cap in USD.
#[derive(Clone, Debug, PartialEq)]
pub struct BudgetData {
    pub name: String,
    pub primary_amount: f64,
    pub amount_used: f64,
}

/// Upstream's `AlertData`.
#[derive(Clone, Debug, PartialEq)]
pub struct AlertData {
    pub message: String,
    pub icon: IconData,
}

/// Upstream's `DetailedEventData`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DetailedEventData {
    pub title: &'static str,
    pub date: Date,
    pub amount: f64,
}

/// Upstream's `UserDetailData`.
#[derive(Clone, Debug, PartialEq)]
pub struct UserDetailData {
    pub title: String,
    pub value: String,
}

/// Upstream's `DummyDataService`.
///
/// In a real app, this might be replaced with some asynchronous service.
pub struct DummyDataService;

impl DummyDataService {
    /// Upstream's `getAccountDataList`.
    pub fn account_data_list() -> Vec<AccountData> {
        let localizations = GalleryLocalizations::en();
        vec![
            AccountData {
                name: localizations.rally_account_data_checking().to_string(),
                primary_amount: 2215.13,
                account_number: "1234561234".to_string(),
            },
            AccountData {
                name: localizations.rally_account_data_home_savings().to_string(),
                primary_amount: 8678.88,
                account_number: "8888885678".to_string(),
            },
            AccountData {
                name: localizations.rally_account_data_car_savings().to_string(),
                primary_amount: 987.48,
                account_number: "8888889012".to_string(),
            },
            AccountData {
                name: localizations.rally_account_data_vacation().to_string(),
                primary_amount: 253.0,
                account_number: "1231233456".to_string(),
            },
        ]
    }

    /// Upstream's `getAccountDetailList`.
    pub fn account_detail_list() -> Vec<UserDetailData> {
        let localizations = GalleryLocalizations::en();
        vec![
            UserDetailData {
                title: localizations
                    .rally_account_detail_data_annual_percentage_yield()
                    .to_string(),
                value: formatters::percent_format(0.001, 2),
            },
            UserDetailData {
                title: localizations
                    .rally_account_detail_data_interest_rate()
                    .to_string(),
                value: formatters::usd_with_sign_format(1676.14, 2),
            },
            UserDetailData {
                title: localizations
                    .rally_account_detail_data_interest_ytd()
                    .to_string(),
                value: formatters::usd_with_sign_format(81.45, 2),
            },
            UserDetailData {
                title: localizations
                    .rally_account_detail_data_interest_paid_last_year()
                    .to_string(),
                value: formatters::usd_with_sign_format(987.12, 2),
            },
            UserDetailData {
                title: localizations
                    .rally_account_detail_data_next_statement()
                    .to_string(),
                value: formatters::short_date_format(Date::new(2019, 12, 25)),
            },
            UserDetailData {
                title: localizations
                    .rally_account_detail_data_account_owner()
                    .to_string(),
                value: "Philip Cao".to_string(),
            },
        ]
    }

    /// Upstream's `getDetailedEventItems`. The titles are not localized
    /// upstream either -- they are product/brand names.
    pub fn detailed_event_items() -> Vec<DetailedEventData> {
        vec![
            DetailedEventData {
                title: "Genoe",
                date: Date::new(2019, 1, 24),
                amount: -16.54,
            },
            DetailedEventData {
                title: "Fortnightly Subscribe",
                date: Date::new(2019, 1, 5),
                amount: -12.54,
            },
            DetailedEventData {
                title: "Circle Cash",
                date: Date::new(2019, 1, 5),
                amount: 365.65,
            },
            DetailedEventData {
                title: "Crane Hospitality",
                date: Date::new(2019, 1, 4),
                amount: -705.13,
            },
            DetailedEventData {
                title: "ABC Payroll",
                date: Date::new(2018, 12, 15),
                amount: 1141.43,
            },
            DetailedEventData {
                title: "Shrine",
                date: Date::new(2018, 12, 15),
                amount: -88.88,
            },
            DetailedEventData {
                title: "Foodmates",
                date: Date::new(2018, 12, 4),
                amount: -11.69,
            },
        ]
    }

    /// Upstream's `getBillDataList`. The names are not localized upstream
    /// either -- they are product/brand names.
    pub fn bill_data_list() -> Vec<BillData> {
        vec![
            BillData {
                name: "RedPay Credit".to_string(),
                primary_amount: 45.36,
                due_date: formatters::date_format_abbreviated_month_day(Date::new(2019, 1, 29)),
                is_paid: false,
            },
            BillData {
                name: "Rent".to_string(),
                primary_amount: 1200.0,
                due_date: formatters::date_format_abbreviated_month_day(Date::new(2019, 2, 9)),
                is_paid: true,
            },
            BillData {
                name: "TabFine Credit".to_string(),
                primary_amount: 87.33,
                due_date: formatters::date_format_abbreviated_month_day(Date::new(2019, 2, 22)),
                is_paid: false,
            },
            BillData {
                name: "ABC Loans".to_string(),
                primary_amount: 400.0,
                // Upstream's `DateTime.utc(2019, 2, 29)`: 2019 is not a leap
                // year, and both constructors roll the overflow over.
                due_date: formatters::date_format_abbreviated_month_day(Date::new(2019, 2, 29)),
                is_paid: false,
            },
        ]
    }

    /// Upstream's `getBillDetailList`.
    pub fn bill_detail_list(due_total: f64, paid_total: f64) -> Vec<UserDetailData> {
        let localizations = GalleryLocalizations::en();
        vec![
            UserDetailData {
                title: localizations.rally_bill_detail_total_amount().to_string(),
                value: formatters::usd_with_sign_format(paid_total + due_total, 2),
            },
            UserDetailData {
                title: localizations.rally_bill_detail_amount_paid().to_string(),
                value: formatters::usd_with_sign_format(paid_total, 2),
            },
            UserDetailData {
                title: localizations.rally_bill_detail_amount_due().to_string(),
                value: formatters::usd_with_sign_format(due_total, 2),
            },
        ]
    }

    /// Upstream's `getBudgetDataList`.
    pub fn budget_data_list() -> Vec<BudgetData> {
        let localizations = GalleryLocalizations::en();
        vec![
            BudgetData {
                name: localizations
                    .rally_budget_category_coffee_shops()
                    .to_string(),
                primary_amount: 70.0,
                amount_used: 45.49,
            },
            BudgetData {
                name: localizations.rally_budget_category_groceries().to_string(),
                primary_amount: 170.0,
                amount_used: 16.45,
            },
            BudgetData {
                name: localizations
                    .rally_budget_category_restaurants()
                    .to_string(),
                primary_amount: 170.0,
                amount_used: 123.25,
            },
            BudgetData {
                name: localizations.rally_budget_category_clothing().to_string(),
                primary_amount: 70.0,
                amount_used: 19.45,
            },
        ]
    }

    /// Upstream's `getBudgetDetailList`.
    pub fn budget_detail_list(cap_total: f64, used_total: f64) -> Vec<UserDetailData> {
        let localizations = GalleryLocalizations::en();
        vec![
            UserDetailData {
                title: localizations.rally_budget_detail_total_cap().to_string(),
                value: formatters::usd_with_sign_format(cap_total, 2),
            },
            UserDetailData {
                title: localizations.rally_budget_detail_amount_used().to_string(),
                value: formatters::usd_with_sign_format(used_total, 2),
            },
            UserDetailData {
                title: localizations.rally_budget_detail_amount_left().to_string(),
                value: formatters::usd_with_sign_format(cap_total - used_total, 2),
            },
        ]
    }

    /// Upstream's `getSettingsTitles`.
    pub fn settings_titles() -> Vec<String> {
        let localizations = GalleryLocalizations::en();
        vec![
            localizations.rally_settings_manage_accounts().to_string(),
            localizations.rally_settings_tax_documents().to_string(),
            localizations
                .rally_settings_passcode_and_touch_id()
                .to_string(),
            localizations.rally_settings_notifications().to_string(),
            localizations
                .rally_settings_personal_information()
                .to_string(),
            localizations
                .rally_settings_paperless_settings()
                .to_string(),
            localizations.rally_settings_find_atms().to_string(),
            localizations.rally_settings_help().to_string(),
            localizations.rally_settings_sign_out().to_string(),
        ]
    }

    /// Upstream's `getAlerts`.
    pub fn alerts() -> Vec<AlertData> {
        let localizations = GalleryLocalizations::en();
        vec![
            AlertData {
                message: localizations
                    .rally_alerts_message_heads_up_shopping(formatters::percent_format(0.9, 0)),
                icon: rally_icons::SORT,
            },
            AlertData {
                message: localizations.rally_alerts_message_spent_on_restaurants(
                    formatters::usd_with_sign_format(120.0, 0),
                ),
                icon: rally_icons::SORT,
            },
            AlertData {
                message: localizations
                    .rally_alerts_message_atm_fees(formatters::usd_with_sign_format(24.0, 0)),
                icon: rally_icons::CREDIT_CARD,
            },
            AlertData {
                message: localizations
                    .rally_alerts_message_checking_account(formatters::percent_format(0.04, 0)),
                icon: rally_icons::ATTACH_MONEY,
            },
            AlertData {
                message: localizations.rally_alerts_message_unassigned_transactions(16),
                icon: rally_icons::NOT_INTERESTED,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_are_upstream() {
        let accounts = DummyDataService::account_data_list();
        assert_eq!(accounts.len(), 4);
        assert_eq!(accounts[0].name, "Checking");
        assert_eq!(accounts[0].primary_amount, 2215.13);
        assert_eq!(accounts[0].account_number, "1234561234");
        assert_eq!(accounts[1].name, "Home Savings");
        assert_eq!(accounts[1].primary_amount, 8678.88);
        assert_eq!(accounts[2].name, "Car Savings");
        assert_eq!(accounts[2].primary_amount, 987.48);
        assert_eq!(accounts[3].name, "Vacation");
        assert_eq!(accounts[3].primary_amount, 253.0);

        let bills = DummyDataService::bill_data_list();
        assert_eq!(bills.len(), 4);
        assert_eq!(bills[0].name, "RedPay Credit");
        assert_eq!(bills[0].due_date, "Jan 29");
        assert!(!bills[0].is_paid);
        assert_eq!(bills[1].name, "Rent");
        assert_eq!(bills[1].due_date, "Feb 9");
        assert!(bills[1].is_paid);
        assert_eq!(bills[2].name, "TabFine Credit");
        assert_eq!(bills[2].due_date, "Feb 22");
        // Dart's `DateTime.utc(2019, 2, 29)` rolls over to March 1, 2019.
        assert_eq!(bills[3].due_date, "Mar 1");

        let budgets = DummyDataService::budget_data_list();
        assert_eq!(budgets.len(), 4);
        assert_eq!(budgets[0].name, "Coffee Shops");
        assert_eq!(budgets[0].primary_amount, 70.0);
        assert_eq!(budgets[0].amount_used, 45.49);
        assert_eq!(budgets[2].name, "Restaurants");
        assert_eq!(budgets[2].amount_used, 123.25);
    }

    #[test]
    fn the_sums_are_of_the_primary_amounts() {
        let accounts = DummyDataService::account_data_list();
        let total = sum_account_data_primary_amount(&accounts);
        assert!((total - (2215.13 + 8678.88 + 987.48 + 253.0)).abs() < 1e-9);

        let bills = DummyDataService::bill_data_list();
        let due = sum_bill_data_primary_amount(&bills);
        assert!((due - (45.36 + 1200.0 + 87.33 + 400.0)).abs() < 1e-9);
        // Only the paid bills count towards the paid total.
        assert_eq!(sum_bill_data_paid_amount(&bills), 1200.0);

        let budgets = DummyDataService::budget_data_list();
        assert_eq!(sum_budget_data_primary_amount(&budgets), 480.0);
        assert!(
            (sum_budget_data_amount_used(&budgets) - (45.49 + 16.45 + 123.25 + 19.45)).abs() < 1e-9
        );
    }

    #[test]
    fn the_details_are_formatted_english() {
        let details = DummyDataService::account_detail_list();
        assert_eq!(details[0].title, "Annual Percentage Yield");
        assert_eq!(details[0].value, "0.10%");
        assert_eq!(details[1].value, "$1,676.14");
        assert_eq!(details[4].value, "12/25/2019");
        assert_eq!(details[5].value, "Philip Cao");

        let bills = DummyDataService::bill_detail_list(1732.69, 1200.0);
        assert_eq!(bills[0].value, "$2,932.69");
        assert_eq!(bills[1].value, "$1,200.00");
        assert_eq!(bills[2].value, "$1,732.69");
    }

    #[test]
    fn the_events_and_alerts_are_upstream() {
        let events = DummyDataService::detailed_event_items();
        assert_eq!(events.len(), 7);
        assert_eq!(events[0].title, "Genoe");
        assert_eq!(events[0].amount, -16.54);
        assert_eq!(events[4].title, "ABC Payroll");
        assert_eq!(events[4].date, Date::new(2018, 12, 15));

        let alerts = DummyDataService::alerts();
        assert_eq!(alerts.len(), 5);
        assert_eq!(
            alerts[0].message,
            "Heads up, you've used up 90% of your Shopping budget for this month."
        );
        assert_eq!(
            alerts[1].message,
            "You've spent $120 on Restaurants this week."
        );
        assert_eq!(
            alerts[4].message,
            "Increase your potential tax deduction! Assign categories to 16 unassigned transactions."
        );
    }

    #[test]
    fn the_settings_titles_are_upstream() {
        let titles = DummyDataService::settings_titles();
        assert_eq!(titles.len(), 9);
        assert_eq!(titles[0], "Manage Accounts");
        assert_eq!(titles[8], "Sign out");
    }
}
