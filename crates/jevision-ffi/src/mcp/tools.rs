//! The tools assistants call: what each one is for, its input schema, and the work it does.
//! Paths are relative to the project folder and must stay inside it, symbolic links included.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use jevision_core::{
    Flow, Issue, NEXT, PROJECT_FILE, ProjectSettings, Rect, START, Severity, Slot, TestCard,
    TestFilter, describe, parse, parse_block, to_canonical_json, validate_in,
};
use jevision_runner::{MaskedImage, Run, StepStatus, Verdict};

use crate::{BugReport, draft_bug_report};
use serde_json::{Map, Value, json};

use super::jobs::RunChoices;
use super::{Server, blocks, hands_message, text};

/// Longest a `get_run` call waits for a run to end.
const MAX_WAIT_SECONDS: u64 = 600;
/// Most pictures one `get_report` call returns.
const MAX_IMAGES: usize = 30;
const TEST_SUFFIX: &str = ".flow.json";

type Content = Result<Vec<Value>, String>;

/// Every tool with its description and the JSON Schema of its arguments.
pub(super) fn definitions() -> Vec<Value> {
    let path = |what: &str| json!({ "type": "string", "description": what });
    let test_path =
        path("Test file, relative to the project folder, such as login/sign-in.flow.json.");
    let run_id = json!({
        "type": "string",
        "description": "Run id from run_test. Leave out for the newest run.",
    });
    vec![
        tool(
            "list_tests",
            "List tests",
            "Lists the tests (*.flow.json files) in the project with their name, app, tags, status and how many problems each has. Give query to find tests by words in their path, name, description, app or tags, and testType, priority, status, author or flaky to keep only the tests whose card says so.",
            json!({
                "query": { "type": "string", "description": "Words to look for; every word must match. Case doesn't matter." },
                "folder": path("Only list tests in this folder, relative to the project folder."),
                "testType": { "type": "string", "enum": ["smoke", "regression", "acceptance"], "description": "Only tests of this type." },
                "priority": { "type": "string", "enum": ["critical", "high", "medium", "low"], "description": "Only tests with this priority." },
                "status": { "type": "string", "enum": ["draft", "ready", "outdated"], "description": "Only tests with this status." },
                "author": { "type": "string", "description": "Only tests written by this author, as the card names them. Case doesn't matter." },
                "flaky": { "type": "boolean", "description": "true keeps only the tests marked flaky in their card." },
            }),
            &[],
            read_only(),
        ),
        tool(
            "read_test",
            "Read a test",
            "Returns the JSON of a test file exactly as saved, and the problems found in it. Secrets appear only as ${secret.NAME}; their values are never available.",
            json!({ "path": test_path }),
            &["path"],
            read_only(),
        ),
        tool(
            "write_test",
            "Write a test",
            "Creates or replaces a test file with the given JSON, saved in canonical form (fixed key order, blocks in reading order, the click button, scroll unit, comparison and timeout written out, settings that are off or empty left out). Nothing is saved if the test has errors; the answer then lists them with the block and field each concerns. Call list_block_types first to learn the format.",
            json!({
                "path": path("Test file to write, relative to the project folder; must end in .flow.json. Missing folders are created."),
                "test": {
                    "type": ["object", "string"],
                    "description": "The whole test: a JSON object (or its text) with format, name, app, start and blocks.",
                },
            }),
            &["path", "test"],
            json!({ "readOnlyHint": false, "destructiveHint": true, "idempotentHint": true, "openWorldHint": false }),
        ),
        tool(
            "add_blocks",
            "Add blocks",
            "Adds blocks to a test as one chain: each leads to the next, and the last to whatever came after the place they go. Give each block as in a test file, without an id or edges (next, then, else, body, and then inside elseIf); ids are made from the block types and returned. Without after, the blocks go at the end of the main chain. The test is saved; the answer lists the problems left in it.",
            json!({
                "path": test_path,
                "blocks": {
                    "type": "array",
                    "minItems": 1,
                    "items": { "type": "object" },
                    "description": "Blocks in order, such as { \"type\": \"pressKeys\", \"keys\": \"cmd+s\" }. Only the last one may be an if block.",
                },
                "after": { "type": "string", "description": "Id of the block to add them after." },
                "atStart": { "type": "boolean", "description": "true to add them at the very beginning of the test, before its first block." },
                "edge": {
                    "type": "string",
                    "pattern": "^(next|then|else|body|elseIf[1-9][0-9]*)$",
                    "description": "Which way out of the after block they go on: then, elseIf1, elseIf2… or else for an if block, body to put them inside a repeat or useData block. Default: next.",
                },
            }),
            &["path", "blocks"],
            json!({ "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false }),
        ),
        tool(
            "remove_block",
            "Remove a block",
            "Removes a block from a test and joins its neighbours: whatever led to it then leads to what came after it. Removing a repeat or useData block keeps the blocks inside it; removing an if block keeps its first branch that leads anywhere. The test is saved.",
            json!({
                "path": test_path,
                "block": { "type": "string", "description": "Id of the block to remove." },
            }),
            &["path", "block"],
            json!({ "readOnlyHint": false, "destructiveHint": true, "idempotentHint": false, "openWorldHint": false }),
        ),
        tool(
            "validate_test",
            "Check tests",
            "Checks a test file, or every test in a folder, for errors and warnings, each with the block and field it concerns. Tests with errors can't run.",
            json!({ "path": path("Test file or folder, relative to the project folder. Use \".\" for the whole project.") }),
            &["path"],
            read_only(),
        ),
        tool(
            "list_block_types",
            "Describe the test format",
            "Describes the test file format: the fields of a test, every block type with its fields and an example, how blocks lead to each other, and the shapes of targets, references, areas and durations.",
            json!({}),
            &[],
            read_only(),
        ),
        tool(
            "run_test",
            "Run tests",
            "Runs a test, or every test in a folder one after another, on this Mac for real: JEVision opens the app, clicks and types in it. Works only if the person at this Mac allowed assistants to run tests in JEVision's settings and the screen is unlocked. Returns at once with a run id; call get_run to wait for the result. One run at a time.",
            json!({
                "path": path("Test file or folder, relative to the project folder."),
                "retries": { "type": "integer", "minimum": 0, "maximum": jevision_core::MAX_RETRIES, "description": "Try a failed test this many more times, instead of the number in the test." },
                "env": { "type": "string", "description": "Environment from the project's jevision.environments.json, such as staging: its values fill ${env.NAME} in the tests and its secrets are used. Leave out to use the file's \"default\" environment, or none when it names no default." },
                "rows": { "type": "string", "description": "Rows of a data-driven test's CSV table to run, counted from 1: all (the default), one row such as 2, or a range such as 2-4. Each row runs the whole test once and gets its own verdict." },
            }),
            &["path"],
            json!({ "readOnlyHint": false, "destructiveHint": true, "idempotentHint": false, "openWorldHint": true }),
        ),
        tool(
            "get_run",
            "Get a run",
            "How a run started with run_test goes: running or finished, and for each test its verdict (passed, failed, cancelled, notRun, error, running or waiting), the step it is on, why it failed, and its report name for get_report. Give wait to wait until the run ends.",
            json!({
                "run": run_id,
                "wait": { "type": "integer", "minimum": 0, "maximum": MAX_WAIT_SECONDS, "description": "Seconds to wait for the run to end before answering. Default: 0, answer at once." },
            }),
            &[],
            read_only(),
        ),
        tool(
            "stop_run",
            "Stop a run",
            "Stops a run started with run_test: the test that is running ends as cancelled and the ones after it don't run.",
            json!({ "run": run_id }),
            &[],
            json!({ "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false }),
        ),
        tool(
            "get_report",
            "Get a report",
            "The saved report of one test run: verdict, every step with its status, why it failed, and what the AI model answered for checks by meaning. Give images to also get the screenshots. Secret values are masked.",
            json!({
                "report": { "type": "string", "description": "Report name from get_run. Leave out for the newest report in the project." },
                "images": {
                    "type": "string",
                    "enum": ["none", "failed", "all"],
                    "description": format!("Screenshots to include: none (default), those of failed steps, or all (up to {MAX_IMAGES})."),
                },
            }),
            &[],
            read_only(),
        ),
        tool(
            "screenshot",
            "Take a screenshot",
            "A picture of the front window of an app, as it is now. Works only if the person at this Mac allowed assistants to take screenshots in JEVision's settings.",
            json!({
                "app": { "type": "string", "description": "Bundle id of the app, such as com.apple.TextEdit." },
                "path": path("A test file instead of app: its app is pictured."),
            }),
            &[],
            json!({ "readOnlyHint": true, "openWorldHint": true }),
        ),
        tool(
            "draft_bug_report",
            "Draft a bug report",
            "The bug report JEVision would file about a failed run: a title, and a Markdown text with the cause, the steps up to the failure, what was expected and what was seen, and where it ran, secret values masked. Nothing is sent. To look into the cause, get_report with images: failed shows the failed step's screenshots.",
            json!({ "report": { "type": "string", "description": "Report name from get_run. Leave out for the newest failed run." } }),
            &[],
            read_only(),
        ),
        tool(
            "send_bug_report",
            "Send a bug report",
            "Files the bug report about a failed run as an issue of the project's GitHub repository, with the failed step's screenshots (private areas painted over, secret values masked). If an issue for the same failure is already open, nothing is sent and the answer names it: then add this run to it with commentOn, or give newIssue: true. Show the person the draft from draft_bug_report and send only when they agree.",
            json!({
                "report": { "type": "string", "description": "Report name from get_run. Leave out for the newest failed run." },
                "title": { "type": "string", "description": "Title to use instead of the drafted one." },
                "body": { "type": "string", "description": "Markdown text to use instead of the drafted one, such as the draft with your analysis of the cause added." },
                "screenshots": { "type": "array", "items": { "type": "string" }, "description": "Labels of the screenshots to include, from draft_bug_report. Default: all of them." },
                "commentOn": { "type": "integer", "minimum": 1, "description": "Number of an open issue to add this run to as a comment, instead of opening a new issue." },
                "newIssue": { "type": "boolean", "description": "true to open a new issue even if one for the same failure is open." },
            }),
            &[],
            json!({ "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": true }),
        ),
    ]
}

fn tool(
    name: &str,
    title: &str,
    description: &str,
    properties: Value,
    required: &[&str],
    annotations: Value,
) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false,
        },
        "annotations": annotations,
    })
}

