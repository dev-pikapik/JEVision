# App Store texts of JEVision Remote

`metadata/` holds what the App Store shows for the iPhone app, one folder per language
(`en-US`, `ru`), plus the notes for App Review in `metadata/review_information/notes.txt`.
The files follow the layout of [fastlane deliver](https://docs.fastlane.tools/actions/deliver/)
because it is the one layout that tools already read: one plain-text file per App Store
Connect field, named after the field, so a change to a text is a one-line diff that is easy to
review, and `fastlane deliver --skip_binary_upload --skip_screenshots` can send the texts as
they are, while the files stay just as easy to paste into App Store Connect by hand. fastlane
itself is not needed to build, check or release the app, and nothing here installs it.

`apps/ios/scripts/check-metadata.py` checks every file against the App Store limits (name and
subtitle 30 characters, keywords 100 bytes, promotional text 170 characters, description and
release notes 4000 characters, review notes 4000 characters), that every language has the same
files, and that the URLs are https and the privacy policy exists in the repository. The
release workflow runs it; run it yourself after editing a text:

```sh
python3 apps/ios/scripts/check-metadata.py
```

`release_notes.txt` comes from the iPhone part of the version's section in `CHANGELOG.md`;
update both together. Screenshots are taken by `apps/ios/scripts/screenshots.sh`.
