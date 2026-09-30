#!/usr/bin/env bash
set -euo pipefail

docs_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
src_js="${docs_root}/node_modules/@scalar/api-reference/dist/browser/standalone.js"
src_json="${docs_root}/src/assets/openapi.json"
src_html="${docs_root}/src/assets/http-api-reference.html"
dest="${docs_root}/public/docs/developer/rustdoc/http"

if [[ ! -f "${src_js}" ]]; then
  printf '%s\n' "missing ${src_js}; run npm ci in docs/" >&2
  exit 1
fi
if [[ ! -f "${src_json}" ]]; then
  printf '%s\n' "missing ${src_json}" >&2
  exit 1
fi
if [[ ! -f "${src_html}" ]]; then
  printf '%s\n' "missing ${src_html}" >&2
  exit 1
fi

mkdir -p "${dest}"
cp "${src_js}" "${dest}/standalone.js"
cp "${src_json}" "${dest}/openapi.json"
cp "${src_html}" "${dest}/index.html"

for name in index.html openapi.json standalone.js; do
  if [[ ! -f "${dest}/${name}" ]]; then
    printf '%s\n' "copy failed: ${dest}/${name}" >&2
    exit 1
  fi
done
