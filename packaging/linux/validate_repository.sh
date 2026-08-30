#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "Usage: $0 REPOSITORY_DIR" >&2
    exit 2
fi

repository_dir="$(realpath "$1")"
public_key="${repository_dir}/ltbox-repo.asc"
temporary_dir="$(mktemp -d)"
server_pid=""

cleanup() {
    if [[ -n "${server_pid}" ]]; then
        kill "${server_pid}" 2>/dev/null || true
        wait "${server_pid}" 2>/dev/null || true
    fi
    rm -rf "${temporary_dir}"
}
trap cleanup EXIT

export GNUPGHOME="${temporary_dir}/gnupg"
mkdir -m 0700 "${GNUPGHOME}"
gpg --batch --quiet --import "${public_key}"
gpg --batch --verify \
    "${repository_dir}/dists/stable/Release.gpg" \
    "${repository_dir}/dists/stable/Release"
gpg --batch --verify "${repository_dir}/dists/stable/InRelease"

for arch in x86_64 aarch64; do
    gpg --batch --verify \
        "${repository_dir}/yum/${arch}/repodata/repomd.xml.asc" \
        "${repository_dir}/yum/${arch}/repodata/repomd.xml"
done

for arch in amd64 arm64; do
    packages_file="${repository_dir}/dists/stable/main/binary-${arch}/Packages"
    [[ "$(grep -c '^Package: ltbox$' "${packages_file}")" -eq 1 ]]
    grep -qx "Architecture: ${arch}" "${packages_file}"
    grep -qx 'Filename: pool/main/l/ltbox/.*' "${packages_file}"
done

rpm_db="${temporary_dir}/rpmdb"
mkdir "${rpm_db}"
rpm --dbpath "${rpm_db}" --initdb
rpmkeys --dbpath "${rpm_db}" --import "${public_key}"
for package in \
    "${repository_dir}"/yum/x86_64/*.rpm \
    "${repository_dir}"/yum/aarch64/*.rpm
do
    rpmkeys --dbpath "${rpm_db}" --checksig "${package}"
done

port=18080
python3 -m http.server "${port}" \
    --bind 127.0.0.1 \
    --directory "${repository_dir}" \
    > "${temporary_dir}/http.log" 2>&1 &
server_pid="$!"
for _ in {1..30}; do
    if curl --fail --silent "http://127.0.0.1:${port}/index.html" > /dev/null; then
        break
    fi
    sleep 1
done
curl --fail --silent "http://127.0.0.1:${port}/index.html" > /dev/null

for arch in amd64 arm64; do
    docker run --rm --network host \
        -e APT_ARCH="${arch}" \
        -v "${repository_dir}:/repository:ro" \
        debian:stable-slim \
        sh -ceu '
            rm -f /etc/apt/sources.list /etc/apt/sources.list.d/*
            install -d -m 0755 /etc/apt/keyrings
            cp /repository/ltbox-repo.asc /etc/apt/keyrings/ltbox-repo.asc
            echo "deb [arch=${APT_ARCH} signed-by=/etc/apt/keyrings/ltbox-repo.asc] http://127.0.0.1:18080 stable main" > /etc/apt/sources.list
            apt-get -o "APT::Architecture=${APT_ARCH}" update
            apt-cache -o "APT::Architecture=${APT_ARCH}" show ltbox > /dev/null
        '
done

for arch in x86_64 aarch64; do
    docker run --rm --network host \
        -e RPM_ARCH="${arch}" \
        -v "${repository_dir}:/repository:ro" \
        fedora:latest \
        sh -ceu '
            cp /repository/ltbox-repo.asc /tmp/ltbox-repo.asc
            cat > /etc/yum.repos.d/ltbox.repo <<EOF
[ltbox]
name=LTBox
baseurl=http://127.0.0.1:18080/yum/\$basearch
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=file:///tmp/ltbox-repo.asc
EOF
            dnf -q -y --forcearch="${RPM_ARCH}" --disablerepo="*" --enablerepo=ltbox makecache
            dnf -q --forcearch="${RPM_ARCH}" --disablerepo="*" --enablerepo=ltbox list --available ltbox
        '
done

echo "Validated APT, YUM, metadata, and RPM signatures in ${repository_dir}."
