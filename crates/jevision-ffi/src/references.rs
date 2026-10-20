//! A picture that several tests compare with: finding the blocks that use it, and changing the
//! settings that go with it in all of them or in none.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use jevision_core::{
    BLOCK_SUFFIX, Reference, SettingsError, TEST_SUFFIX, describe, parse, resolve_test,
    set_reference_settings, to_canonical_json,
};

/// A block, in a test or a custom block of the project, that uses a picture file.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ReferenceUse {
    /// The test or custom block file, relative to the project folder, with `/` between folders.
    pub test: String,
    pub block: String,
    /// The `type` of the block, like `check`.
    pub block_type: String,
    /// Where in the block the picture is: `reference`, `until.appears`, `until.disappears`,
    /// `elseIf.1.reference`, `target`, `from`, `to`, `at`, `into` or `inputs.0.value`.
    pub field: String,
    /// The picture as the block names it.
    pub image: String,
    /// The file is the dark picture of `image`, which the block uses when the app is dark.
    pub dark: bool,
    /// How the block compares the screen with the picture; `None` for a block that only looks
    /// for the picture on screen or hands it to a script.
    pub settings: Option<Reference>,
}

/// New comparison settings for the picture of one block.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ReferenceEdit {
    pub test: String,
    pub block: String,
    /// Which picture of the block, as in [`ReferenceUse::field`].
    pub field: String,
    /// The compare area, ignored and hidden parts and similarity to use; the picture it names
    /// is not changed.
    pub settings: Reference,
}

/// What a set of edits did to the files, so that it can be taken back and done again.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ReferenceChange {
    /// The files that changed; an edit that leaves the settings as they are changes none.
    pub files: Vec<ChangedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ChangedFile {
    /// Relative to the project folder.
    pub path: String,
    pub before: String,
    pub after: String,
}

/// The edits can't be made, or taken back; no file was changed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum ReferenceError {
    #[error("{test} can't be read: {reason}")]
    Unreadable { test: String, reason: String },
    #[error("{test} has no block {block} with a picture at {field}")]
    NoSuchPicture {
        test: String,
        block: String,
        field: String,
    },
    #[error("The settings of block {block} in {test} don't fit: {reason}")]
    Rejected {
        test: String,
        block: String,
        reason: String,
    },
    #[error("{test} was changed meanwhile, so it was left as it is")]
    Changed { test: String },
    #[error("{test} can't be saved: {reason}")]
    Unwritable { test: String, reason: String },
}

/// The blocks of the tests and custom blocks of `project` that use the picture `reference`, a
/// path relative to the project: a test file after test file, each in the order its blocks are
/// saved. A block that names the light picture of a dark `reference` uses it too. Files that
/// can't be read as a test are left out.
#[uniffi::export]
pub fn reference_uses(project: String, reference: String) -> Vec<ReferenceUse> {
    let project = Path::new(&project);
    // A path in no test's folder in particular, so only `.` and `..` are resolved.
    let Some(picture) = resolve_test("x", &reference) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for test in test_files(project) {
        let Some(flow) = fs::read_to_string(project.join(&test))
            .ok()
            .and_then(|text| parse(&text).ok())
        else {
            continue;
        };
        for u in jevision_core::reference_uses(&flow, &test, &picture) {
            found.push(ReferenceUse {
                test: test.clone(),
                block: u.block,
                block_type: u.block_type,
                field: u.field,
                image: u.image,
                dark: u.dark,
                settings: u.settings,
            });
        }
    }
    found
}

/// Gives the pictures named by `edits` their new settings, in every test at once: when one
/// edit can't be made, no file is written.
#[uniffi::export]
pub fn apply_reference_edits(
    project: String,
    edits: Vec<ReferenceEdit>,
) -> Result<ReferenceChange, ReferenceError> {
    let project = Path::new(&project);
    let change = plan(project, &edits)?;
    swap(project, &change.files, true)?;
    Ok(change)
}

/// Puts the tests back as they were before `change`. A test changed since is left alone, and
/// then none is touched.
#[uniffi::export]
pub fn undo_reference_edits(
    project: String,
    change: ReferenceChange,
) -> Result<(), ReferenceError> {
    swap(Path::new(&project), &change.files, false)
}

/// Makes `change` again after [`undo_reference_edits`].
#[uniffi::export]
pub fn redo_reference_edits(
    project: String,
    change: ReferenceChange,
) -> Result<(), ReferenceError> {
    swap(Path::new(&project), &change.files, true)
}

