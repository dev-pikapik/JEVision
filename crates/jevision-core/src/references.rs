//! Which blocks of a test use a picture file, and the settings that go with it: what a person
//! must look at again when the picture is replaced.

use crate::issue::{Issue, Severity};
use crate::validate::{placeholders, reference as validate_reference};
use crate::{BlockKind, CheckOf, Condition, Flow, Reference, Target, ValueKind, WaitUntil};
use crate::{dark_variant, resolve_test};

/// A block that uses a picture file.
#[derive(Debug, Clone, PartialEq)]
pub struct ReferenceUse {
    pub block: String,
    /// The `type` of the block, like `check`.
    pub block_type: String,
    /// Where in the block the picture is, named as in the problems of a test: `reference`,
    /// `until.appears`, `until.disappears`, `elseIf.1.reference`, `target`, `from`, `to`, `at`,
    /// `into` or `inputs.0.value`.
    pub field: String,
    /// The picture as the block names it.
    pub image: String,
    /// The file is the dark picture of `image`, which the block uses when the app is dark.
    pub dark: bool,
    /// How the block compares the screen with the picture; `None` for a block that only looks
    /// for the picture on screen or hands it to a script.
    pub settings: Option<Reference>,
}

/// The blocks of `flow`, saved as the test `file` of a project, that use the picture `picture`
/// (a path relative to the project), in the order they are saved. A block that names the light
/// picture of `picture` uses it too, in dark appearance.
pub fn reference_uses(flow: &Flow, file: &str, picture: &str) -> Vec<ReferenceUse> {
    let mut found = Vec::new();
    for block in &flow.blocks {
        for named in pictures(&block.kind) {
            let Some(dark) = uses(file, named.image, picture, named.has_dark) else {
                continue;
            };
            found.push(ReferenceUse {
                block: block.id.clone(),
                block_type: block.kind.type_name().to_owned(),
                field: named.field,
                image: named.image.to_owned(),
                dark,
                settings: named.settings.cloned(),
            });
        }
    }
    found
}

/// Whether `image`, written in the test `file`, is `picture` (`Some(false)`) or its light
/// picture (`Some(true)`).
fn uses(file: &str, image: &str, picture: &str, has_dark: bool) -> Option<bool> {
    if image.is_empty() || !placeholders(image).is_empty() {
        return None;
    }
    if resolve_test(file, image).is_some_and(|path| path == picture) {
        return Some(false);
    }
    let dark =
        has_dark && resolve_test(file, &dark_variant(image)).is_some_and(|path| path == picture);
    dark.then_some(true)
}

/// A picture a block names.
struct Named<'a> {
    field: String,
    image: &'a str,
    /// How the screen is compared with it, for a block that does.
    settings: Option<&'a Reference>,
    /// A run in dark appearance looks for a dark picture of it first.
    has_dark: bool,
}

fn located<'a>(field: &str, target: Option<&'a Target>) -> Option<Named<'a>> {
    match target {
        Some(Target::Image(path)) => Some(Named {
            field: field.to_owned(),
            image: path,
            settings: None,
            has_dark: true,
        }),
        _ => None,
    }
}

fn compared<'a>(field: String, reference: &'a Reference) -> Named<'a> {
    Named {
        field,
        image: &reference.image,
        settings: Some(reference),
        has_dark: true,
    }
}

fn shown<'a>(field: String, condition: &'a Condition) -> Option<Named<'a>> {
    condition.reference().map(|r| compared(field, r))
}

