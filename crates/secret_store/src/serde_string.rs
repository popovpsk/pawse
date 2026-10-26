use serde::ser::Error as _;
use serde::{Deserialize, Deserializer, Serializer};

pub fn serialize<S: Serializer>(value: &str, serializer: S) -> Result<S::Ok, S::Error> {
    let sealed = crate::conceal(value).map_err(S::Error::custom)?;
    serializer.serialize_str(&sealed)
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    String::deserialize(deserializer).map(crate::reveal)
}
