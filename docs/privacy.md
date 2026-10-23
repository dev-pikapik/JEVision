# Privacy policy

*Last updated: October 21, 2026*

This policy covers **JEVision Remote**, the iPhone app, and **JEVision**, the Mac app. Both are
free and open source, so everything below can be checked in the code in this repository.

## In short

- There is no account and no sign-in. Neither app has ads, tracking or analytics, and neither
  collects data about how you use it.
- The project runs no servers of its own and receives no data from either app.
- Your tests, screenshots, reports and secrets stay on your Mac. The iPhone fetches only what you
  open and keeps nothing but the list of Macs it is paired with.
- The connection between the iPhone and the Mac is end-to-end encrypted.

## JEVision Remote for iPhone

### What stays on your iPhone

- **The list of paired Macs**: each Mac's name, its network identity and the addresses to reach
  it, and the iPhone's ID on that Mac. It is kept in one file in the app's own storage, protected
  by iOS data protection, and is never sent anywhere. Forgetting a Mac removes it from the list.
- **The device key**: a key made in the iPhone's Secure Enclave the first time you pair. It cannot
  be copied out of the iPhone, is not included in backups, and works only after Face ID or the
  passcode.
- **One setting**: how long the app may stay in the background before it locks again.

Tests, pictures, reports and notes that you open are shown and then dropped; the app does not
write them to the iPhone.

### What goes to your Mac

When you pair, the iPhone sends the Mac the name you give it on the pairing screen (the iPhone's
name unless you change it) and the public half of the device key. After that, the iPhone sends
the Mac only your requests: which project, test, run or picture to show, and which test to start
or stop. The Mac lists the paired iPhone with its name and keeps an activity log of pairings,
connections and runs. That information stays on your Mac, which you control.

### How the iPhone reaches the Mac

The iPhone connects to the Mac directly when it can, for example on the same Wi-Fi. When it
can't, the encrypted connection goes through a **relay** server, and the iPhone finds the Mac's
current relay with an **address lookup** by the Mac's public key. The networking comes from
[iroh](https://iroh.computer), an open-source library; the relay and the address lookup are
servers run by n0, the company that makes iroh, unless the Mac is set to use a relay of your
own (JEVision Settings → iPhone & Remote → Relay).

These servers see the IP addresses of the iPhone and the Mac and the size and timing of the
traffic. They cannot read it: it is encrypted end to end between the iPhone and the Mac. The
iPhone publishes only the address of its relay, under a network key it makes anew each time the
app starts. The app connects to nothing at all until you pair a Mac, and the built-in example
uses no network.

### Permissions

- **Camera**, to scan the pairing code that the Mac shows. The picture is read on the iPhone,
  and is neither saved nor sent.
- **Local network**, to reach the Mac directly on the same network.
- **Face ID**, to lock the app and to confirm pairing. The app never sees your face data; iOS
  only tells it whether the check passed.

Each one is asked for only when it is first needed, and the app works without the camera if you
paste the pairing link instead.

### Crash reports

If you turn on *Share With App Developers* in iOS Settings → Privacy & Security → Analytics &
Improvements, Apple may pass crash reports of the app to its developer. Apple decides what they
contain and gives them without your name; the app adds nothing to them.

## JEVision for Mac

JEVision sends nothing from your Mac unless a feature you set up needs it, and then only to the
service you chose:

- **Checks by meaning and AI assistants** send a screenshot and the text of the check to the AI
  model you configure, which may be a model on your own Mac. Password fields and hidden areas are
  covered first.
- **Git and GitHub** features (pull, push, sign-in, pull requests) talk to your project's git
  server and to GitHub with your account.
- **Notifications** go only to what you add: Slack, Telegram or an email address.
- **Remote access** for the iPhone and remote assistants uses the relay described above.
- **Updates**: the app downloads the list of releases from GitHub to see whether a new version
  exists. GitHub sees your IP address and the app's version, as with any download.

Crash reports are off until you turn them on, stay on your Mac, and leave it only when you add
one to a bug report yourself. Secrets and API keys are kept in the Keychain and are never written
to logs or reports.

## Children

Neither app is directed at children, and neither collects personal information from anyone.

## Changes

Changes to this policy are made in this file, and its history on GitHub shows every one of them.

## Contact

Ask about this policy in [GitHub Discussions](https://github.com/dev-pikapik/JEVision/discussions).
Report a security problem privately as described in [SECURITY.md](../SECURITY.md).