/// Every picture a block names.
fn pictures(kind: &BlockKind) -> Vec<Named<'_>> {
    match kind {
        BlockKind::Click { target, .. } | BlockKind::Hover { target, .. } => {
            located("target", Some(target)).into_iter().collect()
        }
        BlockKind::Drag { from, to, .. } => [located("from", Some(from)), located("to", Some(to))]
            .into_iter()
            .flatten()
            .collect(),
        BlockKind::Scroll { at, .. } => located("at", at.as_ref()).into_iter().collect(),
        BlockKind::Type { into, .. } => located("into", into.as_ref()).into_iter().collect(),
        BlockKind::WaitFor {
            until: WaitUntil::Appears(r),
            ..
        } => vec![compared("until.appears".to_owned(), r)],
        BlockKind::WaitFor {
            until: WaitUntil::Disappears(r),
            ..
        } => vec![compared("until.disappears".to_owned(), r)],
        BlockKind::Check {
            of: CheckOf::Screen(reference),
            ..
        } => vec![compared("reference".to_owned(), reference)],
        BlockKind::If {
            condition, else_if, ..
        } => {
            shown("reference".to_owned(), condition)
                .into_iter()
                .chain(else_if.iter().enumerate().filter_map(|(i, case)| {
                    shown(format!("elseIf.{i}.reference"), &case.condition)
                }))
                .collect()
        }
        BlockKind::Script { inputs, .. } => inputs
            .iter()
            .enumerate()
            .filter(|(_, input)| input.kind == ValueKind::Image)
            .map(|(i, input)| Named {
                field: format!("inputs.{i}.value"),
                image: &input.value,
                settings: None,
                has_dark: false,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Why [`set_reference_settings`] changed nothing.
#[derive(Debug, Clone, PartialEq)]
pub enum SettingsError {
    /// The test has no such block, or the block has no picture to compare with at that field.
    Missing,
    /// The settings don't fit: an area outside the picture, a similarity out of range.
    Invalid(Vec<Issue>),
}

/// Gives the picture at `field` of block `block` the comparison settings of `settings`: its
/// compare area, ignored and hidden parts and how much must match. The picture itself stays.
pub fn set_reference_settings(
    flow: &mut Flow,
    block: &str,
    field: &str,
    settings: &Reference,
) -> Result<(), SettingsError> {
    let kind = &mut flow
        .blocks
        .iter_mut()
        .find(|b| b.id == block)
        .ok_or(SettingsError::Missing)?
        .kind;
    let reference = reference_mut(kind, field).ok_or(SettingsError::Missing)?;
    let new = Reference {
        image: reference.image.clone(),
        ..settings.clone()
    };
    let mut problems = Vec::new();
    validate_reference(block, field, &new, &mut problems);
    problems.retain(|p| p.severity == Severity::Error);
    if !problems.is_empty() {
        return Err(SettingsError::Invalid(problems));
    }
    *reference = new;
    Ok(())
}

fn reference_mut<'a>(kind: &'a mut BlockKind, field: &str) -> Option<&'a mut Reference> {
    let condition = match (kind, field) {
        (
            BlockKind::Check {
                of: CheckOf::Screen(reference),
                ..
            },
            "reference",
        ) => return Some(reference),
        (
            BlockKind::WaitFor {
                until: WaitUntil::Appears(r),
                ..
            },
            "until.appears",
        )
        | (
            BlockKind::WaitFor {
                until: WaitUntil::Disappears(r),
                ..
            },
            "until.disappears",
        ) => return Some(r),
        (BlockKind::If { condition, .. }, "reference") => condition,
        (BlockKind::If { else_if, .. }, _) => {
            let i = field.strip_prefix("elseIf.")?.strip_suffix(".reference")?;
            &mut else_if.get_mut(i.parse::<usize>().ok()?)?.condition
        }
        _ => return None,
    };
    match condition {
        Condition::Screen(r) => Some(r),
        Condition::Value(_) | Condition::Element(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Compare, Rect, parse};

    fn flow(blocks: &str) -> Flow {
        parse(&format!(
            r#"{{"format":1,"name":"T","app":"com.example.App","start":"a","blocks":{{{blocks}}}}}"#
        ))
        .unwrap()
    }

    fn rect(x: f64) -> Rect {
        Rect {
            x,
            y: 0.1,
            width: 0.2,
            height: 0.2,
        }
    }

    const MANY: &str = r#"
        "a":{"type":"check","reference":{"image":"../refs/home.png","compare":"similar","threshold":0.9,"area":{"x":0,"y":0,"width":1,"height":0.5},"ignore":[{"x":0.1,"y":0.1,"width":0.1,"height":0.1}]},"next":"b"},
        "b":{"type":"waitFor","until":{"appears":{"image":"../refs/home.png"}},"timeout":"5s","next":"c"},
        "c":{"type":"if","reference":{"image":"../refs/other.png"},"then":"d","elseIf":[{"reference":{"image":"../refs/home.dark.png"},"then":"d"}]},
        "d":{"type":"click","target":{"image":"../refs/home.png"},"next":"e"},
        "e":{"type":"drag","from":{"point":{"x":0.1,"y":0.1}},"to":{"image":"../refs/./home.png"},"next":"f"},
        "f":{"type":"click","target":{"image":"../refs/button.png"}}"#;

    #[test]
    fn finds_every_block_that_shows_the_picture_whatever_the_type() {
        let flow = flow(MANY);
        let uses = reference_uses(&flow, "tests/a.flow.json", "refs/home.png");
        let found: Vec<_> = uses
            .iter()
            .map(|u| {
                (
                    u.block.as_str(),
                    u.block_type.as_str(),
                    u.field.as_str(),
                    u.dark,
                )
            })
            .collect();
        assert_eq!(
            found,
            [
                ("a", "check", "reference", false),
                ("b", "waitFor", "until.appears", false),
                ("d", "click", "target", false),
                ("e", "drag", "to", false),
            ]
        );
        let check = uses[0].settings.as_ref().unwrap();
        assert_eq!(check.compare, Compare::Similar { threshold: 0.9 });
        assert!(check.area.is_some() && check.ignore.len() == 1);
        assert_eq!(uses[2].settings, None, "a click only looks for the picture");
        assert!(reference_uses(&flow, "a.flow.json", "refs/home.png").is_empty());
    }

    #[test]
    fn the_dark_picture_belongs_to_the_blocks_that_name_the_light_one() {
        let flow = flow(MANY);
        let dark = reference_uses(&flow, "tests/a.flow.json", "refs/home.dark.png");
        let found: Vec<_> = dark.iter().map(|u| (u.block.as_str(), u.dark)).collect();
        assert_eq!(
            found,
            [
                ("a", true),
                ("b", true),
                ("c", false),
                ("d", true),
                ("e", true)
            ]
        );
        assert_eq!(dark[2].field, "elseIf.0.reference");
    }

    #[test]
    fn a_script_picture_has_no_dark_one() {
        let flow = flow(
            r#""a":{"type":"script","language":"python3","code":"print(1)","inputs":[{"name":"p","type":"image","value":"home.png"}]}"#,
        );
        let uses = reference_uses(&flow, "x.flow.json", "home.png");
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].field, "inputs.0.value");
        assert!(reference_uses(&flow, "x.flow.json", "home.dark.png").is_empty());
    }

    #[test]
    fn settings_replace_the_comparison_and_keep_the_picture() {
        let mut flow = flow(MANY);
        let new = Reference {
            image: "elsewhere.png".into(),
            compare: Compare::Similar { threshold: 0.8 },
            area: None,
            ignore: vec![rect(0.5)],
            hide: vec![],
        };
        for (block, field) in [
            ("a", "reference"),
            ("b", "until.appears"),
            ("c", "elseIf.0.reference"),
        ] {
            set_reference_settings(&mut flow, block, field, &new).unwrap();
        }
        let a = reference_uses(&flow, "tests/x.flow.json", "refs/home.png");
        let a = a[0].settings.clone().unwrap();
        assert_eq!(a.image, "../refs/home.png");
        assert_eq!(
            (a.compare, a.area, a.ignore),
            (new.compare, None, new.ignore)
        );
        let b = reference_uses(&flow, "tests/x.flow.json", "refs/home.png");
        assert_eq!(b[1].settings.as_ref().unwrap().ignore, vec![rect(0.5)]);
    }

    #[test]
    fn settings_that_do_not_fit_are_refused_and_change_nothing() {
        let mut flow = flow(MANY);
        let before = flow.clone();
        let bad = |edit: fn(&mut Reference)| {
            let mut r = Reference {
                image: String::new(),
                compare: Compare::Exact,
                area: None,
                ignore: vec![],
                hide: vec![],
            };
            edit(&mut r);
            r
        };
        let outside = bad(|r| r.hide = vec![rect(0.95)]);
        let threshold = bad(|r| r.compare = Compare::Similar { threshold: 1.5 });
        for settings in [&outside, &threshold] {
            assert!(matches!(
                set_reference_settings(&mut flow, "a", "reference", settings),
                Err(SettingsError::Invalid(_))
            ));
        }
        for (block, field) in [
            ("a", "until.appears"),
            ("d", "reference"),
            ("zzz", "reference"),
            ("c", "elseIf.7.reference"),
        ] {
            assert_eq!(
                set_reference_settings(&mut flow, block, field, &outside),
                Err(SettingsError::Missing)
            );
        }
        assert_eq!(flow, before);
    }
}
