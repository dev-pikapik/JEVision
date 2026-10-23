//! The language the core writes in is one for the whole process, so it is set only here, in a
//! test binary of its own.

use jevision_ffi::*;

#[test]
fn texts_follow_the_language_set_until_it_changes() {
    let before = current_language();
    let count = |n| localized_text("mcp.tests.count".into(), vec![TextArg::Number { value: n }]);
    let no_key = || Failure {
        code: "run.vision.noKey".into(),
        params: [("provider".to_owned(), "Anthropic".to_owned())].into(),
        message: String::new(),
    };
    let unreadable = || load_flow("{".into()).unwrap_err().to_string();

    set_language("ru-RU".into());
    assert_eq!(current_language(), "ru");
    assert_eq!(
        [count(1), count(2), count(5)],
        ["1 тест.", "2 теста.", "5 тестов."]
    );
    assert_eq!(
        localized_text(
            "mcp.run.notReady".into(),
            vec![TextArg::Text {
                value: "Экран заблокирован.".into()
            }]
        ),
        "Этот Mac сейчас не может запускать тесты. Экран заблокирован."
    );
    assert_eq!(
        describe_vision_failure(no_key()),
        "Ключ API для Anthropic не задан. Задайте его в Настройках → ИИ."
    );
    assert_eq!(unreadable(), "не удаётся прочитать файл теста");
    assert!(nothing_checked().starts_with("Прогон завершён"));

    set_language("en".into());
    assert_eq!(
        [count(1), count(2), count(5)],
        ["1 test.", "2 tests.", "5 tests."]
    );
    assert_eq!(
        describe_vision_failure(no_key()),
        "The API key for Anthropic isn’t set. Set it in Settings → AI."
    );
    assert_eq!(unreadable(), "test file cannot be read");
    assert!(nothing_checked().starts_with("Finished, but nothing was checked"));

    set_language(before.clone());
    assert_eq!(current_language(), before);
}
