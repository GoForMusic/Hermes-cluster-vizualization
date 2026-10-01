//! `google.protobuf.Struct` <-> JSON. Node meta is free-form on the wire; the browser wants plain JSON.

use prost_types::{ListValue, Struct, Value, value::Kind};
use serde_json::{Map, Number, Value as Json};

/// A JSON object as a `Struct`. Anything that is not an object becomes an empty one.
pub fn struct_from_json(json: Json) -> Struct {
    match json {
        Json::Object(map) => Struct {
            fields: map
                .into_iter()
                .map(|(k, v)| (k, value_from_json(v)))
                .collect(),
        },
        _ => Struct::default(),
    }
}

pub fn json_from_struct(s: &Struct) -> Json {
    Json::Object(
        s.fields
            .iter()
            .map(|(k, v)| (k.clone(), json_from_value(v)))
            .collect::<Map<_, _>>(),
    )
}

pub fn value_from_json(json: Json) -> Value {
    let kind = match json {
        Json::Null => Kind::NullValue(0),
        Json::Bool(b) => Kind::BoolValue(b),
        Json::Number(n) => Kind::NumberValue(n.as_f64().unwrap_or_default()),
        Json::String(s) => Kind::StringValue(s),
        Json::Array(items) => Kind::ListValue(ListValue {
            values: items.into_iter().map(value_from_json).collect(),
        }),
        Json::Object(_) => Kind::StructValue(struct_from_json(json)),
    };
    Value { kind: Some(kind) }
}

pub fn json_from_value(value: &Value) -> Json {
    match &value.kind {
        None | Some(Kind::NullValue(_)) => Json::Null,
        Some(Kind::BoolValue(b)) => Json::Bool(*b),
        Some(Kind::NumberValue(n)) => number(*n),
        Some(Kind::StringValue(s)) => Json::String(s.clone()),
        Some(Kind::ListValue(list)) => {
            Json::Array(list.values.iter().map(json_from_value).collect())
        }
        Some(Kind::StructValue(s)) => json_from_struct(s),
    }
}

/// Protobuf has one number type, a double. A whole number goes back to the browser as `3`, not `3.0`.
fn number(n: f64) -> Json {
    if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 {
        return Json::Number(Number::from(n as i64));
    }
    Number::from_f64(n).map_or(Json::Null, Json::Number) // NaN and infinity have no JSON form
}

#[cfg(test)]
#[path = "../tests/unit/value.rs"]
mod tests;
