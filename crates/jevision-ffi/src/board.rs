//! Boards of a project for the app: reading, writing and editing their `*.canvas` files.

use std::path::Path;

use jevision_core::{
    Board, BoardEdge, BoardHeading, BoardLayout, BoardNode, BoardNodeKind, BoardVisible,
    FrameTests, LaidBoardEdge, LaidNode, Point, Rect,
};

/// The text is not a board this build can read.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum BoardError {
    #[error("{message}")]
    Invalid { message: String },
    /// The data or the name given for a board's picture is not one JEVision can show.
    #[error("{}", jevision_l10n::tr!("board.picture.unsupported"))]
    NotAPicture,
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

/// Height of the title bar of an open test's card; the blocks are drawn under it.
#[uniffi::export]
pub fn board_open_header() -> f64 {
    jevision_core::OPEN_HEADER
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

/// For each frame of the board, the tests whose cards lie inside it, folded away or not.
#[uniffi::export]
pub fn board_frame_tests(board: Board) -> Vec<FrameTests> {
    board.frame_tests()
}

/// Adds a node of `kind` centered on `at`.
#[uniffi::export]
pub fn add_to_board(mut board: Board, kind: BoardNodeKind, at: Point) -> BoardEdit {
    let id = board.add(kind, at);
    BoardEdit { board, id }
}

/// Adds a picture of `width` by `height` pixels, shown from the file `file` relative to the
/// project, centered on `at`.
#[uniffi::export]
pub fn add_image_to_board(
    mut board: Board,
    file: String,
    width: f64,
    height: f64,
    at: Point,
) -> BoardEdit {
    let id = board.add_image(&file, width, height, at);
    BoardEdit { board, id }
}

/// Whether the board shows the file `file` as a picture.
#[uniffi::export]
pub fn is_board_image(file: String) -> bool {
    jevision_core::is_image_file(&file)
}

/// Keeps the picture `data`, named `name` or like it, in the project's boards folder and
/// returns where, relative to the project: boards keep their pictures in one place that goes
/// along with the project. A name that is taken gets a number.
#[uniffi::export]
pub fn store_board_image(
    project: String,
    name: String,
    data: Vec<u8>,
) -> Result<String, BoardError> {
    let name = name.rsplit('/').next().unwrap_or_default().trim();
    let (stem, ending) = name.rsplit_once('.').unwrap_or((name, ""));
    let stem = stem.trim().trim_start_matches('.');
    if !jevision_core::is_image_file(&format!("{stem}.{ending}")) {
        return Err(BoardError::NotAPicture);
    }
    let stem = if stem.is_empty() { "Image" } else { stem };
    let folder = Path::new(&project)
        .join(jevision_core::BOARDS_FOLDER)
        .join(jevision_core::IMAGES_FOLDER);
    std::fs::create_dir_all(&folder).map_err(|e| BoardError::Invalid {
        message: e.to_string(),
    })?;
    let ending = ending.to_ascii_lowercase();
    for n in 1.. {
        let file = if n == 1 {
            format!("{stem}.{ending}")
        } else {
            format!("{stem} {n}.{ending}")
        };
        let path = folder.join(&file);
        let mut out = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(out) => out,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(BoardError::Invalid {
                    message: e.to_string(),
                });
            }
        };
        std::io::Write::write_all(&mut out, &data).map_err(|e| BoardError::Invalid {
            message: e.to_string(),
        })?;
        return Ok(format!(
            "{}/{}/{file}",
            jevision_core::BOARDS_FOLDER,
            jevision_core::IMAGES_FOLDER
        ));
    }
    unreachable!("some number is free")
}

/// Opens the card of the test `id` on the board, so it shows the blocks of the test inside
/// the folder `project`; `None` when `id` isn't a test's card, it is open already or the test
/// can't be read.
#[uniffi::export]
pub fn open_test_on_board(project: String, mut board: Board, id: String) -> Option<Board> {
    let file = board.nodes.iter().find(|n| n.id == id)?.test()?.to_owned();
    let text = std::fs::read_to_string(Path::new(&project).join(file)).ok()?;
    let laid = jevision_core::layout(&jevision_core::parse(&text).ok()?);
    board
        .open_test(&id, (laid.width, laid.height))
        .then_some(board)
}

/// Folds the open test `id` back into its card; `None` when it wasn't open.
#[uniffi::export]
pub fn close_test_on_board(mut board: Board, id: String) -> Option<Board> {
    board.close_test(&id).then_some(board)
}

