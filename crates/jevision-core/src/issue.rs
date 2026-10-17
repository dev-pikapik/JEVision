use serde::ser::{SerializeMap, Serializer};

/// How serious a problem is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// The test cannot run as written.
    Error,
    /// The test runs, but probably not the way its author meant.
    Warning,
}

/// A problem found in a test file.
///
/// An issue carries a localization key (`code`) and named parameters instead of a finished
/// sentence, so every frontend shows it in its own language. Parameters keep the order in which
/// they were added.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Issue {
    pub code: String,
    pub severity: Severity,
    #[serde(serialize_with = "params_as_map")]
    pub params: Vec<IssueParam>,
}

/// One named value referenced by an issue's localized text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueParam {
    pub name: String,
    pub value: String,
}

impl Issue {
    pub(crate) fn error(code: &str) -> Self {
        Self {
            code: code.to_owned(),
            severity: Severity::Error,
            params: Vec::new(),
        }
    }

    pub(crate) fn warning(code: &str) -> Self {
        Self {
            severity: Severity::Warning,
            ..Self::error(code)
        }
    }

    pub(crate) fn with(mut self, name: &str, value: impl ToString) -> Self {
        self.params.push(IssueParam {
            name: name.to_owned(),
            value: value.to_string(),
        });
        self
    }

    /// Value of the parameter called `name`, if the issue has one.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.value.as_str())
    }
}

fn params_as_map<S: Serializer>(params: &[IssueParam], s: S) -> Result<S::Ok, S::Error> {
    let mut map = s.serialize_map(Some(params.len()))?;
    for p in params {
        map.serialize_entry(&p.name, &p.value)?;
    }
    map.end()
}

/// The text is not a readable test file. Holds every problem found while reading it.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("test file cannot be read: {}", .issues.iter().map(|i| i.code.as_str()).collect::<Vec<_>>().join(", "))]
pub struct ParseError {
    pub issues: Vec<Issue>,
}
