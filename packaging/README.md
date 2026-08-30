# Package-manager release inputs

Publishing a GitHub Release triggers `.github/workflows/publish-packages.yml`.
The workflow reads the release's committed `.sha256` sidecars, renders the
Scoop manifest and Homebrew cask, builds `.deb` and `.rpm` files from the
already-built Linux release binaries with nFPM, attaches the Linux packages to
that release, and replaces the hosted APT and YUM repositories with the current
release.

The tag must be `vX.Y.Z`. Before publishing the draft, clear its **prerelease**
flag if it should become visible to LTBox's stable update check and package
managers.

## External repositories and credentials

The external repositories are deliberately not created by automation. Create
and initialize these repositories with a `main` branch when they are ready:

- `miner7222/scoop-bucket` receives `bucket/ltbox.json`.
- `miner7222/homebrew-tap` receives `Casks/ltbox.rb`.
- `miner7222/ltbox-repo` receives the APT/YUM tree and is served by GitHub Pages
  from the root of its `main` branch.

Configure these LTBox repository Actions secrets:

- `SCOOP_BUCKET_TOKEN`: a fine-grained GitHub personal access token with
  **Contents: Read and write** access to `miner7222/scoop-bucket`.
- `HOMEBREW_TAP_TOKEN`: a fine-grained GitHub personal access token with
  **Contents: Read and write** access to `miner7222/homebrew-tap`.
- `REPO_GITHUB_TOKEN`: a fine-grained GitHub personal access token with
  **Contents: Read and write** access to `miner7222/ltbox-repo`. Its access must
  permit the workflow's intentional force-push to the default branch.
- `REPO_GPG_PRIVATE_KEY`: the complete ASCII-armored private LTBox repository
  signing key, including its `BEGIN PGP PRIVATE KEY BLOCK` and `END PGP PRIVATE
  KEY BLOCK` lines.
- `REPO_GPG_PASSPHRASE`: the passphrase protecting that private key.

If a secret is absent, its repository is absent or inaccessible, or the token
cannot push, the workflow emits a clear notice/warning and skips only that
external publication. Linux release packages are independent of those tokens.

No repository secret is needed for `.deb`/`.rpm` attachment; the workflow uses
the release repository's short-lived `GITHUB_TOKEN`. When both repository GPG
secrets are configured, nFPM signs the attached RPMs as it builds them. The
repository job publishes only when all three repository secrets are present.

The repository branch is deliberately replaced by a single orphan commit on
each publication. It contains only the current release; older `.deb` and `.rpm`
files remain downloadable from the GitHub Releases page instead of accumulating
in Git history.

## APT repository (Debian and Ubuntu)

The repository supports `amd64` and `arm64`. These commands install the stable
public key at its own stable URL, bind the source to that key, and install LTBox:

```sh
sudo install -d -m 0755 /etc/apt/keyrings
curl -fsSL https://miner7222.github.io/ltbox-repo/ltbox-repo.asc | sudo tee /etc/apt/keyrings/ltbox-repo.asc >/dev/null
echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/ltbox-repo.asc] https://miner7222.github.io/ltbox-repo stable main" | sudo tee /etc/apt/sources.list.d/ltbox.list >/dev/null
sudo apt update
sudo apt install ltbox
```

APT verifies `dists/stable/InRelease` with the LTBox repository signing key.
`dists/stable/Release.gpg` carries a detached signature by the same key for APT
clients that use the detached form. Later releases are picked up with:

```sh
sudo apt update
sudo apt upgrade ltbox
```

## YUM repository (Fedora)

The repository supports `x86_64` and `aarch64`. These commands import the same
LTBox repository key, require both package and repository-metadata signatures,
and install LTBox:

```sh
sudo rpm --import https://miner7222.github.io/ltbox-repo/ltbox-repo.asc
cat <<'EOF' | sudo tee /etc/yum.repos.d/ltbox.repo >/dev/null
[ltbox]
name=LTBox
baseurl=https://miner7222.github.io/ltbox-repo/yum/$basearch
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=https://miner7222.github.io/ltbox-repo/ltbox-repo.asc
EOF
sudo dnf install ltbox
```

nFPM signs each RPM with the LTBox repository signing key. The workflow signs
each architecture's `repodata/repomd.xml` with that same key. Later releases are
picked up with:

```sh
sudo dnf upgrade ltbox
```

## Repository signing key setup

Generate the dedicated key once on a trusted maintainer machine with GnuPG. The
following creates a passphrase-protected RSA signing key and exports the exact
values needed by the Actions secrets:

```bash
export GNUPGHOME="$(mktemp -d)"
chmod 700 "$GNUPGHOME"
read -rsp 'Repository key passphrase: ' REPO_GPG_PASSPHRASE; echo
export REPO_GPG_PASSPHRASE
gpg --batch --pinentry-mode loopback \
  --passphrase "$REPO_GPG_PASSPHRASE" \
  --quick-generate-key \
  'LTBox Repository <16506343+miner7222@users.noreply.github.com>' \
  rsa4096 cert,sign 2y
REPO_GPG_FINGERPRINT="$(gpg --batch --with-colons --list-secret-keys | awk -F: '$1 == "fpr" { print $10; exit }')"
gpg --batch --armor --export-secret-keys "$REPO_GPG_FINGERPRINT" > ltbox-repo-private.asc
gpg --batch --armor --export "$REPO_GPG_FINGERPRINT" > ltbox-repo.asc
printf 'Signing-key fingerprint: %s\n' "$REPO_GPG_FINGERPRINT"
unset REPO_GPG_PASSPHRASE
rm -rf "$GNUPGHOME"
unset GNUPGHOME
```

Store the full contents of `ltbox-repo-private.asc` as
`REPO_GPG_PRIVATE_KEY`, store the entered passphrase as
`REPO_GPG_PASSPHRASE`, back up both securely, and publish the printed
fingerprint through a separately authenticated maintainer channel. The workflow
derives the signing key ID; it does not need another key-ID secret.

Before the first publication, enable GitHub Pages for
`miner7222/ltbox-repo` from the default branch's repository root and allow the
token to force-push that branch. Run **Publish package-manager artifacts**
manually with an existing `vX.Y.Z` tag and leave
`publish_linux_repository` false. The workflow builds, signs, validates with
stock APT and DNF clients, and uploads the complete `linux-repository-*`
artifact without pushing. Inspect that artifact, then rerun with
`publish_linux_repository` true. Published-release runs update the hosted
repository automatically.

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
