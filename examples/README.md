# Examples

| Example | Runs on your Mac? | What it shows |
| --- | --- | --- |
| [`textedit/`](textedit) | Yes, on any Mac | Opens a copy of `note.txt` in TextEdit, adds a line, checks how the window looks and quits TextEdit. The first run saves the reference picture in `textedit/refs/`; later runs compare with it. |
| [`textedit/stamp-the-note.flow.json`](textedit/stamp-the-note.flow.json) | Yes, on any Mac | The `dateTime` and `clipboard` blocks: keeps today's date, puts a line with it on the clipboard, pastes it into a copy of `note.txt` in TextEdit and checks the clipboard after copying the note back. It replaces what is on your clipboard. |
| [`textedit/add-a-line-by-accessibility.flow.json`](textedit/add-a-line-by-accessibility.flow.json) | Yes, on any Mac | [Interface elements](../docs/writing-tests.html#element): waits for the text of the note, clicks the text area and checks the line it added by what TextEdit tells macOS accessibility, with no reference picture, so it passes on the first run and in dark mode too. |
| [`textedit/blocks/org.jevision.add-line.jevblock/`](textedit/blocks/org.jevision.add-line.jevblock) | Yes, on any Mac | A [custom block](../docs/custom-blocks.html#package) as a package someone shares: `block.json` says who made it and what it asks for, `flow.json` holds the steps. Copy the folder anywhere and install it with **File → Install Block…** or `jevision blocks install`. |
| [`textedit/add-a-line-with-a-block.flow.json`](textedit/add-a-line-with-a-block.flow.json) | Yes, on any Mac | A test that runs that block with `runTest` and gives it the line to add. The first run saves the reference picture. |
| [`textedit/blocks/org.jevision.add-date.jevblock/`](textedit/blocks/org.jevision.add-date.jevblock) | Yes, on any Mac | A [block that uses another block](../docs/custom-blocks.html#dependencies): it keeps today's date and runs `org.jevision.add-line` to type it, so its `block.json` lists that block under `dependencies`. Install the `textedit/blocks` folder into another project to get both blocks in one list; the one it needs is installed first. |
| [`sign-in.flow.json`](sign-in.flow.json) | No | Signing in and waiting for the inbox: typing into fields found by picture, a secret, a wait and a check with ignored and hidden areas. |
| [`open-mail.flow.json`](open-mail.flow.json) | No | A condition with several exits: a sign-in window, an update dialog or the inbox. |

`sign-in` and `open-mail` test AcmeMail, a mail app that doesn't exist, so a run stops at the first
block and says the app isn't installed. The pictures in [`refs/`](refs) are drawings of its screens;
they show how a test points at a field or a button by picture.

To try the TextEdit example, open the `textedit` folder with **Open Project…** in JEVision and press
**Run**. The app's first screen also offers the same test as **Run the Demo Test**.