fn read_only() -> Value {
    json!({ "readOnlyHint": true, "openWorldHint": false })
}

impl Server {
    /// The content `name` returns, an error in words for the assistant, or `None` for a tool that
    /// doesn't exist.
    pub(super) fn call_tool(&self, name: &str, args: &Value) -> Option<Content> {
        Some(match name {
            "list_tests" => self.list_tests(args),
            "read_test" => self.read_test(args),
            "write_test" => self.write_test(args),
            "add_blocks" => self.add_blocks(args),
            "remove_block" => self.remove_one(args),
            "validate_test" => self.validate_tests(args),
            "list_block_types" => Ok(vec![pretty(&blocks::guide())]),
            "run_test" => self.run_test(args),
            "get_run" => self.get_run(args),
            "stop_run" => self.stop_run(args),
            "get_report" => self.get_report(args),
            "screenshot" => self.screenshot(args),
            "draft_bug_report" => self.draft_bug_report(args),
            "send_bug_report" => self.send_bug_report(args),
            _ => return None,
        })
    }

    fn list_tests(&self, args: &Value) -> Content {
        let folder = match optional(args, "folder")? {
            Some(folder) => self.resolve(folder)?,
            None => self.project.clone(),
        };
        let words: Vec<String> = optional(args, "query")?
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_lowercase)
            .collect();
        let flaky = match args.get("flaky") {
            None | Some(Value::Null) => false,
            Some(v) => v.as_bool().ok_or("\"flaky\" must be true or false.")?,
        };
        let filter = TestFilter::from_words(
            optional(args, "testType")?,
            optional(args, "priority")?,
            optional(args, "status")?,
            optional(args, "author")?,
            flaky,
        )?;
        let mut tests = Vec::new();
        for file in self.collect(&folder)? {
            let path = self.relative(&file);
            let entry = match fs::read_to_string(&file).map(|t| parse(&t)) {
                // A test that can't be read has no card to choose it by.
                Err(_) | Ok(Err(_)) if !filter.is_empty() => continue,
                Err(e) => json!({ "path": path, "error": format!("can't be read: {e}") }),
                Ok(Err(e)) => {
                    json!({ "path": path, "errors": e.issues.len(), "problems": described(&e.issues) })
                }
                Ok(Ok(flow)) if !filter.matches(&TestCard::of(&flow, false)) => continue,
                Ok(Ok(flow)) => {
                    let issues = checked(&flow, &file);
                    let errors = issues
                        .iter()
                        .filter(|i| i.severity == Severity::Error)
                        .count();
                    json!({
                        "path": path,
                        "name": flow.name,
                        "description": flow.description,
                        "app": flow.app,
                        "tags": flow.tags,
                        "testType": flow.test_type,
                        "priority": flow.priority,
                        "status": flow.status,
                        "preconditions": flow.preconditions,
                        "author": flow.author,
                        "platform": flow.platform,
                        "link": flow.link,
                        "flaky": flow.flaky,
                        "blocks": flow.blocks.len(),
                        "errors": errors,
                        "warnings": issues.len() - errors,
                    })
                }
            };
            let haystack = entry.to_string().to_lowercase();
            if words.iter().all(|w| haystack.contains(w.as_str())) {
                tests.push(entry);
            }
        }
        let summary = match (tests.len(), words.is_empty()) {
            (0, true) if filter.is_empty() => "The project has no tests yet.".to_owned(),
            (0, _) => "No test matches.".to_owned(),
            (1, _) => "1 test.".to_owned(),
            (n, _) => format!("{n} tests."),
        };
        Ok(vec![text(summary), pretty(&json!({ "tests": tests }))])
    }

    fn read_test(&self, args: &Value) -> Content {
        let file = self.test_file(required(args, "path")?)?;
        let content = fs::read_to_string(&file).map_err(|e| self.io_error(&file, &e))?;
        let issues = match parse(&content) {
            Ok(flow) => checked(&flow, &file),
            Err(e) => e.issues,
        };
        Ok(vec![text(content), text(problems(&issues))])
    }

    fn write_test(&self, args: &Value) -> Content {
        let path = required(args, "path")?;
        let content = match args.get("test") {
            Some(Value::String(s)) => s.clone(),
            Some(object @ Value::Object(_)) => object.to_string(),
            _ => return Err("Give the test as a JSON object in \"test\".".to_owned()),
        };
        let file = self.test_file(path)?;
        let flow = parse(&content).map_err(|e| not_saved(&e.issues))?;
        let issues = checked(&flow, &file);
        if issues.iter().any(|i| i.severity == Severity::Error) {
            return Err(not_saved(&issues));
        }
        let existed = file.exists();
        self.save(&file, &flow)?;
        let done = if existed { "Saved" } else { "Created" };
        Ok(vec![text(format!(
            "{done} {}. {}",
            self.relative(&file),
            problems(&issues)
        ))])
    }

    fn add_blocks(&self, args: &Value) -> Content {
        let file = self.test_file(required(args, "path")?)?;
        let Some(Value::Array(items)) = args
            .get("blocks")
            .filter(|b| b.as_array().is_some_and(|a| !a.is_empty()))
        else {
            return Err("Give the blocks to add as a list in \"blocks\".".to_owned());
        };
        let mut flow = self.load(&file)?;
        let mut kinds = Vec::with_capacity(items.len());
        let mut wrong = Vec::new();
        for (n, item) in items.iter().enumerate() {
            match parse_block(&without_edges(item)) {
                Ok(kind) => kinds.push(kind),
                Err(e) => wrong.extend(
                    e.issues
                        .iter()
                        .map(|i| format!("- blocks[{n}] — {}", describe(i))),
                ),
            }
        }
        if !wrong.is_empty() {
            return Err(format!("Nothing was added:\n{}", wrong.join("\n")));
        }
        let at_start = match args.get("atStart") {
            None | Some(Value::Null) => false,
            Some(v) => v.as_bool().ok_or("\"atStart\" must be true or false.")?,
        };
        let slot = place(
            &flow,
            at_start,
            optional(args, "after")?,
            optional(args, "edge")?,
        )?;
        let ids = flow.insert_blocks(kinds, slot.as_ref()).ok_or(
            "Nothing was added: only the last of the blocks may be an if block, since the blocks after it would have no single way in.",
        )?;
        self.save(&file, &flow)?;
        Ok(vec![
            text(format!(
                "Added {} to {}. {}",
                ids.iter()
                    .map(|id| format!("\"{id}\""))
                    .collect::<Vec<_>>()
                    .join(", "),
                self.relative(&file),
                problems(&checked(&flow, &file))
            )),
            pretty(&json!({ "added": ids })),
        ])
    }

    fn remove_one(&self, args: &Value) -> Content {
        let file = self.test_file(required(args, "path")?)?;
        let id = required(args, "block")?;
        let mut flow = self.load(&file)?;
        if !flow.blocks.iter().any(|b| b.id == id) {
            return Err(no_block(&flow, id));
        }
        flow.remove_block(id);
        self.save(&file, &flow)?;
        Ok(vec![text(format!(
            "Removed \"{id}\" from {}. {}",
            self.relative(&file),
            problems(&checked(&flow, &file))
        ))])
    }

    fn validate_tests(&self, args: &Value) -> Content {
        let target = self.resolve(required(args, "path")?)?;
        let files = self.tests_at(&target)?;
        let mut lines = Vec::new();
        for file in &files {
            let issues = match fs::read_to_string(file).map(|t| parse(&t)) {
                Err(e) => {
                    lines.push(format!("{}: can't be read: {e}", self.relative(file)));
                    continue;
                }
                Ok(Err(e)) => e.issues,
                Ok(Ok(flow)) => checked(&flow, file),
            };
            lines.push(format!("{}: {}", self.relative(file), problems(&issues)));
        }
        Ok(vec![text(lines.join("\n"))])
    }

    fn run_test(&self, args: &Value) -> Content {
        if !self.host.permissions().run_tests {
            return Err("Running tests is turned off. Tests click and type on this Mac, so the person at it decides: they can turn on \"Allow assistants to run tests\" in JEVision → Settings → AI → Assistants. Ask them, then try again.".to_owned());
        }
        let target = self.resolve(required(args, "path")?)?;
        let retries = match args.get("retries") {
            None | Some(Value::Null) => None,
            Some(v) => Some(
                v.as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| *n <= jevision_core::MAX_RETRIES)
                    .ok_or(format!(
                        "\"retries\" must be a whole number from 0 to {}.",
                        jevision_core::MAX_RETRIES
                    ))?,
            ),
        };
        let known = jevision_core::Environments::load(&self.project).map_err(|reason| {
            format!(
                "{} can't be read: {reason}",
                jevision_core::ENVIRONMENTS_FILE
            )
        })?;
        let environment = match optional(args, "env")? {
            None => known.default,
            Some(name) => {
                if known.get(name).is_none() {
                    let names: Vec<&str> = known.all.keys().map(String::as_str).collect();
                    return Err(if names.is_empty() {
                        format!(
                            "The project has no environments: add them to {}.",
                            jevision_core::ENVIRONMENTS_FILE
                        )
                    } else {
                        format!(
                            "There is no environment \"{name}\". The project has: {}.",
                            names.join(", ")
                        )
                    });
                }
                Some(name.to_owned())
            }
        };
        let rows = match optional(args, "rows")? {
            None => None,
            Some(text) => jevision_core::parse_row_choice(text)?.map(|r| crate::DataRowRange {
                first: u32::try_from(*r.start()).unwrap_or(u32::MAX),
                last: u32::try_from(*r.end()).unwrap_or(u32::MAX),
            }),
        };
        let tests: Vec<String> = self
            .tests_at(&target)?
            .iter()
            .map(|f| self.relative(f))
            .collect();
        if let Some(problem) = self.host.readiness_problem() {
            return Err(format!("This Mac can't run tests right now. {problem}"));
        }
        let job = self.jobs.start(
            &self.project,
            tests.clone(),
            RunChoices {
                retries,
                environment,
                rows,
            },
            self.host.clone(),
        )?;
        Ok(vec![
            text(format!(
                "Run {} started. Call get_run with run \"{}\" and wait to get the result.",
                job.id, job.id
            )),
            pretty(&json!({ "run": job.id, "tests": tests })),
        ])
    }

    fn get_run(&self, args: &Value) -> Content {
        let job = self.jobs.get(optional(args, "run")?)?;
        let wait = match args.get("wait") {
            None | Some(Value::Null) => 0,
            Some(v) => v
                .as_u64()
                .filter(|s| *s <= MAX_WAIT_SECONDS)
                .ok_or(format!(
                    "\"wait\" must be a whole number of seconds from 0 to {MAX_WAIT_SECONDS}."
                ))?,
        };
        Ok(vec![pretty(&job.status(Duration::from_secs(wait)))])
    }

    fn stop_run(&self, args: &Value) -> Content {
        let job = self.jobs.get(optional(args, "run")?)?;
        job.stop();
        Ok(vec![text(format!(
            "Run {} is stopping. Call get_run to see how it ended.",
            job.id
        ))])
    }

    /// The run of the report named in `args`; `None` when none is named.
    fn named_run(&self, args: &Value) -> Result<Option<Run>, String> {
        let Some(name) = optional(args, "report")? else {
            return Ok(None);
        };
        let mut parts = Path::new(name).components();
        let (Some(Component::Normal(_)), None) = (parts.next(), parts.next()) else {
            return Err(format!(
                "\"{name}\" isn't a report name. Report names come from get_run, such as 2026-10-18-120000-sign-in."
            ));
        };
        let dir = self.project.join(".jevision").join("runs").join(name);
        Run::load(&dir)
            .map(Some)
            .map_err(|_| format!("There is no readable report \"{name}\"."))
    }

    /// The bug report about the run of the report named in `args`, or the newest failed one.
    fn bug_draft(&self, args: &Value) -> Result<(Run, BugReport), String> {
        let run = match self.named_run(args)? {
            Some(run) => run,
            None => Run::list(&self.project)
                .ok()
                .and_then(|runs| {
                    runs.into_iter()
                        .find(|r| r.report.verdict == Verdict::Failed)
                })
                .ok_or("No run in this project has failed. Name the report with report.")?,
        };
        let draft = draft_bug_report(
            self.project.to_string_lossy().into_owned(),
            run.dir.to_string_lossy().into_owned(),
            self.host.secrets(),
        )
        .map_err(|e| e.to_string())?
        .ok_or("That run didn't fail, so there is no bug to report.")?;
        Ok((run, draft))
    }

    fn draft_bug_report(&self, args: &Value) -> Content {
        let (run, draft) = self.bug_draft(args)?;
        Ok(vec![pretty(&json!({
            "report": run.dir.file_name().map(|n| n.to_string_lossy()),
            "title": draft.title,
            "body": draft.body,
            "screenshots": draft.screenshots.iter().map(|s| &s.label).collect::<Vec<_>>(),
        }))])
    }

    fn send_bug_report(&self, args: &Value) -> Content {
        let (run, mut draft) = self.bug_draft(args)?;
        if let Some(title) = optional(args, "title")? {
            draft.title = title.to_owned();
        }
        if let Some(body) = optional(args, "body")? {
            draft.body = body.to_owned();
        }
        if let Some(chosen) = args.get("screenshots") {
            let chosen: Vec<&str> = chosen
                .as_array()
                .and_then(|labels| labels.iter().map(Value::as_str).collect())
                .ok_or("\"screenshots\" must be a list of labels from draft_bug_report.")?;
            draft
                .screenshots
                .retain(|s| chosen.contains(&s.label.as_str()));
        }
        let comment_on = match args.get("commentOn") {
            None => None,
            Some(number) => Some(
                number
                    .as_u64()
                    .ok_or("\"commentOn\" must be an issue number.")?,
            ),
        };
        let project = self.project.to_string_lossy().into_owned();
        let (repo, hub) = crate::git::on_github(&project).map_err(|e| github_message(&e))?;
        let github = repo.github().map_err(|e| github_message(&e))?.1;
        if comment_on.is_none() && args.get("newIssue").and_then(Value::as_bool) != Some(true) {
            let open = hub
                .open_issue_with(&github, &crate::bug::marker(&draft.fingerprint))
                .map_err(|e| github_message(&e))?;
            if let Some(issue) = open {
                return Err(format!(
                    "Issue #{} \"{}\" ({}) is already open for this failure. Call again with commentOn: {} to add this run to it, or with newIssue: true to open another issue anyway.",
                    issue.number, issue.title, issue.url, issue.number
                ));
            }
        }
        let url = crate::bug::send(
            &hub,
            &github,
            &self.project,
            &run.dir,
            &draft,
            comment_on,
            &*self.host.secrets(),
        )
        .map_err(|e| github_message(&e))?;
        Ok(vec![text(match comment_on {
            Some(number) => format!("Added the run to issue #{number}: {url}"),
            None => format!("Opened the issue: {url}"),
        })])
    }

    fn get_report(&self, args: &Value) -> Content {
        let run = match self.named_run(args)? {
            Some(run) => run,
            None => Run::list(&self.project)
                .ok()
                .and_then(|runs| runs.into_iter().next())
                .ok_or("The project has no reports yet. Run a test with run_test.")?,
        };
        let images = optional(args, "images")?.unwrap_or("none");
        if !["none", "failed", "all"].contains(&images) {
            return Err("\"images\" must be none, failed or all.".to_owned());
        }
        let mut report = serde_json::to_value(&run.report).map_err(|e| e.to_string())?;
        explain_failures(&mut report);
        report["report"] = json!(run.dir.file_name().map(|n| n.to_string_lossy()));
        report["html"] = json!(run.dir.join("report.html").to_string_lossy());
        let mut content = vec![pretty(&report)];
        if images == "none" {
            return Ok(content);
        }
        let hide = match self.shareable_areas() {
            Ok(hide) => hide,
            Err(why) => {
                content.push(text(why));
                return Ok(content);
            }
        };
        let dir = run
            .dir
            .canonicalize()
            .map_err(|e| self.io_error(&run.dir, &e))?;
        let mut shown = 0;
        'steps: for attempt in &run.report.attempts {
            for step in &attempt.steps {
                if images == "failed" && step.status != StepStatus::Failed {
                    continue;
                }
                let pictures = [
                    ("before", &step.before),
                    ("after", &step.after),
                    ("expected", &step.expected),
                    ("difference", &step.diff),
                ];
                for (label, picture) in pictures {
                    // Saved pictures are masked already; areas the project hides since are too.
                    let Some(picture) = picture
                        .as_ref()
                        .and_then(|p| read_inside(&dir, p))
                        .and_then(|png| MaskedImage::from_png(&png, &hide).ok())
                    else {
                        continue;
                    };
                    if shown == MAX_IMAGES {
                        content.push(text(format!("Only the first {MAX_IMAGES} screenshots are included; the HTML report has them all.")));
                        break 'steps;
                    }
                    content.push(text(format!(
                        "Attempt {}, step {} \"{}\" ({}): {label}",
                        attempt.number, step.seq, step.block, step.block_type
                    )));
                    content.push(image(&picture));
                    shown += 1;
                }
            }
        }
        if shown == 0 {
            content.push(text(match images {
                "failed" => "No failed step has screenshots.",
                _ => "This report has no screenshots.",
            }));
        }
        Ok(content)
    }

    fn screenshot(&self, args: &Value) -> Content {
        if !self.host.permissions().screenshots {
            return Err("Screenshots are turned off. The window may show private things, so the person at this Mac decides: they can turn on \"Allow assistants to take screenshots\" in JEVision → Settings → AI → Assistants. Ask them, then try again.".to_owned());
        }
        let app = match (optional(args, "app")?, optional(args, "path")?) {
            (Some(app), _) => app.to_owned(),
            (None, Some(path)) => self.load(&self.test_file(path)?)?.app,
            (None, None) => {
                return Err(
                    "Name the app, such as com.apple.TextEdit, or a test whose app to picture."
                        .to_owned(),
                );
            }
        };
        let hide = self.shareable_areas()?;
        let shot = self
            .host
            .capture(app.clone())
            .map_err(|e| hands_message(&e))?;
        let areas: Vec<_> = hide.iter().chain(&shot.secure).copied().collect();
        let picture = MaskedImage::from_png(&shot.png, &areas)
            .map_err(|reason| format!("The screenshot can't be read: {reason}."))?;
        let mut said = format!("The front window of {app}.");
        if picture.is_masked() {
            said.push_str(" Grey boxes cover private parts of it on purpose.");
        }
        Ok(vec![text(said), image(&picture)])
    }

    /// Areas the project hides in every screenshot; `Err` says why no picture may leave this
    /// Mac at all: an assistant may pass it on to a cloud service.
    fn shareable_areas(&self) -> Result<Vec<Rect>, String> {
        let settings = ProjectSettings::load(&self.project)
            .map_err(|reason| format!("{PROJECT_FILE} can't be read: {reason}"))?;
        if settings.local_only || self.host.run_options().local_only {
            return Err("Screenshots stay on this Mac: \"Only models on this Mac\" is turned on for this project or in JEVision → Settings → AI, and an assistant may pass pictures on to a cloud service. Work from the report's text instead.".to_owned());
        }
        Ok(settings.hide)
    }

    // MARK: Files

    /// `path` as a real path inside the project. Its last parts may not exist yet.
    fn resolve(&self, path: &str) -> Result<PathBuf, String> {
        let outside = || {
            format!(
                "\"{path}\" isn't inside the project folder {}. Use a path relative to it, without ..",
                self.project.display()
            )
        };
        let given = Path::new(path.trim());
        if given.as_os_str().is_empty() {
            return Err("Give a path relative to the project folder.".to_owned());
        }
        let mut joined = if given.is_absolute() {
            PathBuf::from("/")
        } else {
            self.project.clone()
        };
        for part in given.components() {
            match part {
                Component::Normal(name) => joined.push(name),
                Component::CurDir | Component::RootDir => {}
                Component::ParentDir | Component::Prefix(_) => return Err(outside()),
            }
        }
        // The deepest part that exists, with every symbolic link followed, decides.
        let mut existing = joined.as_path();
        let mut missing = Vec::new();
        while existing.symlink_metadata().is_err() {
            missing.push(existing.file_name().ok_or_else(outside)?);
            existing = existing.parent().ok_or_else(outside)?;
        }
        let mut real = existing.canonicalize().map_err(|_| outside())?;
        if !real.starts_with(&self.project) {
            return Err(outside());
        }
        real.extend(missing.into_iter().rev());
        Ok(real)
    }

    /// `path` as a test file inside the project, outside hidden folders.
    fn test_file(&self, path: &str) -> Result<PathBuf, String> {
        let file = self.resolve(path)?;
        if !file.to_string_lossy().ends_with(TEST_SUFFIX) {
            return Err(format!(
                "\"{path}\" isn't a test file: their names end in {TEST_SUFFIX}."
            ));
        }
        let hidden = file.strip_prefix(&self.project).is_ok_and(|rel| {
            rel.components()
                .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
        });
        if hidden {
            return Err(format!(
                "\"{path}\" is in a hidden folder, where JEVision keeps no tests."
            ));
        }
        Ok(file)
    }

    /// The test file `target`, or the tests in the folder `target`.
    fn tests_at(&self, target: &Path) -> Result<Vec<PathBuf>, String> {
        if target.is_dir() {
            let files = self.collect(target)?;
            if files.is_empty() {
                return Err(format!("{} has no tests.", self.relative(target)));
            }
            Ok(files)
        } else {
            let file = self.test_file(&self.relative(target))?;
            if !file.is_file() {
                return Err(format!("There is no test {}.", self.relative(&file)));
            }
            Ok(vec![file])
        }
    }

    /// Test files in `folder` and the folders inside it, sorted by path. Hidden files and folders
    /// and symbolic links are left out.
    fn collect(&self, folder: &Path) -> Result<Vec<PathBuf>, String> {
        fn walk(folder: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
            for entry in fs::read_dir(folder)? {
                let entry = entry?;
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.') {
                    continue;
                }
                let kind = entry.file_type()?;
                if kind.is_dir() {
                    walk(&entry.path(), files)?;
                } else if kind.is_file() && name.ends_with(TEST_SUFFIX) {
                    files.push(entry.path());
                }
            }
            Ok(())
        }
        if !folder.is_dir() {
            return Err(format!("There is no folder {}.", self.relative(folder)));
        }
        let mut files = Vec::new();
        walk(folder, &mut files).map_err(|e| self.io_error(folder, &e))?;
        files.sort();
        Ok(files)
    }

    fn load(&self, file: &Path) -> Result<Flow, String> {
        let content = fs::read_to_string(file).map_err(|e| self.io_error(file, &e))?;
        parse(&content).map_err(|e| {
            format!(
                "{} can't be read as a test. Fix it with write_test first:\n{}",
                self.relative(file),
                list(&e.issues)
            )
        })
    }

    /// Writes `flow` in canonical form; a reader never sees half a file.
    fn save(&self, file: &Path, flow: &Flow) -> Result<(), String> {
        let content = to_canonical_json(flow);
        if fs::read_to_string(file).is_ok_and(|old| old == content) {
            return Ok(());
        }
        let folder = file.parent().unwrap_or(&self.project);
        fs::create_dir_all(folder).map_err(|e| self.io_error(folder, &e))?;
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        let temporary = folder.join(format!(".{name}.saving"));
        fs::write(&temporary, content)
            .and_then(|()| fs::rename(&temporary, file))
            .map_err(|e| {
                let _ = fs::remove_file(&temporary);
                self.io_error(file, &e)
            })
    }

    fn relative(&self, path: &Path) -> String {
        match path.strip_prefix(&self.project) {
            Ok(rel) if rel.as_os_str().is_empty() => ".".to_owned(),
            Ok(rel) => rel.to_string_lossy().into_owned(),
            Err(_) => path.to_string_lossy().into_owned(),
        }
    }

    fn io_error(&self, path: &Path, e: &std::io::Error) -> String {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!("There is no file {}.", self.relative(path))
        } else {
            format!("{}: {e}.", self.relative(path))
        }
    }
}

