# Release checklist (v1, macOS)

PRD §10 M5 and NFR-7. Run top to bottom; every box must be checked before a public release.

## Each version, start to finish

Dostoevsky ships in three places from the same commit and the same version number:

| Channel | Built by | How writers update |
|---|---|---|
| GitHub Release (`.dmg`) | `scripts/bundle.sh`, or the release workflow run by hand | the app's update notice (opt-in), then Download |
| Homebrew (`meroving/tap`) | `scripts/cask.sh` from that `.dmg` | `brew upgrade` (the notice says so) |
| Mac App Store | the maintainers' sandboxed build, uploaded to App Store Connect | the App Store |

**Only the `.dmg` checks for updates.** It's the `updates` cargo feature (TECH-NOTES §14), which
`scripts/bundle.sh` turns on for a build signed by the project's team (`UPDATES_TEAM`), so forks
don't ask dostoevsky.app. The App Store build is made without it, so it has no update code at
all: App Review 2.4.5 forbids an app from updating itself or sending people elsewhere for new
versions. Its build fails if the update URL turns up in the binary.

1. **Version and commit.** Set `version` in `Cargo.toml`, run §1–§3, commit. The build number is
   the commit count plus the commits before the public history (`bundle.sh`), so it only grows if
   commits are added. Squashing `main` again keeps the count where it was: raise the base number in
   the bundle scripts first, or App Store Connect rejects the upload as a build it already has.
2. **App Store.** Build the sandboxed `.pkg` from that commit, upload it and submit it for review.
   Review takes a day or more; the other channels needn't wait for it.
3. **GitHub Release.** Tag `vX.Y.Z` on the same commit and push the tag, build the `.dmg` (§4), then
   `gh release create vX.Y.Z target/bundle/Dostoevsky-X.Y.Z.dmg --verify-tag --title "Dostoevsky X.Y.Z"`.
   Or run the release workflow on the tag (Actions → Release → Run workflow): it builds, signs,
   notarizes and drafts the release; look it over and publish.
