<p align="center">
    <img alt="qs" src="./logos/banner_default.png" width="800" />
</p>

# qs

A querystring parsing and stringifying library with some added security, ported from the
[qs](https://github.com/ljharb/qs) JavaScript library (version 6.17.1) to Rust.

Lead Maintainer of the original: [Jordan Harband](https://github.com/ljharb). The **qs** module was originally created
and maintained by [TJ Holowaychuk](https://github.com/visionmedia/node-querystring).

The crate behaves exactly like the JavaScript library, down to the details JavaScript itself decides: strings are
sequences of UTF-16 code units, objects enumerate integer-like keys first, numbers print as JavaScript prints them, and
every option is checked the way the library checks it. Values and options are therefore JavaScript values
(`qs::Value`), and the crate brings the small part of the JavaScript runtime it needs (`qs::js`): UTF-16 strings,
`JSON.parse` and `JSON.stringify`, number conversions and `Date` printing.

## Usage

```rust
use qs::js::json;
use qs::{JsError, Value};

fn main() -> Result<(), JsError> {
    let parsed = qs::parse(&Value::str("a[b]=c&a[d][]=e"), &Value::Undefined)?;
    let text = json::stringify(&parsed).map(|t| t.to_string()).unwrap_or_default();
    assert_eq!(text, r#"{"a":{"b":"c","d":["e"]}}"#);

    let options = json::parse_str(r#"{"arrayFormat":"brackets","encodeValuesOnly":true}"#)?;
    let value = json::parse_str(r#"{"a":["b","c"]}"#)?;
    assert_eq!(qs::stringify(&value, &options)?.to_string(), "a[]=b&a[]=c");
    Ok(())
}
```

### Parsing

`qs::parse(input, options)` turns a query string into nested objects and arrays: `foo[bar]=baz` becomes
`{"foo":{"bar":"baz"}}`, `a[]=b&a[]=c` and `a=b&a=c` become `{"a":["b","c"]}`, and `a[1]=c&a[0]=b` becomes
`{"a":["b","c"]}`. An input that is an object instead of a string supplies keys and values that are already split.

| option | default | meaning |
| --- | --- | --- |
| `allowDots` | `false` | read `a.b` as `a[b]` |
| `allowEmptyArrays` | `false` | `a[]` with an empty value is an empty array |
| `allowPrototypes` | `false` | keep keys that `Object.prototype` owns, such as `hasOwnProperty` |
| `allowSparse` | `false` | keep the holes of arrays (`a[1]=b&a[3]=c`) |
| `arrayLimit` | `20` | indices at or above it, and arrays that would grow past it, make objects |
| `charset` | `"utf-8"` | `"utf-8"` or `"iso-8859-1"` for percent-encoded octets |
| `charsetSentinel` | `false` | a `utf8=` parameter (`&#10003;` or `✓`) selects the charset and is dropped |
| `comma` | `false` | `a=b,c` is `{"a":["b","c"]}` |
| `decodeDotInKeys` | `false` | `%2E` in a key is a literal dot (implies `allowDots`) |
| `decoder` | | a decoder function for keys and values |
| `delimiter` | `"&"` | a string, or a character class regular expression |
| `depth` | `5` | how many bracket groups nest; the rest stays one key |
| `duplicates` | `"combine"` | `"combine"`, `"first"` or `"last"` for repeated keys |
| `ignoreQueryPrefix` | `false` | drop a leading `?` |
| `interpretNumericEntities` | `false` | decode `&#NNN;` with `iso-8859-1` |
| `parameterLimit` | `1000` | how many parameters are read |
| `parseArrays` | `true` | `false` makes every array an object |
| `plainObjects` | `false` | objects without a prototype |
| `strictDepth` | `false` | keys deeper than `depth` are a `RangeError` |
| `strictMerge` | `true` | a value given for both an object and a plain key makes a two-element array |
| `strictNullHandling` | `false` | `a` without `=` is `null` |
| `throwOnLimitExceeded` | `false` | exceeding `arrayLimit` or `parameterLimit` is a `RangeError` |

### Stringifying

`qs::stringify(value, options)` is the reverse: `{"a":{"b":"c"}}` becomes `a%5Bb%5D=c`.

| option | default | meaning |
| --- | --- | --- |
| `addQueryPrefix` | `false` | start with `?` |
| `allowDots` | `false` | nested keys as `a.b` |
| `allowEmptyArrays` | `false` | an empty array as `a[]` |
| `arrayFormat` | `"indices"` | `"indices"`, `"brackets"`, `"repeat"` or `"comma"` |
| `charset` | `"utf-8"` | `"iso-8859-1"` writes other characters as numeric entities |
| `charsetSentinel` | `false` | start with the `utf8=` sentinel of the charset |
| `commaRoundTrip` | `false` | a one-element array keeps `[]` with the comma format |
| `delimiter` | `"&"` | the separator between pairs |
| `depth` | none | nesting beyond it is a `RangeError` |
| `encode` | `true` | percent-encode keys and values |
| `encodeDotInKeys` | `false` | dots in keys as `%2E` (implies `allowDots`) |
| `encoder` | | an encoder function |
| `encodeValuesOnly` | `false` | leave keys unencoded |
| `filter` | | an array of keys to keep, or a function of each key and value |
| `format` | `"RFC3986"` | `"RFC1738"` and `"WHATWG"` write spaces as `+` |
| `indices` | | `false` is the `repeat` array format |
| `serializeDate` | `toISOString` | how dates are written |
| `skipNulls` | `false` | leave out `null` values |
| `sort` | | a comparator for the keys |
| `strictNullHandling` | `false` | `null` as a bare key |

## The `qs` command

The `qs` binary answers JSON-lines requests on standard input, one JSON response line per request on standard output:

```sh
$ printf '%s\n' '{"op":"parse","input":"a[b]=c"}' '{"op":"stringify","value":{"a":[1,2]},"options":{"arrayFormat":"brackets"}}' | qs
{"result":{"a":{"b":"c"}}}
{"result":"a%5B%5D=1&a%5B%5D=2"}
```

- `{"op":"parse","input":V,"options":O}` answers `{"result":...}` with the parsed value.
- `{"op":"stringify","value":V,"options":O}` answers `{"result":"..."}`; in `V`, `{"$date":N}` is a date and
  `{"$undefined":true}` is `undefined`.
- The function-valued options are chosen by name: the decoder and encoder `"kinds"`, the filters `"trace"` (which adds
  the calls it saw as `"calls"`), `"dropEmpty"` and `"emptyToNull"`, the sorts `"asc"`, `"desc"` and `"length"`, and
  the date serializers `"epoch"` and `"utc"`. A parse delimiter `{"regexp":"[;,]"}` is a character class.
- A failed request answers `{"error":{"name":...,"message":...}}` and makes the exit status 1. Arguments are a usage
  error (status 2).

## Building and testing

```sh
cargo build --release
cargo test
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
```

The crate has no dependencies beyond the Rust standard library.

## Security

Please email [@ljharb](https://github.com/ljharb) or see https://tidelift.com/security if you have a potential security
vulnerability to report.

## License

BSD-3-Clause, see [LICENSE.md](LICENSE.md).
