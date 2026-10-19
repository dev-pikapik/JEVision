//! Parts of tests kept for reuse: templates, whose blocks are copied into a test, and custom
//! blocks, tests with `${input.NAME}` placeholders that other tests run as one block.

use std::collections::HashSet;

use crate::canvas::CanvasItem;
use crate::decode::parse;
use crate::model::{BlockKind, Flow};
use crate::to_canonical_json;
use crate::validate::is_identifier;

/// Templates every project has, by id: the text of a template file.
pub const BUILTIN_TEMPLATES: &[(&str, &str)] = &[
    ("login", include_str!("../templates/login.template.json")),
    (
        "onboarding",
        include_str!("../templates/onboarding.template.json"),
    ),
    (
        "payment",
        include_str!("../templates/payment.template.json"),
    ),
];

/// A field of a block that a custom block can ask for each time it is used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputField {
    pub block: String,
    /// `text`, or `files.N` for the N-th file of an `openFile` block.
    pub field: String,
    pub value: String,
}

/// A field turned into an input of a custom block, named `name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockInput {
    pub block: String,
    pub field: String,
    pub name: String,
}

impl Flow {
    /// `items` as a template file named `name`.
    pub fn template(&self, items: &[CanvasItem], name: &str) -> String {
        let mut part = parse(&self.copy_items(items)).expect("copied blocks are a test");
        part.name = name.trim().to_owned();
        to_canonical_json(&part)
    }

    /// Text fields of the blocks in `items` that can become inputs of a custom block, in
    /// reading order.
    pub fn input_fields(&self, items: &[CanvasItem]) -> Vec<InputField> {
        let chosen: HashSet<&str> = items
            .iter()
            .filter_map(|i| match i {
                CanvasItem::Block { id } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        let mut found = Vec::new();
        for b in self.blocks_in_flow_order() {
            if !chosen.contains(b.id.as_str()) {
                continue;
            }
            match &b.kind {
                BlockKind::Type { text, .. } => found.push(InputField {
                    block: b.id.clone(),
                    field: "text".to_owned(),
                    value: text.clone(),
                }),
                BlockKind::OpenFile { files, .. } => {
                    found.extend(files.iter().enumerate().map(|(i, f)| InputField {
                        block: b.id.clone(),
                        field: format!("files.{i}"),
                        value: f.clone(),
                    }));
                }
                _ => {}
            }
        }
        found
    }

    /// `items` as a custom block file: a test named `name`, shown with the SF Symbol `icon`,
    /// whose `inputs` fields read `${input.NAME}`. Names become identifiers, each used once.
    pub fn custom_block(
        &self,
        items: &[CanvasItem],
        name: &str,
        icon: &str,
        inputs: &[BlockInput],
    ) -> String {
        let mut part = parse(&self.copy_items(items)).expect("copied blocks are a test");
        part.name = name.trim().to_owned();
        part.icon = Some(icon.to_owned()).filter(|i| !i.is_empty());
        let mut taken = HashSet::new();
        for input in inputs {
            let base = input_name(&input.name);
            let name = std::iter::once(base.clone())
                .chain((2..).map(|n| format!("{base}_{n}")))
                .find(|n| !taken.contains(n))
                .expect("an unused name always exists");
            let Some(b) = part.blocks.iter_mut().find(|b| b.id == input.block) else {
                continue;
            };
            let field = match (&mut b.kind, input.field.as_str()) {
                (BlockKind::Type { text, .. }, "text") => Some(text),
                (BlockKind::OpenFile { files, .. }, f) => f
                    .strip_prefix("files.")
                    .and_then(|i| i.parse::<usize>().ok())
                    .and_then(|i| files.get_mut(i)),
                _ => None,
            };
            if let Some(value) = field {
                *value = format!("${{input.{name}}}");
                taken.insert(name);
            }
        }
        to_canonical_json(&part)
    }
}

/// `label` as the name of an input: letters, digits and `_`, not starting with a digit.
fn input_name(label: &str) -> String {
    let mut name: String = label
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if !name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        name.insert_str(0, "input_");
    }
    debug_assert!(is_identifier(&name));
    name
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate::{required_inputs, validate};

    fn flow() -> Flow {
        parse(
            r#"{"format":1,"name":"Shop","app":"com.example.App","start":"open","blocks":{
                "open":{"type":"openFile","files":["cart.txt","https://example.com"],"next":"email"},
                "email":{"type":"type","text":"me@example.com","next":"keys"},
                "keys":{"type":"pressKeys","keys":"return"}}}"#,
        )
        .unwrap()
    }

    fn blocks(ids: &[&str]) -> Vec<CanvasItem> {
        ids.iter()
            .map(|id| CanvasItem::Block { id: (*id).into() })
            .collect()
    }

    #[test]
    fn builtin_templates_are_tests_without_errors_but_the_parts_to_choose() {
        for (id, text) in BUILTIN_TEMPLATES {
            let flow = parse(text).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert_eq!(to_canonical_json(&flow), *text, "{id} is canonical");
            let unexpected: Vec<_> = validate(&flow)
                .into_iter()
                .filter(|i| i.code != "flow.field.empty")
                .collect();
            assert!(unexpected.is_empty(), "{id}: {unexpected:?}");
        }
    }

    #[test]
    fn a_template_keeps_the_chosen_blocks_under_its_name() {
        let t = parse(&flow().template(&blocks(&["email", "keys"]), " Fill in ")).unwrap();
        assert_eq!(t.name, "Fill in");
        assert_eq!(t.start, "email");
        assert_eq!(t.blocks.len(), 2);
    }

    #[test]
    fn a_custom_block_asks_for_the_chosen_fields() {
        let f = flow();
        let items = blocks(&["open", "email", "keys"]);
        let fields: Vec<(String, String)> = f
            .input_fields(&items)
            .into_iter()
            .map(|i| (i.block, i.field))
            .collect();
        assert_eq!(
            fields,
            [
                ("open".into(), "files.0".into()),
                ("open".into(), "files.1".into()),
                ("email".into(), "text".into()),
            ]
        );
        let input = |block: &str, field: &str, name: &str| BlockInput {
            block: block.into(),
            field: field.into(),
            name: name.into(),
        };
        let text = f.custom_block(
            &items,
            "Sign in",
            "person.badge.key",
            &[
                input("email", "text", "e-mail"),
                input("open", "files.0", "e-mail"),
                input("open", "files.9", "lost"),
            ],
        );
        let block = parse(&text).unwrap();
        assert_eq!(block.icon.as_deref(), Some("person.badge.key"));
        assert_eq!(required_inputs(&block), ["e_mail", "e_mail_2"]);
        assert!(validate(&block).is_empty());
        assert_eq!(input_name("2 cards"), "input_2_cards");
    }
}