/// Where `add_blocks` puts its blocks.
fn place(
    flow: &Flow,
    at_start: bool,
    after: Option<&str>,
    edge: Option<&str>,
) -> Result<Option<Slot>, String> {
    let Some(after) = after else {
        return match (at_start, edge) {
            (false, None) => Ok(None),
            (true, None) => Ok(Some(Slot {
                from: String::new(),
                edge: START.to_owned(),
            })),
            (_, Some(_)) => {
                Err("Name the block in \"after\" whose way out \"edge\" is.".to_owned())
            }
        };
    };
    if at_start {
        return Err("Give either \"after\" or \"atStart\", not both.".to_owned());
    }
    let block = flow
        .blocks
        .iter()
        .find(|b| b.id == after)
        .ok_or_else(|| no_block(flow, after))?;
    let edge = edge.unwrap_or(NEXT);
    let edges = block.kind.edge_names();
    if !edges.iter().any(|e| e == edge) {
        return Err(format!(
            "A {} block has no \"{edge}\" way out; it has: {}.",
            block.kind.type_name(),
            edges.join(", ")
        ));
    }
    Ok(Some(Slot {
        from: after.to_owned(),
        edge: edge.to_owned(),
    }))
}

fn no_block(flow: &Flow, id: &str) -> String {
    let mut ids: Vec<&str> = flow.blocks.iter().map(|b| b.id.as_str()).collect();
    ids.sort_unstable();
    format!(
        "There is no block \"{id}\". The blocks are: {}.",
        ids.join(", ")
    )
}

