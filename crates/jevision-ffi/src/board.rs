//! Boards of a project for the app: reading, writing and editing their `*.canvas` files.

use std::path::Path;

use jevision_core::{
    Board, BoardEdge, BoardLayout, BoardNode, BoardNodeKind, LaidBoardEdge, LaidNode, Point, Rect,
};

/// The text is not a board this build can read.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum BoardError {
    #[error("{message}")]
    Invalid { message: String },
}

/// A board after something was added, and the id of what was added.
#[derive(uniffi::Record)]
pub struct BoardEdit {
    pub board: Board,
    pub id: String,
}

/// Folder of the project that holds its boards.
#[uniffi::export]
pub fn boards_folder() -> String {
    jevision_core::BOARDS_FOLDER.to_owned()
}

/// Ending of a board's file name.
#[uniffi::export]
pub fn board_suffix() -> String {
    jevision_core::BOARD_SUFFIX.to_owned()
}

/// Height of a frame's title bar, all that shows of a folded frame.
#[uniffi::export]
pub fn board_frame_header() -> f64 {
    jevision_core::FRAME_HEADER
}

/// Reads the text of a board file, from JEVision or from Obsidian.
#[uniffi::export]
pub fn load_board(text: String) -> Result<Board, BoardError> {
    Board::parse(&text).map_err(|message| BoardError::Invalid { message })
}

/// The board in canonical form, ready to be saved to its `*.canvas` file.
#[uniffi::export]
pub fn save_board(board: Board) -> String {
    board.to_json()
}

/// What is shown of the board: the nodes outside folded frames, the arrows between them and
/// what they cover.
#[uniffi::export]
pub fn lay_out_board(board: Board) -> BoardLayout {
    board.layout()
}

/// Adds a node of `kind` centered on `at`.
#[uniffi::export]
pub fn add_to_board(mut board: Board, kind: BoardNodeKind, at: Point) -> BoardEdit {
    let id = board.add(kind, at);
    BoardEdit { board, id }
}

/// Moves the nodes `ids`, with what lies inside the frames among them, by `by`; with `snap`
/// onto the grid.
#[uniffi::export]
pub fn move_on_board(mut board: Board, ids: Vec<String>, by: Point, snap: bool) -> Board {
    board.move_nodes(&ids, by, snap);
    board
}

/// Gives node `id` the place and size `rect`, on the grid.
#[uniffi::export]
pub fn resize_on_board(mut board: Board, id: String, rect: Rect) -> Board {
    board.resize(&id, rect);
    board
}

/// Removes the nodes `ids` and their arrows.
#[uniffi::export]
pub fn remove_from_board(mut board: Board, ids: Vec<String>) -> Board {
    board.remove(&ids);
    board
}

/// Draws an arrow from node `from` to node `to`; `None` when it can't be drawn or is there.
#[uniffi::export]
pub fn connect_on_board(mut board: Board, from: String, to: String) -> Option<BoardEdit> {
    let id = board.connect(&from, &to)?;
    Some(BoardEdit { board, id })
}

/// The nodes `ids` and everything inside the frames among them: what a drag moves.
#[uniffi::export]
pub fn board_with_contents(board: Board, ids: Vec<String>) -> Vec<String> {
    board.with_contents(&ids)
}

/// On the board of all tests, adds the tests `tests` it lacks and drops the ones that are gone;
/// `None` when it already shows them all.
#[uniffi::export]
pub fn sync_board_tests(mut board: Board, tests: Vec<String>, root_title: String) -> Option<Board> {
    board.sync_tests(&tests, &root_title).then_some(board)
}

