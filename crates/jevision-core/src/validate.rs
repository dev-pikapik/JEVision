//! Checks a well-formed test for problems in its meaning: broken edges, forbidden cycles,
//! empty or out-of-range parameters, malformed placeholders.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::Path;

use crate::issue::Issue;
use crate::model::{
    AI_PROVIDERS, BODY, Block, BlockKind, Compare, Flow, Point, Rect, Reference, Target,
    format_duration,
};

pub const MAX_RETRIES: u32 = 10;
pub const MAX_REPEAT: u32 = 10_000;
pub const MAX_TIMEOUT_MS: u64 = 60 * 60 * 1000;
/// Most a scroll block may scroll, in lines or in pixels.
pub const MAX_SCROLL: u32 = 10_000;
const MAX_ID_LEN: usize = 64;

/// Lists every problem in `flow`, errors and warnings, in a stable order.
pub fn validate(flow: &Flow) -> Vec<Issue> {
    let mut out = Vec::new();
    root(flow, &mut out);

    let mut blocks: Vec<&Block> = flow.blocks.iter().collect();
    blocks.sort_by(|a, b| a.id.cmp(&b.id));
    let mut seen = HashSet::new();
    for b in &blocks {
        if !seen.insert(b.id.as_str()) {
            out.push(Issue::error("flow.block.duplicateId").with("block", &b.id));
        }
    }
    if !seen.contains(flow.start.as_str()) {
        out.push(Issue::error("flow.start.unknownBlock").with("target", &flow.start));
    }
    for b in &blocks {
        block(b, &seen, &mut out);
    }
    graph(flow, &blocks, &mut out);
    // A test that runs another one may check the screen there.
    if !flow
        .blocks
        .iter()
        .any(|b| b.kind.checks_screen() || matches!(b.kind, BlockKind::RunTest { .. }))
    {
        out.push(Issue::warning("flow.noCheck"));
    }
    out
}

/// [`validate`], plus warnings about files the test reads that are not in `dir`, the folder of
/// the test file. A missing reference of a check block is expected before the first run, which
/// saves the screen as that reference; other missing files stop the run at their block.
pub fn validate_in(flow: &Flow, dir: &Path) -> Vec<Issue> {
    let mut out = validate(flow);
    let checked = out.len();
    let mut blocks: Vec<&Block> = flow.blocks.iter().collect();
    blocks.sort_by(|a, b| a.id.cmp(&b.id));
    let mut saved_by_check = HashSet::new();
    let mut read = Vec::new();
    for &b in &blocks {
        if let BlockKind::Check { reference: r, .. } = &b.kind {
            if is_relative_path(&r.image) && !dir.join(&r.image).exists() {
                saved_by_check.insert(r.image.as_str());
                out.push(
                    Issue::warning("flow.reference.notSaved")
                        .with("block", &b.id)
                        .with("field", "reference.image")
                        .with("path", &r.image),
                );
            }
        }
        read.extend(
            files_read(&b.kind)
                .into_iter()
                .map(|(field, path)| (&b.id, field, path)),
        );
    }
    for (block, field, path) in read {
        // A path with placeholders names a file only once a run fills them in.
        if is_relative_path(path)
            && placeholders(path).is_empty()
            && !saved_by_check.contains(path)
            && !dir.join(path).exists()
        {
            out.push(
                Issue::warning("flow.file.missing")
                    .with("block", block)
                    .with("field", field)
                    .with("path", path),
            );
        }
    }
    out.extend(crate::env::validate_environment_values(flow, dir));
    out.extend(crate::data::validate_data(flow, dir));
    out[checked..].sort_by(|a, b| a.param("block").cmp(&b.param("block")));
    out
}

