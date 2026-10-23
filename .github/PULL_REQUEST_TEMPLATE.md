## What and why

<!-- What does this change do, and why is it needed? Link the issue: Closes #… -->

## How to check

<!-- Steps or a test file that shows the change works. -->

## Checklist

- [ ] One focused change; commit messages follow Conventional Commits
- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` pass
- [ ] Mac app or `jevision` changes: `apps/macos/scripts/build.sh` (also runs `check-localization.py`) and `swift test --package-path apps/macos` pass
- [ ] iPhone app changes: built and tested with `xcodebuild` as in `apps/ios/README.md`
- [ ] New interface strings have English and Russian
- [ ] `CHANGELOG.md` updated under Unreleased for user-visible changes
- [ ] Docs and examples updated if behaviour changed
