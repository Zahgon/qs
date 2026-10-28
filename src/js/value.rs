// SPDX-License-Identifier: BSD-3-Clause
// Copyright (c) 2014, Nathan LaFreniere and other contributors (https://github.com/ljharb/qs/graphs/contributors)

//! JavaScript values as the library sees them: the values JSON brings in, plus dates and the native functions an
//! options object may carry. Arrays may be sparse, objects enumerate their keys in JavaScript order, and both carry
//! the marks the library keeps beside a container (array overflow and merge conflicts).

use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;

use crate::formats::{Charset, Kind};

use super::date;
use super::error::JsError;
use super::number::number_to_string;
use super::string::{array_index, JsString};

/// A JavaScript value.
#[derive(Clone, Debug, Default)]
pub enum Value {
    /// `undefined`.
    #[default]
    Undefined,
    /// `null`.
    Null,
    /// A boolean.
    Bool(bool),
    /// A number.
    Number(f64),
    /// A string.
    String(JsString),
    /// An array, possibly with holes.
    Array(Array),
    /// An ordinary object.
    Object(Object),
    /// A `Date`, by its time value (`NaN` for an invalid date).
    Date(f64),
    /// A native function an options object carries.
    Function(Function),
    /// A regular expression of one character class of literal code units (`/[;,]/`), by those code units.
    RegExp(JsString),
}

/// The decoder signature: `(str, defaultDecoder, charset, kind)`; the default decoder is passed as the second
/// argument the way the library passes it.
pub type DecoderFn = dyn Fn(&JsString, &Charset, Kind) -> Result<Value, JsError>;
/// The encoder signature: `(value, defaultEncoder, charset, kind, format)`.
pub type EncoderFn = dyn Fn(&Value, &Charset, Kind, &Value) -> Result<Value, JsError>;
/// The filter signature: `(prefix, value)`.
pub type FilterFn = dyn Fn(&JsString, &Value) -> Result<Value, JsError>;
/// The comparator signature of `Array.prototype.sort`.
pub type SortFn = dyn Fn(&Value, &Value) -> f64;
/// The date serializer signature.
pub type DateFn = dyn Fn(f64) -> Result<Value, JsError>;

/// A native function value; each kind can only be called the way the library calls that option.
#[derive(Clone)]
pub enum Function {
    /// A `decoder` option.
    Decoder(Rc<DecoderFn>),
    /// An `encoder` option.
    Encoder(Rc<EncoderFn>),
    /// A `filter` option.
    Filter(Rc<FilterFn>),
    /// A `sort` option.
    Sort(Rc<SortFn>),
    /// A `serializeDate` option.
    SerializeDate(Rc<DateFn>),
}

impl fmt::Debug for Function {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Function::Decoder(_) => "decoder",
            Function::Encoder(_) => "encoder",
            Function::Filter(_) => "filter",
            Function::Sort(_) => "sort",
            Function::SerializeDate(_) => "serializeDate",
        };
        write!(f, "[Function {kind}]")
    }
}

/// An array: its elements by index (missing indices are holes), its `length`, and any properties whose keys are
/// not array indices (an index of 2^32 - 1 or more is an ordinary property of an array).
#[derive(Clone, Debug, Default)]
pub struct Array {
    items: BTreeMap<u32, Value>,
    length: u64,
    named: Vec<(JsString, Value)>,
    /// The highest index of a container built from a merge conflict, when this array is one.
    pub conflict: Option<f64>,
}

/// The largest array index, 2^32 - 2.
const MAX_INDEX: u64 = 4_294_967_294;

impl Array {
    /// An empty array.
    pub fn new() -> Self {
        Array::default()
    }

    /// An array of the given elements, without holes.
    pub fn from_values(values: Vec<Value>) -> Self {
        let mut array = Array::new();
        for value in values {
            array.push(value);
        }
        array
    }

    /// `length`.
    pub fn len(&self) -> u64 {
        self.length
    }

    /// Whether `length` is 0.
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// The element at `index`, `None` for a hole.
    pub fn get(&self, index: u32) -> Option<&Value> {
        self.items.get(&index)
    }

    /// Whether the array has an element (not a hole) at `index`.
    pub fn has(&self, index: u32) -> bool {
        self.items.contains_key(&index)
    }

    /// `array[index] = value`, growing `length` past the index.
    pub fn set(&mut self, index: u32, value: Value) {
        self.items.insert(index, value);
        self.length = self.length.max(u64::from(index) + 1);
    }

