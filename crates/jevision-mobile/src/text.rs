//! The words the phone shows for what the Mac reports. Every phrase is a key of the shared
//! catalog; the app picks the language once with [`set_language`] and gets ready sentences, so
//! the phone words a failure exactly as the Mac does.

use crate::Failure;

/// Sets the language of every text this library produces. A tag as the system writes it
/// (`ru`, `ru-RU`, `en_US`); a language without a catalog falls back to English.
#[uniffi::export]
pub fn set_language(tag: String) {
    jevision_l10n::set_language(&tag);
}

/// Sets how numbers are written in the language of [`set_language`]: the system's decimal
/// separator and the one between groups of thousands, such as `,` and a no-break space. An empty
/// `decimal` goes back to the language's own separator.
#[uniffi::export]
pub fn set_number_format(decimal: String, grouping: String) {
    jevision_l10n::set_number_format(&decimal, &grouping);
}

/// The language the texts are in now: a code of the catalog.
#[uniffi::export]
pub fn current_language() -> String {
    jevision_l10n::language()
}

/// A value to put into a catalog text.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum TextArg {
    Text { value: String },
    Number { value: i64 },
}

/// The text of a catalog key with `args` put in, in the current language.
#[uniffi::export]
pub fn localized_text(key: String, args: Vec<TextArg>) -> String {
    let args: Vec<jevision_l10n::Arg> = args
        .into_iter()
        .map(|a| match a {
            TextArg::Text { value } => jevision_l10n::Arg::from(value),
            TextArg::Number { value } => jevision_l10n::Arg::from(value),
        })
        .collect();
    jevision_l10n::text(&key, &args)
}

impl From<&Failure> for jevision_runner::Failure {
    fn from(f: &Failure) -> Self {
        Self {
            code: f.code.clone(),
            params: f.params.clone().into_iter().collect(),
        }
    }
}

/// The sentence for a failure the Mac reported, in the current language.
#[uniffi::export]
pub fn describe_failure(failure: Failure) -> String {
    jevision_runner::describe(&(&failure).into())
}

/// The sentence for a problem with an AI model, without the block it happened in.
#[uniffi::export]
pub fn describe_vision_failure(failure: Failure) -> String {
    jevision_runner::describe_vision(&(&failure).into())
}

/// Which way an `if` went: `Took “Condition 2: Inbox is empty”`. `None` for other blocks.
#[uniffi::export]
pub fn branch_text(
    branch: String,
    branches: Option<u32>,
    condition: Option<String>,
) -> Option<String> {
    jevision_runner::branch_text(&branch, branches, condition.as_deref())
}