/// Points every board of the folder `project` that shows the file or folder `old` at `new`,
/// after it was renamed, and returns the boards it changed, relative to the project. Boards
/// that can't be read are left alone.
#[uniffi::export]
pub fn rename_on_boards(project: String, old: String, new: String) -> Vec<String> {
    let folder = Path::new(&project).join(jevision_core::BOARDS_FOLDER);
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return Vec::new();
    };
    let mut changed = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(jevision_core::BOARD_SUFFIX) {
            continue;
        }
        let path = entry.path();
        let Some(mut board) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| Board::parse(&text).ok())
        else {
            continue;
        };
        if board.rename_file(&old, &new) && std::fs::write(&path, board.to_json()).is_ok() {
            changed.push(format!("{}/{name}", jevision_core::BOARDS_FOLDER));
        }
    }
    changed.sort();
    changed
}

#[uniffi::remote(Record)]
pub struct Board {
    pub nodes: Vec<BoardNode>,
    pub edges: Vec<BoardEdge>,
    pub all_tests: bool,
    pub extra: String,
}

#[uniffi::remote(Record)]
pub struct BoardNode {
    pub id: String,
    pub kind: BoardNodeKind,
    pub rect: Rect,
    pub color: String,
    pub extra: String,
}

#[uniffi::remote(Enum)]
pub enum BoardNodeKind {
    Text {
        text: String,
    },
    File {
        file: String,
    },
    Link {
        url: String,
    },
    Group {
        label: String,
        collapsed: bool,
        folder: Option<String>,
    },
}

#[uniffi::remote(Record)]
pub struct BoardEdge {
    pub id: String,
    pub from: String,
    pub from_side: String,
    pub to: String,
    pub to_side: String,
    pub from_arrow: bool,
    pub to_arrow: bool,
    pub label: String,
    pub color: String,
    pub extra: String,
}

#[uniffi::remote(Record)]
pub struct BoardLayout {
    pub nodes: Vec<LaidNode>,
    pub edges: Vec<LaidBoardEdge>,
    pub bounds: Option<Rect>,
}

#[uniffi::remote(Record)]
pub struct LaidNode {
    pub id: String,
    pub rect: Rect,
}

#[uniffi::remote(Record)]
pub struct LaidBoardEdge {
    pub id: String,
    pub from: Point,
    pub from_side: String,
    pub to: Point,
    pub to_side: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renames_a_test_on_every_board_of_the_project() {
        let root = std::env::temp_dir().join(format!("jevision-ffi-boards-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("boards")).unwrap();
        let card = r#"{"nodes":[{"id":"a","type":"file","file":"smoke/a.flow.json","x":0,"y":0,"width":224,"height":96}]}"#;
        std::fs::write(root.join("boards/One.canvas"), card).unwrap();
        std::fs::write(root.join("boards/Two.canvas"), "{}").unwrap();
        std::fs::write(root.join("boards/Broken.canvas"), "{ not json").unwrap();
        std::fs::write(root.join("boards/notes.md"), card).unwrap();

        let project = root.to_string_lossy().into_owned();
        let changed = rename_on_boards(project.clone(), "smoke".into(), "checks".into());
        assert_eq!(changed, ["boards/One.canvas"]);
        let one =
            load_board(std::fs::read_to_string(root.join("boards/One.canvas")).unwrap()).unwrap();
        assert_eq!(
            one.nodes[0].kind,
            BoardNodeKind::File {
                file: "checks/a.flow.json".into()
            }
        );
        assert_eq!(
            std::fs::read_to_string(root.join("boards/notes.md")).unwrap(),
            card
        );
        assert_eq!(
            std::fs::read_to_string(root.join("boards/Broken.canvas")).unwrap(),
            "{ not json"
        );
        assert!(rename_on_boards(project, "x".into(), "y".into()).is_empty());
        assert!(rename_on_boards("/nowhere/at/all".into(), "x".into(), "y".into()).is_empty());
        let _ = std::fs::remove_dir_all(&root);

        let Err(BoardError::Invalid { message }) = load_board("[]".into()) else {
            panic!("a list isn't a board");
        };
        assert!(message.contains("JSON object"), "{message}");
    }
}
