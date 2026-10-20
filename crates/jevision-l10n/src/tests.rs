use super::*;

const SAMPLE: &str = r#"{
  "sourceLanguage": "en",
  "strings": {
    "t.plain": {"localizations": {
      "en": {"stringUnit": {"state": "translated", "value": "Block “%@” failed: %@ (%lld%%)"}},
      "ru": {"stringUnit": {"state": "translated", "value": "Блок «%2$@» упал: %1$@ (%3$lld%%)"}}
    }},
    "t.enOnly": {"localizations": {
      "en": {"stringUnit": {"state": "translated", "value": "English only"}}
    }},
    "t.rows": {"localizations": {
      "en": {"variations": {"plural": {
        "one": {"stringUnit": {"state": "translated", "value": "%2$lld of %1$lld row passed"}},
        "other": {"stringUnit": {"state": "translated", "value": "%2$lld of %1$lld rows passed"}}
      }}}
    }},
    "t.files": {"localizations": {
      "en": {"variations": {"plural": {
        "one": {"stringUnit": {"state": "translated", "value": "%lld file in %@"}},
        "other": {"stringUnit": {"state": "translated", "value": "%lld files in %@"}}
      }}},
      "ru": {"variations": {"plural": {
        "one": {"stringUnit": {"state": "translated", "value": "%lld файл в %@"}},
        "few": {"stringUnit": {"state": "translated", "value": "%lld файла в %@"}},
        "many": {"stringUnit": {"state": "translated", "value": "%lld файлов в %@"}},
        "other": {"stringUnit": {"state": "translated", "value": "%lld файла в %@"}}
      }}}
    }}
  }
}"#;

fn sample(lang: &str, key: &str, args: &[Arg]) -> String {
    render(&parse(SAMPLE), lang, key, args)
}

#[test]
fn puts_values_in_order_and_by_position() {
    let args = [Arg::from("boom"), Arg::from("Sign in"), Arg::from(50)];
    assert_eq!(
        sample("en", "t.plain", &args),
        "Block “boom” failed: Sign in (50%)"
    );
    assert_eq!(
        sample("ru", "t.plain", &args),
        "Блок «Sign in» упал: boom (50%)"
    );
}

#[test]
fn falls_back_to_english_then_to_the_key() {
    assert_eq!(sample("ru", "t.enOnly", &[]), "English only");
    assert_eq!(
        sample("de", "t.plain", &["a".into(), "b".into(), 1.into()]),
        "Block “a” failed: b (1%)"
    );
    assert_eq!(sample("ru", "t.missing", &[]), "t.missing");
}

#[test]
fn reads_the_language_from_any_tag() {
    assert_eq!(base_language("ru_RU.UTF-8"), "ru");
    assert_eq!(base_language("en-US"), "en");
    assert_eq!(base_language("pt-BR"), "pt");
    assert_eq!(base_language("C"), "en");
    assert_eq!(base_language(""), "en");
    let args = [Arg::from("x"), Arg::from("y"), Arg::from(2)];
    assert_eq!(sample("ru-RU", "t.plain", &args), "Блок «y» упал: x (2%)");
}

#[test]
fn chooses_the_russian_plural_form() {
    let one_to_five = [
        (1, "файл"),
        (2, "файла"),
        (5, "файлов"),
        (21, "файл"),
        (11, "файлов"),
        (22, "файла"),
        (111, "файлов"),
        (0, "файлов"),
    ];
    for (n, word) in one_to_five {
        let text = sample("ru", "t.files", &[n.into(), "src".into()]);
        assert_eq!(text, format!("{n} {word} в src"), "n = {n}");
    }
}

#[test]
fn chooses_the_english_plural_form() {
    assert_eq!(
        sample("en", "t.files", &[1.into(), "src".into()]),
        "1 file in src"
    );
    assert_eq!(
        sample("en", "t.files", &[2.into(), "src".into()]),
        "2 files in src"
    );
    assert_eq!(
        sample("en", "t.files", &[0.into(), "src".into()]),
        "0 files in src"
    );
}

