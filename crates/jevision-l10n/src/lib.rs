//! Texts for people, read from the one string catalog the apps use.
//!
//! `apps/macos/Resources/Localizable.xcstrings` is the only place where a phrase lives. The core
//! hands out a key and the values it refers to; whoever shows the text translates it here, in the
//! language the person chose. A key has the English text and every other language next to it, and
//! a missing language falls back to English, then to the key itself (which the catalog check keeps
//! from ever being shown).
//!
//! Values go in with the format the catalog already uses: `%@` for text, `%lld` for whole numbers,
//! `%1$@` when a language needs another order, `%%` for a percent sign. A phrase that depends on a
//! count has plural variations and is chosen by the first number among the values.
//!
//! A new language needs nothing but its column in the catalog; only a language with plural
//! forms other than "one" and "other" needs a rule in [`plural_category`].

use serde_json::Value;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

const CATALOG: &str = include_str!("../../../apps/macos/Resources/Localizable.xcstrings");

/// The language the catalog's keys are written in, and where a missing translation falls back to.
pub const SOURCE_LANGUAGE: &str = "en";

/// A value a phrase refers to. Text goes into `%@`, a number into `%lld`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Arg {
    Text(String),
    Number(i64),
}

impl Arg {
    fn text(&self) -> String {
        match self {
            Arg::Text(t) => t.clone(),
            Arg::Number(n) => n.to_string(),
        }
    }

    fn number(&self) -> Option<i64> {
        match self {
            Arg::Number(n) => Some(*n),
            Arg::Text(t) => t.trim().parse().ok(),
        }
    }
}

impl From<&str> for Arg {
    fn from(value: &str) -> Self {
        Arg::Text(value.to_owned())
    }
}

impl From<String> for Arg {
    fn from(value: String) -> Self {
        Arg::Text(value)
    }
}

impl From<&String> for Arg {
    fn from(value: &String) -> Self {
        Arg::Text(value.clone())
    }
}

macro_rules! number_args {
    ($($t:ty),*) => {$(
        impl From<$t> for Arg {
            fn from(value: $t) -> Self {
                Arg::Number(i64::from(value))
            }
        }
    )*};
}
number_args!(i8, i16, i32, i64, u8, u16, u32);

impl From<usize> for Arg {
    fn from(value: usize) -> Self {
        Arg::Number(i64::try_from(value).unwrap_or(i64::MAX))
    }
}

impl From<u64> for Arg {
    fn from(value: u64) -> Self {
        Arg::Number(i64::try_from(value).unwrap_or(i64::MAX))
    }
}

/// One language's text for a key.
enum Text {
    Plain(String),
    Plural(HashMap<String, String>),
}

type Catalog = HashMap<String, HashMap<String, Text>>;

fn catalog() -> &'static Catalog {
    static LOADED: OnceLock<Catalog> = OnceLock::new();
    LOADED.get_or_init(|| parse(CATALOG))
}

fn parse(json: &str) -> Catalog {
    let root: Value = serde_json::from_str(json).expect("the string catalog is valid JSON");
    let mut catalog = Catalog::new();
    let Some(strings) = root.get("strings").and_then(Value::as_object) else {
        return catalog;
    };
    for (key, entry) in strings {
        let Some(localizations) = entry.get("localizations").and_then(Value::as_object) else {
            continue;
        };
        let mut languages = HashMap::new();
        for (language, localization) in localizations {
            if let Some(text) = text_of(localization) {
                languages.insert(language.clone(), text);
            }
        }
        catalog.insert(key.clone(), languages);
    }
    catalog
}

fn text_of(localization: &Value) -> Option<Text> {
    if let Some(value) = localization
        .pointer("/stringUnit/value")
        .and_then(Value::as_str)
    {
        return Some(Text::Plain(value.to_owned()));
    }
    let plural = localization.pointer("/variations/plural")?.as_object()?;
    let forms = plural
        .iter()
        .filter_map(|(category, form)| {
            let value = form.pointer("/stringUnit/value")?.as_str()?;
            Some((category.clone(), value.to_owned()))
        })
        .collect();
    Some(Text::Plural(forms))
}

