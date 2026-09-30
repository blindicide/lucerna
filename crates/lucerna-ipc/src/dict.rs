//! `a{sv}` dictionaries.

use std::collections::HashMap;

use zbus::zvariant::{OwnedValue, Value};

/// The extensible payload type: string keys to variants.
pub type Dict = HashMap<String, OwnedValue>;

fn own(value: Value<'_>) -> OwnedValue {
    // Only fails for values holding file descriptors, which Lucerna never sends.
    OwnedValue::try_from(value).unwrap_or_else(|_| OwnedValue::from(0u8))
}

/// Builds a [`Dict`] fluently.
#[derive(Default)]
pub struct DictBuilder(Dict);

impl DictBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn str(mut self, key: &str, value: &str) -> Self {
        self.0
            .insert(key.to_owned(), own(Value::from(value.to_owned())));
        self
    }

    pub fn bool(mut self, key: &str, value: bool) -> Self {
        self.0.insert(key.to_owned(), own(Value::from(value)));
        self
    }

    pub fn u32(mut self, key: &str, value: u32) -> Self {
        self.0.insert(key.to_owned(), own(Value::from(value)));
        self
    }

    pub fn i32(mut self, key: &str, value: i32) -> Self {
        self.0.insert(key.to_owned(), own(Value::from(value)));
        self
    }

    pub fn u64(mut self, key: &str, value: u64) -> Self {
        self.0.insert(key.to_owned(), own(Value::from(value)));
        self
    }

    pub fn strings(mut self, key: &str, values: &[String]) -> Self {
        self.0
            .insert(key.to_owned(), own(Value::from(values.to_vec())));
        self
    }

    pub fn dicts(mut self, key: &str, values: Vec<Dict>) -> Self {
        self.0.insert(
            key.to_owned(),
            own(Value::Array(zbus::zvariant::Array::from(values))),
        );
        self
    }

    pub fn build(self) -> Dict {
        self.0
    }
}

/// Reads typed values from a [`Dict`], ignoring unknown keys and tolerating absent ones.
pub struct DictReader<'a>(pub &'a Dict);

impl DictReader<'_> {
    fn get(&self, key: &str) -> Option<OwnedValue> {
        self.0.get(key).and_then(|v| v.try_clone().ok())
    }

    pub fn str(&self, key: &str) -> Option<String> {
        self.get(key).and_then(|v| String::try_from(v).ok())
    }

    pub fn bool(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(|v| bool::try_from(v).ok())
    }

    pub fn u32(&self, key: &str) -> Option<u32> {
        self.get(key).and_then(|v| u32::try_from(v).ok())
    }

    pub fn i32(&self, key: &str) -> Option<i32> {
        self.get(key).and_then(|v| i32::try_from(v).ok())
    }

    pub fn u64(&self, key: &str) -> Option<u64> {
        self.get(key).and_then(|v| u64::try_from(v).ok())
    }

    pub fn strings(&self, key: &str) -> Vec<String> {
        self.get(key)
            .and_then(|v| Vec::<String>::try_from(v).ok())
            .unwrap_or_default()
    }

    pub fn dicts(&self, key: &str) -> Vec<Dict> {
        self.get(key)
            .and_then(|v| Vec::<Dict>::try_from(v).ok())
            .unwrap_or_default()
    }

    pub fn str_or_empty(&self, key: &str) -> String {
        self.str(key).unwrap_or_default()
    }

    pub fn bool_or_false(&self, key: &str) -> bool {
        self.bool(key).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_type_round_trips() {
        let inner = DictBuilder::new().str("k", "v").build();
        let d = DictBuilder::new()
            .str("s", "hello — ünïcode")
            .bool("b", true)
            .u32("u", 7)
            .i32("i", -3)
            .u64("t", 1 << 40)
            .strings("as", &["a".to_owned(), "b".to_owned()])
            .dicts("aa", vec![inner.clone(), inner])
            .build();
        let r = DictReader(&d);
        assert_eq!(r.str("s").as_deref(), Some("hello — ünïcode"));
        assert_eq!(r.bool("b"), Some(true));
        assert_eq!(r.u32("u"), Some(7));
        assert_eq!(r.i32("i"), Some(-3));
        assert_eq!(r.u64("t"), Some(1 << 40));
        assert_eq!(r.strings("as"), ["a", "b"]);
        let nested = r.dicts("aa");
        assert_eq!(nested.len(), 2);
        assert_eq!(DictReader(&nested[0]).str("k").as_deref(), Some("v"));
    }

    #[test]
    fn missing_or_mistyped_keys_read_as_none_and_empty() {
        let d = DictBuilder::new().str("s", "x").build();
        let r = DictReader(&d);
        assert_eq!(r.bool("s"), None, "wrong type");
        assert_eq!(r.u32("absent"), None);
        assert!(r.strings("absent").is_empty());
        assert!(r.dicts("s").is_empty());
        assert_eq!(r.str_or_empty("absent"), "");
        assert!(!r.bool_or_false("absent"));
    }
}