#[test]
fn plural_follows_the_numeric_value_even_when_text_comes_first() {
    assert_eq!(
        sample("ru", "t.files", &[3.into(), "src".into()]),
        "3 файла в src"
    );
}

#[test]
fn the_shipped_catalog_has_both_languages_for_run_texts() {
    let text = tr_in!("en", "run.secret.missing", "TOKEN");
    assert_eq!(text, "The secret TOKEN isn’t set. Set it in Test details.");
    let text = tr_in!("ru", "run.secret.missing", "TOKEN");
    assert!(text.contains("TOKEN") && !text.contains("isn’t"), "{text}");
    assert_eq!(tr_in!("en", "no.such.key"), "no.such.key");
    assert!(has_key("run.secret.missing"));
    assert!(is_translated("ru", "run.secret.missing"));
}

#[test]
fn the_shipped_plural_phrase_changes_with_the_count() {
    let key = "%@ is connected with %lld tools.";
    assert_eq!(
        text_in("en", key, &["Claude".into(), 1.into()]),
        "Claude is connected with 1 tool."
    );
    assert_eq!(
        text_in("en", key, &["Claude".into(), 3.into()]),
        "Claude is connected with 3 tools."
    );
    assert!(text_in("ru", key, &["Claude".into(), 5.into()]).ends_with("5 инструментов."));
}

// The language and the number format are one for the whole process, so this is the only test
// that sets them, and it puts them back.
#[test]
fn the_language_and_its_numbers_follow_what_the_host_sets() {
    assert_eq!(language(), "en");
    assert_eq!(decimal("en", 1234.5, 1), "1234.5");
    assert_eq!(decimal("ru", 1.5, 1), "1,5");
    assert_eq!(decimal("ru-RU", 12.0, 0), "12");

    set_language("ru_RU");
    assert_eq!(language(), "ru");
    assert_eq!(
        tr!("run.secret.missing", "A"),
        tr_in!("ru", "run.secret.missing", "A")
    );
    assert_eq!(decimal("ru", 1234.5, 1), "1234,5");
    set_number_format(",", "\u{a0}");
    assert_eq!(decimal("ru", -12345.25, 2), "-12\u{a0}345,25");
    assert_eq!(decimal("ru_RU.UTF-8", 999.0, 0), "999");
    // The host's separators are for its own language only.
    assert_eq!(decimal("en", 1234.5, 1), "1234.5");

    set_language("en-US");
    set_number_format(".", ",");
    assert_eq!(decimal("en", 1234567.0, 0), "1,234,567");
    assert_eq!(decimal("en", 0.25, 2), "0.25");
    assert_eq!(decimal("ru", 1234.5, 1), "1234,5");

    set_number_format("", "");
    set_language("");
    assert_eq!(language(), "en");
    assert_eq!(decimal("en", 1234.5, 1), "1234.5");
}

#[test]
fn plural_counts_the_number_that_comes_first_in_the_arguments() {
    // `%1$lld` is the total, so it picks the form even when the text shows it second.
    assert_eq!(
        sample("en", "t.rows", &[1.into(), 1.into()]),
        "1 of 1 row passed"
    );
    assert_eq!(
        sample("en", "t.rows", &[1.into(), 5.into()]),
        "5 of 1 row passed"
    );
    assert_eq!(
        sample("en", "t.rows", &[3.into(), 1.into()]),
        "1 of 3 rows passed"
    );
}

#[test]
fn lists_the_languages_of_the_catalog_with_english_first() {
    let found = languages();
    assert_eq!(found.first().map(String::as_str), Some("en"));
    assert!(found.iter().any(|language| language == "ru"));
    assert_eq!(
        found.len(),
        found.iter().collect::<std::collections::HashSet<_>>().len()
    );
}