/// Fields of a block that name a file to read, other than a check block's reference.
fn files_read<'a>(kind: &'a BlockKind) -> Vec<(String, &'a str)> {
    use crate::model::WaitUntil::*;
    let image = |field: &str, t: Option<&'a Target>| match t {
        Some(Target::Image(path)) => vec![(format!("{field}.image"), path)],
        _ => Vec::new(),
    };
    let found: Vec<(String, &'a String)> = match kind {
        BlockKind::OpenFile { files, .. } => files
            .iter()
            .enumerate()
            .filter(|(_, f)| !is_web_address(f))
            .map(|(i, f)| (format!("files.{i}"), f))
            .collect(),
        BlockKind::Click { target, .. } | BlockKind::Hover { target, .. } => {
            image("target", Some(target))
        }
        BlockKind::Drag { from, to, .. } => {
            let mut both = image("from", Some(from));
            both.extend(image("to", Some(to)));
            both
        }
        BlockKind::Scroll { at, .. } => image("at", at.as_ref()),
        BlockKind::Type { into, .. } => image("into", into.as_ref()),
        BlockKind::WaitFor {
            until: Appears(r), ..
        } => vec![("until.appears.image".to_owned(), &r.image)],
        BlockKind::WaitFor {
            until: Disappears(r),
            ..
        } => vec![("until.disappears.image".to_owned(), &r.image)],
        BlockKind::If {
            reference: r,
            else_if,
            ..
        } => std::iter::once(("reference.image".to_owned(), &r.image))
            .chain(
                else_if
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (format!("elseIf.{i}.reference.image"), &c.reference.image)),
            )
            .collect(),
        BlockKind::UseData { file, .. } => vec![("file".to_owned(), file)],
        BlockKind::RunTest { test, .. } => vec![("test".to_owned(), test)],
        _ => Vec::new(),
    };
    found.into_iter().map(|(f, p)| (f, p.as_str())).collect()
}

/// Names of the secrets the test reads through `${secret.NAME}`, sorted, without repeats.
pub fn required_secrets(flow: &Flow) -> Vec<String> {
    let mut names = BTreeSet::new();
    for b in &flow.blocks {
        for (_, value) in placeholder_fields(&b.kind) {
            for p in placeholders(value).into_iter().flatten() {
                if p.0 == "secret" {
                    names.insert(p.1.to_owned());
                }
            }
        }
    }
    names.into_iter().collect()
}

/// Fields of a block that may contain `${secret.NAME}`, `${env.NAME}` and `${data.COLUMN}`
/// placeholders: text to type and the paths and addresses of files the block reads.
pub(crate) fn placeholder_fields(kind: &BlockKind) -> Vec<(String, &str)> {
    match kind {
        BlockKind::Type { text, .. } => vec![("text".to_owned(), text.as_str())],
        BlockKind::OpenFile { files, .. } => files
            .iter()
            .enumerate()
            .map(|(i, f)| (format!("files.{i}"), f.as_str()))
            .collect(),
        BlockKind::UseData { file, .. } => vec![("file".to_owned(), file.as_str())],
        BlockKind::RunTest { test, .. } => vec![("test".to_owned(), test.as_str())],
        _ => Vec::new(),
    }
}

fn root(flow: &Flow, out: &mut Vec<Issue>) {
    if flow.name.trim().is_empty() {
        out.push(Issue::error("flow.field.empty").with("field", "name"));
    }
    if !is_bundle_id(&flow.app) {
        out.push(
            Issue::error("flow.app.invalid")
                .with("field", "app")
                .with("value", &flow.app),
        );
    }
    let mut tags = HashSet::new();
    for tag in &flow.tags {
        if tag.trim().is_empty() {
            out.push(Issue::error("flow.field.empty").with("field", "tags"));
        } else if !tags.insert(tag) {
            out.push(Issue::error("flow.tags.duplicate").with("tag", tag));
        }
    }
    if flow.retries.is_some_and(|retries| retries > MAX_RETRIES) {
        out.push(range(None, "retries", 0, MAX_RETRIES));
    }
}

