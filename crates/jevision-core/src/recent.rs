//! The projects someone opened lately, for the app's Projects screen and its File → Open Recent
//! menu. The list is the app's, not any project's: the app keeps the text of
//! [`recent_projects_encode`] in its settings and reads it back with [`recent_projects_decode`].
//!
//! Every function returns the list in the order it is shown: pinned projects first, then the
//! most recently opened. A folder is in it once, by its absolute path without a `/` at the end;
//! paths are compared exactly. Projects stay until explicitly removed from the list.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use icu_normalizer::DecomposingNormalizerBorrowed;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::is_false;

/// Suggested maximum for a compact Open Recent menu; the project library keeps every folder.
pub const MAX_RECENT_PROJECTS: usize = 30;

/// A project in the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentProject {
    /// The project folder: an absolute path without a `/` at the end.
    pub path: String,
    pub name: String,
    /// The app's own data for opening the folder again, such as a macOS bookmark; may be empty.
    pub bookmark: Vec<u8>,
    /// Kept at the top, and in the list however long ago it was opened.
    pub pinned: bool,
    /// When the project was last opened, in seconds since 1970.
    pub opened_at: i64,
}

/// A project as the text of the list keeps it, with the bookmark in Base64.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    path: String,
    #[serde(default)]
    name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    bookmark: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pinned: bool,
    #[serde(default)]
    opened_at: i64,
}

/// Reads the list from the text [`recent_projects_encode`] wrote. Text that isn't a list reads
/// as an empty one; entries that can't be read are left out and a folder listed twice is kept
/// once, with what both entries knew.
pub fn recent_projects_decode(json: &str) -> Vec<RecentProject> {
    let entries: Vec<Value> = serde_json::from_str(json).unwrap_or_default();
    let projects = entries
        .into_iter()
        .filter_map(|entry| serde_json::from_value::<Entry>(entry).ok())
        .filter_map(|entry| {
            let path = normalized(&entry.path)?;
            Some(RecentProject {
                name: name_or_folder(&entry.name, &path),
                path,
                bookmark: STANDARD.decode(entry.bookmark).unwrap_or_default(),
                pinned: entry.pinned,
                opened_at: entry.opened_at,
            })
        })
        .collect();
    tidy(projects)
}

/// The list as text for the app's settings: compact JSON with the bookmarks in Base64.
pub fn recent_projects_encode(list: &[RecentProject]) -> String {
    let entries: Vec<Entry> = list
        .iter()
        .map(|p| Entry {
            path: p.path.clone(),
            name: p.name.clone(),
            bookmark: STANDARD.encode(&p.bookmark),
            pinned: p.pinned,
            opened_at: p.opened_at,
        })
        .collect();
    serde_json::to_string(&entries).expect("entries always serialize")
}

/// The list after the project in `path` was opened at `now`: added, or moved up with its new
/// name and bookmark. An empty `name` stands for the folder's name and an empty `bookmark`
/// keeps the one the list has. The project stays pinned if it was. A path that isn't absolute
/// changes nothing.
pub fn recent_projects_opened(
    list: Vec<RecentProject>,
    path: &str,
    name: &str,
    bookmark: Vec<u8>,
    now: i64,
) -> Vec<RecentProject> {
    let Some(path) = normalized(path) else {
        return tidy(list);
    };
    let mut list = list;
    let name = name_or_folder(name, &path);
    match list.iter_mut().find(|p| p.path == path) {
        Some(project) => {
            project.name = name;
            if !bookmark.is_empty() {
                project.bookmark = bookmark;
            }
            project.opened_at = now;
        }
        None => list.push(RecentProject {
            path,
            name,
            bookmark,
            pinned: false,
            opened_at: now,
        }),
    }
    tidy(list)
}

/// The list with the project in `path` pinned or not. Unpinned, it takes its place among the
/// others by when it was opened, without removing older projects.
pub fn recent_projects_set_pinned(
    list: Vec<RecentProject>,
    path: &str,
    pinned: bool,
) -> Vec<RecentProject> {
    let path = normalized(path);
    let mut list = list;
    for project in &mut list {
        if Some(&project.path) == path.as_ref() {
            project.pinned = pinned;
        }
    }
    tidy(list)
}

/// The list without the project in `path`. Its folder stays where it is.
pub fn recent_projects_removed(list: Vec<RecentProject>, path: &str) -> Vec<RecentProject> {
    let path = normalized(path);
    let mut list = list;
    list.retain(|p| Some(&p.path) != path.as_ref());
    tidy(list)
}

