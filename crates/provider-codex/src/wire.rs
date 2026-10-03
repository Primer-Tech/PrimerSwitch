use crate::CodexError;
use serde::{
    Deserialize, Deserializer,
    de::{Error, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value};
use std::fmt;
use zeroize::Zeroize;
pub(crate) const MAX_FRAME: usize = 1024 * 1024;
pub(crate) struct SecretJson(pub Value);
impl SecretJson {
    pub fn parse(bytes: &[u8]) -> Result<Self, CodexError> {
        if bytes.len() > MAX_FRAME {
            return Err(CodexError::OutputLimit);
        }
        let mut decoder = serde_json::Deserializer::from_slice(bytes);
        let mut parsed =
            UniqueValue::deserialize(&mut decoder).map_err(|_| CodexError::Protocol)?;
        decoder.end().map_err(|_| CodexError::Protocol)?;
        Ok(Self(parsed.0.take()))
    }
}
struct UniqueValue(Value);
impl Drop for UniqueValue {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}
impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("JSON value")
            }
            fn visit_bool<E: Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Bool(value)))
            }
            fn visit_i64<E: Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(UniqueValue(value.into()))
            }
            fn visit_u64<E: Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(UniqueValue(value.into()))
            }
            fn visit_f64<E: Error>(self, value: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(|v| UniqueValue(Value::Number(v)))
                    .ok_or_else(|| E::custom("invalid number"))
            }
            fn visit_str<E: Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::String(value.into())))
            }
            fn visit_string<E: Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::String(value)))
            }
            fn visit_none<E: Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
            fn visit_unit<E: Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut values = SecretJson(Value::Array(Vec::new()));
                while let Some(mut value) = access.next_element::<UniqueValue>()? {
                    values
                        .0
                        .as_array_mut()
                        .ok_or_else(|| A::Error::custom("invalid array"))?
                        .push(value.0.take());
                }
                Ok(UniqueValue(values.0.take()))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut values = SecretJson(Value::Object(Map::new()));
                while let Some(key) = access.next_key::<String>()? {
                    let key = zeroize::Zeroizing::new(key);
                    let map = values
                        .0
                        .as_object_mut()
                        .ok_or_else(|| A::Error::custom("invalid object"))?;
                    if map.contains_key(key.as_str()) {
                        return Err(A::Error::custom("duplicate key"));
                    }
                    let mut value = access.next_value::<UniqueValue>()?;
                    map.insert(key.to_string(), value.0.take());
                }
                Ok(UniqueValue(values.0.take()))
            }
        }
        decoder.deserialize_any(UniqueVisitor)
    }
}
impl Drop for SecretJson {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}
pub(crate) fn wipe(value: &mut Value) {
    match value {
        Value::String(s) => s.zeroize(),
        Value::Array(a) => a.iter_mut().for_each(wipe),
        Value::Object(o) => {
            for (mut key, mut value) in std::mem::take(o) {
                key.zeroize();
                wipe(&mut value);
            }
        }
        _ => {}
    }
}
impl fmt::Debug for SecretJson {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretJson([REDACTED])")
    }
}
