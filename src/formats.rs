// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2014, Nathan LaFreniere and other contributors (https://github.com/ljharb/qs/graphs/contributors)

//! The output formats of `stringify` and the charsets and call kinds shared by the encoder and the decoder.

use crate::js::JsString;

/// A percent-encoding format: which characters stay literal and how a space is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// RFC 1738: spaces as `+`, `(` and `)` literal.
    Rfc1738,
    /// RFC 3986, the default: spaces as `%20`.
    Rfc3986,
    /// The WHATWG URL standard's form encoding: spaces as `+`, `*` literal, `~` encoded.
    Whatwg,
}

impl Format {
    /// The format named by an option value's property key, as `formats.formatters[name]` finds it.
    pub fn from_name(name: &JsString) -> Option<Format> {
        [Format::Rfc1738, Format::Rfc3986, Format::Whatwg]
            .into_iter()
            .find(|format| name.eq_str(format.name()))
    }

    /// The format's name.
    pub fn name(self) -> &'static str {
        match self {
            Format::Rfc1738 => "RFC1738",
            Format::Rfc3986 => "RFC3986",
            Format::Whatwg => "WHATWG",
        }
    }

    /// The format's formatter, applied to every encoded key and value: `%20` becomes `+` for RFC 1738 and WHATWG.
    pub fn apply(self, value: JsString) -> JsString {
        match self {
            Format::Rfc3986 => value,
            Format::Rfc1738 | Format::Whatwg => value.replace_all("%20", "+"),
        }
    }
}

/// The charset of percent-encoded octets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Charset {
    /// UTF-8, the default.
    Utf8,
    /// ISO-8859-1.
    Iso88591,
}

impl Charset {
    /// The charset named exactly by an option value, if any.
    pub fn from_name(name: &JsString) -> Option<Charset> {
        if name.eq_str("utf-8") {
            Some(Charset::Utf8)
        } else if name.eq_str("iso-8859-1") {
            Some(Charset::Iso88591)
        } else {
            None
        }
    }

    /// The name the library passes to decoders and encoders.
    pub fn name(self) -> &'static str {
        match self {
            Charset::Utf8 => "utf-8",
            Charset::Iso88591 => "iso-8859-1",
        }
    }
}

/// Whether a decoder or encoder call is for a key or for a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A key.
    Key,
    /// A value.
    Value,
}

impl Kind {
    /// The string the library passes: `"key"` or `"value"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Key => "key",
            Kind::Value => "value",
        }
    }
}
