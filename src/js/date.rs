// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2014, Nathan LaFreniere and other contributors (https://github.com/ljharb/qs/graphs/contributors)

//! The `Date` behaviour the library reaches: `new Date(number)`, `getTime`, `toISOString`, `toUTCString` and
//! `toString`, all in UTC (the command runs with the UTC time zone).

use super::error::JsError;

/// Milliseconds in a day.
const MS_PER_DAY: f64 = 86_400_000.0;
/// The largest time value a `Date` can hold, in either direction.
const MAX_TIME: f64 = 8.64e15;

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `TimeClip`: the time value `new Date(number)` stores (`NaN` when out of range, otherwise truncated toward zero,
/// with negative zero made positive).
pub fn time_clip(time: f64) -> f64 {
    if !time.is_finite() || time.abs() > MAX_TIME {
        return f64::NAN;
    }
    time.trunc() + 0.0
}

/// The broken-down UTC fields of a valid time value.
struct Fields {
    year: i64,
    month: usize,
    day: i64,
    weekday: usize,
    hours: i64,
    minutes: i64,
    seconds: i64,
    millis: i64,
}

fn fields(time: f64) -> Fields {
    // A valid time value is an integer of at most 8.64e15, exactly representable as i64.
    let time = time as i64;
    let day_ms = MS_PER_DAY as i64;
    let days = time.div_euclid(day_ms);
    let in_day = time.rem_euclid(day_ms);
    let (year, month, day) = civil_from_days(days);
    Fields {
        year,
        month,
        day,
        weekday: (days + 4).rem_euclid(7) as usize,
        hours: in_day / 3_600_000,
        minutes: in_day / 60_000 % 60,
        seconds: in_day / 1000 % 60,
        millis: in_day % 1000,
    }
}

/// The proleptic Gregorian date of a day number (days since 1970-01-01).
fn civil_from_days(days: i64) -> (i64, usize, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, (month - 1) as usize, day)
}

/// `Date.prototype.toISOString`; a `RangeError` for an invalid date.
pub fn to_iso_string(time: f64) -> Result<String, JsError> {
    if time.is_nan() {
        return Err(JsError::new("RangeError", "Invalid time value"));
    }
    let f = fields(time);
    let year = if (0..=9999).contains(&f.year) {
        format!("{:04}", f.year)
    } else if f.year < 0 {
        format!("-{:06}", -f.year)
    } else {
        format!("+{:06}", f.year)
    };
    Ok(format!(
        "{year}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        f.month + 1,
        f.day,
        f.hours,
        f.minutes,
        f.seconds,
        f.millis
    ))
}

/// V8 prints years with at least four digits, and negative years with a sign and at least four digits.
fn year_text(year: i64) -> String {
    if year < 0 {
        format!("-{:04}", -year)
    } else {
        format!("{year:04}")
    }
}

/// `Date.prototype.toUTCString`.
pub fn to_utc_string(time: f64) -> String {
    if time.is_nan() {
        return "Invalid Date".to_string();
    }
    let f = fields(time);
    format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
        WEEKDAYS[f.weekday],
        f.day,
        MONTHS[f.month],
        year_text(f.year),
        f.hours,
        f.minutes,
        f.seconds
    )
}

/// `Date.prototype.toString` in the UTC time zone.
pub fn to_string(time: f64) -> String {
    if time.is_nan() {
        return "Invalid Date".to_string();
    }
    let f = fields(time);
    format!(
        "{} {} {:02} {} {:02}:{:02}:{:02} GMT+0000 (Coordinated Universal Time)",
        WEEKDAYS[f.weekday],
        MONTHS[f.month],
        f.day,
        year_text(f.year),
        f.hours,
        f.minutes,
        f.seconds
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_v8() {
        assert_eq!(to_iso_string(0.0).ok().as_deref(), Some("1970-01-01T00:00:00.000Z"));
        assert_eq!(
            to_iso_string(-62_198_755_200_001.0).ok().as_deref(),
            Some("-000002-12-31T23:59:59.999Z")
        );
        assert_eq!(
            to_iso_string(253_402_300_800_000.0).ok().as_deref(),
            Some("+010000-01-01T00:00:00.000Z")
        );
        assert_eq!(to_utc_string(-62_198_755_200_000.0), "Fri, 01 Jan -0001 00:00:00 GMT");
        assert_eq!(to_utc_string(-8.64e15), "Tue, 20 Apr -271821 00:00:00 GMT");
        assert_eq!(
            to_string(0.0),
            "Thu Jan 01 1970 00:00:00 GMT+0000 (Coordinated Universal Time)"
        );
        assert!(to_iso_string(f64::NAN).is_err());
        assert!(time_clip(8.64e15 + 1.0).is_nan());
        assert_eq!(time_clip(-0.5).to_bits(), 0.0f64.to_bits());
    }
}
