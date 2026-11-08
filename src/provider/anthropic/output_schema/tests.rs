//! Tests for the structured-outputs schema: closed objects, removed bounds,
//! clamped `minItems`, at every level.

use serde_json::json;

use super::conformant;

/// A nested object gets `additionalProperties: false`, as does the root.
#[test]
fn every_object_is_closed() {
    let schema = json!({"type": "object", "properties": {
        "author": {"type": "object", "properties": {"name": {"type": "string"}}}
    }});

    let out = conformant(&schema);

    assert_eq!(out["additionalProperties"], false);
    assert_eq!(out["properties"]["author"]["additionalProperties"], false);
    assert!(out["properties"]["author"]["properties"]["name"]
        .get("additionalProperties")
        .is_none());
}

/// An open `additionalProperties` (a map's value schema) becomes `false`.
#[test]
fn additional_properties_other_than_false_is_replaced() {
    let schema = json!({"type": "object", "additionalProperties": {"type": "string"}});

    assert_eq!(conformant(&schema)["additionalProperties"], false);
}

/// `maxItems` and `uniqueItems` are removed, `minItems: 3` becomes 1 and
/// `minItems: 1` stays.
#[test]
fn array_bounds_are_reduced_to_min_items_one() {
    let schema = json!({"type": "object", "properties": {
        "points": {"type": "array", "minItems": 3, "maxItems": 5, "uniqueItems": true,
                   "items": {"type": "string"}},
        "tags": {"type": "array", "minItems": 1, "items": {"type": "string"}}
    }});

    let out = conformant(&schema);

    let points = &out["properties"]["points"];
    assert_eq!(points["minItems"], 1);
    assert!(points.get("maxItems").is_none());
    assert!(points.get("uniqueItems").is_none());
    assert_eq!(out["properties"]["tags"]["minItems"], 1);
}

/// Numeric and string-length bounds are removed inside `items`, `anyOf`
/// and `$defs`; `format`, `enum` and `required` stay.
#[test]
fn bounds_are_removed_at_every_level() {
    let schema = json!({
        "type": "object",
        "required": ["n"],
        "properties": {
            "n": {"type": "integer", "minimum": 0, "maximum": 9, "multipleOf": 3},
            "list": {"type": "array", "items": {"type": "string", "minLength": 2, "maxLength": 8}},
            "either": {"anyOf": [{"$ref": "#/$defs/Word"}, {"type": "null"}]}
        },
        "$defs": {"Word": {"type": "string", "maxLength": 4, "format": "email", "enum": ["a@b.c"]}}
    });

    let out = conformant(&schema);

    assert_eq!(out["properties"]["n"], json!({"type": "integer"}));
    assert_eq!(
        out["properties"]["list"]["items"],
        json!({"type": "string"})
    );
    assert_eq!(
        out["$defs"]["Word"],
        json!({"type": "string", "format": "email", "enum": ["a@b.c"]})
    );
    assert_eq!(out["required"], json!(["n"]));
    assert_eq!(
        out["properties"]["either"]["anyOf"][1],
        json!({"type": "null"})
    );
}
