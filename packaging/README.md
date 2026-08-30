# Package-manager release inputs

Publishing a GitHub Release triggers `.github/workflows/publish-packages.yml`.
The workflow reads the release's committed `.sha256` sidecars, renders the
Scoop manifest and Homebrew cask, builds `.deb` and `.rpm` files from the
already-built Linux release binaries with nFPM, and attaches the Linux packages
to that release.

The tag must be `vX.Y.Z`. Before publishing the draft, clear its **prerelease**
flag if it should become visible to LTBox's stable update check and package
managers.

## External repositories and credentials

The external repositories are deliberately not created by automation. Create
and initialize these repositories with a `main` branch when they are ready:

- `miner7222/scoop-bucket` receives `bucket/ltbox.json`.
- `miner7222/homebrew-tap` receives `Casks/ltbox.rb`.

Configure these LTBox repository Actions secrets:

- `SCOOP_BUCKET_TOKEN`: a fine-grained GitHub personal access token with
  **Contents: Read and write** access to `miner7222/scoop-bucket`.
- `HOMEBREW_TAP_TOKEN`: a fine-grained GitHub personal access token with
  **Contents: Read and write** access to `miner7222/homebrew-tap`.

If a secret is absent, its repository is absent or inaccessible, or the token
cannot push, the workflow emits a clear notice/warning and skips only that
external publication. Linux release packages are independent of those tokens.

No repository secret is needed for `.deb`/`.rpm` attachment; the workflow uses
the release repository's short-lived `GITHUB_TOKEN`.

## Linux package layout

Both formats install:

```text
/usr/bin/ltbox
/usr/lib/udev/rules.d/51-ltbox-qcom.rules
/usr/share/applications/io.github.miner7222.LTBox.desktop
/usr/share/icons/hicolor/scalable/apps/io.github.miner7222.LTBox.svg
/usr/share/ltbox/ltbox.install-source       # `deb` or `rpm`
/usr/share/doc/ltbox/README.md
```

The Debian package installs the GPL text as
`/usr/share/doc/ltbox/copyright`. The RPM installs it as
`/usr/share/licenses/ltbox/LICENSE`.

APT/YUM repository hosting and GPG signing are intentionally deferred. The
generated `.deb` and `.rpm` files are direct GitHub Release assets.
