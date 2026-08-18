// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/rally/formatters.dart` (flutter/gallery @ d12640d),
//! upstream's currency, percent and date formatters.
//!
//! Upstream builds these on `intl`'s `NumberFormat`/`DateFormat` over the
//! ambient locale. The port is English-only (PORTING.md), so each formatter
//! is the `en_US` pattern as a plain function of the value: grouping with
//! commas, `$` on the left with the sign before it, month names English.
//! Dates are the framework's [`Date`] (the UTC civil date), standing in for
//! upstream's `DateTime.utc(...)` values.

use rustflutter::pickers::Date;

/// Upstream's `usdWithSignFormat`: `NumberFormat.currency(name: '$')` in
/// `en_US` -- `$1,234.56`, and `-$16.54` for a debit.
pub fn usd_with_sign_format(amount: f64, decimal_digits: usize) -> String {
    let magnitude = grouped(amount.abs(), decimal_digits);
    if amount.is_sign_negative() && amount != 0.0 {
        format!("-${magnitude}")
    } else {
        format!("${magnitude}")
    }
}

/// Upstream's `percentFormat`: `NumberFormat.decimalPercentPattern` -- the
/// value is a fraction, so 0.001 at two digits is `0.10%`.
pub fn percent_format(value: f64, decimal_digits: usize) -> String {
    format!("{}%", grouped(value * 100.0, decimal_digits))
}

/// Upstream's `shortDateFormat`: `DateFormat.yMd` -- `12/25/2019`.
pub fn short_date_format(date: Date) -> String {
    format!("{}/{}/{}", date.month, date.day, date.year)
}

/// Upstream's `longDateFormat`: `DateFormat.yMMMMd` -- `December 25, 2019`.
pub fn long_date_format(date: Date) -> String {
    format!(
        "{} {}, {}",
        MONTH_NAMES[(date.month - 1) as usize],
        date.day,
        date.year
    )
}

/// Upstream's `dateFormatAbbreviatedMonthDay`: `DateFormat.MMMd` -- `Jan 29`.
pub fn date_format_abbreviated_month_day(date: Date) -> String {
    format!(
        "{} {}",
        MONTH_NAMES_ABBREVIATED[(date.month - 1) as usize],
        date.day
    )
}

/// Upstream's `dateFormatMonthYear`: `DateFormat.yMMM` -- `Dec 2018`.
pub fn date_format_month_year(date: Date) -> String {
    format!(
        "{} {}",
        MONTH_NAMES_ABBREVIATED[(date.month - 1) as usize],
        date.year
    )
}

const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const MONTH_NAMES_ABBREVIATED: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The absolute value rounded to `decimal_digits` with thousands grouped:
/// `1234567.891` at two digits is `1,234,567.89`.
fn grouped(value: f64, decimal_digits: usize) -> String {
    let rounded = format!("{value:.decimal_digits$}");
    let (whole, fraction) = match rounded.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (rounded.as_str(), ""),
    };
    let mut grouped = String::new();
    for (i, digit) in whole.chars().enumerate() {
        let remaining = whole.len() - i;
        if i > 0 && remaining % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    if fraction.is_empty() {
        grouped
    } else {
        format!("{grouped}.{fraction}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn currency_groups_and_signs_the_en_us_way() {
        // The values `data.rs` formats.
        assert_eq!(usd_with_sign_format(2215.13, 2), "$2,215.13");
        assert_eq!(usd_with_sign_format(1200.0, 2), "$1,200.00");
        assert_eq!(usd_with_sign_format(253.0, 2), "$253.00");
        assert_eq!(usd_with_sign_format(120.0, 0), "$120");
        assert_eq!(usd_with_sign_format(-16.54, 2), "-$16.54");
        assert_eq!(usd_with_sign_format(0.0, 2), "$0.00");
    }

    #[test]
    fn percents_are_hundredths() {
        // `NumberFormat.decimalPercentPattern`: 0.001 -> "0.10%".
        assert_eq!(percent_format(0.001, 2), "0.10%");
        assert_eq!(percent_format(0.9, 0), "90%");
        assert_eq!(percent_format(0.04, 0), "4%");
    }

    #[test]
    fn dates_follow_their_patterns() {
        let christmas = Date::new(2019, 12, 25);
        assert_eq!(short_date_format(christmas), "12/25/2019");
        assert_eq!(long_date_format(christmas), "December 25, 2019");
        assert_eq!(
            date_format_abbreviated_month_day(Date::new(2019, 1, 29)),
            "Jan 29"
        );
        assert_eq!(date_format_month_year(Date::new(2018, 12, 1)), "Dec 2018");
    }

    #[test]
    fn grouping_inserts_commas_every_thousand() {
        assert_eq!(grouped(1234567.891, 2), "1,234,567.89");
        assert_eq!(grouped(45.36, 2), "45.36");
        assert_eq!(grouped(1141.43, 2), "1,141.43");
        assert_eq!(grouped(70.0, 0), "70");
    }
}
