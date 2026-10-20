//! Choosing tests by the fields of their cards, for the sidebar and Run All.

use crate::decode::{PRIORITIES, STATUSES, TEST_TYPES};
use crate::model::{Flow, Priority, TestStatus, TestType};

/// What a test's card says that tests can be chosen by.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TestCard {
    pub test_type: Option<TestType>,
    pub priority: Option<Priority>,
    pub status: Option<TestStatus>,
    /// Who wrote the test or looks after it; `None` when the card doesn't say.
    pub author: Option<String>,
    /// Marked flaky in the card, or found flaky from its runs.
    pub flaky: bool,
}

impl TestCard {
    /// The card of `flow`; `flaky_by_runs` says its runs found it flaky.
    pub fn of(flow: &Flow, flaky_by_runs: bool) -> Self {
        Self {
            test_type: flow.test_type,
            priority: flow.priority,
            status: flow.status,
            author: flow.author.clone(),
            flaky: flow.flaky || flaky_by_runs,
        }
    }
}

/// Which tests to choose: those whose cards say everything the filter asks for. A field left
/// out asks nothing; a test whose card doesn't say a field the filter asks for isn't chosen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TestFilter {
    pub test_type: Option<TestType>,
    pub priority: Option<Priority>,
    pub status: Option<TestStatus>,
    /// The author's name, compared without regard to case or blanks around it.
    pub author: Option<String>,
    /// Only flaky tests.
    pub flaky: bool,
}

impl TestFilter {
    /// The filter for what a person or an assistant wrote: the type, priority and status as
    /// they appear in test files (`smoke`, `high`, `ready`; case doesn't matter), the author's
    /// name, whether only flaky tests count. A blank word asks nothing; `Err` says which word
    /// isn't one and what would be.
    pub fn from_words(
        test_type: Option<&str>,
        priority: Option<&str>,
        status: Option<&str>,
        author: Option<&str>,
        flaky: bool,
    ) -> Result<Self, String> {
        Ok(Self {
            test_type: word("type", test_type, TEST_TYPES)?,
            priority: word("priority", priority, PRIORITIES)?,
            status: word("status", status, STATUSES)?,
            author: author
                .map(str::trim)
                .filter(|a| !a.is_empty())
                .map(str::to_owned),
            flaky,
        })
    }

    /// The filter asks nothing: every test is chosen.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn matches(&self, card: &TestCard) -> bool {
        self.test_type.is_none_or(|t| card.test_type == Some(t))
            && self.priority.is_none_or(|p| card.priority == Some(p))
            && self.status.is_none_or(|s| card.status == Some(s))
            && self.author.as_deref().is_none_or(|wanted| {
                card.author
                    .as_deref()
                    .is_some_and(|a| a.trim().eq_ignore_ascii_case(wanted.trim()))
            })
            && (!self.flaky || card.flaky)
    }

    /// Positions of the cards the filter chooses, in order.
    pub fn choose(&self, cards: &[TestCard]) -> Vec<usize> {
        (0..cards.len())
            .filter(|&i| self.matches(&cards[i]))
            .collect()
    }
}

fn word<T: Copy>(what: &str, text: Option<&str>, names: &[(&str, T)]) -> Result<Option<T>, String> {
    let Some(text) = text.map(str::trim).filter(|t| !t.is_empty()) else {
        return Ok(None);
    };
    names
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(text))
        .map(|&(_, value)| Some(value))
        .ok_or_else(|| {
            let known: Vec<&str> = names.iter().map(|(name, _)| *name).collect();
            format!(
                "\"{text}\" isn't a test {what}; use {} or {}",
                known[..known.len() - 1].join(", "),
                known[known.len() - 1]
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(priority: Option<Priority>, author: Option<&str>, flaky: bool) -> TestCard {
        TestCard {
            test_type: Some(TestType::Smoke),
            priority,
            status: Some(TestStatus::Ready),
            author: author.map(str::to_owned),
            flaky,
        }
    }

    #[test]
    fn an_empty_filter_chooses_every_test() {
        let filter = TestFilter::default();
        assert!(filter.is_empty());
        assert!(filter.matches(&TestCard::default()));
    }

    #[test]
    fn a_test_must_say_everything_the_filter_asks() {
        let cards = [
            card(Some(Priority::High), Some("Anna Lee"), false),
            card(Some(Priority::Low), Some("anna lee "), true),
            card(None, None, false),
        ];
        let by = |filter: TestFilter| filter.choose(&cards);
        let priority = TestFilter {
            priority: Some(Priority::High),
            ..Default::default()
        };
        assert_eq!(by(priority.clone()), [0]);
        let author = TestFilter {
            author: Some(" Anna Lee".into()),
            ..Default::default()
        };
        assert_eq!(by(author.clone()), [0, 1]);
        let flaky = TestFilter {
            flaky: true,
            ..Default::default()
        };
        assert_eq!(by(flaky.clone()), [1]);
        let all = TestFilter {
            test_type: Some(TestType::Smoke),
            status: Some(TestStatus::Ready),
            ..author
        };
        assert_eq!(by(all), [0, 1]);
        assert!(
            by(TestFilter {
                status: Some(TestStatus::Draft),
                ..priority
            })
            .is_empty()
        );
        assert!(!flaky.is_empty());
    }

    #[test]
    fn a_filter_is_made_from_words() {
        let filter = TestFilter::from_words(
            Some("Smoke"),
            Some(" high "),
            Some(""),
            Some(" Anna "),
            true,
        )
        .unwrap();
        assert_eq!(
            filter,
            TestFilter {
                test_type: Some(TestType::Smoke),
                priority: Some(Priority::High),
                status: None,
                author: Some("Anna".into()),
                flaky: true,
            }
        );
        assert!(
            TestFilter::from_words(None, None, None, Some(" "), false)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            TestFilter::from_words(None, Some("urgent"), None, None, false),
            Err("\"urgent\" isn't a test priority; use critical, high, medium or low".into())
        );
    }

    #[test]
    fn the_card_of_a_flow_is_flaky_when_marked_or_found() {
        let flow = crate::parse(
            r#"{"format":1,"name":"t","app":"a","priority":"high","author":"Anna",
                "start":"s","blocks":{"s":{"type":"openApp"}}}"#,
        )
        .unwrap();
        let card = TestCard::of(&flow, false);
        assert_eq!(card.priority, Some(Priority::High));
        assert_eq!(card.author.as_deref(), Some("Anna"));
        assert!(!card.flaky);
        assert!(TestCard::of(&flow, true).flaky);
    }
}