/// A block as given, without the fields that join it to others: `add_blocks` makes those.
pub(super) fn without_edges(block: &Value) -> Value {
    let mut block = block.clone();
    if let Some(map) = block.as_object_mut() {
        for edge in ["id", "next", "then", "else", "body"] {
            map.remove(edge);
        }
        if let Some(Value::Array(cases)) = map.get_mut("elseIf") {
            for case in cases.iter_mut().filter_map(Value::as_object_mut) {
                case.remove("then");
            }
        }
        if matches!(
            map.get("type").and_then(Value::as_str),
            Some("repeat" | "useData")
        ) {
            map.insert("body".to_owned(), json!(""));
        }
    }
    block
}

/// Adds a `message` in words next to every failure's code.
fn explain_failures(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(failure) = map.get_mut("failure").and_then(Value::as_object_mut) {
                add_message(failure);
            }
            for (_, v) in map.iter_mut() {
                explain_failures(v);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(explain_failures),
        _ => {}
    }
}

fn add_message(failure: &mut Map<String, Value>) {
    if failure.contains_key("message") {
        return;
    }
    let Ok(parsed) =
        serde_json::from_value::<jevision_runner::Failure>(Value::Object(failure.clone()))
    else {
        return;
    };
    failure.insert(
        "message".to_owned(),
        json!(jevision_runner::describe(&parsed)),
    );
}

