//! Ranking for the editor's search fields: the block palette and the command palette.

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
}