    /// `array[n] = value` for any non-negative integer `n`: an element up to the largest array index, an ordinary
    /// property beyond it.
    pub fn set_index(&mut self, index: u64, value: Value) {
        match u32::try_from(index) {
            Ok(index) if u64::from(index) <= MAX_INDEX => self.set(index, value),
            _ => self.set_named(JsString::from(index.to_string()), value),
        }
    }

    /// Defines a property whose key is not an array index.
    pub fn set_named(&mut self, key: JsString, value: Value) {
        match self.named.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = value,
            None => self.named.push((key, value)),
        }
    }

    /// The value of a property whose key is not an array index.
    pub fn get_named(&self, key: &JsString) -> Option<&Value> {
        self.named.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// Sets `length`, as `concat` does after copying a source with trailing holes.
    pub fn set_length(&mut self, length: u64) {
        self.length = self.length.max(length);
    }

    /// `array[array.length] = value`.
    pub fn push(&mut self, value: Value) {
        self.set_index(self.length, value);
    }

    /// Takes the element at `index` out, leaving a hole.
    pub fn take(&mut self, index: u32) -> Value {
        self.items.remove(&index).unwrap_or_default()
    }

    /// The elements that are not holes, in index order, as `forEach` visits them.
    pub fn entries(&self) -> impl Iterator<Item = (u32, &Value)> {
        self.items.iter().map(|(index, value)| (*index, value))
    }

    /// Takes the elements out, in index order (holes skipped).
    pub fn into_entries(self) -> impl Iterator<Item = (u32, Value)> {
        self.items.into_iter()
    }

    /// Takes every own enumerable property out with its key, in `Object.keys` order: the elements, then the other
    /// properties.
    pub fn into_keyed_entries(self) -> Vec<(JsString, Value)> {
        let mut out: Vec<(JsString, Value)> = self
            .items
            .into_iter()
            .map(|(index, value)| (JsString::from(index.to_string()), value))
            .collect();
        out.extend(self.named);
        out
    }

    /// The keys `Object.keys` lists: the indices of the elements, then the other properties.
    pub fn keys(&self) -> Vec<JsString> {
        self.items
            .keys()
            .map(|index| JsString::from(index.to_string()))
            .chain(self.named.iter().map(|(k, _)| k.clone()))
            .collect()
    }

    /// The number of elements that are not holes.
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// Every slot from 0 to `length - 1`, `None` for holes.
    pub fn slots(&self) -> impl Iterator<Item = Option<&Value>> {
        (0..self.length).map(|index| u32::try_from(index).ok().and_then(|i| self.items.get(&i)))
    }

    /// Takes every slot from 0 to `length - 1` out, `None` for holes.
    pub fn into_slots(self) -> impl Iterator<Item = Option<Value>> {
        let mut items = self.items;
        (0..self.length).map(move |index| u32::try_from(index).ok().and_then(|i| items.remove(&i)))
    }
}

/// An ordinary object: own properties in insertion order, enumerated in JavaScript order (array index keys first,
/// in ascending numeric order, then the other keys in insertion order).
#[derive(Clone, Debug, Default)]
pub struct Object {
    entries: Vec<(JsString, Value)>,
    /// Whether the object has no prototype (`{ __proto__: null }`).
    pub null_proto: bool,
    /// The highest numeric index of an object built from an array that overflowed `arrayLimit`, when this object
    /// is one.
    pub overflow: Option<f64>,
    /// The highest index of a container built from a merge conflict, when this object is one.
    pub conflict: Option<f64>,
}

impl Object {
    /// An empty object with `Object.prototype` as its prototype.
    pub fn new() -> Self {
        Object::default()
    }

    /// An empty object with or without a prototype.
    pub fn with_proto(null_proto: bool) -> Self {
        Object {
            null_proto,
            ..Object::default()
        }
    }