fn block(b: &Block, ids: &HashSet<&str>, out: &mut Vec<Issue>) {
    let id = b.id.as_str();
    let err = |code: &str, field: &str| Issue::error(code).with("block", id).with("field", field);
    if !is_block_id(id) {
        out.push(Issue::error("flow.block.invalidId").with("block", id));
    }
    for (edge, target) in b.kind.edges() {
        if !ids.contains(target) {
            out.push(
                Issue::error("flow.edge.unknownTarget")
                    .with("block", id)
                    .with("edge", edge)
                    .with("target", target),
            );
        }
    }
    match &b.kind {
        BlockKind::OpenApp { app, .. } => app_id(id, app.as_deref(), out),
        BlockKind::OpenFile { files, app, .. } => {
            if files.is_empty() {
                out.push(err("flow.field.empty", "files"));
            }
            for (i, f) in files.iter().enumerate() {
                if !is_web_address(f) {
                    relative_path(id, &format!("files.{i}"), f, out);
                }
            }
            app_id(id, app.as_deref(), out);
        }
        BlockKind::Click { target: t, .. } | BlockKind::Hover { target: t, .. } => {
            target(id, "target", t, out)
        }
        BlockKind::Drag { from, to, .. } => {
            target(id, "from", from, out);
            target(id, "to", to, out);
            let same = match (from, to) {
                (Target::Image(a), Target::Image(b)) => !a.is_empty() && a == b,
                (Target::Point(a), Target::Point(b)) => a == b,
                _ => false,
            };
            if same {
                out.push(err("flow.drag.samePlace", "to"));
            }
        }
        BlockKind::Scroll { amount, at, .. } => {
            if !(1..=MAX_SCROLL).contains(amount) {
                out.push(range(Some(id), "amount", 1, MAX_SCROLL));
            }
            if let Some(t) = at {
                target(id, "at", t, out);
            }
        }
        BlockKind::Type { text, into, .. } => {
            if text.is_empty() {
                out.push(err("flow.field.empty", "text"));
            }
            if let Some(t) = into {
                target(id, "into", t, out);
            }
        }
        BlockKind::PressKeys { keys, .. } => {
            if keys.is_empty() {
                out.push(err("flow.field.empty", "keys"));
            } else if !is_key_combo(keys) {
                out.push(err("flow.keys.invalid", "keys").with("value", keys));
            }
        }
        BlockKind::WaitFor {
            until, timeout_ms, ..
        } => {
            use crate::model::WaitUntil::*;
            match until {
                Appears(r) => reference(id, "until.appears", r, out),
                Disappears(r) => reference(id, "until.disappears", r, out),
                ScreenStable => {}
            }
            if !(1..=MAX_TIMEOUT_MS).contains(timeout_ms) {
                out.push(
                    err("flow.field.range", "timeout")
                        .with("min", format_duration(1))
                        .with("max", format_duration(MAX_TIMEOUT_MS)),
                );
            }
        }
        BlockKind::Check { reference: r, .. } => reference(id, "reference", r, out),
        BlockKind::If {
            reference: r,
            else_if,
            ..
        } => {
            reference(id, "reference", r, out);
            for (i, case) in else_if.iter().enumerate() {
                let field = format!("elseIf.{i}.reference");
                reference(id, &field, &case.reference, out);
                let earlier = std::iter::once(r).chain(else_if[..i].iter().map(|c| &c.reference));
                // A case not filled in yet is reported as empty, not as a repeat.
                let filled = !case.reference.image.is_empty()
                    || matches!(&case.reference.compare, Compare::Meaning { expect, .. } if !expect.trim().is_empty());
                if filled && earlier.into_iter().any(|e| *e == case.reference) {
                    out.push(
                        Issue::warning("flow.if.sameCondition")
                            .with("block", id)
                            .with("field", field),
                    );
                }
            }
        }
        BlockKind::Repeat { times, body, .. } => {
            if !(1..=MAX_REPEAT).contains(times) {
                out.push(range(Some(id), "times", 1, MAX_REPEAT));
            }
            loop_body(id, body, out);
        }
        BlockKind::UseData { file, body, .. } => {
            path_with_extension(id, "file", file, ".csv", out);
            loop_body(id, body, out);
        }
        BlockKind::RunTest { test, .. } => path_with_extension(id, "test", test, ".flow.json", out),
    }
    for (field, value) in placeholder_fields(&b.kind) {
        for bad in placeholders(value).into_iter().filter_map(Result::err) {
            out.push(err("flow.placeholder.invalid", &field).with("placeholder", bad));
        }
    }
}

fn range(block: Option<&str>, field: &str, min: u32, max: u32) -> Issue {
    let issue = Issue::error("flow.field.range");
    let issue = match block {
        Some(b) => issue.with("block", b),
        None => issue,
    };
    issue.with("field", field).with("min", min).with("max", max)
}

fn app_id(block: &str, app: Option<&str>, out: &mut Vec<Issue>) {
    if let Some(app) = app.filter(|a| !is_bundle_id(a)) {
        out.push(
            Issue::error("flow.app.invalid")
                .with("block", block)
                .with("field", "app")
                .with("value", app),
        );
    }
}

