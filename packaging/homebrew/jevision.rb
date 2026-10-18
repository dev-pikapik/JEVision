cask "jevision" do
  version "0.1.0"
  sha256 "0000000000000000000000000000000000000000000000000000000000000000"

  url "https://github.com/dev-pikapik/JEVision/releases/download/v#{version}/JEVision-#{version}.dmg"
  name "JEVision"
  desc "Flowchart-based UI test automation for apps"
  homepage "https://github.com/dev-pikapik/JEVision"

  livecheck do
    url "https://dev-pikapik.github.io/JEVision/appcast.xml"
    strategy :sparkle, &:short_version
  end

  auto_updates true
  depends_on macos: :tahoe

  app "JEVision.app"
  binary "#{appdir}/JEVision.app/Contents/Helpers/jevision"

  zap script: {
        executable: "/bin/sh",
        args:       [
          "-c",
          'for agent in "$HOME"/Library/LaunchAgents/io.github.dev-pikapik.jevision.schedule.*.plist; ' \
          'do [ -e "$agent" ] && launchctl bootout "gui/$(id -u)" "$agent"; done; true',
        ],
      },
      trash:  [
        "~/Library/Application Support/JEVision",
        "~/Library/Caches/io.github.dev-pikapik.jevision",
        "~/Library/HTTPStorages/io.github.dev-pikapik.jevision",
        "~/Library/LaunchAgents/io.github.dev-pikapik.jevision.schedule.*.plist",
        "~/Library/Preferences/io.github.dev-pikapik.jevision.plist",
        "~/Library/Saved Application State/io.github.dev-pikapik.jevision.savedState",
        "~/Library/WebKit/io.github.dev-pikapik.jevision",
      ]
end
