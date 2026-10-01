#!/usr/bin/env bash
# Install Ubuntu packages on a CI runner without letting a stalled package
# mirror use up the job's time.
#
#   ./scripts/ci-apt-install.sh libfontconfig1-dev libxkbcommon-dev
#
# Every workflow job that needs system packages calls this, so the limits are
# set in one place. A plain `apt-get update && apt-get install` has no time
# limit: on 2026-10-01 it ran for 20 minutes or more in three pull requests,
# and the job's own limit cancelled the Rust tests (issue #1034).
#
# Two limits apply, because a stall has two shapes:
#
# - apt's own settings end a connection that sends nothing for
#   APT_NET_TIMEOUT seconds and fetch the file again, up to three times.
# - `timeout` ends the whole `apt-get update` after APT_UPDATE_LIMIT seconds
#   and the whole `apt-get install` after APT_INSTALL_LIMIT seconds. That
#   covers a mirror that sends a few bytes at a time, which apt's own timeout
#   never sees, and a package script that hangs.
#
# A failed or timed-out attempt is run again, APT_ATTEMPTS attempts in all.
# With the defaults the script gives up after about 13 minutes at worst and
# the step fails with the reason in its log. A healthy run takes under two
# minutes.
#
# For CI runners only: it calls sudo and changes installed packages.
set -euo pipefail

ATTEMPTS="${APT_ATTEMPTS:-3}"
UPDATE_LIMIT="${APT_UPDATE_LIMIT:-60}"
INSTALL_LIMIT="${APT_INSTALL_LIMIT:-180}"
NET_TIMEOUT="${APT_NET_TIMEOUT:-20}"
RETRY_WAIT="${APT_RETRY_WAIT:-10}"

if [[ $# -eq 0 ]]; then
    echo "usage: $0 <package>..." >&2
    exit 2
fi

APT_OPTIONS=(
    -o "Acquire::Retries=3"
    -o "Acquire::http::Timeout=${NET_TIMEOUT}"
    -o "Acquire::https::Timeout=${NET_TIMEOUT}"
    -o "DPkg::Lock::Timeout=60"
)

# `timeout` runs under sudo so that it can signal apt-get, which runs as root.
# --kill-after ends an apt-get that ignores the first signal.
limited_apt_get() {
    local limit="$1"
    shift
    sudo env DEBIAN_FRONTEND=noninteractive \
        timeout --kill-after=10 "${limit}" \
        apt-get "${APT_OPTIONS[@]}" "$@"
}

# `timeout` exits with 124 when it ends the command, and with 137 when the
# command had to be killed.
report_failure() {
    local step="$1" status="$2" limit="$3"
    if [[ ${status} -eq 124 || ${status} -eq 137 ]]; then
        echo "apt-get ${step} passed the ${limit}-second limit and was stopped" >&2
    else
        echo "apt-get ${step} failed with status ${status}" >&2
    fi
}

attempt_install() {
    local status=0
    limited_apt_get "${UPDATE_LIMIT}" update || status=$?
    if [[ ${status} -ne 0 ]]; then
        report_failure update "${status}" "${UPDATE_LIMIT}"
        return 1
    fi
    limited_apt_get "${INSTALL_LIMIT}" install -y "$@" || status=$?
    if [[ ${status} -ne 0 ]]; then
        report_failure install "${status}" "${INSTALL_LIMIT}"
        return 1
    fi
}

for ((attempt = 1; attempt <= ATTEMPTS; attempt++)); do
    echo "Installing packages, attempt ${attempt} of ${ATTEMPTS}: $*"
    if attempt_install "$@"; then
        exit 0
    fi
    if ((attempt < ATTEMPTS)); then
        # An install ended part-way leaves packages unpacked but not set up,
        # and apt-get refuses to run until dpkg finishes them.
        sudo timeout --kill-after=10 "${INSTALL_LIMIT}" dpkg --configure -a || true
        sleep "${RETRY_WAIT}"
    fi
done

echo "Package install failed ${ATTEMPTS} times: $*" >&2
exit 1