    /// The value of an own property.
    pub fn get(&self, key: &JsString) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// The value of an own property named by Rust text.
    pub fn get_str(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k.eq_str(key)).map(|(_, v)| v)
    }

    /// Whether the object owns a property.
    pub fn has_own(&self, key: &JsString) -> bool {
        self.entries.iter().any(|(k, _)| k == key)
    }

    /// Whether the object owns a property named by Rust text.
    pub fn has_own_str(&self, key: &str) -> bool {
        self.get_str(key).is_some()
    }

    /// Defines an own data property, as `Object.defineProperty` (and the library's `setProperty`) do: a new key
    /// goes after the existing ones, an existing key keeps its place.
    pub fn define(&mut self, key: JsString, value: Value) {
        match self.entries.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = value,
            None => self.entries.push((key, value)),
        }
    }

    /// `object[key] = value`. On an object with a prototype, assigning `__proto__` that is not an own property goes
    /// to the inherited accessor and creates no property.
    pub fn assign(&mut self, key: JsString, value: Value) {
        if !self.null_proto && key.eq_str("__proto__") && !self.has_own(&key) {
            return;
        }
        self.define(key, value);
    }

    /// Takes an own property's value out, leaving `undefined` in its place.
    pub fn take(&mut self, key: &JsString) -> Value {
        self.entries
            .iter_mut()
            .find(|(k, _)| k == key)
            .map(|(_, v)| std::mem::take(v))
            .unwrap_or_default()
    }

    /// The keys in enumeration order, as `Object.keys`.
    pub fn keys(&self) -> Vec<JsString> {
        self.order().into_iter().map(|i| self.entries[i].0.clone()).collect()
    }

    /// The number of own properties.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the object has no own properties.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The properties in enumeration order.
    pub fn iter(&self) -> impl Iterator<Item = (&JsString, &Value)> {
        self.order()
            .into_iter()
            .map(move |i| (&self.entries[i].0, &self.entries[i].1))
    }

    /// Takes the properties out, in enumeration order.
    pub fn into_entries(self) -> Vec<(JsString, Value)> {
        let order = self.order();
        let mut slots: Vec<Option<(JsString, Value)>> = self.entries.into_iter().map(Some).collect();
        order.into_iter().filter_map(|i| slots[i].take()).collect()
    }

    fn order(&self) -> Vec<usize> {
        let mut indexed: Vec<(u32, usize)> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, (k, _))| array_index(k.units()).map(|n| (n, i)))
            .collect();
        indexed.sort_unstable();
        let named = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, (k, _))| array_index(k.units()).is_none())
            .map(|(i, _)| i);
        indexed.into_iter().map(|(_, i)| i).chain(named).collect()
    }
}

/// The own properties of `Object.prototype` in Node.js 22: the names `Object.prototype.hasOwnProperty.call(
/// Object.prototype, name)` is true for.
pub const OBJECT_PROTOTYPE_KEYS: [&str; 12] = [
    "constructor",
    "__defineGetter__",
    "__defineSetter__",
    "hasOwnProperty",
    "__lookupGetter__",
    "__lookupSetter__",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toString",
    "valueOf",
    "__proto__",
    "toLocaleString",
];

/// Whether `Object.prototype` owns a property with this key.
pub fn is_object_prototype_key(key: &JsString) -> bool {
    OBJECT_PROTOTYPE_KEYS.iter().any(|name| key.eq_str(name))
}

impl Value {
    /// A string value from Rust text.
    pub fn str(text: &str) -> Value {
        Value::String(JsString::from(text))
    }

    /// Whether the value is `undefined`.
    pub fn is_undefined(&self) -> bool {
        matches!(self, Value::Undefined)
    }

    /// Whether the value is `null`.
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Whether the value is `null` or `undefined` (`value == null`).
    pub fn is_nullish(&self) -> bool {
        matches!(self, Value::Undefined | Value::Null)
    }

    /// `Array.isArray(value)`.
    pub fn is_array(&self) -> bool {
        matches!(self, Value::Array(_))
    }

    /// The array, when the value is one.
    pub fn as_array(&self) -> Option<&Array> {
        match self {
            Value::Array(array) => Some(array),
            _ => None,
        }
    }

