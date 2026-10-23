# Self-test

JEVision runs the tests in this project against an isolated copy of itself. The driver and the
app under test use separate profiles; the tests do not read or change your projects, preferences,
saved passwords or scheduled jobs.

Run from the repository root:

```sh
scripts/selftest.sh
scripts/selftest.sh --no-build onboarding settings-language
```

The screen must be unlocked. The signed application needs Screen Recording and Accessibility
permissions. The build uses a Developer ID Application certificate from the Keychain; set
`SIGN_IDENTITY` to choose a different certificate. `--no-build` uses the existing signed bundle
in `apps/macos/build/JEVision.app`.

If a keyboard utility replaces the standard quit and close shortcuts, pass the keys it expects:

```sh
JEVISION_SELFTEST_QUIT_KEYS=cmd+shift+q JEVISION_SELFTEST_CLOSE_KEYS=cmd+shift+w scripts/selftest.sh --no-build
```

The overrides apply only to the test project's disposable copy. The utility's settings stay as
configured. Keep the screen free while the test clicks and types; real input pauses the run.

Each run copies the application, project and examples into a new `build/selftest/<time>` folder.
`build/selftest/latest` points to the latest run. HTML reports, JUnit results and screenshots of
failures remain there. The last five run folders are kept unless `--keep` is supplied.

The current scenarios cover first launch, navigation through the interface and changing the
interface language. They use accessibility identifiers and element checks instead of images
whose appearance changes between macOS versions. These scenarios complement the Rust and Swift
unit and integration tests; they do not replace the manual release checklist in
[`docs/maintainers/release-checklist.md`](../docs/maintainers/release-checklist.md).

A failed or skipped self-test blocks release. Inspect its reports, fix the problem and run it
again on the commit intended for release.