/// Where the blocks of an open test, whose own canvas is `width` by `height`, are drawn inside
/// its card `card`.
#[uniffi::export]
pub fn board_blocks_area(card: Rect, width: f64, height: f64) -> Rect {
    jevision_core::blocks_area(&card, (width, height))
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

/// Where things are on a laid-out board, so a window works with what it shows and not with every
/// node: positions are those in the layout's lists.
#[derive(uniffi::Object)]
pub struct BoardIndex {
    index: jevision_core::BoardIndex,
}

#[uniffi::export]
impl BoardIndex {
    #[uniffi::constructor]
    pub fn new(layout: BoardLayout) -> Self {
        Self {
            index: jevision_core::BoardIndex::new(&layout),
        }
    }

    /// The nodes and arrows that touch `area`, in drawing order.
    pub fn visible(&self, area: Rect) -> BoardVisible {
        self.index.visible(&area)
    }

    /// The node to go to from node `from` when moving `heading`, for the arrow keys.
    pub fn neighbour(&self, from: u32, heading: BoardHeading) -> Option<u32> {
        self.index.neighbour(from, heading)
    }
}

#[uniffi::remote(Record)]
pub struct BoardVisible {
    pub nodes: Vec<u32>,
    pub edges: Vec<u32>,
}

#[uniffi::remote(Enum)]
pub enum BoardHeading {
    Up,
    Down,
    Left,
    Right,
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
    pub open: bool,
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
pub struct FrameTests {
    pub frame: String,
    pub tests: Vec<String>,
}

#[uniffi::remote(Record)]
pub struct LaidNode {
    pub id: String,
    pub rect: Rect,
    pub frame: bool,
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

    #[test]
    fn keeps_pictures_in_the_boards_folder_without_overwriting() {
        let root = std::env::temp_dir().join(format!("jevision-ffi-images-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let project = root.to_string_lossy().into_owned();
        let store = |name: &str, data: &[u8]| {
            store_board_image(project.clone(), name.into(), data.to_vec())
        };

        assert_eq!(store("Shot.PNG", b"one").unwrap(), "boards/images/Shot.png");
        assert_eq!(
            store("../../Shot.png", b"two").unwrap(),
            "boards/images/Shot 2.png"
        );
        assert_eq!(
            store(".hidden.jpg", b"x").unwrap(),
            "boards/images/hidden.jpg"
        );
        assert_eq!(store(".png", b"x").unwrap(), "boards/images/Image.png");
        assert_eq!(
            std::fs::read(root.join("boards/images/Shot.png")).unwrap(),
            b"one"
        );
        assert_eq!(
            std::fs::read(root.join("boards/images/Shot 2.png")).unwrap(),
            b"two"
        );
        assert!(store("notes.txt", b"x").is_err());
        assert!(store("noending", b"x").is_err());
        assert!(is_board_image("boards/images/Shot.png".into()));
        assert!(!is_board_image("smoke/a.flow.json".into()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn opens_a_test_on_the_board_to_the_size_of_its_blocks() {
        let root = std::env::temp_dir().join(format!("jevision-ffi-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("smoke")).unwrap();
        let click = r#""type":"click","target":{"point":{"x":0.5,"y":0.5}},"button":"left""#;
        std::fs::write(
            root.join("smoke/a.flow.json"),
            format!(
                r#"{{"format":1,"name":"A","app":"com.example.App","start":"s0","blocks":{{"s0":{{{click},"next":"s1"}},"s1":{{{click}}}}}}}"#
            ),
        )
        .unwrap();
        let project = root.to_string_lossy().into_owned();
        let mut board = Board {
            all_tests: true,
            ..Board::default()
        };
        board.sync_tests(&["smoke/a.flow.json".into(), "gone.flow.json".into()], "P");
        let id_of = |board: &Board, file: &str| {
            board
                .nodes
                .iter()
                .find(|n| n.test() == Some(file))
                .unwrap()
                .id
                .clone()
        };
        let (a, gone) = (
            id_of(&board, "smoke/a.flow.json"),
            id_of(&board, "gone.flow.json"),
        );

        let open = open_test_on_board(project.clone(), board.clone(), a.clone()).unwrap();
        let card = open.nodes.iter().find(|n| n.id == a).unwrap();
        assert!(card.open && card.rect.height > jevision_core::CARD_HEIGHT);
        assert!(open_test_on_board(project.clone(), open.clone(), a.clone()).is_none());
        assert!(
            open_test_on_board(project.clone(), board.clone(), gone).is_none(),
            "unreadable"
        );
        let closed = close_test_on_board(open, a.clone()).unwrap();
        let (before, after) = (
            board.nodes.iter().find(|n| n.id == a).unwrap(),
            closed.nodes.iter().find(|n| n.id == a).unwrap(),
        );
        assert_eq!(before, after, "folded back into the card it was");
        assert!(close_test_on_board(closed, a).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
