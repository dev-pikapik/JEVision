//! What this build reads and speaks, so scripts and other tools can tell whether their files
//! and connections fit it: the format each file is written in and the protocols of the iPhone
//! app and of assistants.

use std::path::Path;

use jevision_core::{
    BLOCK_SUFFIX, ENVIRONMENTS_FILE, ENVIRONMENTS_FORMAT, FORMAT_VERSION, LAST_RESULTS_FILE,
    MANIFEST_FILE, PACKAGE_FORMAT, PROJECT_FILE, PROJECT_FORMAT, SCHEDULE_FILE, SCHEDULE_FORMAT,
    SCHEDULE_HISTORY_FILE, SCHEDULE_HISTORY_FORMAT, TEST_SUFFIX,
};
use jevision_runner::{LAST_RESULTS_FORMAT, REPORT_FILE, REPORT_FORMAT};

/// A file JEVision writes and the version of its format. `file` is a name, or `*` and the end
/// of a name, such as `*.flow.json`.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct FileFormat {
    pub file: String,
    pub version: u32,
}

/// What this build reads and speaks.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Compatibility {
    /// Every file JEVision writes, with the newest format this build reads and writes.
    pub formats: Vec<FileFormat>,
    /// The oldest and newest version of the connection with the iPhone app this Mac accepts.
    pub iphone_protocol_oldest: u32,
    pub iphone_protocol_newest: u32,
    /// The versions of MCP the assistant server speaks, newest first.
    pub mcp_protocols: Vec<String>,
}

/// The formats and protocols of this build.
#[uniffi::export]
pub fn compatibility() -> Compatibility {
    let iphone = protocol_version(jevision_remote::ALPN);
    Compatibility {
        formats: formats(),
        iphone_protocol_oldest: iphone,
        iphone_protocol_newest: iphone,
        mcp_protocols: crate::mcp::PROTOCOL_VERSIONS
            .iter()
            .map(|v| (*v).to_owned())
            .collect(),
    }
}

/// Whether the file at `path` is one JEVision writes and was saved in a newer format than this
/// build reads. A file that can't be read, isn't JSON or names no format is not: whatever
/// reads it says what is wrong.
#[uniffi::export]
pub fn needs_newer_jevision(path: String) -> bool {
    let path = Path::new(&path);
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let Some(newest) = formats()
        .into_iter()
        .find(|f| match f.file.strip_prefix('*') {
            Some(end) => name.ends_with(end),
            None => name == f.file,
        })
        .map(|f| f.version)
    else {
        return false;
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|json| json.get("format")?.as_u64())
        .is_some_and(|found| found > u64::from(newest))
}

/// Whether an issue or a run failure with `code` says its file was saved by a newer JEVision.
#[uniffi::export]
pub fn asks_for_newer_jevision(code: String) -> bool {
    [
        "flow.format.tooNew",
        "project.format.tooNew",
        "block.version.needsNewer",
        "run.test.needsNewer",
        "run.block.needsNewer",
    ]
    .contains(&code.as_str())
}

fn formats() -> Vec<FileFormat> {
    let format = |file: String, version: u32| FileFormat { file, version };
    // The files in `.jevision` by name, as the report in each run folder.
    let by_name = |path: &str| path.rsplit('/').next().unwrap_or_default().to_owned();
    vec![
        format(format!("*{TEST_SUFFIX}"), FORMAT_VERSION),
        format(format!("*{BLOCK_SUFFIX}"), FORMAT_VERSION),
        format(PROJECT_FILE.to_owned(), PROJECT_FORMAT),
        format(ENVIRONMENTS_FILE.to_owned(), ENVIRONMENTS_FORMAT),
        format(SCHEDULE_FILE.to_owned(), SCHEDULE_FORMAT),
        format(MANIFEST_FILE.to_owned(), PACKAGE_FORMAT),
        format(REPORT_FILE.to_owned(), REPORT_FORMAT),
        format(jevision_notify::FILE.to_owned(), jevision_notify::FORMAT),
        format(by_name(LAST_RESULTS_FILE), LAST_RESULTS_FORMAT),
        format(by_name(SCHEDULE_HISTORY_FILE), SCHEDULE_HISTORY_FORMAT),
    ]
}

/// The version at the end of a protocol name such as `jevision/remote/1`.
fn protocol_version(alpn: &[u8]) -> u32 {
    std::str::from_utf8(alpn)
        .ok()
        .and_then(|name| name.rsplit('/').next()?.parse().ok())
        .expect("a protocol name ends in its version")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_format_comes_from_its_constant() {
        let about = compatibility();
        let version = |file: &str| {
            about
                .formats
                .iter()
                .find(|f| f.file == file)
                .map(|f| f.version)
        };
        assert_eq!(version("*.flow.json"), Some(jevision_core::FORMAT_VERSION));
        assert_eq!(version("*.block.json"), Some(jevision_core::FORMAT_VERSION));
        assert_eq!(
            version("jevision.project.json"),
            Some(jevision_core::PROJECT_FORMAT)
        );
        assert_eq!(
            version("jevision.environments.json"),
            Some(jevision_core::ENVIRONMENTS_FORMAT)
        );
        assert_eq!(
            version("jevision.schedule.json"),
            Some(jevision_core::SCHEDULE_FORMAT)
        );
        assert_eq!(version("block.json"), Some(jevision_core::PACKAGE_FORMAT));
        assert_eq!(version("report.json"), Some(jevision_runner::REPORT_FORMAT));
        assert_eq!(
            version("jevision.notifications.json"),
            Some(jevision_notify::FORMAT)
        );
        assert_eq!(
            version("last-results.json"),
            Some(jevision_runner::LAST_RESULTS_FORMAT)
        );
        assert!(
            jevision_core::LAST_RESULTS_FILE.ends_with("/last-results.json"),
            "named as it is in .jevision"
        );
        assert_eq!(
            version("schedule-history.json"),
            Some(jevision_core::SCHEDULE_HISTORY_FORMAT)
        );
        assert!(
            jevision_core::SCHEDULE_HISTORY_FILE.ends_with("/schedule-history.json"),
            "named as it is in .jevision"
        );
        assert_eq!(
            (about.iphone_protocol_oldest, about.iphone_protocol_newest),
            (1, 1)
        );
        assert_eq!(about.mcp_protocols, crate::mcp::PROTOCOL_VERSIONS);
    }

    #[test]
    fn a_file_from_a_newer_jevision_is_told_apart() {
        let dir = std::env::temp_dir().join(format!("jevision-ffi-about-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = |name: &str, text: &str| {
            let path = dir.join(name);
            std::fs::write(&path, text).unwrap();
            path.to_string_lossy().into_owned()
        };
        let newer = |path: String| needs_newer_jevision(path);
        assert!(newer(file(
            "jevision.environments.json",
            r#"{"format": 99, "environments": {}, "added": true}"#
        )));
        assert!(!newer(file(
            "jevision.environments.json",
            r#"{"format": 1, "environments": {}}"#
        )));
        assert!(!newer(file("jevision.environments.json", "{")));
        assert!(newer(file("sign-in.flow.json", r#"{"format": 99}"#)));
        assert!(!newer(file("notes.json", r#"{"format": 99}"#)));
        assert!(newer(file("last-results.json", r#"{"format": 99}"#)));
        assert!(newer(file("schedule-history.json", r#"{"format": 99}"#)));
        assert!(newer(file(
            "jevision.notifications.json",
            r#"{"format": 99}"#
        )));
        assert!(!newer(
            dir.join("gone.flow.json").to_string_lossy().into_owned()
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