fn loop_body(block: &str, body: &str, out: &mut Vec<Issue>) {
    if body == block {
        out.push(Issue::error("flow.loop.emptyBody").with("block", block));
    }
}

/// Reports an empty or absolute path; returns whether the path can be used.
fn relative_path(block: &str, field: &str, path: &str, out: &mut Vec<Issue>) -> bool {
    let issue = |code| Issue::error(code).with("block", block).with("field", field);
    if path.is_empty() {
        out.push(issue("flow.field.empty"));
    } else if !is_relative_path(path) {
        out.push(issue("flow.path.invalid").with("value", path));
    } else {
        return true;
    }
    false
}

fn path_with_extension(block: &str, field: &str, path: &str, ext: &str, out: &mut Vec<Issue>) {
    let wrong_extension = !path.ends_with(ext) || path.ends_with(&format!("/{ext}")) || path == ext;
    if relative_path(block, field, path, out) && wrong_extension {
        out.push(
            Issue::error("flow.path.extension")
                .with("block", block)
                .with("field", field)
                .with("value", path)
                .with("expected", ext),
        );
    }
}

fn target(block: &str, field: &str, t: &Target, out: &mut Vec<Issue>) {
    match t {
        Target::Image(path) => {
            relative_path(block, &format!("{field}.image"), path, out);
        }
        Target::Point(p) => point(block, &format!("{field}.point"), p, out),
    }
}

fn point(block: &str, field: &str, p: &Point, out: &mut Vec<Issue>) {
    if !(is_fraction(p.x) && is_fraction(p.y)) {
        out.push(
            Issue::error("flow.point.invalid")
                .with("block", block)
                .with("field", field),
        );
    }
}

/// The rectangle has a size and lies inside the window or image.
pub(crate) fn rect_fits(r: &Rect) -> bool {
    [r.x, r.y, r.width, r.height].into_iter().all(is_fraction)
        && r.width > 0.0
        && r.height > 0.0
        && r.x + r.width <= 1.0
        && r.y + r.height <= 1.0
}

fn rect(block: &str, field: &str, r: &Rect, out: &mut Vec<Issue>) {
    if !rect_fits(r) {
        out.push(
            Issue::error("flow.area.invalid")
                .with("block", block)
                .with("field", field),
        );
    }
}

fn reference(block: &str, field: &str, r: &Reference, out: &mut Vec<Issue>) {
    let image_optional =
        matches!(&r.compare, Compare::Meaning { expect, .. } if !expect.trim().is_empty());
    if !(image_optional && r.image.is_empty()) {
        relative_path(block, &format!("{field}.image"), &r.image, out);
    }
    if let Compare::Meaning { provider, .. } = &r.compare
        && !provider.is_empty()
        && !AI_PROVIDERS.contains(&provider.as_str())
    {
        out.push(
            Issue::error("flow.reference.unknownProvider")
                .with("block", block)
                .with("field", format!("{field}.provider"))
                .with("provider", provider)
                .with("allowed", AI_PROVIDERS.join(", ")),
        );
    }
    if let Compare::Similar { threshold } = r.compare {
        if !(threshold > 0.0 && threshold <= 1.0) {
            out.push(
                Issue::error("flow.field.range")
                    .with("block", block)
                    .with("field", format!("{field}.threshold"))
                    .with("min", 0)
                    .with("max", 1),
            );
        }
    }
    if let Some(a) = &r.area {
        rect(block, &format!("{field}.area"), a, out);
    }
    for (name, list) in [("ignore", &r.ignore), ("hide", &r.hide)] {
        for (i, a) in list.iter().enumerate() {
            rect(block, &format!("{field}.{name}.{i}"), a, out);
        }
    }
}