/// A language tag as the catalog names it: `ru_RU.UTF-8` and `ru-RU` both give `ru`.
pub fn base_language(tag: &str) -> String {
    let tag = tag.split(['.', '@']).next().unwrap_or("");
    let base = tag.split(['-', '_']).next().unwrap_or("");
    match base.to_ascii_lowercase().as_str() {
        "" | "c" | "posix" => SOURCE_LANGUAGE.to_owned(),
        other => other.to_owned(),
    }
}

static LANGUAGE: RwLock<String> = RwLock::new(String::new());

/// The language of texts built without an explicit one, such as an error's `Display`.
pub fn language() -> String {
    let current = LANGUAGE.read().map(|l| l.clone()).unwrap_or_default();
    if current.is_empty() {
        SOURCE_LANGUAGE.to_owned()
    } else {
        current
    }
}

/// Sets the language for texts built without an explicit one. Any tag works; the catalog decides
/// what is translated.
pub fn set_language(tag: &str) {
    if let Ok(mut current) = LANGUAGE.write() {
        *current = base_language(tag);
    }
}

/// The text of `key` in `lang` with `args` put in. Falls back to English, then to the key.
pub fn text_in(lang: &str, key: &str, args: &[Arg]) -> String {
    render(catalog(), lang, key, args)
}

fn render(catalog: &Catalog, lang: &str, key: &str, args: &[Arg]) -> String {
    let Some(languages) = catalog.get(key) else {
        return key.to_owned();
    };
    let lang = base_language(lang);
    let used = if languages.contains_key(&lang) {
        lang.as_str()
    } else {
        SOURCE_LANGUAGE
    };
    // A key without an English text is itself the English text, as in the apps' catalog.
    let Some(found) = languages.get(used) else {
        return substitute(key, args);
    };
    match found {
        Text::Plain(template) => substitute(template, args),
        Text::Plural(forms) => {
            let count = plural_count(forms, args);
            let category = count.map_or("other", |n| plural_category(used, n));
            let template = count
                .filter(|&n| n == 0)
                .and_then(|_| forms.get("zero"))
                .or_else(|| forms.get(category))
                .or_else(|| forms.get("other"))
                .map_or("", String::as_str);
            substitute(template, args)
        }
    }
}

/// [`text_in`] in the language set with [`set_language`].
pub fn text(key: &str, args: &[Arg]) -> String {
    text_in(&language(), key, args)
}

/// Whether the catalog has `key` at all.
pub fn has_key(key: &str) -> bool {
    catalog().contains_key(key)
}

/// Whether `key` has a text in `lang` itself, with no fallback to English.
pub fn is_translated(lang: &str, key: &str) -> bool {
    catalog()
        .get(key)
        .is_some_and(|languages| languages.contains_key(&base_language(lang)))
}

/// Every key of the catalog.
pub fn keys() -> impl Iterator<Item = &'static str> {
    catalog().keys().map(String::as_str)
}

/// The languages the catalog has texts in: the source language first, then the others by code.
/// A language added to the catalog shows up here without a change in code.
pub fn languages() -> &'static [String] {
    static FOUND: OnceLock<Vec<String>> = OnceLock::new();
    FOUND.get_or_init(|| {
        let mut found: Vec<String> = catalog()
            .values()
            .flat_map(|languages| languages.keys().cloned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        found.retain(|language| language != SOURCE_LANGUAGE);
        found.insert(0, SOURCE_LANGUAGE.to_owned());
        found
    })
}

/// `text_in` with the values written inline: `tr_in!(lang, "run.secret.missing", name)`.
#[macro_export]
macro_rules! tr_in {
    ($lang:expr, $key:expr $(, $arg:expr)* $(,)?) => {
        $crate::text_in($lang, $key, &[$($crate::Arg::from($arg)),*])
    };
}

