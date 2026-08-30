#!/usr/bin/env bash

set -euo pipefail

usage() {
    echo "Usage: $0 --tag vX.Y.Z --packages-dir DIR --output-dir DIR --private-key FILE" >&2
}

tag=""
packages_dir=""
output_dir=""
private_key=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --tag)
            tag="$2"
            shift 2
            ;;
        --packages-dir)
            packages_dir="$2"
            shift 2
            ;;
        --output-dir)
            output_dir="$2"
            shift 2
            ;;
        --private-key)
            private_key="$2"
            shift 2
            ;;
        *)
            usage
            exit 2
            ;;
    esac
done

if [[ -z "${tag}" || -z "${packages_dir}" || -z "${output_dir}" || -z "${private_key}" ]]; then
    usage
    exit 2
fi
if [[ ! "${tag}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]]; then
    echo "Release tag is not v-prefixed SemVer: ${tag}" >&2
    exit 2
fi
if [[ -z "${REPO_GPG_PASSPHRASE:-}" ]]; then
    echo "REPO_GPG_PASSPHRASE is required." >&2
    exit 2
fi

packages_dir="$(realpath "${packages_dir}")"
output_dir="$(realpath -m "${output_dir}")"
private_key="$(realpath "${private_key}")"
version="${tag#v}"

declare -A deb_arches=(
    [amd64]="ltbox_${version}_amd64.deb"
    [arm64]="ltbox_${version}_arm64.deb"
)
declare -A rpm_arches=(
    [x86_64]="ltbox-${version}-1.x86_64.rpm"
    [aarch64]="ltbox-${version}-1.aarch64.rpm"
)

for package in "${deb_arches[@]}" "${rpm_arches[@]}"; do
    if [[ ! -f "${packages_dir}/${package}" ]]; then
        echo "Missing package: ${packages_dir}/${package}" >&2
        exit 1
    fi
    if [[ ! -f "${packages_dir}/${package}.sha256" ]]; then
        echo "Missing package checksum: ${packages_dir}/${package}.sha256" >&2
        exit 1
    fi
    (cd "${packages_dir}" && sha256sum --check "${package}.sha256")
done

rm -rf "${output_dir}"
mkdir -p \
    "${output_dir}/pool/main/l/ltbox" \
    "${output_dir}/dists/stable/main/binary-amd64" \
    "${output_dir}/dists/stable/main/binary-arm64" \
    "${output_dir}/yum/x86_64" \
    "${output_dir}/yum/aarch64"

for package in "${deb_arches[@]}"; do
    install -m 0644 \
        "${packages_dir}/${package}" \
        "${output_dir}/pool/main/l/ltbox/${package}"
done
for arch in "${!rpm_arches[@]}"; do
    package="${rpm_arches[${arch}]}"
    install -m 0644 \
        "${packages_dir}/${package}" \
        "${output_dir}/yum/${arch}/${package}"
done

for arch in amd64 arm64; do
    packages_file="dists/stable/main/binary-${arch}/Packages"
    (
        cd "${output_dir}"
        apt-ftparchive --arch "${arch}" packages pool/main/l/ltbox \
            > "${packages_file}"
        gzip -9n -c "${packages_file}" > "${packages_file}.gz"
    )
done

(
    cd "${output_dir}"
    apt-ftparchive \
        -o APT::FTPArchive::Release::Origin="LTBox" \
        -o APT::FTPArchive::Release::Label="LTBox" \
        -o APT::FTPArchive::Release::Suite="stable" \
        -o APT::FTPArchive::Release::Codename="stable" \
        -o APT::FTPArchive::Release::Version="${version}" \
        -o APT::FTPArchive::Release::Architectures="amd64 arm64" \
        -o APT::FTPArchive::Release::Components="main" \
        -o APT::FTPArchive::Release::Description="LTBox current release" \
        release dists/stable > dists/stable/Release
)

for arch in x86_64 aarch64; do
    createrepo_c --quiet "${output_dir}/yum/${arch}"
done

gnupg_home="$(mktemp -d)"
cleanup() {
    rm -rf "${gnupg_home}"
}
trap cleanup EXIT
chmod 0700 "${gnupg_home}"
export GNUPGHOME="${gnupg_home}"

gpg --batch --quiet --import "${private_key}"
signing_fingerprint="$(
    gpg --batch --with-colons --list-secret-keys |
        awk -F: '
            ($1 == "sec" || $1 == "ssb") && tolower($12) ~ /s/ {
                want_fingerprint = 1
                next
            }
            want_fingerprint && $1 == "fpr" {
                print $10
                exit
            }
        '
)"
if [[ -z "${signing_fingerprint}" ]]; then
    echo "The repository private key has no signing-capable key." >&2
    exit 1
fi

gpg --batch --armor --export "${signing_fingerprint}" \
    > "${output_dir}/ltbox-repo.asc"

gpg_sign() {
    gpg --batch --yes --quiet \
        --pinentry-mode loopback \
        --passphrase "${REPO_GPG_PASSPHRASE}" \
        --local-user "${signing_fingerprint}" \
        --digest-algo SHA256 \
        "$@"
}

gpg_sign --clearsign \
    --output "${output_dir}/dists/stable/InRelease" \
    "${output_dir}/dists/stable/Release"
gpg_sign --armor --detach-sign \
    --output "${output_dir}/dists/stable/Release.gpg" \
    "${output_dir}/dists/stable/Release"
for arch in x86_64 aarch64; do
    gpg_sign --armor --detach-sign \
        --output "${output_dir}/yum/${arch}/repodata/repomd.xml.asc" \
        "${output_dir}/yum/${arch}/repodata/repomd.xml"
done

cat > "${output_dir}/index.html" <<HTML
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>LTBox Linux repositories</title>
</head>
<body>
  <main>
    <h1>LTBox Linux repositories</h1>
    <p>This site contains only the current LTBox release, <strong>${tag}</strong>. Older packages remain available on the <a href="https://github.com/miner7222/LTBox/releases">GitHub Releases page</a>.</p>
    <p>The LTBox repository key signs APT release metadata, YUM repository metadata, and the RPM packages. The same public key is available at the stable <a href="ltbox-repo.asc">ltbox-repo.asc</a> URL. Its OpenPGP fingerprint is <code>${signing_fingerprint}</code>.</p>

    <h2>Debian and Ubuntu (APT)</h2>
    <pre><code>sudo install -d -m 0755 /etc/apt/keyrings
curl -fsSL https://miner7222.github.io/ltbox-repo/ltbox-repo.asc | sudo tee /etc/apt/keyrings/ltbox-repo.asc &gt;/dev/null
echo "deb [arch=\$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/ltbox-repo.asc] https://miner7222.github.io/ltbox-repo stable main" | sudo tee /etc/apt/sources.list.d/ltbox.list &gt;/dev/null
sudo apt update
sudo apt install ltbox</code></pre>

    <h2>Fedora (DNF)</h2>
    <pre><code>sudo rpm --import https://miner7222.github.io/ltbox-repo/ltbox-repo.asc
cat &lt;&lt;'EOF' | sudo tee /etc/yum.repos.d/ltbox.repo &gt;/dev/null
[ltbox]
name=LTBox
baseurl=https://miner7222.github.io/ltbox-repo/yum/\$basearch
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=https://miner7222.github.io/ltbox-repo/ltbox-repo.asc
EOF
sudo dnf install ltbox</code></pre>
  </main>
</body>
</html>
HTML

touch "${output_dir}/.nojekyll"
echo "Built signed LTBox APT and YUM repositories for ${tag} in ${output_dir}."
