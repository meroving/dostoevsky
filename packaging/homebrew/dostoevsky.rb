# Homebrew cask for the .dmg on GitHub Releases (dsky-a8n.23). A template: scripts/cask.sh fills in
# the version and the checksum of a release (docs/RELEASE.md §5).
cask "dostoevsky" do
  version "__VERSION__"
  sha256 "__SHA256__"

  url "https://github.com/meroving/dostoevsky/releases/download/v#{version}/Dostoevsky-#{version}.dmg"
  name "Dostoevsky"
  desc "Writing app that keeps alternatives of any phrase in place"
  homepage "https://dostoevsky.app/"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on macos: :ventura

  app "Dostoevsky.app"

  # Only the .dmg build's own data. A sandboxed build's container isn't Homebrew's to remove.
  zap trash: [
    "~/Library/Application Support/Dostoevsky",
    "~/Library/Saved Application State/app.dostoevsky.Dostoevsky.savedState",
  ]
end
