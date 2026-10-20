# Contributing to JEVision

Thanks for taking the time to help. Ideas and discussion are as valuable as code.

## Ways to help

- **Share an idea or ask a question** — open a [Discussion](../../discussions).
- **Report a bug** — open an [issue](../../issues/new/choose) using the bug template.
- **Propose a feature or a block type** — open an issue using the feature template.
- **Send a pull request** — for anything larger than a typo, please open an issue first so we
  can agree on the approach.

## Pull requests

1. Fork the repository and create a branch from `main`:
   `feat/short-name`, `fix/short-name` or `docs/short-name`.
2. Keep the change focused: one topic per pull request.
3. Write commit messages in the [Conventional Commits](https://www.conventionalcommits.org/)
   style, for example `feat(runner): add wait block` or `docs: describe if block`.
4. Update the documentation and examples if behaviour changes.
5. Open the pull request and fill in the template.

## Build and checks

You need macOS 26, Xcode 26 and Rust from [rustup](https://rustup.rs). The core is Rust in
[`crates/`](crates); the macOS app and the `jevision` tool are Swift in [`apps/macos`](apps/macos).

```sh
apps/macos/scripts/build.sh                 # core, app and jevision tool → apps/macos/build/JEVision.app
open apps/macos/build/JEVision.app
```

Before opening a pull request, run the same checks as CI:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
apps/macos/scripts/build.sh                 # warnings are errors; also checks the localization
swift test --package-path apps/macos
apps/macos/build/JEVision.app/Contents/Helpers/jevision fmt --check examples
apps/macos/build/JEVision.app/Contents/Helpers/jevision validate examples
```

CI also builds the core for Swift on its own and tests the generated bindings:
`scripts/build-core-apple.sh && swift test --package-path build/apple/JEVisionCore`.

## Languages and localization

Every text a person reads lives in one file,
[`Localizable.xcstrings`](apps/macos/Resources/Localizable.xcstrings): the Mac app, the reports,
bug drafts, notifications, the command line and the iPhone app all take their words from it.
English is the source language and Russian is translated; a key with no text in a language is
an error, not a blank.

- The Rust core never holds a phrase in code, only a key and its values: a failed block is
  `run.check.mismatch` with `block`, `image`, `similarity` and `threshold`. Whoever shows
  it builds the words in the language it was asked for: `tr!("key", …)` in the current language,
  `tr_in!(lang, "key", …)` in a given one (crate `jevision-l10n`). The saved report keeps
  codes and values, so a report written in one language can be read in another.
- A key is either dotted (`run.secret.missing`) with an `en` text of its own, or the English
  text itself (`%@ is connected with %lld tools.`). Arguments are `%@` for text and `%lld` for
  numbers, `%1$@` to reorder them. A text with a count is a plural with the forms the language
  needs (`one`, `few`, `many`, `other` for Russian), and the first number in the arguments picks
  the form.
- The app uses its own language setting, the command line takes `--lang en|ru` (the system
  language when it is not given, English in CI), and the core reads the language it is handed.
  Dates and numbers follow the system formatting; don't write them into a text.
- `apps/macos/scripts/check-localization.py` runs in `build.sh` and in CI. It fails when a text
  or a key the core asks for is missing from the catalog, a key is no longer used by the app or
  the core, a language lacks a translation or a plural form, or the arguments of a translation
  differ from the English text. Write every key out in full: a key built with `format!` can't be
  checked.
- To add a language, add its translation of every text to the catalog (Xcode's String Catalog
  editor works). The check, the language picker and the core pick it up; a language whose
  plural forms are not `one`/`other` also needs its rule in `plural_category`
  (`crates/jevision-l10n`) and in `PLURALS` of `check-localization.py`.

## Test files

Example tests live in [`examples/`](examples). Each test is one `*.flow.json` file.
Keep examples small, readable and pointed at public demo sites only.

## Code of Conduct

By participating you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
