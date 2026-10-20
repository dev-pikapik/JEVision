//! The test file format in words, for `list_block_types`: every block type with its fields and
//! an example, and the shapes blocks share. Tests check that the list matches the core.

use jevision_core::{FORMAT_VERSION, MAX_REPEAT, MAX_RETRIES, MAX_SCROLL, catalog};
use serde_json::{Value, json};

/// Fields and an example for every block type, in palette order.
fn details(type_name: &str) -> (&'static str, Value, Value) {
    match type_name {
        "openApp" => (
            "Launches an app and brings it to the front: the app under test, or another one.",
            json!({ "app": "optional bundle id of another app; leave out for the app under test" }),
            json!({ "type": "openApp" }),
        ),
        "openFile" => (
            "Opens files (each from a fresh copy, so the original stays intact) or web addresses.",
            json!({
                "files": "list of file paths relative to the test file, or http(s):// addresses",
                "app": "optional bundle id of the app to open them in; default: the system's choice",
                "newInstance": "optional, true to open them in a new instance of the app",
                "closeAfter": "optional, true to close the opened files when the test ends, passed or not, and quit the app if this block started it",
            }),
            json!({ "type": "openFile", "files": ["fixtures/note.txt"], "app": "com.apple.TextEdit" }),
        ),
        "click" => (
            "Clicks an element found by its picture or its description in words, or a fixed point of the window.",
            json!({
                "target": "where to click: a target (see shapes)",
                "button": "optional, \"left\" (default) or \"right\"",
                "double": "optional, true for a double click",
                "waitForChange": "optional, true to wait until the screen changes after the click and fail if it doesn't",
            }),
            json!({ "type": "click", "target": { "image": "refs/sign-in-button.png" }, "button": "left" }),
        ),
        "hover" => (
            "Moves the pointer onto an element and leaves it there, to bring up a tooltip or a menu that opens on hover. Follow it with a waitFor or a check of what appears.",
            json!({
                "target": "where to move the pointer: a target (see shapes)",
                "waitForChange": "optional, true to wait until the screen changes after the move and fail if it doesn't",
            }),
            json!({ "type": "hover", "target": { "image": "refs/help-button.png" }, "waitForChange": true }),
        ),
        "drag" => (
            "Presses the left button on one element, moves to another holding it down, and lets go there: drag and drop, moving a slider, selecting by dragging. Both ends are found on the screen as it is before the drag.",
            json!({
                "from": "where to press: a target (see shapes)",
                "to": "where to let go: a target (see shapes), not the same as from",
                "waitForChange": "optional, true to wait until the screen changes after the drop and fail if it doesn't",
            }),
            json!({ "type": "drag", "from": { "image": "refs/report-file.png" }, "to": { "image": "refs/archive-folder.png" } }),
        ),
        "scroll" => (
            "Turns the scroll wheel with the pointer over a place in the window: the middle, or a target.",
            json!({
                "direction": "\"up\", \"down\", \"left\" or \"right\"; down brings what is further down into sight",
                "amount": format!("how far, 1 to {MAX_SCROLL}"),
                "unit": "optional, \"lines\" (default; one notch of a mouse wheel each) or \"pixels\" (points on screen)",
                "at": "optional target (see shapes) to scroll over; default: the middle of the window",
                "waitForChange": "optional, true to wait until the screen changes after scrolling and fail if it doesn't",
            }),
            json!({ "type": "scroll", "direction": "down", "amount": 5, "unit": "lines", "at": { "image": "refs/message-list.png" } }),
        ),
        "type" => (
            "Types text, optionally clicking into a field first.",
            json!({
                "text": "text to type; may hold ${secret.NAME}, ${env.NAME} and ${data.COLUMN} placeholders",
                "into": "optional target (see shapes) to click before typing",
            }),
            json!({ "type": "type", "text": "${secret.ACME_PASSWORD}", "into": { "image": "refs/password-field.png" } }),
        ),
        "pressKeys" => (
            "Presses one key combination.",
            json!({ "keys": "lowercase modifiers (cmd, shift, option, ctrl) and one key, joined with +: a character, or return, tab, space, delete, forwarddelete, escape, up, down, left, right, home, end, pageup or pagedown. Examples: cmd+shift+s, return" }),
            json!({ "type": "pressKeys", "keys": "cmd+s" }),
        ),
        "waitFor" => (
            "Waits until a picture appears or disappears, or the screen stops changing; fails after the timeout.",
            json!({
                "until": "\"screenStable\", or { \"appears\": reference } or { \"disappears\": reference } (see shapes)",
                "timeout": "optional duration (see shapes), 10s by default",
            }),
            json!({ "type": "waitFor", "until": { "appears": { "image": "refs/inbox.png" } }, "timeout": "10s" }),
        ),
        "pause" => (
            "Waits a fixed time, whatever the screen shows.",
            json!({ "duration": "how long to wait: a duration (see shapes), up to 1h" }),
            json!({ "type": "pause", "duration": "2s" }),
        ),
        "check" => (
            "Fails the test unless the screen matches the reference.",
            json!({ "reference": "what the screen should show: a reference (see shapes)" }),
            json!({ "type": "check", "reference": { "compare": "meaning", "expect": "The inbox lists at least one message" } }),
        ),
        "if" => (
            "Goes to then if its condition holds; otherwise to the then of the first elseIf whose condition holds; otherwise to else. A condition is either reference (the screen matches it) or value (a comparison of text or numbers). All screen conditions are checked on the same screenshot.",
            json!({
                "reference": "what to look for: a reference (see shapes); or instead value",
                "value": "{ \"of\": text, usually ${var.NAME}, \"is\": \"equals\" | \"notEquals\" | \"contains\" | \"greaterThan\" | \"lessThan\", \"to\": text }. Equal numbers are equal however written (3 and 3.0); greaterThan and lessThan need numbers",
                "elseIf": "optional list of more conditions, checked in order when the ones before don't hold: [{ \"reference\": reference or \"value\": value, \"then\": block id }]. Their ways out are called elseIf1, elseIf2…",
            }),
            json!({ "type": "if", "reference": { "image": "refs/welcome-dialog.png" }, "elseIf": [{ "reference": { "image": "refs/update-dialog.png" } }] }),
        ),
        "repeat" => (
            "Runs the blocks on its body a number of times, then goes to next. The last block of the body leads back to the repeat block.",
            json!({ "times": format!("how many times, 1 to {MAX_REPEAT}") }),
            json!({ "type": "repeat", "times": 3 }),
        ),
        "runTest" => (
            "Runs another test file as one step, or a custom block: a .block.json file whose ${input.NAME} placeholders take the values in inputs.",
            json!({
                "test": "path of the other test file (.flow.json) or custom block (.block.json), relative to this test file",
                "inputs": "optional object: a value for every ${input.NAME} the custom block reads; values may use placeholders",
            }),
            json!({ "type": "runTest", "test": "sign-in.flow.json" }),
        ),
        "useData" => (
            "Runs the blocks on its body once for every row of a CSV file, then goes to next; ${data.COLUMN} is the value of the current row. The last block of the body leads back to the useData block.",
            json!({ "file": "CSV file with a header row, relative to the test file" }),
            json!({ "type": "useData", "file": "data/accounts.csv" }),
        ),
        "http" => (
            "Sends an HTTP request and fails unless the answer has the expected status; can save parts of the answer for ${var.NAME}.",
            json!({
                "method": "optional: GET (default), POST, PUT, PATCH, DELETE or HEAD",
                "url": "http:// or https:// address",
                "headers": "optional list of { \"name\", \"value\" }",
                "body": "optional text to send; JSON is sent as application/json unless a Content-Type header says otherwise",
                "expectStatus": "optional: \"success\" (default, any 2xx), \"any\", or a status code such as 201",
                "save": "optional list of { \"name\", \"from\" }: from is status, body, body.PATH into a JSON answer (items[0].id) or header.NAME",
                "timeout": "optional duration, default 30s",
            }),
            json!({ "type": "http", "method": "POST", "url": "${env.API}/orders", "body": "{\"item\": 1}", "expectStatus": 201, "save": [{ "name": "order", "from": "body.id" }] }),
        ),
        "database" => (
            "Runs one SQL statement on an SQLite file; can save the first column of the first row and wait until the result is as expected.",
            json!({
                "file": "SQLite file: absolute, ~/… or relative to the test file",
                "query": "one SQL statement",
                "saveAs": "optional name to save the first value as, for ${var.NAME}",
                "expect": "optional { \"value\": text } for the first value or { \"rows\": count }; the query runs again until it holds or the timeout passes",
                "timeout": "optional duration, default 10s",
            }),
            json!({ "type": "database", "file": "~/Library/Containers/com.example.App/Data/app.sqlite", "query": "select count(*) from notes", "expect": { "value": "1" } }),
        ),
        "checkFile" => (
            "Waits until a file appears, disappears, changes after the run started or contains a text.",
            json!({
                "path": "file path: absolute, ~/… or relative to the test file",
                "until": "\"exists\", \"missing\", \"changed\" or { \"contains\": text }",
                "timeout": "optional duration, default 10s",
            }),
            json!({ "type": "checkFile", "path": "~/Downloads/report.pdf", "until": "exists" }),
        ),
        "command" => (
            "Runs a command line in zsh and fails unless it ends with the expected exit code; can save what it printed.",
            json!({
                "command": "the command line",
                "folder": "optional folder to run in, relative to the test file; default: the test's folder",
                "exitCode": "optional expected exit code, default 0",
                "saveOutput": "optional name to save what it printed as, for ${var.NAME}",
                "timeout": "optional duration, default 30s",
            }),
            json!({ "type": "command", "command": "defaults read com.example.App lastUser", "saveOutput": "user" }),
        ),
        "script" => (
            "Runs a script and fails if it ends with an exit code other than 0. Inputs reach it as environment variables; it saves outputs by writing name=value lines to the file in $JEV_OUTPUT.",
            json!({
                "language": "sh, zsh (default), python3, node or appleScript",
                "code": "the script",
                "inputs": "optional list of { \"name\", \"value\" } passed as environment variables",
                "outputs": "optional list of { \"name\", \"type\": \"text\" | \"number\" | \"boolean\" } the script must write; each becomes ${var.NAME}",
                "timeout": "optional duration, default 30s",
            }),
            json!({ "type": "script", "language": "python3", "code": "import os\nopen(os.environ['JEV_OUTPUT'], 'a').write('total=3\\n')", "outputs": [{ "name": "total", "type": "number" }] }),
        ),
        other => unreachable!("block type {other} has no description"),
    }
}

