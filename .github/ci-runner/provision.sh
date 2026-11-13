#!/usr/bin/env bash
# Installs what CI's self-hosted runner needs on a fresh Ubuntu 24.04
# droplet: the build tools, Rust (the RUST_TOOLCHAIN of ci.yml), Python,
# the GitHub Actions runner, its job-started hook and the boot script.
# The droplet is then powered off and snapshotted as `ci-runner-base-<date>`
# (.github/workflows/ci-runner-image.yml); every CI droplet starts from the
# newest such snapshot. Nothing here is secret, and nothing secret is left
# on the image: the registration token comes with each droplet's user data.
set -euxo pipefail
TOOLCHAIN="${1:-1.100.0}"
export DEBIAN_FRONTEND=noninteractive
apt-get update
# No package starts a service while this runs from cloud-init: a start (or a
# stop) of postgresql's units from inside cloud-final waits on boot to finish,
# which waits on this script, and the image build hung until its deadline.
printf '#!/bin/sh\nexit 101\n' > /usr/sbin/policy-rc.d
chmod +x /usr/sbin/policy-rc.d
apt-get install -y --no-install-recommends build-essential pkg-config git curl ca-certificates jq unzip \
  python3 python3-venv python3-pip libssl-dev clang lld postgresql
rm -f /usr/sbin/policy-rc.d
# Postgres as GitHub's ubuntu image has it: installed, its service off.
# chloedb-server's tests/postgres.rs starts its own from initdb, and
# requires one on a Linux runner of GitHub Actions.
systemctl disable postgresql || true
id runner >/dev/null 2>&1 || useradd -m -s /bin/bash runner
# Rust for the runner user; dtolnay/rust-toolchain in each job installs the
# toolchain the job names, which is this one, so it is already there.
sudo -u runner bash -c "curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain $TOOLCHAIN -c clippy"
# The Actions runner, unconfigured: boot.sh registers it.
ver=$(curl -sSf https://api.github.com/repos/actions/runner/releases/latest | jq -r .tag_name | sed 's/^v//')
mkdir -p /opt/actions-runner
curl -sSfL "https://github.com/actions/runner/releases/download/v${ver}/actions-runner-linux-x64-${ver}.tar.gz" | tar -xz -C /opt/actions-runner
/opt/actions-runner/bin/installdependencies.sh
chown -R runner:runner /opt/actions-runner
install -d /opt/ci-runner
install -m 0755 "$(dirname "$0")/boot.sh" /opt/ci-runner/boot.sh
install -m 0755 "$(dirname "$0")/job-started.sh" /opt/ci-runner/job-started.sh
# No sshd: nothing logs in to a CI droplet. Its console is DigitalOcean's.
systemctl disable --now ssh.socket ssh.service 2>/dev/null || true
cloud-init clean --logs
