// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2014, Nathan LaFreniere and other contributors (https://github.com/ljharb/qs/graphs/contributors)

//! JavaScript number conversions: `Number.prototype.toString()`, `parseInt`, `ToNumber`, `ToUint32` and
//! `ToUint16`.

use super::string::is_js_whitespace;

/// `Number::toString(x)` in radix 10: the shortest decimal that round-trips, in fixed notation for exponents from
/// -7 to 20 and in exponential notation (`1e+21`, `1.5e-7`) otherwise. Negative zero is `"0"`.
pub fn number_to_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value == 0.0 {
        return "0".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    let sign = if value < 0.0 { "-" } else { "" };
    let (digits, exponent) = shortest_digits(value.abs());
    let k = digits.len() as i32;
    // The decimal point sits after `n` digits: value = 0.digits * 10^n.
    let n = exponent + 1;
    let body = if k <= n && n <= 21 {
        let mut s = digits.clone();
        s.extend(std::iter::repeat_n('0', (n - k) as usize));
        s
    } else if 0 < n && n <= 21 {
        let (int, frac) = digits.split_at(n as usize);
        format!("{int}.{frac}")
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let exp = if e >= 0 { format!("e+{e}") } else { format!("e-{}", -e) };
        if k == 1 {
            format!("{digits}{exp}")
        } else {
            let (first, rest) = digits.split_at(1);
            format!("{first}.{rest}{exp}")
        }
    };
    format!("{sign}{body}")
}

/// The shortest round-trip decimal digits of a positive finite number and the exponent of its first digit. When the
/// number lies exactly halfway between two shortest candidates, the even one is chosen, as the specification of
/// `Number::toString` requires.
fn shortest_digits(value: f64) -> (String, i32) {
    let (digits, exponent) = split_exponential(&format!("{value:e}"));
    match even_tie_digits(value, &digits, exponent) {
        Some(even) => (even, exponent),
        None => (digits, exponent),
    }
}