/// Cycles are allowed only through the `body` edge of a loop block; reachability is checked
/// from `start`.
fn graph(flow: &Flow, blocks: &[&Block], out: &mut Vec<Issue>) {
    let index: HashMap<&str, usize> = blocks
        .iter()
        .enumerate()
        .map(|(i, b)| (b.id.as_str(), i))
        .collect();
    let edges = |include_body: bool| -> Vec<Vec<usize>> {
        blocks
            .iter()
            .map(|b| {
                b.kind
                    .edges()
                    .into_iter()
                    .filter(|(name, _)| include_body || *name != BODY)
                    .filter_map(|(_, t)| index.get(t).copied())
                    .collect()
            })
            .collect()
    };
    let all = edges(true);

    for cycle in cycles(&edges(false)) {
        let ids: Vec<&str> = cycle.iter().map(|&i| blocks[i].id.as_str()).collect();
        out.push(
            Issue::error("flow.cycle")
                .with("block", ids[0])
                .with("blocks", ids.join(", ")),
        );
    }

    if let Some(&start) = index.get(flow.start.as_str()) {
        let reached = reachable(&all, start);
        for (i, b) in blocks.iter().enumerate() {
            if !reached[i] {
                out.push(Issue::warning("flow.block.unreachable").with("block", &b.id));
            }
        }
    }

    for (i, b) in blocks.iter().enumerate() {
        let body = match &b.kind {
            BlockKind::Repeat { body, .. } | BlockKind::UseData { body, .. } => body,
            _ => continue,
        };
        if let Some(&body) = index.get(body.as_str()) {
            if body != i && !reachable(&all, body)[i] {
                out.push(Issue::warning("flow.loop.noReturn").with("block", &b.id));
            }
        }
    }
}

pub(crate) fn reachable(edges: &[Vec<usize>], from: usize) -> Vec<bool> {
    let mut seen = vec![false; edges.len()];
    let mut queue = VecDeque::from([from]);
    seen[from] = true;
    while let Some(n) = queue.pop_front() {
        for &m in &edges[n] {
            if !seen[m] {
                seen[m] = true;
                queue.push_back(m);
            }
        }
    }
    seen
}

/// One cycle per back edge found by depth-first search, as block indexes in edge order.
fn cycles(edges: &[Vec<usize>]) -> Vec<Vec<usize>> {
    const NEW: u8 = 0;
    const ON_PATH: u8 = 1;
    const DONE: u8 = 2;
    let mut state = vec![NEW; edges.len()];
    let mut found = Vec::new();
    for root in 0..edges.len() {
        if state[root] != NEW {
            continue;
        }
        state[root] = ON_PATH;
        let mut path: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(top) = path.last_mut() {
            let (node, next_edge) = *top;
            let Some(&to) = edges[node].get(next_edge) else {
                state[node] = DONE;
                path.pop();
                continue;
            };
            top.1 += 1;
            match state[to] {
                NEW => {
                    state[to] = ON_PATH;
                    path.push((to, 0));
                }
                ON_PATH => {
                    let at = path.iter().position(|&(n, _)| n == to).unwrap_or(0);
                    found.push(path[at..].iter().map(|&(n, _)| n).collect());
                }
                _ => {}
            }
        }
    }
    found
}

/// Splits `${namespace.name}` placeholders out of text; malformed ones are returned as `Err`.
pub(crate) fn placeholders(text: &str) -> Vec<Result<(&str, &str), &str>> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("${") {
        rest = &rest[at..];
        let Some(end) = rest.find('}') else {
            found.push(Err(rest));
            break;
        };
        let whole = &rest[..=end];
        let parsed = whole[2..end]
            .split_once('.')
            .filter(|(ns, name)| matches!(*ns, "secret" | "env" | "data") && is_identifier(name));
        found.push(parsed.ok_or(whole));
        rest = &rest[end + 1..];
    }
    found
}

/// `text` for display, with a word joiner (U+2060) between the characters of every `${…}`
/// placeholder, so a line wraps before or after one and never inside: "Hello, ${data.name}!"
/// doesn't end a line with "Hello, $".
pub fn unbreakable_placeholders(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("${") {
        let Some(len) = rest[at..].find('}') else {
            break;
        };
        out.push_str(&rest[..at]);
        for (i, c) in rest[at..=at + len].chars().enumerate() {
            if i > 0 {
                out.push('\u{2060}');
            }
            out.push(c);
        }
        rest = &rest[at + len + 1..];
    }
    out.push_str(rest);
    out
}

fn is_fraction(v: f64) -> bool {
    (0.0..=1.0).contains(&v)
}

pub(crate) fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_block_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_ID_LEN
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Reverse-DNS application identifier, e.g. `com.apple.TextEdit`.
fn is_bundle_id(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() >= 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
}

fn is_web_address(s: &str) -> bool {
    ["http://", "https://"]
        .iter()
        .any(|scheme| s.len() > scheme.len() && s.starts_with(scheme))
}

