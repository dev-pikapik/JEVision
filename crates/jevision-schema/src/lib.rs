//! JSON Schemas of the JEVision files, made from the types that read and write them, kept in
//! `docs/schemas` and published at [`BASE_URL`]. Only the tests of the crates that own the
//! files use this crate, so the apps never carry it.

use std::path::PathBuf;
use std::{env, fs};

use schemars::generate::SchemaSettings;
use schemars::transform::{RecursiveTransform, Transform};
use schemars::{JsonSchema, Schema};
use serde_json::{Value, json};

/// Where the schemas are published: a schema's address is this, its kind and `.schema.json`.
pub const BASE_URL: &str = "https://dev-pikapik.github.io/JEVision/schemas/";

/// A semantic version as JEVision reads one: `MAJOR.MINOR.PATCH`, an optional `-pre.release`
/// part whose numbers have no leading zeros, and an optional `+build` part that is ignored.
pub const SEMVER_PATTERN: &str = r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)(\.(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*)?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$";

/// Set to `1` to write the schemas into `docs/schemas` instead of comparing them.
const WRITE: &str = "JEVISION_WRITE_SCHEMAS";

/// One kind of file, and what its schema says about it.
pub struct Kind {
    /// The name in the schema's address, such as `test`.
    pub name: &'static str,
    pub title: &'static str,
    /// What the file is and where it lives, for people who open the schema.
    pub description: &'static str,
    /// The values `format` can have; empty for a file without `format`.
    pub formats: &'static [u32],
    /// The file may name its schema in `$schema` and the oldest JEVision that opens it in
    /// `minVersion`. Readers must check the requirement before decoding the rest of the file.
    pub header: bool,
}

impl Kind {
    /// The address the schema is published at, which is also its `$id`.
    pub fn url(&self) -> String {
        format!("{BASE_URL}{}.schema.json", self.name)
    }

    /// The schema of a file of this kind, with `T` the type that holds the whole file.
    pub fn schema<T: JsonSchema>(&self) -> Value {
        let mut schema = SchemaSettings::draft2020_12()
            .into_generator()
            .into_root_schema_for::<T>();
        RecursiveTransform(tidy_descriptions).transform(&mut schema);
        self.with_header(schema.to_value())
    }

    /// Adds the schema's address and title, `format`, and with [`Kind::header`] the keys
    /// `$schema` and `minVersion`.
    fn with_header(&self, mut schema: Value) -> Value {
        let top = schema.as_object_mut().expect("a file is a JSON object");
        top.insert("$id".into(), self.url().into());
        top.insert("title".into(), self.title.into());
        top.insert("description".into(), self.description.into());
        let properties = top
            .entry("properties")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("properties are an object");
        if self.header {
            properties.insert(
                "$schema".into(),
                json!({
                    "description": "Address of this schema, so that editors check the file and \
                        suggest its keys.",
                    "type": "string",
                }),
            );
            properties.insert(
                "minVersion".into(),
                json!({
                    "description": "The oldest JEVision that can open the file, such as 1.4.0. \
                        An older one asks to be updated instead of opening it.",
                    "type": "string",
                    "pattern": SEMVER_PATTERN,
                }),
            );
        }
        let mut format = json!({
            "description": "Version of the layout of the file. JEVision doesn't open a file in \
                a format newer than it knows.",
        });
        match self.formats {
            [] => return schema,
            [one] => format["const"] = (*one).into(),
            many => format["enum"] = many.into(),
        }
        properties.insert("format".into(), format);
        let required = top
            .entry("required")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .expect("required is a list");
        required.retain(|key| key != "format");
        required.insert(0, "format".into());
        schema
    }

    /// Fails when `docs/schemas` holds another schema for this kind than `schema`; with
    /// `JEVISION_WRITE_SCHEMAS=1`, writes `schema` there instead.
    pub fn check_published(&self, schema: &Value) {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/schemas")
            .join(format!("{}.schema.json", self.name));
        let text = serde_json::to_string_pretty(schema).expect("a schema is JSON") + "\n";
        if env::var(WRITE).is_ok_and(|v| v == "1") {
            fs::write(&path, text).expect("docs/schemas is writable");
            return;
        }
        let published = fs::read_to_string(&path).unwrap_or_default();
        assert!(
            published == text,
            "{} is out of date: run `{WRITE}=1 cargo test` and commit the result",
            path.display()
        );
    }
}

/// Checks files against a schema.
pub struct Validator {
    schemas: boon::Schemas,
    index: boon::SchemaIndex,
}

impl Validator {
    /// A validator for `schema`.
    pub fn new(schema: &Value) -> Self {
        let schema = schema.clone();
        let url = schema["$id"]
            .as_str()
            .expect("a schema has an $id")
            .to_owned();
        let mut compiler = boon::Compiler::new();
        let mut schemas = boon::Schemas::new();
        compiler
            .add_resource(&url, schema)
            .expect("the address of a schema is a URL");
        let index = compiler
            .compile(&url, &mut schemas)
            .unwrap_or_else(|e| panic!("{url} is not a valid schema: {e:#}"));
        Self { schemas, index }
    }

    /// `Err` says why `file` doesn't fit the schema.
    pub fn check(&self, file: &Value) -> Result<(), String> {
        self.schemas
            .validate(file, self.index)
            .map_err(|e| format!("{e:#}"))
    }
}

/// Takes away the `null` that `Option` allows, for a file whose reader refuses `null` where a
/// key may be left out.
pub fn without_null(mut schema: Value) -> Value {
    fn strip(schema: &mut Schema) {
        let Some(object) = schema.as_object_mut() else {
            return;
        };
        if let Some(Value::Array(types)) = object.get_mut("type") {
            types.retain(|t| t != "null");
            if let [only] = &types[..] {
                let only = only.clone();
                object.insert("type".into(), only);
            }
        }
        if let Some(Value::Array(values)) = object.get_mut("enum") {
            values.retain(|v| !v.is_null());
        }
        let null = json!({ "type": "null" });
        if let Some(Value::Array(any)) = object.get("anyOf")
            && any.len() == 2
            && any.contains(&null)
            && let Some(Value::Object(other)) = any.iter().find(|s| **s != null).cloned()
        {
            object.remove("anyOf");
            for (key, value) in other {
                object.entry(key).or_insert(value);
            }
        }
    }
    let mut as_schema = Schema::try_from(schema.take()).expect("a schema is an object");
    RecursiveTransform(strip).transform(&mut as_schema);
    as_schema.to_value()
}

/// Turns doc comments into descriptions that read well outside the code: lines wrapped in the
/// source join back into one, and links to other items become plain code.
fn tidy_descriptions(schema: &mut Schema) {
    let Some(Value::String(text)) = schema.get_mut("description") else {
        return;
    };
    *text = text
        .split("\n\n")
        .map(|paragraph| paragraph.replace('\n', " "))
        .collect::<Vec<_>>()
        .join("\n\n")
        .replace("[`", "`")
        .replace("`]", "`");
}
