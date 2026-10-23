//! A CI runner starts every job in a fresh checkout, without the run folders of the jobs before.

use std::collections::HashMap;

use jevision_runner::{
    Event, Hands, HandsError, Modifiers, MouseButton, Openable, Point, RunControl, RunOptions,
    Runner, Screenshot, ScrollDirection, ScrollUnit, WindowPosition, WindowSize,
};

use super::*;

/// Hands that press keys, or fail to when `broken`, with no window to look at.
struct Keys {
    broken: bool,
}

impl Hands for Keys {
    fn open_app(&self, _: &str, _: bool) -> Result<(), HandsError> {
        Ok(())
    }
    fn arrange_window(
        &self,
        _: &str,
        _: Option<WindowSize>,
        _: Option<WindowPosition>,
    ) -> Result<(), HandsError> {
        Ok(())
    }
    fn open(&self, _: &[Openable], _: Option<&str>, _: bool) -> Result<(), HandsError> {
        Ok(())
    }
    fn close(&self, _: &[Openable], _: Option<&str>) -> Result<(), HandsError> {
        Ok(())
    }
    fn activate(&self, _: &str) -> Result<(), HandsError> {
        Ok(())
    }
    fn quit(&self, _: &str) -> Result<(), HandsError> {
        Ok(())
    }
    fn capture(&self, app: &str) -> Result<Screenshot, HandsError> {
        Err(HandsError::NoWindow(app.to_owned()))
    }
    fn locate(&self, _: &str, _: &[u8], _: f64) -> Result<Option<Point>, HandsError> {
        Ok(None)
    }
    fn click(
        &self,
        _: &str,
        _: Point,
        _: MouseButton,
        _: bool,
        _: Modifiers,
    ) -> Result<(), HandsError> {
        Ok(())
    }
    fn hover(&self, _: &str, _: Point, _: Modifiers) -> Result<(), HandsError> {
        Ok(())
    }
    fn drag(&self, _: &str, _: &[Point], _: MouseButton, _: Modifiers) -> Result<(), HandsError> {
        Ok(())
    }
    fn scroll(
        &self,
        _: &str,
        _: Point,
        _: ScrollDirection,
        _: u32,
        _: ScrollUnit,
        _: Modifiers,
    ) -> Result<(), HandsError> {
        Ok(())
    }
    fn type_text(&self, _: &str) -> Result<(), HandsError> {
        Ok(())
    }
    fn press_keys(&self, keys: &str) -> Result<(), HandsError> {
        if self.broken {
            Err(HandsError::Failed(format!("{keys} did nothing")))
        } else {
            Ok(())
        }
    }
    fn clipboard(&self) -> Result<String, HandsError> {
        Ok(String::new())
    }
    fn set_clipboard(&self, _: &str) -> Result<(), HandsError> {
        Ok(())
    }
    fn clipboard_image(&self) -> Result<Option<Vec<u8>>, HandsError> {
        Ok(None)
    }
    fn clipboard_files(&self) -> Result<Vec<String>, HandsError> {
        Ok(Vec::new())
    }
    fn set_clipboard_image(&self, _: &[u8]) -> Result<(), HandsError> {
        Ok(())
    }
    fn user_input_detected(&self) -> bool {
        false
    }
    fn start_video(&self, _: &str, _: &Path, _: &[jevision_core::Rect]) -> Result<(), HandsError> {
        Ok(())
    }
    fn stop_video(&self) -> Result<(), HandsError> {
        Ok(())
    }
}

#[test]
fn a_fresh_checkout_tells_changed_and_fixed_from_the_kept_last_results() {
    let project = folder("ci");
    let cache = folder("ci-cache");
    fs::write(
        project.join("t.flow.json"),
        r#"{"format":1,"name":"T","app":"com.example.App","start":"keys","blocks":{"keys":{"type":"pressKeys","keys":"cmd+n"}}}"#,
    )
    .unwrap();
    // One job: what the cache kept is put back into a checkout without `.jevision`, the test
    // runs, and the cache keeps the file again, as the generated workflow does.
    let job = |broken: bool| {
        fs::remove_dir_all(project.join(".jevision")).ok();
        if let Ok(kept) = fs::read(cache.join("last-results.json")) {
            fs::create_dir_all(project.join(".jevision")).unwrap();
            fs::write(project.join(".jevision/last-results.json"), kept).unwrap();
        }
        let secrets = HashMap::new();
        let events = |_: &Event| {};
        let runner = Runner {
            hands: &Keys { broken },
            secrets: &secrets,
            events: &events,
            control: RunControl::new(),
            vision: None,
            fallback: None,
            providers: &[],
            usage: None,
            locator: None,
        };
        let run = runner
            .run(&project, Path::new("t.flow.json"), &RunOptions::default())
            .unwrap();
        let runs = fs::read_dir(project.join(".jevision/runs")).unwrap();
        assert_eq!(runs.count(), 1, "the job sees only its own run");
        fs::copy(
            project.join(".jevision/last-results.json"),
            cache.join("last-results.json"),
        )
        .ok();
        Finished {
            previous: previous(&project, &run),
            run,
        }
    };
    let title = |when: NotifyWhen, now: &Finished| {
        notice_for_in(
            "en",
            &slack_only(when),
            "shop",
            std::slice::from_ref(now),
            true,
        )
        .map(|n| n.message.title)
    };

    let passed = job(false);
    assert_eq!(passed.run.report.verdict, Verdict::Passed);
    assert_eq!(title(NotifyWhen::Changed, &passed), None);

    let broke = job(true);
    assert_eq!(broke.run.report.verdict, Verdict::Failed);
    assert_eq!(
        title(NotifyWhen::Changed, &broke).as_deref(),
        Some("Failed: T")
    );

    let still = job(true);
    assert_eq!(
        title(NotifyWhen::FailedOrFixed, &still).as_deref(),
        Some("Failed: T")
    );
    assert_eq!(
        title(NotifyWhen::Changed, &still),
        None,
        "a test that keeps failing says nothing more"
    );

    let fixed = job(false);
    assert_eq!(
        title(NotifyWhen::FailedOrFixed, &fixed).as_deref(),
        Some("Fixed: T")
    );
    assert_eq!(
        title(NotifyWhen::Changed, &fixed).as_deref(),
        Some("Fixed: T")
    );
    fs::remove_dir_all(project).unwrap();
    fs::remove_dir_all(cache).unwrap();
}
