<p align="center">
  <img src=".github/assets/banner.svg" alt="JEVision — UI tests you can see" width="100%">
</p>

<p align="center">
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-5eead4?style=flat-square"></a>
  <img alt="Status: early design" src="https://img.shields.io/badge/status-early%20design-818cf8?style=flat-square">
  <a href="CONTRIBUTING.md"><img alt="PRs welcome" src="https://img.shields.io/badge/PRs-welcome-34d399?style=flat-square"></a>
</p>

<p align="center">
  <b>Visual, flowchart-driven UI testing for any application.</b><br>
  Draw the path a user takes. JEVision walks it, block by block, and tells you where it broke.
</p>

---

## Why JEVision

UI tests usually live as long scripts that only their author can read. When one fails, you dig
through code to find out *which step* went wrong and *why*.

JEVision treats a test as what it really is — a **flowchart**:

- **One test, one file.** Each test is a self-contained JSON file. Easy to review, diff, share and version.
- **Blocks, not scripts.** Launch, click, type, wait, check, branch, loop — each step is a block you can see.
- **Readable by everyone.** QA, developers, designers and product people look at the same diagram.
- **Failures point at a block.** A red block on the chart beats a stack trace in a log.
- **Any app, not just yours.** Built to test interfaces you don't own and can't instrument.

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
  comes from the environment or the Keychain at run time. `${data.COLUMN}` is a value from the
  current row of a `useData` table. Both are allowed in the text of a `type` block.
- **Arrows** are `next`, `then`/`else` and `body`. A loop is only possible through the `body` of
  `repeat` or `useData`; any other cycle is an error.
- **Optional metadata:** `description`, `testType` (`smoke`, `regression`, `acceptance`),
  `priority` (`critical`, `high`, `medium`, `low`), `status` (`draft`, `ready`, `outdated`),
  `tags` and `retries`.

The `jevision` command line tool checks and formats test files:

```sh
jevision validate tests/         # report errors and warnings (--json for machines)
jevision fmt tests/              # rewrite files in canonical form (--check to only report)
```

The canonical form has a fixed key order, lists blocks in reading order and leaves out default
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
| `check` | wait and check | Compares the screen with a reference image: `exact`, `similar` or `meaning` |
| `if` | logic | Goes to `then` or `else` depending on what is on screen |
| `repeat` | logic | Runs its `body` a given number of times |
| `runTest` | logic | Runs another test file as a step |
| `useData` | data | Runs its `body` once for every row of a CSV file |

A reference can limit the comparison to an `area`, `ignore` regions that change on their own and
`hide` regions that must not appear in reports. Areas use fractions of the window, from 0 to 1.

## Roadmap

- [ ] Test file format and JSON Schema
- [ ] Runner: execute a test file block by block
- [ ] Reports: highlight the failed block on the diagram
- [ ] Visual editor for building tests as flowcharts
- [ ] Reusable sub-flows and shared variables
- [ ] CI integration

## Project status

JEVision is in **early design**. The repository is open from day one so the format and the
architecture can be shaped together. Star or watch the repo to follow along.

## Contributing

Ideas, questions and pull requests are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md)
and please follow the [Code of Conduct](CODE_OF_CONDUCT.md).
Security issues: see [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE)