fn plan(project: &Path, edits: &[ReferenceEdit]) -> Result<ReferenceChange, ReferenceError> {
    let mut tests: BTreeMap<&str, Vec<&ReferenceEdit>> = BTreeMap::new();
    for edit in edits {
        tests.entry(&edit.test).or_default().push(edit);
    }
    let mut files = Vec::new();
    for (test, edits) in &tests {
        let unreadable = |reason: String| ReferenceError::Unreadable {
            test: (*test).to_owned(),
            reason,
        };
        let path = inside(project, test)
            .ok_or_else(|| unreadable("it is not a file of the project".to_owned()))?;
        let before = fs::read_to_string(path).map_err(|e| unreadable(e.to_string()))?;
        let flow = parse(&before)
            .map_err(|e| unreadable(e.issues.iter().map(describe).collect::<Vec<_>>().join(" ")))?;
        let mut edited = flow.clone();
        for edit in edits {
            set_reference_settings(&mut edited, &edit.block, &edit.field, &edit.settings).map_err(
                |error| match error {
                    SettingsError::Missing => ReferenceError::NoSuchPicture {
                        test: (*test).to_owned(),
                        block: edit.block.clone(),
                        field: edit.field.clone(),
                    },
                    SettingsError::Invalid(issues) => ReferenceError::Rejected {
                        test: (*test).to_owned(),
                        block: edit.block.clone(),
                        reason: issues.iter().map(describe).collect::<Vec<_>>().join(" "),
                    },
                },
            )?;
        }
        if edited != flow {
            files.push(ChangedFile {
                path: (*test).to_owned(),
                before,
                after: to_canonical_json(&edited),
            });
        }
    }
    Ok(ReferenceChange { files })
}