/// The list after the folder of a project moved from `old_path` to `new_path`, where it is
/// called `name` (empty for the folder's name) and opened with `bookmark` (empty to keep the
/// one the list has). The project stays pinned and keeps when it was opened; if the list
/// already had `new_path`, the two become one.
pub fn recent_projects_relocated(
    list: Vec<RecentProject>,
    old_path: &str,
    new_path: &str,
    name: &str,
    bookmark: Vec<u8>,
) -> Vec<RecentProject> {
    let (Some(old_path), Some(new_path)) = (normalized(old_path), normalized(new_path)) else {
        return tidy(list);
    };
    let mut list = list;
    let Some(at) = list.iter().position(|p| p.path == old_path) else {
        return tidy(list);
    };
    let moved = list.remove(at);
    let name = name_or_folder(name, &new_path);
    match list.iter_mut().find(|p| p.path == new_path) {
        Some(there) => {
            there.name = name;
            there.pinned |= moved.pinned;
            there.opened_at = there.opened_at.max(moved.opened_at);
            if !bookmark.is_empty() {
                there.bookmark = bookmark;
            } else if there.bookmark.is_empty() {
                there.bookmark = moved.bookmark;
            }
        }
        None => list.push(RecentProject {
            path: new_path,
            name,
            bookmark: if bookmark.is_empty() {
                moved.bookmark
            } else {
                bookmark
            },
            ..moved
        }),
    }
    tidy(list)
}

/// The list with only its pinned projects, as File → Open Recent → Clear Menu leaves it.
pub fn recent_projects_cleared(list: Vec<RecentProject>) -> Vec<RecentProject> {
    let mut list = list;
    list.retain(|p| p.pinned);
    tidy(list)
}

/// The projects a search for `query` finds: those whose name or folder has every word of the
/// query, whatever the case and accents (`cafe` finds `Café`, `елка` finds `Ёлка`). A blank
/// query finds them all.
pub fn recent_projects_matching(list: Vec<RecentProject>, query: &str) -> Vec<RecentProject> {
    let words: Vec<String> = query.split_whitespace().map(folded).collect();
    let mut list = tidy(list);
    if !words.is_empty() {
        list.retain(|p| {
            let (name, path) = (folded(&p.name), folded(&p.path));
            words
                .iter()
                .all(|w| name.contains(w.as_str()) || path.contains(w.as_str()))
        });
    }
    list
}

/// `path` without a `/` at its end, doubled `/` and `.` parts; `None` unless it is absolute.
fn normalized(path: &str) -> Option<String> {
    let path = Path::new(path);
    path.is_absolute().then(|| {
        path.components()
            .collect::<PathBuf>()
            .to_string_lossy()
            .into_owned()
    })
}

/// `name` without the spaces around it, or the name of the folder `path` when that leaves
/// nothing.
fn name_or_folder(name: &str, path: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        Path::new(path)
            .file_name()
            .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned())
    } else {
        name.to_owned()
    }
}

/// Text as a search compares it: lowercase, with letters split from their accents and the
/// accents dropped.
fn folded(text: &str) -> String {
    DecomposingNormalizerBorrowed::new_nfd()
        .normalize(&text.to_lowercase())
        .chars()
        .filter(|c| {
            !matches!(c, '\u{0300}'..='\u{036F}' | '\u{1AB0}'..='\u{1AFF}' | '\u{1DC0}'..='\u{1DFF}' | '\u{20D0}'..='\u{20FF}' | '\u{FE20}'..='\u{FE2F}')
        })
        .collect()
}