/// The whole format guide.
pub(crate) fn guide() -> Value {
    let blocks: Vec<Value> = catalog()
        .into_iter()
        .map(|spec| {
            let (description, fields, example) = details(&spec.type_name);
            json!({
                "type": spec.type_name,
                "group": spec.group,
                "edges": spec.edges,
                "description": description,
                "fields": fields,
                "example": example,
            })
        })
        .collect();
    json!({
        "file": {
            "format": format!("file format version, always {FORMAT_VERSION}"),
            "name": "name of the test",
            "app": "bundle id of the app under test, such as com.apple.TextEdit",
            "start": "id of the first block",
            "blocks": "object of blocks by id; ids use letters, digits, - and _",
            "description": "optional, what the test checks",
            "testType": "optional: smoke, regression or acceptance",
            "priority": "optional: critical, high, medium or low",
            "status": "optional: draft, ready or outdated",
            "tags": "optional list of words for finding tests",
            "preconditions": "optional, what must be true before the test starts",
            "author": "optional, who wrote the test or looks after it",
            "platform": "optional, where the test is meant to run, such as macOS 26",
            "link": "optional web address (https://…) of the issue or requirement the test covers",
            "flaky": "optional, true when the test sometimes fails for reasons outside the app; it still runs and counts",
            "retries": format!("optional, how many times a failed run is tried again, 0 to {MAX_RETRIES}"),
            "icon": "optional SF Symbol name a custom block (.block.json) is shown with",
        },
        "edges": "Blocks lead to each other by id: next for the block after; then, elseIf1, elseIf2… and else for an if; body for the blocks inside a repeat or useData. A block without next ends the test (inside a loop: the current pass). Only a loop body may lead back to an earlier block.",
        "blocks": blocks,
        "shapes": {
            "target": "{ \"image\": \"refs/button.png\" } finds the element by its picture; { \"description\": \"the blue Save button\" } has JEV (TypeSafe AI) find the element described in words; { \"point\": { \"x\": 0.5, \"y\": 0.2 } } is a fixed point in fractions of the window, from its top-left corner",
            "reference": "{ \"image\": path, \"compare\": \"similar\" (default) | \"exact\" | \"meaning\", \"threshold\": 0–1 for similar (default 0.95), \"expect\": sentence for meaning, \"area\": rect, \"ignore\": [rect], \"hide\": [rect] }. With compare meaning, an AI model checks that the screen shows what expect says; the image is then optional.",
            "rect": "{ \"x\", \"y\", \"width\", \"height\" } in fractions (0–1) of the window or image, from the top-left corner. area limits the comparison, ignore leaves parts out (clocks, counters), hide paints parts over with grey before screenshots are stored or sent anywhere, as do the hide areas of jevision.project.json and password fields",
            "duration": "text like 500ms, 10s or 2m",
            "paths": "images, files and tests are relative to the folder of the test file and must stay inside the project folder. When the reference image of a check doesn't exist yet, the first run saves the screen as that image.",
            "secrets": "${secret.NAME} in a type block's text is typed as the secret's value, which the person keeps in JEVision or the environment. Secret values never appear in tests, reports or here.",
            "variables": "${var.NAME} anywhere a block takes text is a value an earlier block of the run saved: the outputs of a script, saveOutput of a command, save of an http block, saveAs of a database block.",
            "environments": "${env.NAME} in a type block's text is the value NAME of the environment chosen for the run, from jevision.environments.json in the project folder.",
            "library": "templates/NAME.template.json in the project folder are templates: blocks copied into a test as they are. blocks/NAME.block.json are custom blocks: tests run as one block through runTest, reading ${input.NAME} from its inputs.",
        },
    })
}
