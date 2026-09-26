use serde::{Deserialize, Deserializer};

pub fn number<'de, D: Deserializer<'de>, T: std::str::FromStr>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Number(number) => number
            .as_i64()
            .map(|n| n.to_string())
            .or_else(|| number.as_f64().map(|f| (f.round() as i64).to_string()))
            .and_then(|text| text.parse().ok()),
        serde_json::Value::String(text) => text.trim().parse().ok(),
        _ => None,
    })
}

pub fn boolean<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Bool(flag) => flag,
        serde_json::Value::String(text) => text.trim().eq_ignore_ascii_case("true"),
        _ => false,
    })
}

pub fn opt_text<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::String(text) => Some(text),
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    })
}

pub fn text<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Ok(opt_text(deserializer)?.unwrap_or_default())
}

pub fn id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    opt_text(deserializer)?
        .filter(|id| !id.is_empty())
        .ok_or_else(|| serde::de::Error::custom("missing id"))
}

pub fn opt_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Ok(opt_text(deserializer)?.filter(|id| !id.is_empty()))
}

pub fn list<'de, D: Deserializer<'de>, T: for<'a> Deserialize<'a>>(
    deserializer: D,
) -> Result<Vec<T>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Array(items) => items
            .into_iter()
            .filter_map(|item| serde_json::from_value(item).ok())
            .collect(),
        _ => Vec::new(),
    })
}

pub fn object<'de, D: Deserializer<'de>, T: for<'a> Deserialize<'a> + Default>(
    deserializer: D,
) -> Result<T, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}
