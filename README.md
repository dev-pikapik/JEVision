<p align="center">
  <img src=".github/assets/banner.svg" alt="JEVision — UI tests you can see" width="100%">
</p>

<p align="center">
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-5eead4?style=flat-square"></a>
  <img alt="Platform: macOS 26+" src="https://img.shields.io/badge/platform-macOS%2026%2B-818cf8?style=flat-square">
  <a href="CONTRIBUTING.md"><img alt="PRs welcome" src="https://img.shields.io/badge/PRs-welcome-34d399?style=flat-square"></a>
</p>

<p align="center">
  <b>Visual, flowchart-driven UI testing for any application.</b><br>
  Draw the path a user takes. JEVision walks it, block by block, and tells you where it broke.
</p>

---

JEVision tests the interface of any Mac app, including apps you don't own and can't instrument.
It sees the screen and uses the real mouse and keyboard, the way a person does. A test is a
flowchart: one JSON file per test, with a block for every step, so anyone on the team can read
it and a failure points at the block where it happened.

## What it does

- **Flowchart editor.** Add, connect and edit blocks on a canvas; every change is saved to the
  test file at once and can be undone. Problems in a test are listed on the block they concern.
- **Recording.** Click, type and press keys in the app under test; each action becomes a block,
  with a picture of what you clicked. Drag a frame to add a check. Passwords become secrets kept
  in the Keychain, never in the file.
- **Running.** Run a test, run it step by step (⌥⌘R) or start from any block, with breakpoints.
  A small panel shows progress without getting in the way of clicks. Moving the mouse pauses the
  run; pressing Esc twice stops it from any app.
- **Reports and history.** Every run is saved with a screenshot of each step. For a check, the
  report shows the screen, the reference image, their differences or one over the other. Reports
  export as HTML or JUnit XML.
- **Reference images.** When the app changed on purpose, accept a run's screenshot as the new
  reference from the report. The old image goes to the Trash, and Undo brings it back.
