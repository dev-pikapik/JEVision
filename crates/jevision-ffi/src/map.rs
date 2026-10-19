//! The project map: every test of a project, the tests it runs and whether it can run.

use std::path::Path;

use jevision_core::{BlockKind, Point, Rect, Severity};

/// One test on the project map.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MapTest {
    /// Test file, relative to the project folder, with `/` between folders.
    pub file: String,
    /// Name saved in the test; empty when the file can't be read.
    pub name: String,
    pub blocks: u32,
    /// Test files its `runTest` blocks run, relative to the project folder, each once, in the
    /// order the blocks are saved. Paths that lead out of the project are left out.
    pub uses: Vec<String>,
    /// Problems that keep the test from running; 0 when it can run.
    pub errors: u32,
    /// The file is there and is a test this build can read.
    pub readable: bool,
    /// Someone marked the test as flaky in its details.
    pub flaky: bool,
}

/// Reads the tests `tests`, relative to the folder `project`, for the project map, in the
/// order given.
#[uniffi::export]
pub fn project_map(project: String, tests: Vec<String>) -> Vec<MapTest> {
    let project = Path::new(&project);
    tests
        .into_iter()
        .map(|file| map_test(project, file))
        .collect()
}

/// The arrow from the card `from` to the card `to` of the map (y grows downwards), in straight
/// runs that go around the cards `others` instead of through them.
#[uniffi::export]
pub fn map_link_route(from: Rect, to: Rect, others: Vec<Rect>) -> Vec<Point> {
    jevision_core::route_around(&from, &to, &others)
}

fn map_test(project: &Path, file: String) -> MapTest {
    let path = project.join(&file);
    let flow = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| jevision_core::parse(&text).ok());
    let Some(flow) = flow else {
        return MapTest {
            file,
            name: String::new(),
            blocks: 0,
            uses: Vec::new(),
            errors: 0,
            readable: false,
            flaky: false,
        };
    };
    let mut uses: Vec<String> = Vec::new();
    for block in &flow.blocks {
        let BlockKind::RunTest { test, .. } = &block.kind else {
            continue;
        };
        if let Some(used) = resolve(&file, test).filter(|used| !uses.contains(used)) {
            uses.push(used);
        }
    }
    let folder = path.parent().unwrap_or(project);
    let errors = jevision_core::validate_in(&flow, folder)
        .iter()
        .filter(|issue| issue.severity == Severity::Error)
        .count();
    MapTest {
        name: flow.name.clone(),
        blocks: u32::try_from(flow.blocks.len()).unwrap_or(u32::MAX),
        uses,
        errors: u32::try_from(errors).unwrap_or(u32::MAX),
        readable: true,
        flaky: flow.flaky,
        file,
    }
}

/// `path`, relative to the folder of the test `file`, as a path relative to the project;
/// `None` when it leads out of the project.
fn resolve(file: &str, path: &str) -> Option<String> {
    if path.starts_with('/') {
        return None;
    }
    let mut parts: Vec<&str> = file.split('/').collect();
    parts.pop();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_paths_from_the_test_folder() {
        assert_eq!(
            resolve("a.flow.json", "b.flow.json").as_deref(),
            Some("b.flow.json")
        );
        assert_eq!(
            resolve("smoke/a.flow.json", "../shared/./login.flow.json").as_deref(),
            Some("shared/login.flow.json")
        );
        assert_eq!(resolve("a.flow.json", "../outside.flow.json"), None);
        assert_eq!(resolve("a.flow.json", "/etc/x.flow.json"), None);
    }

    #[test]
    fn maps_tests_and_what_they_run() {
        let root = std::env::temp_dir().join(format!("jevision-ffi-map-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("smoke")).unwrap();
        std::fs::create_dir_all(root.join("shared")).unwrap();
        let write = |file: &str, name: &str, start: &str, blocks: &str| {
            std::fs::write(
                root.join(file),
                format!(
                    r#"{{"format":1,"name":"{name}","app":"com.example.App","start":"{start}","blocks":{{{blocks}}}}}"#
                ),
            )
            .unwrap();
        };
        write(
            "shared/login.flow.json",
            "Log in",
            "open",
            r#""open":{"type":"openApp"}"#,
        );
        let login = root.join("shared/login.flow.json");
        let marked = std::fs::read_to_string(&login)
            .unwrap()
            .replace(r#""start""#, r#""flaky":true,"start""#);
        std::fs::write(&login, marked).unwrap();
        write(
            "smoke/inbox.flow.json",
            "Inbox",
            "login",
            r#""login":{"type":"runTest","test":"../shared/login.flow.json","next":"again"},
               "again":{"type":"runTest","test":"../shared/login.flow.json","next":"gone"},
               "gone":{"type":"runTest","test":"missing.flow.json"}"#,
        );
        write(
            "smoke/odd.flow.json",
            "Odd",
            "notes",
            r#""notes":{"type":"runTest","test":"notes.txt"}"#,
        );
        std::fs::write(root.join("smoke/broken.flow.json"), "{ not json").unwrap();

        let map = project_map(
            root.to_string_lossy().into(),
            vec![
                "smoke/inbox.flow.json".into(),
                "shared/login.flow.json".into(),
                "smoke/broken.flow.json".into(),
                "smoke/deleted.flow.json".into(),
                "smoke/odd.flow.json".into(),
            ],
        );
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(map.len(), 5);
        assert_eq!(map[0].name, "Inbox");
        assert_eq!(map[0].blocks, 3);
        assert_eq!(
            map[0].uses,
            ["shared/login.flow.json", "smoke/missing.flow.json"]
        );
        assert_eq!(map[0].errors, 0, "a missing file is only a warning");
        assert!(!map[0].flaky);
        assert_eq!(
            map[1],
            MapTest {
                file: "shared/login.flow.json".into(),
                name: "Log in".into(),
                blocks: 1,
                uses: vec![],
                errors: 0,
                readable: true,
                flaky: true,
            }
        );
        assert!(!map[2].readable);
        assert!(!map[3].readable);
        assert!(map[4].errors > 0, "a test can only run test files");
    }
}
