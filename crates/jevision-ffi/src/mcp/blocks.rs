//! The test file format in words, for `list_block_types`: every block type with its fields and
//! an example, and the shapes blocks share. Tests check that the list matches the core.

use jevision_core::{
    AI_PROVIDERS, ASK_FORMAT, ELEMENT_ROLES, ELEMENT_STATES, LABELS_FORMAT, MAX_REPEAT,
    MAX_RETRIES, MAX_SCROLL, catalog,
};
use serde_json::{Value, json};

/// Fields and an example for every block type, in palette order.
fn details(type_name: &str) -> (&'static str, Value, Value) {
    match type_name {
        "openApp" => (
            "Launches an app and brings it to the front: the app under test, or another one. It can start the app from a clean state and fix the size and position of its window, so screenshots look the same on every run.",
            json!({
                "app": "optional bundle id of another app; leave out for the app under test",
                "resetState": "optional, true to quit a copy that is already running and start a fresh one that does not bring back its last windows",
                "windowSize": "optional {\"width\": 900, \"height\": 600} in points, 100 to 20000; the run fails with a clear reason if the window can't take it",
                "windowPosition": "optional {\"x\": 40, \"y\": 60} in points, from the top-left corner of the main display (negative on a display to its left or above); the run fails if the window can't take it",
                "arguments": "optional list of command-line arguments for the app; they reach only a copy this block starts, so use resetState to be sure. Text may hold ${env.NAME}, ${var.NAME}, ${data.COLUMN} and ${secret.NAME}",
                "environment": "optional object of environment variables for the app, {\"NAME\": \"value\"}; like arguments, only for a copy this block starts, and values may hold the same placeholders",
            }),
            json!({ "type": "openApp", "resetState": true, "windowSize": { "width": 900, "height": 600 }, "arguments": ["-ApplePersistenceIgnoreState", "YES"], "environment": { "API_URL": "${env.API_URL}" } }),
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
                "button": "optional, \"left\" (default), \"right\" or \"middle\"",
                "double": "optional, true for a double click",
                "modifiers": "optional list of keys held down during the click: \"cmd\", \"shift\", \"option\", \"ctrl\"",
                "waitForChange": "optional, true to wait until the screen changes after the click and fail if it doesn't",
            }),
            json!({ "type": "click", "target": { "image": "refs/sign-in-button.png" }, "button": "left" }),
        ),
        "hover" => (
            "Moves the pointer onto an element and leaves it there, to bring up a tooltip or a menu that opens on hover. Follow it with a waitFor or a check of what appears.",
            json!({
                "target": "where to move the pointer: a target (see shapes)",
                "modifiers": "optional list of keys held down while the pointer moves and rests: \"cmd\", \"shift\", \"option\", \"ctrl\"",
                "waitForChange": "optional, true to wait until the screen changes after the move and fail if it doesn't",
            }),
            json!({ "type": "hover", "target": { "image": "refs/help-button.png" }, "modifiers": ["option"], "waitForChange": true }),
        ),
        "drag" => (
            "Presses a mouse button on one element, moves to another holding it down, and lets go there: drag and drop, moving a slider, selecting by dragging. Both ends are found on the screen as it is before the drag.",
            json!({
                "from": "where to press: a target (see shapes)",
                "to": "where to let go: a target (see shapes), not the same as from",
                "button": "optional, \"left\" (default), \"right\" or \"middle\"",
                "modifiers": "optional list of keys held down during the drag: \"cmd\", \"shift\", \"option\", \"ctrl\"; option while dragging a file copies it",
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
                "modifiers": "optional list of keys held down while the wheel turns: \"cmd\", \"shift\", \"option\", \"ctrl\"; cmd zooms in most apps",
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
            "Waits until a picture appears or disappears, an element of the window is in a state, or the screen stops changing; fails after the timeout.",
            json!({
                "until": "\"screenStable\", or { \"appears\": reference } or { \"disappears\": reference } or { \"element\": elementCheck } (see shapes)",
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
            "Fails the test unless the screen matches the reference, or an element of the window is in the expected state.",
            json!({
                "reference": "what the screen should show: a reference (see shapes); or instead element",
                "element": "an elementCheck (see shapes): the element and the state it must be in",
            }),
            json!({ "type": "check", "reference": { "compare": "meaning", "expect": "The inbox lists at least one message" } }),
        ),
        "if" => (
            "Goes to then if its condition holds; otherwise to the then of the first elseIf whose condition holds; otherwise to else. A condition is reference (the screen matches it), value (a comparison of text or numbers) or element (an element of the window is in a state). All screen conditions are checked on the same screenshot.",
            json!({
                "reference": "what to look for: a reference (see shapes); or instead value or element",
                "value": "{ \"of\": text, usually ${var.NAME}, \"is\": \"equals\" | \"notEquals\" | \"contains\" | \"greaterThan\" | \"lessThan\", \"to\": text }. Equal numbers are equal however written (3 and 3.0); greaterThan and lessThan need numbers",
                "element": "an elementCheck (see shapes); several elements that match without an index stop the run",
                "elseIf": "optional list of more conditions, checked in order when the ones before don't hold: [{ \"reference\": reference, \"value\": value or \"element\": elementCheck, \"then\": block id }]. Their ways out are called elseIf1, elseIf2…",
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
                "package": "optional { \"version\": \"1.2.0\", \"source\": \"https://…\" } for a block from a community package (blocks/ID.jevblock/flow.json): the version the test was written with and, optionally, the https address of its repository, release or zip file, so that whoever lacks the package knows what to install",
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
                "method": "optional: \"GET\" (default), \"POST\", \"PUT\", \"PATCH\", \"DELETE\" or \"HEAD\"",
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
                "language": "optional: \"sh\", \"zsh\" (default), \"python3\", \"node\" or \"appleScript\"",
                "code": "the script",
                "inputs": "optional list of { \"name\", \"value\", \"type\" } passed as environment variables; type is \"text\" (default), \"number\", \"boolean\" (true or false), \"image\" (the path of a PNG picture in the project, handed over as an absolute path) or \"area\" (x,y,width,height in fractions of the window)",
                "outputs": "optional list of { \"name\", \"type\" } the script must write, type being text (default), number, boolean, image (the path of a PNG picture) or area (x,y,width,height in fractions of the window); each becomes ${var.NAME}",
                "timeout": "optional duration, default 30s",
            }),
            json!({ "type": "script", "language": "python3", "code": "import os\nopen(os.environ['JEV_OUTPUT'], 'a').write('total=3\\n')", "outputs": [{ "name": "total", "type": "number" }] }),
        ),
        "clipboard" => (
            "Puts text or a picture on the clipboard, keeps what is on the clipboard (its text, a picture or the copied files) as a value, or waits until the clipboard holds a text. Put replaces what was on the clipboard and leaves it there after the run; never put a secret on it unless that is the point of the test.",
            json!({
                "action": "\"put\", \"read\" or \"check\"",
                "text": "for put: the text, which may use placeholders; give text or image",
                "image": "for put, instead of text: a picture, the path of a PNG file or ${var.NAME} of a picture an earlier block kept",
                "saveAs": "for read: the name to keep it under, for ${var.NAME}",
                "content": "for read, optional: \"text\" (default), \"image\" (the picture, kept as the path of a PNG file, for the image of a target or a reference) or \"files\" (the paths of the copied files, one to a line)",
                "is": "for check: \"equals\", \"notEquals\", \"contains\", \"greaterThan\" or \"lessThan\" (the last two compare numbers)",
                "to": "for check: the text to compare with, which may use placeholders",
                "timeout": "for check, optional duration, default 10s",
            }),
            json!({ "type": "clipboard", "action": "check", "is": "contains", "to": "order ${var.order}", "timeout": "5s" }),
        ),
        "dateTime" => (
            "Keeps the current date and time as a value, written in a format and optionally moved forward or back, for ${var.NAME}: a name for a file or a record that no earlier run has used.",
            json!({
                "saveAs": "the name to keep the text under, for ${var.NAME}",
                "format": "optional strftime pattern, default \"%Y-%m-%d %H:%M:%S\"; for example %d.%m.%Y, %H:%M or %d %B %Y",
                "language": "optional language of month and day names: \"en\" or \"ru\"; default: the language JEVision runs in",
                "timeZone": "optional time zone the time is told in, an IANA name such as \"UTC\", \"Europe/Berlin\" or \"America/New_York\"; default: the time zone of this Mac",
                "shift": "optional { \"amount\": whole number, negative goes back, \"unit\": seconds, minutes, hours, days, weeks, months or years }",
            }),
            json!({ "type": "dateTime", "saveAs": "tomorrow", "format": "%d.%m.%Y", "shift": { "amount": 1, "unit": "days" } }),
        ),
        "ask" => (
            "Asks an AI about the window of the app under test and keeps the answer as a run variable for the following blocks. question \"isShown\" keeps how likely the window shows what expect says, a number from 0 to 1 (1 or 0 from a provider that only answers yes or no), for an if with greaterThan; question \"find\" keeps where the element description names is, an area x,y,width,height in fractions of the window, for the area of a target, and fails the block when the element is not there.",
            json!({
                "question": "\"isShown\" or \"find\"",
                "expect": "for isShown: what the window should show, in words",
                "provider": "for isShown, optional: the AI provider to ask instead of the one chosen in the settings, as in a check by meaning",
                "model": "for isShown, optional: the model of that provider to ask instead of its chosen one",
                "description": "for find: the element to find, in words, such as \"the blue Send button\"; asked of JEV, the AI that finds elements a target describes",
                "saveAs": "the name to keep the answer under, for ${var.NAME}",
            }),
            json!({ "type": "ask", "question": "isShown", "expect": "the dark theme is on", "saveAs": "dark" }),
        ),
        other => unreachable!("block type {other} has no description"),
    }
}

/// The whole format guide.
pub(crate) fn guide() -> Value {
    let providers = AI_PROVIDERS
        .iter()
        .map(|p| format!("\"{p}\""))
        .collect::<Vec<_>>()
        .join(", ");
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
            "format": format!(
                "file format version: 1, {LABELS_FORMAT} when the test names a file by its \
                 label, res: and 16 hex digits, or {ASK_FORMAT} when it has an ask block"
            ),
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
            "target": "{ \"image\": \"refs/button.png\" } finds the element by its picture; { \"description\": \"the blue Save button\" } has JEV (TypeSafe AI) find the element described in words; { \"point\": { \"x\": 0.5, \"y\": 0.2 } } is a fixed point in fractions of the window, from its top-left corner; { \"area\": \"0.1,0.2,0.3,0.1\" } is the middle of an area, x,y,width,height in fractions of the window. image may also be ${var.NAME} of a picture an earlier block kept, and area ${var.NAME} of an area (see variables); { \"element\": element } finds it through accessibility, by what the app says about its controls, without AI",
            "reference": format!("{{ \"image\": path, \"compare\": \"similar\" (default) | \"exact\" | \"meaning\", \"threshold\": 0–1 for similar (default 0.95), \"expect\": sentence for meaning, \"provider\" and \"model\" for meaning, \"area\": rect, \"ignore\": [rect], \"hide\": [rect] }}. With compare meaning, an AI model checks that the screen shows what expect says, or means the same as the image when expect is left out; the image is then optional. provider asks another AI provider than the one chosen in JEVision's settings, one of {providers}; model names a model of that provider instead of its chosen one; leave both out for the settings' choice. image may also be ${{var.NAME}} of a picture an earlier block kept; such a picture must exist, as it is never made from the screen."),
            "element": format!("{{ \"id\": accessibility identifier, \"role\": kind, \"title\": the exact label, \"index\": which of several that match, from 0 in the order they come in the window }}; at least one of id, role and title. role is one of {}. id and title may hold placeholders. A block that needs one element stops with the list of matches when several match without an index, or with similar elements when none does.", ELEMENT_ROLES.iter().map(|r| r.name).collect::<Vec<_>>().join(", ")),
            "elementCheck": format!("an element (see shapes) and \"is\": {}; valueEquals, valueContains, titleEquals and titleContains also need \"to\": text, which may hold placeholders. Example: {{ \"role\": \"button\", \"title\": \"Save\", \"is\": \"enabled\" }}", ELEMENT_STATES.join(" | ")),
            "rect": "{ \"x\", \"y\", \"width\", \"height\" } in fractions (0–1) of the window or image, from the top-left corner. area limits the comparison, ignore leaves parts out (clocks, counters), hide paints parts over with grey before screenshots are stored or sent anywhere, as do the hide areas of jevision.project.json and password fields",
            "duration": "text like 500ms, 10s or 2m",
            "paths": "images, files and tests are relative to the folder of the test file and must stay inside the project folder; a picture from ${var.NAME} may be anywhere on this Mac. When the reference image of a check doesn't exist yet, the first run saves the screen as that image.",
            "secrets": "${secret.NAME} in a type block's text is typed as the secret's value, which the person keeps in JEVision or the environment. Secret values never appear in tests, reports or here.",
            "variables": "${var.NAME} anywhere a block takes text is a value an earlier block of the run saved: the outputs of a script, saveOutput of a command, save of an http block, saveAs of a database block, saveAs of a clipboard read or a dateTime block. A picture (a clipboard read with content image, a script output of type image) is the path of a PNG file, for the image of a target, a reference or a clipboard put; an area (a script output of type area) is x,y,width,height in fractions of the window, for the area of a target.",
            "environments": "${env.NAME} in a type block's text is the value NAME of the environment chosen for the run, from jevision.environments.json in the project folder. ${env.ENVIRONMENT} is the name of that environment (empty when none is chosen), for text to type or an if block's value condition.",
            "library": "templates/NAME.template.json in the project folder are templates: blocks copied into a test as they are. blocks/NAME.block.json are custom blocks: tests run as one block through runTest, reading ${input.NAME} from its inputs.",
        },
    })
}
