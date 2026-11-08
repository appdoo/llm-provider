//! The schema of a typed answer as Anthropic structured outputs accept it.
//! The API takes a subset of JSON Schema: every object closed with
//! `additionalProperties: false`, no numeric, length or item-count bounds,
//! and `minItems` only 0 or 1. Checks the subset cannot carry stay the
//! caller's, after parsing.

use serde_json::{Map, Value};

/// Keywords structured outputs reject: numeric bounds, string lengths and
/// array constraints beyond `minItems` 0 or 1.
const UNSUPPORTED: [&str; 7] = [
    "minimum",
    "maximum",
    "multipleOf",
    "minLength",
    "maxLength",
    "maxItems",
    "uniqueItems",
];

/// Keywords whose value is one subschema.
const SUBSCHEMA: [&str; 3] = ["items", "not", "additionalItems"];

/// Keywords whose value is a list of subschemas.
const SUBSCHEMA_LISTS: [&str; 4] = ["anyOf", "oneOf", "allOf", "prefixItems"];

/// Keywords whose value maps names to subschemas.
const SUBSCHEMA_MAPS: [&str; 3] = ["properties", "$defs", "definitions"];

/// `schema` made conformant to the structured-outputs subset, at every
/// level: unsupported keywords removed, `minItems` above 1 lowered to 1,
/// and `additionalProperties: false` on every object.
pub(super) fn conformant(schema: &Value) -> Value {
    let mut schema = schema.clone();
    conform(&mut schema);
    schema
}

/// Makes `schema` and every subschema in it conformant, in place.
fn conform(schema: &mut Value) {
    let Value::Object(map) = schema else {
        return;
    };

    conform_keywords(map);
    for subschema in subschemas(map) {
        conform(subschema);
    }
}

/// Removes the unsupported keywords of one schema object, lowers its
/// `minItems` to at most 1 and closes it when it describes an object.
fn conform_keywords(map: &mut Map<String, Value>) {
    for keyword in UNSUPPORTED {
        map.remove(keyword);
    }

    if map.get("minItems").and_then(Value::as_u64) > Some(1) {
        map.insert("minItems".to_string(), Value::from(1));
    }

    if is_object(map) {
        map.insert("additionalProperties".to_string(), Value::Bool(false));
    }
}

/// Whether the schema describes an object: its `type` is or includes
/// `"object"`, or it lists `properties`.
fn is_object(map: &Map<String, Value>) -> bool {
    let typed = match map.get("type") {
        Some(Value::String(kind)) => kind == "object",
        Some(Value::Array(kinds)) => kinds.iter().any(|kind| kind == "object"),
        _ => false,
    };
    typed || map.contains_key("properties")
}

/// The direct subschemas of one schema object: under `items`, `not`,
/// `anyOf`, `oneOf`, `allOf`, `prefixItems`, `properties`, `$defs` and
/// `definitions`.
fn subschemas(map: &mut Map<String, Value>) -> Vec<&mut Value> {
    let mut found = Vec::new();
    for (keyword, value) in map.iter_mut() {
        let keyword = keyword.as_str();
        if SUBSCHEMA.contains(&keyword) {
            found.push(value);
        } else if SUBSCHEMA_LISTS.contains(&keyword) {
            found.extend(value.as_array_mut().into_iter().flatten());
        } else if SUBSCHEMA_MAPS.contains(&keyword) {
            found.extend(
                value
                    .as_object_mut()
                    .into_iter()
                    .flat_map(|m| m.values_mut()),
            );
        }
    }
    found
}

#[cfg(test)]
mod tests;
