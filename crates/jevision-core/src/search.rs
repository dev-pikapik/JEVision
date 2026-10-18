//! Ranking for the editor's search fields: the block palette, the command palette and the
//! project's test list.

use crate::{BlockKind, Compare, Flow, Reference, Target, WaitUntil};

/// Something a search can find: what it is called, and other words it answers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchCandidate {
    pub title: String,
    /// Other words the candidate is found by, like its description, group or folder.
    pub keywords: String,
}

/// Indices of the candidates that match `query`, best first; with an empty query, all of them
/// in the order given. Every word of the query must be found in the title or the keywords,
/// ignoring case. Per word, a title that starts with it beats a title with a word that starts
/// with it, then a title that contains it, then a title holding its letters in order, then the
/// keywords. Among equal matches, a title that is the query itself goes first, then the order
/// given.
pub fn rank(query: &str, candidates: &[SearchCandidate]) -> Vec<usize> {
    let query = fold(query);
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut scored: Vec<(u32, bool, usize)> = candidates
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let title = fold(&c.title);
            let keywords = fold(&c.keywords);
            words
                .iter()
                .try_fold(0, |sum, word| Some(sum + score(word, &title, &keywords)?))
                .map(|total| (total, title.split_whitespace().ne(words.iter().copied()), i))
        })
        .collect();
    scored.sort_unstable();
    scored.into_iter().map(|(_, _, i)| i).collect()
}

/// Words a test is found by besides its file name: its name, description and tags, what its
/// blocks type, press, open and look for, and the text of its canvas notes and sections.
pub fn test_keywords(flow: &Flow) -> String {
    let mut words: Vec<&str> = vec![&flow.name];
    words.extend(flow.description.as_deref());
    words.extend(flow.tags.iter().map(String::as_str));
    fn reference<'a>(r: &'a Reference, words: &mut Vec<&'a str>) {
        words.push(&r.image);
        if let Compare::Meaning { expect, model } = &r.compare {
            words.push(expect);
            words.push(model);
        }
    }
    for block in &flow.blocks {
        match &block.kind {
            BlockKind::OpenApp { app, .. } => words.extend(app.as_deref()),
            BlockKind::OpenFile { files, app, .. } => {
                words.extend(files.iter().map(String::as_str));
                words.extend(app.as_deref());
            }
            BlockKind::Click { target, .. } => {
                if let Target::Image(image) = target {
                    words.push(image);
                }
            }
            BlockKind::Type { text, into, .. } => {
                words.push(text);
                if let Some(Target::Image(image)) = into {
                    words.push(image);
                }
            }
            BlockKind::PressKeys { keys, .. } => words.push(keys),
            BlockKind::WaitFor { until, .. } => match until {
                WaitUntil::Appears(r) | WaitUntil::Disappears(r) => reference(r, &mut words),
                WaitUntil::ScreenStable => {}
            },
            BlockKind::Check { reference: r, .. } => reference(r, &mut words),
            BlockKind::If {
                reference: r,
                else_if,
                ..
            } => {
                reference(r, &mut words);
                for branch in else_if {
                    reference(&branch.reference, &mut words);
                }
            }
            BlockKind::Repeat { .. } => {}
            BlockKind::UseData { file, .. } => words.push(file),
            BlockKind::RunTest { test, .. } => words.push(test),
        }
    }
    words.extend(flow.canvas.notes.iter().map(|n| n.text.as_str()));
    words.extend(flow.canvas.sections.iter().map(|s| s.title.as_str()));
    words.retain(|w| !w.is_empty());
    words.join(" ")
}

/// How well `word` matches, lower is better; `None` when it doesn't.
fn score(word: &str, title: &str, keywords: &str) -> Option<u32> {
    let starts_a_word = |text: &str| words(text).any(|w| w.starts_with(word));
    if title.starts_with(word) {
        Some(0)
    } else if starts_a_word(title) {
        Some(1)
    } else if title.contains(word) {
        Some(2)
    } else if word.chars().count() >= 3 && in_order(word, title) {
        Some(4)
    } else if starts_a_word(keywords) {
        Some(5)
    } else if keywords.contains(word) {
        Some(6)
    } else {
        None
    }
}

fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
}

/// Whether the letters of `word` appear in `text` in the same order, with gaps allowed.
fn in_order(word: &str, text: &str) -> bool {
    let mut rest = text.chars();
    word.chars().all(|c| rest.any(|t| t == c))
}

/// Lowercase, with "ё" read as "е" as Russian readers type it.
fn fold(text: &str) -> String {
    text.to_lowercase().replace('ё', "е")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(query: &str, titles: &[(&str, &str)]) -> Vec<String> {
        let candidates: Vec<SearchCandidate> = titles
            .iter()
            .map(|(title, keywords)| SearchCandidate {
                title: (*title).to_owned(),
                keywords: (*keywords).to_owned(),
            })
            .collect();
        rank(query, &candidates)
            .into_iter()
            .map(|i| titles[i].0.to_owned())
            .collect()
    }

    const COMMANDS: &[(&str, &str)] = &[
        ("Record", "steps"),
        ("Run Step by Step", "debug"),
        ("Run", "test start"),
        (
            "Check",
            "Checks: Fails the test unless the screen looks as expected.",
        ),
        ("Click", "Actions"),
        ("Open Report", "last run result"),
    ];

    #[test]
    fn an_empty_query_keeps_everything_in_order() {
        assert_eq!(found("  ", COMMANDS).len(), COMMANDS.len());
        assert_eq!(found("", COMMANDS)[0], "Record");
    }

    #[test]
    fn titles_that_start_with_the_query_come_first() {
        assert_eq!(
            found("ru", COMMANDS),
            ["Run Step by Step", "Run", "Open Report"]
        );
        // The title that is the query itself comes first.
        assert_eq!(
            found("RUN", COMMANDS),
            ["Run", "Run Step by Step", "Open Report"]
        );
    }

    #[test]
    fn a_word_inside_the_title_beats_the_keywords() {
        assert_eq!(found("step", COMMANDS), ["Run Step by Step", "Record"]);
    }

    #[test]
    fn every_word_must_match() {
        assert_eq!(found("run step", COMMANDS), ["Run Step by Step"]);
        assert!(found("run zebra", COMMANDS).is_empty());
    }

    #[test]
    fn letters_in_order_find_a_title() {
        assert_eq!(found("chk", COMMANDS), ["Check"]);
        assert_eq!(found("cnl", &[("Cancel", "")]), ["Cancel"]);
        // Two letters are too few to guess from.
        assert!(found("cl", &[("Cancel", "")]).is_empty());
    }

    #[test]
    fn keywords_find_what_the_title_does_not() {
        assert_eq!(found("debug", COMMANDS), ["Run Step by Step"]);
        assert_eq!(found("result", COMMANDS), ["Open Report"]);
    }

    #[test]
    fn case_and_yo_are_ignored() {
        let titles = &[("Ждать", ""), ("Ещё", "")];
        assert_eq!(found("ЖД", titles), ["Ждать"]);
        assert_eq!(found("еще", titles), ["Ещё"]);
    }

    #[test]
    fn a_test_is_found_by_its_name_description_and_blocks() {
        let flow = crate::parse(
            r#"{"format":1,"name":"Add a line to a note","description":"Keeps the draft",
            "app":"com.apple.Notes","start":"a","blocks":{
            "a":{"type":"type","text":"Hello, ${data.name}!","next":"b"},
            "b":{"type":"check","reference":{"image":"","compare":"meaning","expect":"A saved note"}}}}"#,
        )
        .unwrap();
        let keywords = test_keywords(&flow);
        for words in ["note 01", "draft", "hello", "saved note"] {
            let candidate = SearchCandidate {
                title: "flow-012".to_owned(),
                keywords: keywords.clone(),
            };
            assert_eq!(rank(words, &[candidate]), [0], "{words}");
        }
    }
}
