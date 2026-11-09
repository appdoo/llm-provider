//! The one place a JSON schema is derived from a Rust type, for typed
//! answers and tool inputs alike.

use schemars::generate::SchemaSettings;
use schemars::JsonSchema;

/// The schema `schemars` derives for `T` (draft 2020-12) and its name: the
/// schema title, else the type's schema name. The `$schema` and `title`
/// keys are removed, since neither wire carries them.
pub(crate) fn derive<T: JsonSchema>() -> (String, serde_json::Value) {
    let mut schema = SchemaSettings::draft2020_12()
        .with(|settings| settings.meta_schema = None)
        .into_generator()
        .into_root_schema_for::<T>();

    let title = schema.remove("title");
    let name = title
        .as_ref()
        .and_then(serde_json::Value::as_str)
        .map_or_else(|| T::schema_name().into_owned(), str::to_owned);

    (name, schema.to_value())
}
