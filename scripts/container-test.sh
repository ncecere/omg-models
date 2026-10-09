#!/usr/bin/env bash
# Runtime contract for the omg-models image (CI, image.yml and local use):
#   scripts/container-test.sh <image>
# Checks: runs as 10001:10001, no shell, read-only root filesystem with all
# capabilities dropped, /healthz, the JSON API with CORS and caching headers,
# the security headers on pages, and the exec-form health check.
set -euo pipefail

image="${1:?usage: container-test.sh <image>}"
port="${OMG_MODELS_TEST_PORT:-18089}"
name="omg-models-test-$$"

cleanup() { docker rm -f "${name}" >/dev/null 2>&1 || true; }
trap cleanup EXIT

fail() { echo "FAIL: $*" >&2; docker logs "${name}" >&2 || true; exit 1; }

user=$(docker image inspect --format '{{.Config.User}}' "${image}")
[ "${user}" = "10001:10001" ] || fail "image user is '${user}', expected 10001:10001"

if docker run --rm --entrypoint /bin/sh "${image}" -c true >/dev/null 2>&1; then
  fail "the image has a shell"
fi

version=$(docker run --rm "${image}" --version)
case "${version}" in omg-models\ *) ;; *) fail "unexpected --version output: ${version}" ;; esac

docker run -d --name "${name}" --read-only --cap-drop ALL \
  --security-opt no-new-privileges -p "127.0.0.1:${port}:8080" "${image}" >/dev/null

for _ in $(seq 1 30); do
  if curl -fsS "http://127.0.0.1:${port}/healthz" >/dev/null 2>&1; then break; fi
  sleep 1
done
[ "$(curl -fsS "http://127.0.0.1:${port}/healthz")" = "ok" ] || fail "/healthz"

headers=$(curl -fsS -D - -o /dev/null "http://127.0.0.1:${port}/api/v1/omg-prices.json")
grep -qi '^access-control-allow-origin: \*' <<<"${headers}" || fail "API without CORS *"
grep -qi '^etag: "sha256-' <<<"${headers}" || fail "API without ETag"
grep -qi '^cache-control: public' <<<"${headers}" || fail "API without Cache-Control"

page=$(curl -fsS -D - -o /dev/null "http://127.0.0.1:${port}/")
grep -qi "^content-security-policy: default-src 'none'" <<<"${page}" || fail "page without CSP"
grep -qi '^x-frame-options: DENY' <<<"${page}" || fail "page without X-Frame-Options"
curl -fsS "http://127.0.0.1:${port}/models/claude-sonnet-5-5" | grep -q "Claude Sonnet 5.5" || fail "model page"
css=$(curl -fsS "http://127.0.0.1:${port}/" | grep -o '/_topcoat/assets/tailwind-[^"]*\.css' | head -n1)
[ -n "${css}" ] || fail "no stylesheet link"
curl -fsS -o /dev/null "http://127.0.0.1:${port}${css}" || fail "stylesheet asset"

docker exec "${name}" /usr/local/bin/omg-models healthcheck --timeout 3 || fail "healthcheck subcommand"

echo "container contract ok (${version})"