/// The list in the order it is shown, each folder once. Of two entries for one folder, the one opened later gives the
/// name and bookmark (an empty bookmark doesn't replace one), and a pin on either stays.
fn tidy(list: Vec<RecentProject>) -> Vec<RecentProject> {
    let mut kept: Vec<RecentProject> = Vec::with_capacity(list.len());
    let mut index: HashMap<String, usize> = HashMap::new();
    for project in list {
        let Some(&at) = index.get(&project.path) else {
            index.insert(project.path.clone(), kept.len());
            kept.push(project);
            continue;
        };
        let there = &mut kept[at];
        there.pinned |= project.pinned;
        if project.opened_at >= there.opened_at {
            there.name = project.name;
            there.opened_at = project.opened_at;
            if !project.bookmark.is_empty() {
                there.bookmark = project.bookmark;
            }
        } else if there.bookmark.is_empty() {
            there.bookmark = project.bookmark;
        }
    }
    kept.sort_by(|a, b| {
        (b.pinned, b.opened_at)
            .cmp(&(a.pinned, a.opened_at))
            .then_with(|| a.path.cmp(&b.path))
    });
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(path: &str, opened_at: i64) -> RecentProject {
        RecentProject {
            path: path.into(),
            name: name_or_folder("", path),
            bookmark: Vec::new(),
            pinned: false,
            opened_at,
        }
    }

    fn paths(list: &[RecentProject]) -> Vec<&str> {
        list.iter().map(|p| p.path.as_str()).collect()
    }

    #[test]
    fn the_list_reads_back_what_it_wrote() {
        let mut list = recent_projects_opened(Vec::new(), "/p/shop/", "", vec![0, 1, 254], 10);
        list = recent_projects_opened(list, "/p/bank", "Bank QA", Vec::new(), 20);
        list = recent_projects_set_pinned(list, "/p/shop", true);
        let text = recent_projects_encode(&list);
        assert_eq!(
            text,
            r#"[{"path":"/p/shop","name":"shop","bookmark":"AAH+","pinned":true,"openedAt":10},{"path":"/p/bank","name":"Bank QA","openedAt":20}]"#
        );
        assert_eq!(recent_projects_decode(&text), list);
    }

    #[test]
    fn rubbish_reads_as_an_empty_list_and_doubles_become_one() {
        for text in [
            "",
            "{",
            "null",
            "{\"path\":\"/a\"}",
            "[1, \"x\", {\"name\":\"no path\"}]",
        ] {
            assert_eq!(recent_projects_decode(text), Vec::new(), "{text}");
        }
        let list = recent_projects_decode(
            r#"[{"path":"/a/","openedAt":5,"bookmark":"AQ=="},{"path":"relative","openedAt":9},
                {"path":"/a","name":"A","pinned":true,"openedAt":7,"bookmark":"not base64!"},
                {"path":"/b","openedAt":6}]"#,
        );
        assert_eq!(paths(&list), ["/a", "/b"]);
        assert_eq!(list[0].name, "A", "the later entry names it");
        assert_eq!(
            list[0].bookmark,
            [1],
            "a missing bookmark doesn't erase one"
        );
        assert!(list[0].pinned);
        assert_eq!(list[0].opened_at, 7);
    }

    #[test]
    fn opening_moves_a_project_up_and_keeps_its_pin_and_bookmark() {
        let mut list = vec![project("/a", 1), project("/b", 2)];
        list[0].pinned = true;
        list[0].bookmark = vec![7];
        let list = recent_projects_opened(list, "/a/", "Renamed", Vec::new(), 3);
        let a = &list[0];
        assert_eq!((a.path.as_str(), a.name.as_str()), ("/a", "Renamed"));
        assert_eq!(
            (a.bookmark.as_slice(), a.pinned, a.opened_at),
            (&[7][..], true, 3)
        );
        let list = recent_projects_opened(list, "/b//./", "", vec![9], 4);
        assert_eq!(paths(&list), ["/a", "/b"], "pinned first");
        assert_eq!(list[1].bookmark, [9]);
        assert_eq!(
            recent_projects_opened(list.clone(), "b", "", Vec::new(), 5),
            list
        );
    }

    #[test]
    fn every_project_stays_until_explicitly_removed() {
        let mut list = vec![project("/pinned", 0)];
        list[0].pinned = true;
        for i in 1..=40 {
            list = recent_projects_opened(list, &format!("/p{i}"), "", Vec::new(), i);
        }
        assert_eq!(list.len(), 41);
        assert_eq!(list[0].path, "/pinned");
        assert_eq!(list[1].path, "/p40");
        assert_eq!(list.last().unwrap().path, "/p1");
        let list = recent_projects_set_pinned(list, "/pinned", false);
        assert!(
            list.iter().any(|p| p.path == "/pinned"),
            "unpinning keeps the folder available"
        );
    }

    #[test]
    fn removing_clearing_and_moving_a_folder() {
        let mut list = vec![project("/a", 1), project("/b", 2), project("/c", 3)];
        list[0].pinned = true;
        list[0].bookmark = vec![1];
        assert_eq!(
            paths(&recent_projects_removed(list.clone(), "/b/")),
            ["/a", "/c"]
        );
        assert_eq!(paths(&recent_projects_cleared(list.clone())), ["/a"]);

        let moved = recent_projects_relocated(list.clone(), "/a", "/new/a/", "", Vec::new());
        let a = &moved[0];
        assert_eq!((a.path.as_str(), a.name.as_str()), ("/new/a", "a"));
        assert_eq!(
            (a.bookmark.as_slice(), a.pinned, a.opened_at),
            (&[1][..], true, 1)
        );

        let merged = recent_projects_relocated(list.clone(), "/a", "/c", "C", vec![2]);
        assert_eq!(paths(&merged), ["/c", "/b"]);
        let c = &merged[0];
        assert_eq!((c.name.as_str(), c.bookmark.as_slice()), ("C", &[2][..]));
        assert_eq!((c.pinned, c.opened_at), (true, 3));

        let unknown = recent_projects_relocated(list.clone(), "/x", "/y", "", Vec::new());
        assert_eq!(
            unknown,
            recent_projects_relocated(list, "", "/y", "", Vec::new())
        );
    }

    #[test]
    fn a_search_ignores_case_and_accents() {
        let mut list = vec![
            project("/Users/ann/Café Orders", 1),
            project("/Users/ann/ёлка", 2),
            project("/Volumes/Work/shop-tests", 3),
        ];
        list[0].pinned = true;
        list[2].name = "Shop".into();
        let found = |q| paths(&recent_projects_matching(list.clone(), q)).join(" ");
        assert_eq!(
            found("  "),
            "/Users/ann/Café Orders /Volumes/Work/shop-tests /Users/ann/ёлка"
        );
        assert_eq!(found("CAFE"), "/Users/ann/Café Orders");
        assert_eq!(found("елка"), "/Users/ann/ёлка");
        assert_eq!(found("ЁЛ"), "/Users/ann/ёлка");
        assert_eq!(
            found("shop work"),
            "/Volumes/Work/shop-tests",
            "name and path"
        );
        assert_eq!(found("ann orders"), "/Users/ann/Café Orders");
        assert_eq!(found("bank"), "");
    }
}
