# Security Policy

JEVision controls the screen, keyboard and mouse of a Mac, keeps secrets in its Keychain and
can be driven from a paired iPhone or another computer. We take reports about its security
seriously and thank everyone who sends one.

## Supported versions

| Version | Security fixes |
| --- | --- |
| Latest 1.x release | Yes |
| Older 1.x releases | No: update with **JEVision → Check for Updates…** |
| Builds from before 1.0 | No |

Fixes are released as a new 1.x version. The iPhone app, JEVision Remote, is not in the App
Store yet; it is supported when built from the source of the latest release.

## Reporting a vulnerability

Please **do not** open a public issue, discussion or pull request for a security problem.

Report it privately on GitHub:
**[Report a vulnerability](https://github.com/dev-pikapik/JEVision/security/advisories/new)**.
Only you and the maintainers can see the report.

Please include:

- the JEVision version (**JEVision → About JEVision**, or `jevision --version`) and the macOS
  or iOS version,
- the steps to reproduce, and a small project or test file that shows the problem if you can,
- what an attacker could do with it, and what they need first (for example, being on the same
  Wi-Fi network or getting you to open a project).

Test only on Macs, iPhones and accounts that are yours.

## What happens next

1. We confirm that we got your report within **5 business days**.
2. We check the problem and tell you whether we accept it as a vulnerability, and why.
3. While we work on a fix, we keep you informed and agree with you on when the details go
   public.
4. The fix ships in a new release. We then publish a
   [GitHub security advisory](https://github.com/dev-pikapik/JEVision/security/advisories)
   that names you as the reporter, unless you prefer to stay anonymous.

Please keep the details private until the fixed version is out.

## What counts as a vulnerability

- **Updates.** JEVision installing or running an update that is not signed with its update key,
  or any other way around that check. Sparkle checks every update against the EdDSA public key
  built into the app.
- **iPhone and remote access.** Connecting to a Mac or controlling it without pairing; a paired
  iPhone or computer doing more than the Mac allows, such as running tests when it may only
  view them; getting past the Face ID or passcode lock of JEVision Remote; reading the
  encrypted traffic between a phone and a Mac.
- **Files outside the project.** Reading or changing files outside the project folder through
  the MCP server for assistants, remote access or the iPhone app, symbolic links included.
- **Code that runs without consent.** Commands or code from a project or test file running
  without a person starting the run: for example, just by opening, importing or previewing a
  project, or from a phone or an assistant without the confirmation the Mac asks for.
- **Leaked secrets.** A GitHub token, a Slack webhook, a Telegram bot token, a mail password or a
  `${secret.NAME}` value from the Keychain ending up in a test file, report, log, MCP answer or
  anything else that leaves the Mac.

## What does not count

- **A test doing what it says.** A Command block runs shell commands when you run the test, and
  action blocks click and type. Running a test from someone you don't trust is like running
  their script: read it first.
- **Permissions you granted.** JEVision sees the screen and controls the Mac because you allowed
  Accessibility and Screen Recording in System Settings.
- Attacks that need control of your user account, or your unlocked Mac or iPhone, to begin with.
- Secrets written into a test file as plain text instead of `${secret.NAME}`.
- Problems in the apps you test with JEVision: please report those to their makers.
- Reports from automated scanners that don't show real harm.

## Check that your copy is genuine

JEVision for Mac is signed with a Developer ID certificate and notarized by Apple. Download it
only from [GitHub Releases](https://github.com/dev-pikapik/JEVision/releases) or install it with
Homebrew. To check the app:

```sh
spctl --assess --type execute --verbose=2 /Applications/JEVision.app
# expected: accepted, source=Notarized Developer ID
codesign --display --verbose=2 /Applications/JEVision.app 2>&1 | grep TeamIdentifier
# expected: TeamIdentifier=5N3968Z6Y9
```

To check the disk image before you open it, download `JEVision-1.0.0.dmg.sha256` from the same
release, put it next to the disk image and run:

```sh
shasum -a 256 -c JEVision-1.0.0.dmg.sha256
# expected: JEVision-1.0.0.dmg: OK
```

Homebrew does this comparison by itself and refuses a download that doesn't match.