/// `text` with the values written inline: `tr!("run.secret.missing", name)`.
#[macro_export]
macro_rules! tr {
    ($key:expr $(, $arg:expr)* $(,)?) => {
        $crate::text($key, &[$($crate::Arg::from($arg)),*])
    };
}

/// A decimal number the way `lang` writes it: `1.5` or `1,5`.
pub fn decimal(lang: &str, value: f64, digits: usize) -> String {
    let written = format!("{value:.digits$}");
    match base_language(lang).as_str() {
        "ru" | "uk" | "be" | "de" | "fr" | "es" | "it" | "pt" | "pl" | "cs" | "tr" => {
            written.replace('.', ",")
        }
        _ => written,
    }
}

/// The plural form `lang` uses for the whole number `n`.
pub fn plural_category(lang: &str, n: i64) -> &'static str {
    match base_language(lang).as_str() {
        "ru" | "uk" | "be" => {
            let (ones, tens) = (n.rem_euclid(10), n.rem_euclid(100));
            if ones == 1 && tens != 11 {
                "one"
            } else if (2..=4).contains(&ones) && !(12..=14).contains(&tens) {
                "few"
            } else {
                "many"
            }
        }
        "ja" | "zh" | "ko" | "vi" | "th" => "other",
        _ if n == 1 => "one",
        _ => "other",
    }
}

/// The number a plural phrase is chosen by: the first numeric value among the arguments the
/// text uses, whatever order the text puts them in (`%2$lld of %1$lld rows` counts `%1$lld`).
fn plural_count(forms: &HashMap<String, String>, args: &[Arg]) -> Option<i64> {
    let template = forms.get("other").or_else(|| forms.values().next())?;
    let mut next = 0;
    let counted = specifiers(template)
        .into_iter()
        .filter_map(|spec| {
            let index = spec.position.unwrap_or_else(|| {
                next += 1;
                next - 1
            });
            spec.numeric.then_some(index)
        })
        .min();
    if let Some(index) = counted {
        return args.get(index).and_then(Arg::number);
    }
    args.iter().find_map(|a| match a {
        Arg::Number(n) => Some(*n),
        Arg::Text(_) => None,
    })
}

struct Spec {
    start: usize,
    end: usize,
    position: Option<usize>,
    numeric: bool,
    percent: bool,
}

/// The `%…` pieces of a template, in order.
fn specifiers(template: &str) -> Vec<Spec> {
    let bytes = template.as_bytes();
    let mut specs = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        if bytes.get(i) == Some(&b'%') {
            specs.push(Spec {
                start,
                end: i + 1,
                position: None,
                numeric: false,
                percent: true,
            });
            i += 1;
            continue;
        }
        let digits = i;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        let position = if i > digits && bytes.get(i) == Some(&b'$') {
            let n: usize = template[digits..i].parse().unwrap_or(1);
            i += 1;
            Some(n.saturating_sub(1))
        } else {
            i = digits;
            None
        };
        while matches!(bytes.get(i), Some(b'l' | b'h' | b'z' | b'q')) {
            i += 1;
        }
        match bytes.get(i) {
            Some(b'@' | b's') => {
                specs.push(Spec {
                    start,
                    end: i + 1,
                    position,
                    numeric: false,
                    percent: false,
                });
                i += 1;
            }
            Some(b'd' | b'i' | b'u') => {
                specs.push(Spec {
                    start,
                    end: i + 1,
                    position,
                    numeric: true,
                    percent: false,
                });
                i += 1;
            }
            _ => {}
        }
    }
    specs
}

fn substitute(template: &str, args: &[Arg]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut copied = 0;
    let mut next = 0;
    for spec in specifiers(template) {
        out.push_str(&template[copied..spec.start]);
        copied = spec.end;
        if spec.percent {
            out.push('%');
            continue;
        }
        let index = spec.position.unwrap_or_else(|| {
            next += 1;
            next - 1
        });
        if let Some(arg) = args.get(index) {
            out.push_str(&arg.text());
        }
    }
    out.push_str(&template[copied..]);
    out
}

#[cfg(test)]
mod tests;
