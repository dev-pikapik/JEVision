# Changelog

All notable changes to JEVision are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
In Russian: [CHANGELOG.ru.md](CHANGELOG.ru.md).

## [Unreleased]

### Added

- Tests can run their app on a simulated iPhone or iPad in Simulator: choose the device under Runs On in the test card, and optionally a build that is installed before the test. Clicks and typing go through the Simulator window at any size; screenshots come from the device itself.
- Open File opens links into apps, such as `myapp://inbox`, on the Mac and on a simulated device.

## [1.0.0] - 2026-10-20

The first release of JEVision: UI tests for any Mac app, drawn as flowcharts and run with the real
mouse, keyboard and screen.

### Added

**Editor**

- Build a test as a flowchart of blocks, saved as one readable JSON file per test.
- Press Space to open the block library, grouped by kind, with an icon and one line about each block.
- Right-click the canvas to put a block exactly where you clicked.
- Open any command, block or test from the command palette with ⌘K.
- Zoom the canvas and link blocks with the arrow keys and Return, without the mouse.
- Branch with an If that checks several conditions in order, with Otherwise for everything else.
- Repeat steps a set number of times, or run another test as a single step.
- Save selected blocks as a template in the library and add them to other tests.
- See the chosen environment right on the canvas.
- Leave sticky notes on the canvas.
- Write notes about a block with Markdown, attached files and replies, and mark them resolved.
- Fill in a test card: description, type, priority, status, author and the tests it depends on.
- Filter tests in the sidebar by their card, and open a test it depends on from the card.
- See problems in a test while you edit, each pointing at its block and field.

**Blocks**

- Open the app under test or another app, optionally quitting it first and opening it without its old windows.
- Open the app under test with a fixed window size and position, so pictures match on every run.
- Open files or web pages in the browser of your choice, each from a fresh copy so the original stays intact.
- Click, hover, drag and scroll with the left, right or middle button, holding ⌃ ⌥ ⇧ ⌘ when needed.
- Point at a target by its picture, by a point, by an area of the window, or by describing it in words.
- Type text, optionally into a field found by its picture or description, and press key combinations.
- Wait until a picture appears or disappears or the screen stops changing, and see how long each wait can take.
- Pause a test for a set time.
- Run a shell, Python, Node or AppleScript script and pass typed values, pictures and areas in and out of it.
- Run a command line, send an HTTP request, wait for a file, or query an SQLite database, and check the answer.
- Put text or a picture on the clipboard, read text, a picture or copied files from it, or wait for a value.
- Save the current date and time in any format, shifted by days or hours, in English or Russian and in any time zone.
- Keep values from one block for later blocks as run variables.

**Recording**

- Record a test by using the app: every click, drag, scroll and key press becomes a block.
- Add recorded steps after the selected block of an existing test.
- Get recorded tests laid out neatly in rows on the canvas.

**Running tests**

- Run one test, a folder or every test of the project with Run All.
- Watch each run in a small panel, and stop it at any time by pressing Esc twice.
- Run a test step by step, or mark blocks with Pause Here to stop at them.
- Keep working safely: a run pauses when you move the mouse and waits until you press Resume.
- Let tests that don't use the screen run side by side, while tests that do take turns.
- Make a test depend on others, so they run first and a failure marks the rest as blocked.
- Retry a failed test up to 10 times and see whether it passed on retry, failed differently or failed every time.
- Record a video of the app under test on every run or only on failures, with a mark for each step.
- See which way each If went, in reports, in the app and in the command line.
- Get a clear "unknown" result instead of a wrong one for a run saved by a newer version.

**Checks and references**

- Compare the screen with a reference picture, exactly or by similarity.
- Compare only part of the window, ignore parts that change on their own, and hide parts from reports.
- Let the first run save a missing reference, and accept a new one when the app changed on purpose.
- Replace a picture shared by several tests in one step, choosing which tests take the new one.
- Use separate dark versions of reference pictures for apps shown in dark appearance.
- Check the screen by meaning: describe what it should show in plain words and let an AI model decide.

**Reports and history**

- Open the report of any run, with a screenshot of every step.
- Compare a failed check side by side, one over the other, or as just the pixels that differ.
- Browse the history of every run, filtered by where it came from: the app, a schedule or CI.
- See every test on the project map with its last result, flaky tests and tests in quarantine.
- See project statistics for all time.
- Save HTML and JUnit XML reports for sharing and for CI.
- File a failed run as a GitHub issue in one click, with the steps, the cause and the screenshots.
- Ask an AI model to explain why a run failed.

**Flaky tests and quarantine**

- See which tests are flaky from their history, with the dates and counts as evidence.
- Judge data-driven tests row by row, so only the row that fails now and then is marked flaky.
- Get a suggestion to quarantine a test that keeps failing unreliably.
- Put a test in quarantine until a chosen date, so its failures don't block anyone while it is fixed.

**Data, environments and secrets**

- Run a test once for every row of a CSV file, or for chosen rows only.
- View and edit the data table right in the app.
- Run the same test against staging, production or a local server with environments.
- Keep passwords and tokens as secrets in the Keychain, separately for each environment if needed.

**Project board**

- Lay out the tests of a project on an endless board with a minimap.
- Make several boards and add tests, frames, notes, pictures and links to them.
- Zoom from the whole project down to the blocks of a single test, with live run status.

**Custom blocks**