    /// The object, when the value is an ordinary object.
    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Value::Object(object) => Some(object),
            _ => None,
        }
    }

    /// The string, when the value is one.
    pub fn as_string(&self) -> Option<&JsString> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// The number, when the value is one.
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// `typeof value`.
    pub fn type_of(&self) -> &'static str {
        match self {
            Value::Undefined => "undefined",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Null | Value::Array(_) | Value::Object(_) | Value::Date(_) | Value::RegExp(_) => "object",
            Value::Function(_) => "function",
        }
    }

    /// Whether `typeof value` is `"object"` or `"function"` and the value is not `null`.
    pub fn is_object_like(&self) -> bool {
        matches!(
            self,
            Value::Array(_) | Value::Object(_) | Value::Date(_) | Value::Function(_) | Value::RegExp(_)
        )
    }

    /// JavaScript truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Undefined | Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => !(n.is_nan() || *n == 0.0),
            Value::String(s) => !s.is_empty(),
            Value::Array(_) | Value::Object(_) | Value::Date(_) | Value::Function(_) | Value::RegExp(_) => true,
        }
    }

    /// Whether the value is exactly `true`.
    pub fn is_true(&self) -> bool {
        matches!(self, Value::Bool(true))
    }

    /// The boolean, when the value is one.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// `String(value)`: the conversion of string concatenation, template literals, `Array.prototype.join` and
    /// property keys. Arrays join their elements with commas (holes, `null` and `undefined` give empty strings),
    /// objects are `[object Object]` and dates use `Date.prototype.toString` in UTC.
    pub fn to_js_string(&self) -> JsString {
        match self {
            Value::Undefined => JsString::from("undefined"),
            Value::Null => JsString::from("null"),
            Value::Bool(b) => JsString::from(if *b { "true" } else { "false" }),
            Value::Number(n) => JsString::from(number_to_string(*n)),
            Value::String(s) => s.clone(),
            Value::Array(array) => join(array, &JsString::from(",")),
            Value::Object(_) => JsString::from("[object Object]"),
            Value::Date(time) => JsString::from(date::to_string(*time)),
            Value::Function(_) => JsString::from("function () { [native code] }"),
            Value::RegExp(class) => {
                let mut source = JsString::from("/[");
                source.push_js(class);
                source.push_str("]/");
                source
            }
        }
    }

    /// The property `value[key]` reads for the values a request can produce: an object's own property, an
    /// array's element or `length`, a string's code unit or `length`; anything else is `undefined`.
    pub fn get_property(&self, key: &JsString) -> Value {
        match self {
            Value::Object(object) => object.get(key).cloned().unwrap_or_default(),
            Value::Array(array) => {
                if key.eq_str("length") {
                    return Value::Number(array.len() as f64);
                }
                match array_index(key.units()) {
                    Some(index) => array.get(index).cloned().unwrap_or_default(),
                    None => array.get_named(key).cloned().unwrap_or_default(),
                }
            }
            Value::String(s) => {
                if key.eq_str("length") {
                    return Value::Number(s.len() as f64);
                }
                array_index(key.units())
                    .and_then(|index| s.unit_at(index as usize))
                    .map(|unit| Value::String(JsString::from_unit(unit)))
                    .unwrap_or_default()
            }
            _ => Value::Undefined,
        }
    }
}

/// `Array.prototype.join(separator)`: holes, `null` and `undefined` become empty strings, every other element
/// `String(element)`.
pub fn join(array: &Array, separator: &JsString) -> JsString {
    let mut out = JsString::new();
    for (i, slot) in array.slots().enumerate() {
        if i > 0 {
            out.push_js(separator);
        }
        match slot {
            None | Some(Value::Undefined) | Some(Value::Null) => {}
            Some(value) => out.push_js(&value.to_js_string()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_follow_javascript_order() {
        let mut object = Object::new();
        for key in ["b", "10", "4294967295", "9", "a", "4294967294"] {
            object.define(JsString::from(key), Value::Null);
        }
        let keys: Vec<String> = object.keys().iter().map(JsString::to_string_lossy).collect();
        assert_eq!(keys, ["9", "10", "4294967294", "b", "4294967295", "a"]);
    }

    #[test]
    fn assigning_proto_creates_no_property() {
        let mut object = Object::new();
        object.assign(JsString::from("__proto__"), Value::Bool(true));
        assert!(object.is_empty());
        let mut bare = Object::with_proto(true);
        bare.assign(JsString::from("__proto__"), Value::Bool(true));
        assert_eq!(bare.len(), 1);
    }

    #[test]
    fn converts_to_string() {
        let mut array = Array::from_values(vec![Value::Number(1.0), Value::Null, Value::str("x")]);
        array.set(4, Value::Number(-0.0));
        assert_eq!(Value::Array(array).to_js_string(), JsString::from("1,,x,,0"));
    }

    #[test]
    fn reads_properties() {
        let text = Value::str("abc");
        assert_eq!(text.get_property(&JsString::from("length")).as_number(), Some(3.0));
        assert_eq!(
            text.get_property(&JsString::from("1")).as_string(),
            Some(&JsString::from("b"))
        );
    }
}