4. **Homebrew.** Update the tap's cask (§5).
5. **The update notice, last.** Deploy the landing page (§5): its `version.json` is what tells
   `.dmg` writers about the new version, within a day. Until it's deployed nobody is told, so the
   `.dmg` can be published early and announced when ready. The release page must be public by then
   (the notice's Download opens it).

## 1. Quality gates

- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `PROPTEST_CASES=50000 cargo test --release -p doc-model --test properties`
- [ ] `PROPTEST_CASES=20000 cargo test --release -p doc-format` (round trip, escaping, anchoring)
- [ ] `scripts/run_flows.sh` — all seven PRD §7 flows report `ok`; skim `target/flows/*.png`
- [ ] `scripts/e2e_bundle.sh` — every check `ok` or `known`; each `known` names an open bead
- [ ] Performance (NFR-1): `DOSTOEVSKY_PERF=1 cargo run --release -p dostoevsky <20k-word file>`;
      typing stays under 8 ms per keystroke (replace + layout), cycling likewise
- [ ] `cargo run --release -p doc-format --example bench_format`: 100k-word load < 300 ms

## 2. Data safety (NFR-2: zero data-loss bugs is a release gate)

- [ ] Plain `.md` files round-trip byte for byte (open, edit nothing, save → `cmp`)
- [ ] Kill the app (`kill -9`) with unsaved edits; relaunch restores them (FILE-4)
- [ ] Edit the file in another editor with and without local edits (FILE-5): silent reload,
      Keep mine / Load theirs / Compare; "Load theirs" puts ours in Backups
- [ ] Backups ring keeps 20 versions (Preferences → General → Show in Finder)

## 3. Manual pass

- [ ] VoiceOver: the page reads as text; a group announces "alternative 3 of 5, original: …";
      ghosted text announces "ghosted" (NFR-5)
- [ ] Reduce Motion on: explosion and stash flight become fades (Preferences shows it)
- [ ] IME: Japanese/Chinese input, accents (press and hold), emoji picker (⌃⌘Space), dictation
- [ ] keymap.json: add, override and `null`-remove a binding; a typo shows a toast
- [ ] Native tabs (Window → Merge All Windows), full screen (⌃⌘F)
- [ ] Open from Finder (Open With), drop a file on the Dock icon and on a window
- [ ] Dogfood gate (PRD §12): real writing ≥ 3×/week for 4 weeks with no data loss

## 4. Build, sign, notarize

`.github/workflows/release.yml` can do this on GitHub: run it by hand (Actions → Release → Run
workflow) with a pushed `vX.Y.Z` tag that matches `Cargo.toml`, and it builds, signs, notarizes and
attaches the `.dmg` to a draft release (its header lists the repository secrets it needs; pushing a
tag alone runs nothing). By hand:

One-time: a Developer ID Application certificate in the keychain, and either a notarytool
keychain profile

```sh
xcrun notarytool store-credentials dostoevsky-notary --apple-id <id> --team-id <TEAM> --password <app-specific>
SIGN_IDENTITY="Developer ID Application: <Name> (<TEAM>)" NOTARY_PROFILE=dostoevsky-notary scripts/bundle.sh
```

or an Apple API key. Put the key at `target/secrets/AuthKey_<KEY_ID>.p8` (`target/` is ignored), then:

```sh
SIGN_IDENTITY="Developer ID Application: <Name> (<TEAM>)" \
NOTARY_KEY_ID=<KEY_ID> NOTARY_ISSUER=<ISSUER_ID> scripts/bundle.sh
```

- [ ] `lipo -archs target/bundle/Dostoevsky.app/Contents/MacOS/dostoevsky` → `x86_64 arm64`
- [ ] `spctl --assess -vv target/bundle/Dostoevsky.app` → accepted, source=Notarized Developer ID
- [ ] Fresh macOS 13 VM: the .dmg opens without Gatekeeper warnings; app makes no network
      requests (Little Snitch or `nettop`, NFR-4) except the update check, and that only after
      the writer said yes to it (TECH-NOTES §14)
- [ ] Tag `vX.Y.Z` and push it, then `gh release create` with the .dmg (or run the release
      workflow, which attaches it to a draft); release notes list format changes (there should be
      none), publish

## 5. After a release: the other channels

The `.dmg` on the GitHub Release is the file every other channel points at.

- [ ] **Homebrew.** The tap is [meroving/homebrew-tap](https://github.com/meroving/homebrew-tap),
      one file: `Casks/dostoevsky.rb`. `scripts/cask.sh --release vX.Y.Z` prints it with the new
      version and checksum (after a build by hand, `scripts/cask.sh target/bundle/Dostoevsky-X.Y.Z.dmg`
      does the same from the uploaded file). Homebrew only checks casks inside a tap, so write it
      into your local copy and check it there:

      ```sh
      brew tap meroving/tap && brew trust meroving/tap      # once
      scripts/cask.sh --release vX.Y.Z > "$(brew --repo meroving/tap)/Casks/dostoevsky.rb"
      brew style meroving/tap/dostoevsky && brew audit --cask --strict --online meroving/tap/dostoevsky
      git -C "$(brew --repo meroving/tap)" commit -am "dostoevsky X.Y.Z" && git -C "$(brew --repo meroving/tap)" push
      ```

      Writers install with `brew install meroving/tap/dostoevsky` (naming the tap in full trusts
      it) and update with `brew upgrade`. Once the repository meets Homebrew's notability rules,
      submit the cask to `homebrew/homebrew-cask` and retire the tap.
- [ ] **The update notice.** The landing page's build writes `version.json` from the newest `v*` tag,
      and its deploy publishes it. The `.dmg` build's update check reads it, so deploy only after
      the release is published (its links lead to the release page).