/// The significant digits (trailing zeros removed) and the exponent of a number formatted with `{:e}`.
fn split_exponential(formatted: &str) -> (String, i32) {
    let (mantissa, exponent) = formatted.split_once('e').unwrap_or((formatted, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    // `{:e}` always writes an integer exponent after the `e`.
    (digits.to_string(), exponent.parse().unwrap_or(0))
}

/// The even neighbour of `digits` when `value` is exactly the midpoint between `digits` and that neighbour and the
/// neighbour round-trips too. Ties need more decimal digits than a double has bits of spacing, so only long digit
/// strings are checked.
fn even_tie_digits(value: f64, digits: &str, exponent: i32) -> Option<String> {
    let last = digits.bytes().last()? - b'0';
    if digits.len() < 15 || last.is_multiple_of(2) {
        return None;
    }
    // Every finite double has a terminating decimal expansion of at most 767 significant digits.
    let (exact, exact_exponent) = split_exponential(&format!("{value:.1100e}"));
    if exact_exponent != exponent || exact.len() != digits.len() + 1 || !exact.ends_with('5') {
        return None;
    }
    let low = &exact[..digits.len()];
    let candidate = if low == digits {
        increment(low)?
    } else {
        low.to_string()
    };
    if candidate.len() != digits.len() || !(candidate.as_bytes()[candidate.len() - 1] - b'0').is_multiple_of(2) {
        return None;
    }
    let text = format!("{}.{}e{exponent}", &candidate[..1], &candidate[1..]);
    (text.parse::<f64>() == Ok(value)).then_some(candidate)
}

/// A decimal digit string plus one in its last place, or `None` when that carries into a new digit.
fn increment(digits: &str) -> Option<String> {
    let mut bytes = digits.as_bytes().to_vec();
    for byte in bytes.iter_mut().rev() {
        if *byte == b'9' {
            *byte = b'0';
        } else {
            *byte += 1;
            return String::from_utf8(bytes).ok();
        }
    }
    None
}

/// `parseInt(text, 10)`: leading white space is skipped, then an optional sign and the longest run of decimal
/// digits; `NaN` when there is no digit. The digits are rounded to the nearest double, so long runs lose precision
/// exactly as in JavaScript.
pub fn parse_int(text: &[u16]) -> f64 {
    let mut i = 0;
    while i < text.len() && is_js_whitespace(text[i]) {
        i += 1;
    }
    let mut negative = false;
    if let Some(&sign) = text.get(i) {
        if sign == u16::from(b'-') || sign == u16::from(b'+') {
            negative = sign == u16::from(b'-');
            i += 1;
        }
    }
    let digits: String = text[i..]
        .iter()
        .take_while(|&&unit| (0x30..=0x39).contains(&unit))
        .map(|&unit| char::from(unit as u8))
        .collect();
    if digits.is_empty() {
        return f64::NAN;
    }
    let magnitude: f64 = digits.parse().unwrap_or(f64::INFINITY);
    if negative {
        -magnitude
    } else {
        magnitude
    }
}

/// `ToNumber` of a string (`StringToNumber`): surrounding white space is ignored, the empty string is 0, and the
/// text must be a decimal literal, `Infinity` with an optional sign, or a `0x`, `0o` or `0b` integer.
pub fn string_to_number(text: &[u16]) -> f64 {
    let start = text.iter().position(|&u| !is_js_whitespace(u)).unwrap_or(text.len());
    let end = text
        .iter()
        .rposition(|&u| !is_js_whitespace(u))
        .map_or(start, |i| i + 1);
    let trimmed = &text[start..end.max(start)];
    if trimmed.is_empty() {
        return 0.0;
    }
    if trimmed.iter().any(|&u| u > 0x7F) {
        return f64::NAN;
    }
    let ascii: String = trimmed.iter().map(|&u| char::from(u as u8)).collect();
    let radix = match ascii.get(..2) {
        Some("0x") | Some("0X") => 16,
        Some("0o") | Some("0O") => 8,
        Some("0b") | Some("0B") => 2,
        _ => 10,
    };
    if radix != 10 {
        let digits = &ascii[2..];
        if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
            return f64::NAN;
        }
        return digits.chars().fold(0.0, |acc, c| {
            acc * f64::from(radix) + f64::from(c.to_digit(radix).unwrap_or(0))
        });
    }
    let (sign, body) = match ascii.as_bytes()[0] {
        b'-' => (-1.0, &ascii[1..]),
        b'+' => (1.0, &ascii[1..]),
        _ => (1.0, ascii.as_str()),
    };
    if body == "Infinity" {
        return sign * f64::INFINITY;
    }
    if !is_decimal_literal(body) {
        return f64::NAN;
    }
    // The literal was validated above, so Rust's parser accepts it, rounding to the nearest double as JavaScript does.
    sign * body.parse::<f64>().unwrap_or(f64::NAN)
}

/// Whether text is a `StrUnsignedDecimalLiteral`: digits with an optional fraction (at least one digit in all) and
/// an optional exponent.
fn is_decimal_literal(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    let int_digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    i += int_digits;
    let mut frac_digits = 0;
    if bytes.get(i) == Some(&b'.') {
        i += 1;
        frac_digits = bytes[i..].iter().take_while(|b| b.is_ascii_digit()).count();
        i += frac_digits;
    }
    if int_digits + frac_digits == 0 {
        return false;
    }
    if matches!(bytes.get(i), Some(b'e') | Some(b'E')) {
        i += 1;
        if matches!(bytes.get(i), Some(b'+') | Some(b'-')) {
            i += 1;
        }
        let exp_digits = bytes[i..].iter().take_while(|b| b.is_ascii_digit()).count();
        if exp_digits == 0 {
            return false;
        }
        i += exp_digits;
    }
    i == bytes.len()
}

/// The integer part of a finite number reduced modulo 2^bits, as `ToUint32` and `ToUint16` compute it; `NaN` and
/// the infinities give 0.
fn to_uint(value: f64, bits: i32) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    let modulus = 2f64.powi(bits);
    // `%` on doubles is exact, so even integers far beyond 2^53 reduce correctly.
    let reduced = value.trunc() % modulus;
    if reduced < 0.0 {
        reduced + modulus
    } else {
        reduced + 0.0
    }
}

/// `ToUint32`.
pub fn to_uint32(value: f64) -> u32 {
    to_uint(value, 32) as u32
}

/// `ToUint16`, as `String.fromCharCode` applies it.
pub fn to_uint16(value: f64) -> u16 {
    to_uint(value, 16) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_javascript() {
        let cases = [
            (0.0, "0"),
            (-0.0, "0"),
            (1.5, "1.5"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (0.000001, "0.000001"),
            (1e-7, "1e-7"),
            (5e-324, "5e-324"),
        ];
        for (value, text) in cases {
            assert_eq!(number_to_string(value), text);
        }
    }

    #[test]
    fn converts_like_javascript() {
        let units = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        assert_eq!(parse_int(&units(" \u{a0}-12abc")), -12.0);
        assert!(parse_int(&units("x1")).is_nan());
        assert_eq!(parse_int(&units("100000000000000000001")), 1e20);
        assert_eq!(string_to_number(&units(" 0x1F ")), 31.0);
        assert_eq!(string_to_number(&units("")), 0.0);
        assert!(string_to_number(&units("1e")).is_nan());
        assert!(string_to_number(&units("inf")).is_nan());
        assert_eq!(string_to_number(&units("-Infinity")), f64::NEG_INFINITY);
        assert_eq!(to_uint32(-1.0), u32::MAX);
        assert_eq!(to_uint32(2.5), 2);
        assert_eq!(to_uint16(128_512.0), 0xF600);
        assert_eq!(to_uint16(1e20), 0);
    }
}