/// A portable path relative to the test file's folder, written with `/`.
pub(crate) fn is_relative_path(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('/')
        && !s.starts_with('~')
        && !s.ends_with('/')
        && !s.contains('\\')
        && !s.contains('\0')
        && !s.split('/').any(str::is_empty)
        && !s.as_bytes().get(1).is_some_and(|&b| b == b':')
}

const MODIFIERS: &[&str] = &["cmd", "shift", "option", "ctrl"];
const NAMED_KEYS: &[&str] = &[
    "return",
    "tab",
    "space",
    "delete",
    "forwarddelete",
    "escape",
    "up",
    "down",
    "left",
    "right",
    "home",
    "end",
    "pageup",
    "pagedown",
];

/// `cmd+shift+s`: distinct lowercase modifiers, then one key — a character or a named key.
fn is_key_combo(s: &str) -> bool {
    let tokens: Vec<&str> = s.split('+').collect();
    let Some((key, modifiers)) = tokens.split_last() else {
        return false;
    };
    let mut seen = HashSet::new();
    let modifiers_ok = modifiers
        .iter()
        .all(|m| MODIFIERS.contains(m) && seen.insert(*m));
    let mut chars = key.chars();
    let single_char = matches!((chars.next(), chars.next()), (Some(c), None)
        if c.is_ascii_lowercase() || c.is_ascii_digit() || "-=[]\\;',./`".contains(c));
    let function_key = key
        .strip_prefix('f')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=20).contains(&n) && !key.starts_with("f0"));
    modifiers_ok && (single_char || function_key || NAMED_KEYS.contains(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_combos() {
        for ok in [
            "cmd+s",
            "cmd+shift+s",
            "return",
            "f12",
            "ctrl+option+space",
            "cmd+/",
        ] {
            assert!(is_key_combo(ok), "{ok}");
        }
        for bad in [
            "",
            "cmd+",
            "cmd",
            "cmd+cmd+s",
            "Cmd+S",
            "s+cmd",
            "f0",
            "f21",
            "f01",
            "cmd++",
            "enterr",
        ] {
            assert!(!is_key_combo(bad), "{bad}");
        }
    }

    #[test]
    fn placeholder_parsing() {
        assert_eq!(
            placeholders("${secret.PASS} and ${data.user_1}"),
            vec![Ok(("secret", "PASS")), Ok(("data", "user_1"))]
        );
        assert_eq!(
            placeholders("${env.X} ${host.X} ${secret.} ${secret.A"),
            vec![
                Ok(("env", "X")),
                Err("${host.X}"),
                Err("${secret.}"),
                Err("${secret.A")
            ]
        );
        assert!(placeholders("plain $ text {}").is_empty());
    }

    #[test]
    fn placeholders_never_break_across_lines() {
        let j = "\u{2060}";
        assert_eq!(
            unbreakable_placeholders("Hello, ${data.name}!"),
            format!("Hello, ${j}{{{j}d{j}a{j}t{j}a{j}.{j}n{j}a{j}m{j}e{j}}}!")
        );
        assert_eq!(
            unbreakable_placeholders("${a}${b} $5 {x} ${open"),
            format!("${j}{{{j}a{j}}}${j}{{{j}b{j}}} $5 {{x}} ${{open")
        );
    }

    #[test]
    fn paths_and_ids() {
        for ok in ["refs/a.png", "../shared/login.flow.json", "a.csv"] {
            assert!(is_relative_path(ok), "{ok}");
        }
        for bad in ["", "/etc/x", "~/x", "a//b", "a\\b", "C:x", "dir/"] {
            assert!(!is_relative_path(bad), "{bad}");
        }
        assert!(is_bundle_id("com.apple.TextEdit") && is_bundle_id("org.my-app.x1"));
        assert!(!is_bundle_id("TextEdit") && !is_bundle_id("com..x") && !is_bundle_id("com.a b"));
        assert!(is_block_id("open-app_2") && !is_block_id("open app") && !is_block_id(""));
    }

    #[test]
    fn cycles_are_found_once_per_back_edge() {
        // 0 → 1 → 2 → 0, and 3 → 3.
        let found = cycles(&[vec![1], vec![2], vec![0], vec![3]]);
        assert_eq!(found, vec![vec![0, 1, 2], vec![3]]);
        assert!(cycles(&[vec![1, 2], vec![2], vec![]]).is_empty());
    }
}
