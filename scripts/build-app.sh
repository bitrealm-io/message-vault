#!/usr/bin/env bash
# Build the desktop installers and give them lowercase file names.
#
#   ./scripts/build-app.sh [cargo tauri build arguments]
#
# Tauri names each installer after productName, so a plain build writes
# "Message Vault_0.9.0_amd64.AppImage". productName is also the name the app
# shows in menus, so it stays as it is and the files are renamed afterwards:
# message_vault_0.9.0_amd64.AppImage. The release job in ci.yml runs this
# script too, so a local build and a release carry the same names.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

cargo tauri build "$@"

PRODUCT_NAME="$(sed -n 's/^ *"productName": *"\(.*\)",\{0,1\} *$/\1/p' src-tauri/tauri.conf.json)"
if [[ -z "${PRODUCT_NAME}" ]]; then
  echo "productName not found in src-tauri/tauri.conf.json" >&2
  exit 1
fi

shopt -s nullglob
renamed=0
for file in src-tauri/target/*/bundle/*/"${PRODUCT_NAME}"* \
  src-tauri/target/*/*/bundle/*/"${PRODUCT_NAME}"*; do
  [[ -f "${file}" ]] || continue
  name="$(basename "${file}")"
  target="$(dirname "${file}")/message_vault${name#"${PRODUCT_NAME}"}"
  mv -f "${file}" "${target}"
  echo "==> ${target}"
  renamed=$((renamed + 1))
done

if [[ "${renamed}" -eq 0 ]]; then
  echo "cargo tauri build produced no installer named \"${PRODUCT_NAME}\"*" >&2
  exit 1
fi
