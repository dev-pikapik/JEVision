cask "jevision" do
  version "0.1.0"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"

  url "https://github.com/dev-pikapik/JEVision/releases/download/v#{version}/JEVision-#{version}.dmg"
  name "JEVision"
  desc "Flowchart-based UI test automation for apps"
  homepage "https://github.com/dev-pikapik/JEVision"

  livecheck do
    url "https://github.com/dev-pikapik/JEVision/releases/latest/download/appcast.xml"
    strategy :sparkle, &:short_version
  end

  auto_updates true
  depends_on macos: :tahoe

  app "JEVision.app"
  binary "#{appdir}/JEVision.app/Contents/Helpers/jevision"

  # The app also keeps secrets in the login Keychain (one service per kind, all named "JEVision ..."
  # or "JEVision/<environment>") and a link to its command in /usr/local/bin or ~/.local/bin.
  zap script: {
        executable: "/bin/sh",
        args:       [
          "-c",
          'for agent in "$HOME"/Library/LaunchAgents/io.github.dev-pikapik.jevision.schedule.*.plist; ' \
          'do [ -e "$agent" ] && launchctl bootout "gui/$(id -u)" "$agent"; done; ' \
          "security dump-keychain 2>/dev/null | sed -n 's/^ *\"svce\"<blob>=\"\\(JEVision[^\"]*\\)\"$/\\1/p' | sort -u | " \
          'while IFS= read -r service; do ' \
          'while security delete-generic-password -s "$service" >/dev/null 2>&1; do :; done; done; true',
        ],
      },
      delete: "/usr/local/bin/jevision",
      trash:  [
        "~/.local/bin/jevision",
        "~/Library/Application Support/JEVision",
        "~/Library/Caches/io.github.dev-pikapik.jevision",
        "~/Library/HTTPStorages/io.github.dev-pikapik.jevision",
        "~/Library/LaunchAgents/io.github.dev-pikapik.jevision.schedule.*.plist",
        "~/Library/Logs/JEVision",
        "~/Library/Preferences/io.github.dev-pikapik.jevision.plist",
        "~/Library/Saved Application State/io.github.dev-pikapik.jevision.savedState",
        "~/Library/WebKit/io.github.dev-pikapik.jevision",
      ]
end