- Turn selected blocks into a custom block with inputs and reuse it in any test.
- Share a custom block as a folder or a .zip file, for example in a GitHub repository.
- Install blocks from a file, a folder or a web address, and see what they do before anything is installed.
- Update and remove installed blocks, and install the blocks a project needs when you open it.
- Make your own copy of an installed block to change it.
- Install a missing block right from the error that names it and its version.

**AI models**

- Choose Anthropic, OpenAI, Google Gemini, JEV (TypeSafe AI), Ollama, LM Studio or any OpenAI-compatible server.
- Choose the AI model for the whole project or for a single check.
- Add a fallback provider for when the main one can't be reached or has reached its limit.
- See what AI checks cost and set daily and monthly spending limits.
- Describe a test in words and let an AI model draft it on the canvas for you to accept.
- Mark a provider as one that reads only text, so it never gets screenshots.
- Check the address, key and model of a provider with Test Connection.

**AI assistants (MCP)**

- Let AI assistants list, read, write, check and run tests through the built-in MCP server.
- Copy a ready configuration for your assistant from Settings.
- Choose whether assistants may run tests and take screenshots, and be asked before each run.
- See which assistants connected and when.
- Pair an assistant on another computer and disconnect it at any time.

**Command line**

- Install the `jevision` command from the app, for all users or just for you.
- Run tests from Terminal and CI with `jevision run`, choosing tests by folder, card, environment and data rows.
- Run several tests at once with `--parallel`.
- Check tests with `jevision validate` and tidy them with `jevision fmt`, so changes show as small diffs.
- File a failed run as a GitHub issue with `jevision issue`.
- Install, update, remove and restore custom blocks with `jevision blocks`.
- Get results as JSON and clear exit codes for scripts.

**Continuous integration**

- Add a ready GitHub Actions workflow to the repository from Settings, with a preview first.
- See runs from CI in the run history, marked with their pull request or branch and linked to GitHub.

**Scheduling**

- Run all tests, a folder or chosen tests every day, on chosen days or every few hours.
- Pick the environment and the video setting for each schedule.
- Let schedules run while the app is closed, with one catch-up run after the Mac was asleep.
- Get a notification when a scheduled run fails, is skipped or can't start.

**Notifications**

- Get messages about runs in Slack, Telegram or email.
- Choose for each channel when to send: on failures, on failures and fixes, on changes or after every run.
- Get one summary for a batch of tests instead of a message per test.
- See a screenshot of the failed step in the email, with hidden areas and passwords covered.
- Find your Telegram chat with one click and send a test message to any channel.

**Git and GitHub**

- Commit, pull, push and switch branches without a terminal.
- See a changed test as one flowchart with its added, removed and changed blocks.
- Compare changed pictures side by side, under a slider, one over the other or by their differing pixels.
- Browse the history of a test or folder and restore an earlier version, with Undo.
- Settle pull conflicts block by block.
- Open pull requests and review the tests and pictures they change.
- Sign in to GitHub from Settings through GitHub CLI, or in the browser with a code using your own GitHub OAuth app.

**iPhone**

- Pair an iPhone with the Mac by scanning a QR code and confirming with Face ID.
- Give each iPhone full control or view-only access.
- Browse projects and tests, start a test or one row of its data, and follow the run block by block.
- Read reports with a short reason for each failure, and look through the run history.
- See test cards, sticky notes and runs that are waiting for the screen.
- Look around without a Mac: tap See an Example to browse a Mac built into the app, with tests, notes and runs. It uses no network and saves nothing.

**Virtual machines**

- Run tests inside a macOS virtual machine made with Tart or UTM, so your own Mac stays free.
- See in every report whether it ran on a virtual machine or a physical Mac, and on which display.

**Privacy and security**

- Nothing leaves your Mac unless a feature you turned on needs it, and no usage data is collected.
- Hidden areas and password fields are covered before a screenshot goes to an AI model or an assistant.
- Keep screenshots on the Mac with local models only, for yourself or for the whole project.
- Password fields and hidden areas are covered in run videos too.
- Secrets and API keys are kept in the Keychain and never shown, logged or written to reports.
- The iPhone link is end-to-end encrypted, and each iPhone signs in with its own hardware-protected key.
- Assistants can only reach files inside the project folder.
- Installing a custom block never runs anything.

**Languages and accessibility**

- Use the app, the command line and the iPhone app in English or Russian, and switch language without restarting.
- Get numbers formatted the way your region writes them.
- Use the app with VoiceOver and the keyboard, and the iPhone app at the largest text sizes.
- See keyboard shortcuts with Latin letters on any keyboard layout.

**Updates and installation**

- Install JEVision from a signed and notarized disk image, or with Homebrew.
- Get updates inside the app, checked automatically or with Check for Updates.
- Let updates download and install on their own, or get beta versions to try new features early.
- Keep a copy of your tests and settings, saved before any update that stores them in a new way.
- Use JEVision on macOS 26 or later, on Apple silicon or Intel.
- Build the app and the command line tool from source with one script.

**Help and settings**

- Start with a short welcome and an example project for TextEdit.
- Open the documentation from the Help menu.
- Find any setting in the Settings window by searching, and restore the defaults.
- Keep a log of what JEVision does, and save crash reports on your Mac if you choose.

[Unreleased]: https://github.com/dev-pikapik/JEVision/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/dev-pikapik/JEVision/releases/tag/v1.0.0