/// Writes the `after` text of every file, or with `forward` off the `before` text. Each file
/// must still hold what is to be replaced, and if one can't be written the others are put back.
fn swap(project: &Path, files: &[ChangedFile], forward: bool) -> Result<(), ReferenceError> {
    let paths = files
        .iter()
        .map(|f| {
            inside(project, &f.path).ok_or_else(|| ReferenceError::Unreadable {
                test: f.path.clone(),
                reason: "it is not a file of the project".to_owned(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (file, path) in files.iter().zip(&paths) {
        match fs::read_to_string(path) {
            Ok(text) if text == side(file, !forward) => {}
            Ok(_) => {
                return Err(ReferenceError::Changed {
                    test: file.path.clone(),
                });
            }
            Err(e) => {
                return Err(ReferenceError::Unreadable {
                    test: file.path.clone(),
                    reason: e.to_string(),
                });
            }
        }
    }
    for (done, (file, path)) in files.iter().zip(&paths).enumerate() {
        if let Err(e) = write_whole(path, side(file, forward)) {
            for (file, path) in files.iter().zip(&paths).take(done) {
                let _ = write_whole(path, side(file, !forward));
            }
            return Err(ReferenceError::Unwritable {
                test: file.path.clone(),
                reason: e.to_string(),
            });
        }
    }
    Ok(())
}

/// The text of `file` after the edits, or before them.
fn side(file: &ChangedFile, after: bool) -> &str {
    if after { &file.after } else { &file.before }
}

/// A file of the project at `path`, relative to it; `None` for a path that leads elsewhere.
fn inside(project: &Path, path: &str) -> Option<PathBuf> {
    let relative = Path::new(path);
    relative
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
        .then(|| project.join(relative))
}

/// Writes `text` so that a reader never sees half a file.
fn write_whole(file: &Path, text: &str) -> std::io::Result<()> {
    let name = file.file_name().unwrap_or_default().to_string_lossy();
    let temporary = file.with_file_name(format!(".{name}.saving"));
    fs::write(&temporary, text)
        .and_then(|()| fs::rename(&temporary, file))
        .inspect_err(|_| {
            let _ = fs::remove_file(&temporary);
        })
}

/// Test files and custom blocks in the project folder and the folders inside it, relative to
/// it, sorted by path. Hidden files and folders and symbolic links are left out.
fn test_files(project: &Path) -> Vec<String> {
    fn walk(folder: &Path, prefix: &str, files: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(folder) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            let id = format!("{prefix}{name}");
            if kind.is_dir() {
                walk(&entry.path(), &format!("{id}/"), files);
            } else if kind.is_file() {
                let lower = name.to_lowercase();
                if lower.ends_with(TEST_SUFFIX) || lower.ends_with(BLOCK_SUFFIX) {
                    files.push(id);
                }
            }
        }
    }
    let mut files = Vec::new();
    walk(project, "", &mut files);
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use jevision_core::{Compare, Rect};

    use super::*;

    fn project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("jevision-ffi-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn write(root: &Path, file: &str, blocks: &str) {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            format!(
                r#"{{"format":1,"name":"{file}","app":"com.example.App","start":"a","blocks":{{{blocks}}}}}"#
            ),
        )
        .unwrap();
    }

    /// One picture, `refs/home.png`, in three tests (one of them in another folder, one a
    /// custom block), next to tests that don't use it.
    fn shared(name: &str) -> PathBuf {
        let root = project(name);
        write(
            &root,
            "login.flow.json",
            r#""a":{"type":"check","reference":{"image":"refs/home.png","compare":"similar","threshold":0.9,"ignore":[{"x":0.1,"y":0.1,"width":0.1,"height":0.1}]},"next":"b"},
               "b":{"type":"check","reference":{"image":"refs/login.png"}}"#,
        );
        write(
            &root,
            "smoke/inbox.flow.json",
            r#""a":{"type":"waitFor","until":{"appears":{"image":"../refs/home.png"}},"timeout":"5s","next":"b"},
               "b":{"type":"click","target":{"image":"../refs/home.png"}}"#,
        );
        write(
            &root,
            "blocks/open-home.block.json",
            r#""a":{"type":"if","reference":{"image":"../refs/other.png"},"then":"b","elseIf":[{"reference":{"image":"../refs/home.png","area":{"x":0,"y":0,"width":1,"height":0.5}},"then":"b"}]},
               "b":{"type":"pause","duration":"1s"}"#,
        );
        write(
            &root,
            "other.flow.json",
            r#""a":{"type":"check","reference":{"image":"refs/other.png"}}"#,
        );
        write(
            &root,
            ".hidden/ghost.flow.json",
            r#""a":{"type":"check","reference":{"image":"refs/home.png"}}"#,
        );
        fs::write(root.join("broken.flow.json"), "{ not json").unwrap();
        root
    }

    fn tests_and_blocks(uses: &[ReferenceUse]) -> Vec<(&str, &str)> {
        uses.iter()
            .map(|u| (u.test.as_str(), u.block.as_str()))
            .collect()
    }

    #[test]
    fn finds_the_blocks_of_every_test_that_use_a_shared_picture() {
        let root = shared("finds_the_blocks_of_every_test_that_use_a_shared_picture");
        let uses = reference_uses(root.to_string_lossy().into(), "refs/home.png".into());
        assert_eq!(
            tests_and_blocks(&uses),
            [
                ("blocks/open-home.block.json", "a"),
                ("login.flow.json", "a"),
                ("smoke/inbox.flow.json", "a"),
                ("smoke/inbox.flow.json", "b"),
            ]
        );
        let fields: Vec<_> = uses.iter().map(|u| u.field.as_str()).collect();
        assert_eq!(
            fields,
            ["elseIf.0.reference", "reference", "until.appears", "target"]
        );
        let login = uses[1].settings.as_ref().unwrap();
        assert_eq!(login.compare, Compare::Similar { threshold: 0.9 });
        assert_eq!(login.ignore.len(), 1);
        assert_eq!(uses[3].settings, None);
        let none = reference_uses(root.to_string_lossy().into(), "../refs/home.png".into());
        let _ = fs::remove_dir_all(&root);
        assert!(none.is_empty());
    }

    #[test]
    fn a_dark_picture_is_used_through_the_light_one() {
        let root = shared("a_dark_picture_is_used_through_the_light_one");
        let uses = reference_uses(root.to_string_lossy().into(), "refs/home.dark.png".into());
        let _ = fs::remove_dir_all(&root);
        assert_eq!(uses.len(), 4);
        assert!(uses.iter().all(|u| u.dark && u.image.ends_with("home.png")));
    }

    fn edit(test: &str, block: &str, field: &str, threshold: f64) -> ReferenceEdit {
        ReferenceEdit {
            test: test.into(),
            block: block.into(),
            field: field.into(),
            settings: Reference {
                image: String::new(),
                compare: Compare::Similar { threshold },
                area: None,
                ignore: vec![Rect {
                    x: 0.5,
                    y: 0.5,
                    width: 0.25,
                    height: 0.25,
                }],
                hide: vec![],
            },
        }
    }

    #[test]
    fn edits_land_in_every_test_and_can_be_taken_back_and_made_again() {
        let root = shared("edits_land_in_every_test_and_can_be_taken_back_and_made_again");
        let dir = root.to_string_lossy().to_string();
        let read = |file: &str| fs::read_to_string(root.join(file)).unwrap();
        let originals: Vec<_> = [
            "login.flow.json",
            "smoke/inbox.flow.json",
            "other.flow.json",
        ]
        .map(read)
        .into();
        let change = apply_reference_edits(
            dir.clone(),
            vec![
                edit("login.flow.json", "a", "reference", 0.8),
                edit("smoke/inbox.flow.json", "a", "until.appears", 0.8),
                edit(
                    "blocks/open-home.block.json",
                    "a",
                    "elseIf.0.reference",
                    0.8,
                ),
            ],
        )
        .unwrap();
        assert_eq!(change.files.len(), 3);
        let after = reference_uses(dir.clone(), "refs/home.png".into());
        for u in after.iter().filter(|u| u.settings.is_some()) {
            let s = u.settings.as_ref().unwrap();
            assert_eq!(s.compare, Compare::Similar { threshold: 0.8 }, "{}", u.test);
            assert_eq!(s.ignore.len(), 1);
            assert_eq!(s.area, None);
            assert!(s.image.is_empty() || s.image.ends_with("refs/home.png"));
        }
        assert_eq!(read("other.flow.json"), originals[2]);
        assert_eq!(
            read("smoke/inbox.flow.json").matches("home.png").count(),
            2,
            "the picture stays"
        );

        undo_reference_edits(dir.clone(), change.clone()).unwrap();
        assert_eq!(read("login.flow.json"), originals[0]);
        assert_eq!(read("smoke/inbox.flow.json"), originals[1]);
        redo_reference_edits(dir.clone(), change.clone()).unwrap();
        assert_eq!(read("login.flow.json"), change.files[1].after);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn one_edit_that_cannot_be_made_stops_all_of_them() {
        let root = shared("one_edit_that_cannot_be_made_stops_all_of_them");
        let dir = root.to_string_lossy().to_string();
        let before = fs::read_to_string(root.join("login.flow.json")).unwrap();
        let wrong = [
            (
                edit("smoke/inbox.flow.json", "b", "target", 0.8),
                "NoSuchPicture",
            ),
            (
                edit("smoke/gone.flow.json", "a", "reference", 0.8),
                "Unreadable",
            ),
            (
                edit("../outside.flow.json", "a", "reference", 0.8),
                "Unreadable",
            ),
            (
                edit("smoke/inbox.flow.json", "a", "until.appears", 1.5),
                "Rejected",
            ),
        ];
        for (bad, kind) in wrong {
            let error = apply_reference_edits(
                dir.clone(),
                vec![edit("login.flow.json", "a", "reference", 0.8), bad],
            )
            .unwrap_err();
            assert!(format!("{error:?}").starts_with(kind), "{kind}: {error:?}");
            assert_eq!(
                fs::read_to_string(root.join("login.flow.json")).unwrap(),
                before
            );
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_test_changed_since_is_never_overwritten() {
        let root = shared("a_test_changed_since_is_never_overwritten");
        let dir = root.to_string_lossy().to_string();
        let change = apply_reference_edits(
            dir.clone(),
            vec![
                edit("login.flow.json", "a", "reference", 0.8),
                edit("smoke/inbox.flow.json", "a", "until.appears", 0.8),
            ],
        )
        .unwrap();
        let inbox = root.join("smoke/inbox.flow.json");
        let edited = format!("{} ", fs::read_to_string(&inbox).unwrap());
        fs::write(&inbox, &edited).unwrap();
        let error = undo_reference_edits(dir, change.clone()).unwrap_err();
        assert_eq!(
            error,
            ReferenceError::Changed {
                test: "smoke/inbox.flow.json".into()
            }
        );
        assert_eq!(fs::read_to_string(&inbox).unwrap(), edited);
        assert_eq!(
            fs::read_to_string(root.join("login.flow.json")).unwrap(),
            change.files[0].after,
            "the other test is not put back either"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn edits_that_change_nothing_write_nothing() {
        let root = shared("edits_that_change_nothing_write_nothing");
        let dir = root.to_string_lossy().to_string();
        let uses = reference_uses(dir.clone(), "refs/home.png".into());
        let same = uses
            .iter()
            .find(|u| u.test == "login.flow.json")
            .map(|u| ReferenceEdit {
                test: u.test.clone(),
                block: u.block.clone(),
                field: u.field.clone(),
                settings: u.settings.clone().unwrap(),
            })
            .unwrap();
        let change = apply_reference_edits(dir, vec![same]).unwrap();
        let _ = fs::remove_dir_all(&root);
        assert!(change.files.is_empty());
    }
}