/// The bytes of `relative` inside `dir`, unless it leads outside.
fn read_inside(dir: &Path, relative: &str) -> Option<Vec<u8>> {
    let file = dir.join(relative).canonicalize().ok()?;
    file.starts_with(dir).then(|| fs::read(file).ok()).flatten()
}

fn not_saved(issues: &[Issue]) -> String {
    format!("The test wasn't saved:\n{}", list(issues))
}

fn problems(issues: &[Issue]) -> String {
    if issues.is_empty() {
        "No problems found.".to_owned()
    } else {
        format!("Problems:\n{}", list(issues))
    }
}

/// Every problem of a test saved at `file`, including the files it reads that are missing.
fn checked(flow: &Flow, file: &Path) -> Vec<Issue> {
    validate_in(flow, file.parent().unwrap_or(file))
}

fn list(issues: &[Issue]) -> String {
    described(issues)
        .iter()
        .map(|line| format!("- {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn described(issues: &[Issue]) -> Vec<String> {
    issues
        .iter()
        .map(|i| match i.severity {
            Severity::Error => format!("error: {}", describe(i)),
            Severity::Warning => format!("warning: {}", describe(i)),
        })
        .collect()
}

fn optional<'a>(args: &'a Value, name: &str) -> Result<Option<&'a str>, String> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.as_str())),
        Some(_) => Err(format!("\"{name}\" must be text.")),
    }
}

fn required<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    optional(args, name)?
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("\"{name}\" is required."))
}

fn pretty(value: &Value) -> Value {
    text(serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()))
}

/// What went wrong with GitHub, and what the person can do about it.
fn github_message(error: &jevision_git::GitError) -> String {
    use jevision_git::GitError::*;
    match error {
        NotRepository | NoCommits => "The project isn't in a git repository yet, so there is no GitHub repository to file the issue in. The person can set it up in JEVision → Settings → Git.".to_owned(),
        NoRemote | NotGitHub => "The project's repository isn't on GitHub, so the issue can't be filed there. Give the person the drafted text to file it by hand.".to_owned(),
        NoGitHubCli | GitHubSignedOut | SignIn => "GitHub isn't connected. The person can sign in to GitHub in JEVision → Settings → Git.".to_owned(),
        NoPermission => "The GitHub account can't open issues in this repository.".to_owned(),
        other => format!("GitHub didn't take the bug report: {other}."),
    }
}

fn image(picture: &MaskedImage) -> Value {
    json!({ "type": "image", "data": STANDARD.encode(picture.png()), "mimeType": "image/png" })
}
