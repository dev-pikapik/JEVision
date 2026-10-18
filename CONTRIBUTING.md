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

Every text the app shows lives in
[`Localizable.xcstrings`](apps/macos/Resources/Localizable.xcstrings), translated into every
language. `build.sh` runs `apps/macos/scripts/check-localization.py`, which fails when a text is
missing there, a key is no longer used or a translation doesn't match.

## Test files

Example tests live in [`examples/`](examples). Each test is one `*.flow.json` file.
Keep examples small, readable and pointed at public demo sites only.

## Code of Conduct

By participating you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