- **Checks by meaning.** Where comparing pictures is too strict, describe the screen in words,
  such as “The cart shows 2 items”, and an AI model of your choice decides. See
  [AI models](#ai-models).
- **Command line.** The `jevision` tool inside the app runs, validates and formats tests from
  Terminal, scripts and CI.
- **AI assistants.** Claude, Cursor, Copilot and other MCP clients can find, write, run and
  debug tests. See [Assistants (MCP)](#assistants-mcp).
- **Git and GitHub.** Commit, pull, push and switch branches without a terminal, see what each
  version of a test changed, and open and review pull requests. See [Git](#git).

## Requirements

- macOS 26 or later, on Apple silicon or Intel.
- To build from source: Xcode 26 and Rust 1.85 or later, installed with
  [rustup](https://rustup.rs).

## Build from source

```sh
git clone https://github.com/dev-pikapik/JEVision.git
cd JEVision
apps/macos/scripts/build.sh
open apps/macos/build/JEVision.app
```

`build.sh` builds the Rust core for both architectures, then the app and the `jevision` tool
into `apps/macos/build/JEVision.app`. Set `ARCHS=arm64` to build for one architecture only, and
`SIGN_IDENTITY` to sign with your own certificate instead of an ad-hoc signature.

## First steps

1. **Open Project…** (⌘O) and choose a folder. Tests are the `*.flow.json` files in it; a
   folder per application works well.
2. **New Test…** (⌘N), give it a name and choose the application to test.
3. **Record** (⇧⌘R), then use the app. Press Esc twice or click **Done** to finish; the steps
   appear on the canvas.
4. **Run** (⌘R). The app under test comes to the front and JEVision repeats the steps. When the
   run ends, open its report to see each step with screenshots.

## Permissions

JEVision asks for two permissions in System Settings → Privacy & Security, and explains each
before asking:

- **Screen Recording**, to take screenshots of the app under test: finding what to click,
  waiting for screens and comparing them with reference images.
- **Accessibility**, to click, type and press keys for you; to read the names of controls while
  recording, so blocks and pictures get readable names; and to notice your own mouse and keyboard,
  so a run pauses when you take over.

Settings → Permissions shows what is granted and opens the right page in System Settings.

## How it works

A test is a graph of blocks. Execution starts at `start` and follows the arrows.

```mermaid
flowchart TD
    open([open AcmeMail]) --> email[type email]
    email --> password[type password]
    password --> submit[click &quot;Sign in&quot;]
    submit --> inbox{{wait for the inbox}}
    inbox --> messages{{check the message list}}
```

The same test as a file — [`examples/sign-in.flow.json`](examples/sign-in.flow.json), shortened:

```json
{
  "format": 1,
  "name": "Sign in with valid credentials",
  "app": "com.example.AcmeMail",
  "start": "open",
  "blocks": {
    "open": { "type": "openApp", "next": "email" },
    "email": { "type": "type", "text": "demo@example.com", "into": { "image": "refs/email-field.png" }, "next": "password" },
    "password": { "type": "type", "text": "${secret.ACME_PASSWORD}", "into": { "image": "refs/password-field.png" }, "next": "submit" },
    "submit": { "type": "click", "target": { "image": "refs/sign-in-button.png" }, "button": "left", "next": "inbox" },
    "inbox": { "type": "waitFor", "until": { "appears": { "image": "refs/inbox.png" } }, "timeout": "10s", "next": "messages" },
    "messages": { "type": "check", "reference": { "image": "refs/message-list.png", "compare": "similar", "threshold": 0.9 } }
  }
}
```

- **`format`** is the file format version. Older files are upgraded when read; files from a newer
  JEVision are refused instead of being misread.
- **`app`** is the bundle identifier of the application under test.
- **Paths** (reference images, data files, other tests) are relative to the folder of the test file.
- **Secrets are never stored in the file.** `${secret.NAME}` refers to a secret by name; its value
  comes from the environment or the Keychain at run time. `${env.NAME}` is a value of the chosen
  environment (see below). `${data.COLUMN}` is a value from the current row of a `useData` table.
  All three are allowed in the text of a `type` block.
- **Arrows** are `next`, `then`/`else` and `body`. A loop is only possible through the `body` of
  `repeat` or `useData`; any other cycle is an error.
- **Optional metadata:** `description`, `testType` (`smoke`, `regression`, `acceptance`),
  `priority` (`critical`, `high`, `medium`, `low`), `status` (`draft`, `ready`, `outdated`),
  `tags` and `retries`.
- **`canvas`** (optional) keeps how the test looks in the editor: `positions` of blocks moved by
  hand, `sections` (titled frames around a part of the test) and `notes` (text with its author,
  time and frame). It never changes how the test runs; without it the blocks are laid out
  automatically.

### Environments

One test can run against staging, production or a local server. Values that differ between them,
such as a server address or a user name, go into `jevision.environments.json` in the project
folder, next to the tests, and are committed with them:

```json
{
  "format": 1,
  "environments": {
    "production": { "BASE_URL": "https://example.com" },
    "staging": { "BASE_URL": "https://staging.example.com", "USER": "qa" }
  }
}
```

- Environment names use letters, digits, `-` and `_`; value names use letters, digits and `_`.
- A test reads a value as `${env.BASE_URL}`. `jevision validate` and the editor report a name
  no environment has as an error, and a name only some environments have as a warning.
- The file is never for secrets. A secret can still differ per environment: the app saves it in
  the Keychain for the environment chosen when you set it. At run time a secret comes from the
  environment variable `NAME`, then from the Keychain item of the chosen environment, then from
  the one shared by all environments.
- In the app, pick the environment at the foot of the sidebar or in the Run menu, which also
  picks one data row (or all of them) and the number of retries for the next runs. The choice of
  environment is kept per project on this Mac. Before a run the app lists the names of missing
  values and secrets, never the values. The report says which environment the run used.

`jevision fmt` rewrites test files in canonical form (see [Command line](#command-line)). The canonical form has a fixed key order, lists blocks in reading order and leaves out default
values, so every change shows up as a small, readable diff.

## Block types

| Block | Group | What it does |
| --- | --- | --- |
| `openApp` | actions | Opens the application under test, or another one by bundle identifier |
| `openFile` | actions | Opens files or web addresses, optionally in a new instance of the application |
| `click` | actions | Clicks an image on screen or a point, left or right, single or double |
| `type` | actions | Types text, optionally into a field found by its image |
| `pressKeys` | actions | Presses a key combination such as `cmd+shift+s` |
| `waitFor` | wait and check | Waits until an image appears, disappears or the screen stops changing |
| `check` | wait and check | Compares the screen with a reference image, exactly or by similarity, or checks it by meaning |
| `if` | logic | Goes to `then` or `else` depending on what is on screen |
| `repeat` | logic | Runs its `body` a given number of times |
| `runTest` | logic | Runs another test file as a step |
| `useData` | data | Runs its `body` once for every row of a CSV file |

A reference can limit the comparison to an `area`, `ignore` regions that change on their own and
`hide` regions that must not appear in reports. Areas use fractions of the window, from 0 to 1.

With `"compare": "meaning"`, an AI model looks at the screen and decides whether it shows what
`expect` says, in plain words: `{ "compare": "meaning", "expect": "The cart shows 2 items" }`.
The reference image is optional; with one, the model compares the screen with it. `check`,
`waitFor` and `if` all accept such a reference.

## AI models

Checks by meaning use the model chosen in Settings → AI. Without one, JEVision compares pictures
only and sends nothing anywhere.

| Provider | Runs | API key |
|---|---|---|
| Anthropic | in the cloud | `ANTHROPIC_API_KEY` |
| OpenAI | in the cloud | `OPENAI_API_KEY` |
| Google Gemini | in the cloud | `GEMINI_API_KEY` |
| Ollama | on this Mac | none |
| LM Studio | on this Mac | none |
| OpenAI-compatible server | at the address you give | `OPENAI_COMPATIBLE_API_KEY`, if the server asks for one |

- **What is sent.** For each check by meaning: the compared part of the screen, the reference
  image if there is one, and the expected text. Regions the reference hides or ignores are covered
  before anything leaves the Mac. Nothing else is sent.
- **API keys** come from the environment variable in the table or from the Keychain, where
  Settings → AI saves them. They are never shown, logged or written to reports.
- **Only models on this Mac.** With this setting on, screenshots never go to a cloud service:
  a check that would need one stops with a message.
- **Reports** keep the provider, the model and its explanation for each check by meaning.
- **Test Connection** in Settings asks the model about a small built-in picture, to check the
  address, key and model in one go.

## Command line

The `jevision` command runs and checks tests from Terminal, scripts and CI. It comes inside the
app: choose **JEVision → Install Command Line Tool…** and click **Install…**. macOS asks for an
administrator's password and links the tool into `/usr/local/bin`. Without a password, click
**Install Just for Me** to link it into `~/.local/bin` instead. The window shows whether the tool
is installed and which version.

```sh
jevision run tests/                       # run every *.flow.json in tests/ and save reports
jevision run tests/sign-in.flow.json --start-at submit
jevision run tests/ --env staging         # read ${env.NAME} from the staging environment
jevision run tests/ --junit junit.xml --html reports    # also save JUnit XML and HTML reports
jevision run tests/ --video always        # record a video of every run (off, on-failure, always)
jevision run tests/ --provider ollama --model qwen2.5vl:7b   # check by meaning with another model
jevision validate tests/                  # report errors and warnings (--json for machines)
jevision fmt tests/                       # rewrite files in canonical form (--check to only report)
```

`jevision help <command>` lists every option. Exit status: **0** means all tests passed (or all
files are valid and formatted). **1** means a test failed or was stopped, or a file has problems.
**2** means wrong arguments, a file that can't be read, or a Mac that can't run tests right now.

`jevision run` uses the real mouse, keyboard and screen. These requirements apply wherever it runs:

- **A logged-in user with an unlocked screen.** Tests can't run over SSH, as a system service or
  while the screen is locked. For unattended runs, keep the user logged in and turn off the
  automatic lock in System Settings → Lock Screen.
- **Screen Recording and Accessibility** for the app that starts `jevision`: Terminal, your
  editor or the CI agent. If one is missing, `jevision` names the program that needs it and the
  list in System Settings → Privacy & Security where to turn it on.
- **Secrets** (`${secret.NAME}`) come from the environment variable `NAME` or from the Keychain,
  where the app saves them. The first time `jevision` reads a Keychain secret, macOS may ask to
  allow it; choose Always Allow for unattended runs. Values are never printed. With `--env`, the
  Keychain item of that environment comes first (see [Environments](#environments)).
- **AI models** for checks by meaning are the ones chosen in the app's Settings → AI, unless
  `--provider` and `--model` say otherwise. API keys come from the same environment variables or
  the Keychain.

Press Esc twice or Ctrl-C to stop a run. The stopped test is reported as cancelled.

### GitHub Actions

Hosted runners have no logged-in user, so use a
[self-hosted runner](https://docs.github.com/actions/hosting-your-own-runners) on a Mac.
Install it as a service with `./svc.sh install` while logged in as the user who runs the tests.
On macOS this makes a launch agent that runs in that user's session. Then allow Screen Recording
and Accessibility for the program that the first run names.

```yaml
jobs:
  ui-tests:
    runs-on: [self-hosted, macOS]
    steps:
      - uses: actions/checkout@v4
      - name: Run UI tests
        env:
          ACME_PASSWORD: ${{ secrets.ACME_PASSWORD }}
        run: jevision run tests --junit junit.xml --html reports
      - uses: actions/upload-artifact@v4
        if: always()
        with:
          name: ui-test-reports
          path: |
            junit.xml
            reports/
```

### On a schedule

Use a launch agent, not cron: cron jobs don't run in the user's session, so they can't see the
screen. Save the plist below as `~/Library/LaunchAgents/com.example.ui-tests.plist`. It runs the
tests every day at 03:00 and writes the output to a log:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.example.ui-tests</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/local/bin/jevision</string>
    <string>run</string>
    <string>/Users/me/Projects/AcmeMail/tests</string>
  </array>
  <key>StartCalendarInterval</key>
  <dict>
    <key>Hour</key>
    <integer>3</integer>
    <key>Minute</key>
    <integer>0</integer>
  </dict>
  <key>StandardOutPath</key>
  <string>/Users/me/Library/Logs/ui-tests.log</string>
  <key>StandardErrorPath</key>
  <string>/Users/me/Library/Logs/ui-tests.log</string>
</dict>
</plist>
```

```sh
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.example.ui-tests.plist
```

Reports are saved in the project's `.jevision/runs` folder, where the app shows them in its run
history.

## Assistants (MCP)

`jevision mcp` is a [Model Context Protocol](https://modelcontextprotocol.io) server for AI
assistants on the same Mac. The assistant starts it and talks to it over standard input and
output; nothing listens on the network. Add it to the assistant's MCP servers — Settings → AI →
Assistants in the app shows this configuration ready to copy, with your paths:

```json
{
  "mcpServers": {
    "jevision": {
      "command": "/usr/local/bin/jevision",
      "args": ["mcp", "--project", "/Users/you/ui-tests"]
    }
  }
}
```

Without `--project`, the project is the nearest folder above the current one that has a
`.jevision` folder, otherwise the current folder.

| Tool | What it does |
| --- | --- |
| `list_tests` | Lists and searches the tests of the project |
| `read_test`, `write_test` | Read a test file, or write one; invalid tests are not saved, and each problem names its block and field |
| `add_blocks`, `remove_block` | Edit a test the way the editor does |
| `validate_test` | Checks a test or a folder of tests |
| `list_block_types` | Describes the file format and every block type, with examples |
| `run_test`, `get_run`, `stop_run` | Run a test or a folder (optionally in an environment, `env`), wait for the result, stop it |
| `get_report` | A run's verdict, steps, failures, AI model answers and, on request, screenshots |
| `screenshot` | The front window of an app as it is now |

Safety:

- Running tests and taking screenshots are off until you turn them on in Settings → AI →
  Assistants. Everything else only reads and writes test files.
- Assistants see the same JSON you do. Secrets stay `${secret.NAME}` in tests and are masked in
  reports; API keys of AI providers are never available.
- Paths stay inside the project folder: `..` and symbolic links that lead outside are refused.
- A run started by an assistant pauses while you use the mouse or keyboard, and Esc pressed
  twice stops it.

## Git

Tests are plain files, so git keeps their history. Settings → Git and GitHub shows the project's branch and
changes; the foot of the sidebar shows them too and opens that page.

- **Commit.** Check the changes to include and describe them. A commit contains only your message:
  JEVision adds no trailers or signatures. Git asks for your name and email once.
- **What changed.** A test is shown as one flowchart: added blocks are green, removed ones red,
  changed ones yellow, with what each changed block was and is now. A picture can be compared side
  by side, under a sliding divider, one over the other, or with the differing pixels in red.
- **History.** Show History… on a test or a folder lists the commits that changed it, following
  renames. Restore This Version writes the file back as it was; Edit → Undo takes that back.
- **Pull, push and branches.** Pull merges the server's commits; when they touch the same lines
  as yours, nothing is changed and the app names the files. Push sends a new branch to the
  server too.
- **Not tracked yet.** Start Tracking Changes creates the repository.
- **Pull requests.** When the repository is on GitHub, Pull Requests → New… proposes the current
  branch with your title and description (pushing it first if needed); the default branch is
  the target unless you pick another. Show… lists the open pull requests: each shows the tests
  and pictures it changes, before and after, and opens on GitHub.

JEVision runs the git installed on the Mac (the Command Line Developer Tools include it), so
signing in to the server works the way it does in Terminal: GitHub CLI (`gh auth login`), the
Keychain or SSH keys. For GitHub itself (the account and pull requests) the app uses GitHub CLI's
sign-in: it asks `gh auth token` when it needs it and never stores, shows or shares the token.
The app has no sign-in of its own.

## Roadmap

- Testing remote devices, starting with iPhone
- GitHub in the app: run results on pull requests, sign-in without GitHub CLI
- Windows and Linux, each with an interface of its own

## Contributing

Ideas, questions and pull requests are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md)
and please follow the [Code of Conduct](CODE_OF_CONDUCT.md).
Security issues: see [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE)
