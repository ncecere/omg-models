# Operations

How the catalog data reaches a running server, and how to run, observe and
troubleshoot that path.

## Data flow

```text
 sync.yml (hourly :17)            human PRs
   bot/price-updates PR  ──merge──▶  main  ◀──merge── contributors
                                      │
            push to main ─────────────┤ (bot merges made with GITHUB_TOKEN
            schedule (hourly :20) ────┤  start no push workflows; the hourly
                                      │  schedule publishes them)
                                      ▼
                         publish-data.yml (main HEAD)
              validate ▸ build dist/ ▸ omg-models package
              catalog.tar.gz + catalog.manifest.json (+ .sigstore.json)
                                      │ only if total_sha256 changed
                                      ▼
           GitHub release `data-latest` (rolling; assets overwritten)
                                      │ HTTPS, every OMG_MODELS_DATA_REFRESH
                                      ▼
   omg-models serve (image pinned by digest; data/ baked in at build time)
     start: validate + serve the baked-in snapshot (source=embedded)
     loop:  GET manifest (If-None-Match) ─ 304/same data ─▶ nothing to do
            new data ▸ GET archive (size cap, timeout, allowlisted redirects)
                     ▸ SHA-256 of archive + every file vs manifest
                     ▸ unpack in memory ▸ full validator ▸ build API JSON
                     ▸ atomic swap (catalog + API + pages) (source=remote)
            any failure ▸ keep serving current data, log, /api/status
                                      │
                                      ▼
                       browsers (same-origin only, strict CSP)
```

Code and data ship separately: a new image is only needed for code changes.
Data changes reach a running server within one refresh interval of the next
publish run (at most about an hour plus the interval after a bot merge).

## Snapshot format

`omg-models package --data data --out <dir> --commit <sha>` writes:

- `catalog.tar.gz`: `data/…` (every non-hidden file of the data directory)
  and `dist/…` (exactly what `omg-models build` writes). Reproducible:
  sorted entries, zero mtimes, uid/gid 0, mode 0644.
- `catalog.manifest.json`:

  | Field | Meaning |
  |---|---|
  | `schema_version` | manifest format, currently `1`; servers reject others |
  | `kind` | `omg-models-catalog-snapshot` |
  | `commit` | 40-hex commit of main the snapshot was built from |
  | `built_at` | `YYYY-MM-DDTHH:MM:SSZ` |
  | `generator` | `omg-models <version>` that built it |
  | `archive` | `name`, `sha256`, `size` of the archive (same directory) |
  | `files[]` | `path`, `sha256`, `size` of every archived file, sorted |
  | `data_sha256` | digest over the `data/` files |
  | `total_sha256` | digest over all files |

  Both digests are the SHA-256 of `sha256sum`-style lines
  (`<sha256>  <path>\n`) sorted by path, so they depend only on file
  contents and paths. Reproduce one from an unpacked archive with
  `find data dist -type f | LC_ALL=C sort | xargs sha256sum | sha256sum`.

`publish-data.yml` uploads a new pair only when `total_sha256` differs from
the published manifest's, so the release (and the servers' ETags) change only
with the content. The release tag does not move; the manifest names the
commit. Every run also keeps its manifest as a workflow artifact
(`catalog-manifest-<sha>`, 90 days).

### Signature

The manifest is signed with cosign keyless (Sigstore bundle
`catalog.manifest.json.sigstore.json`). The manifest pins the archive's
SHA-256 and every file's, so a verified manifest covers the archive:

```sh
gh release download data-latest -R ncecere/omg-models
cosign verify-blob catalog.manifest.json \
  --bundle catalog.manifest.json.sigstore.json \
  --certificate-identity https://github.com/ncecere/omg-models/.github/workflows/publish-data.yml@refs/heads/main \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com
sha256sum catalog.tar.gz   # equals .archive.sha256 in the manifest
```

The server does **not** verify the signature yet (follow-up: a Sigstore
bundle verifier in Rust with a pinned trust root). It relies on HTTPS to
GitHub, the host allowlist, the SHA-256 checks and the full validator.

## Server settings

| Flag | Environment | Default | |
|---|---|---|---|
| `--data` | `OMG_MODELS_DATA` | `data` | baked-in snapshot (`/app/data` in the image) |
| `--data-url` | `OMG_MODELS_DATA_URL` | unset (refresh off) | URL of `catalog.manifest.json` |
| `--data-refresh` | `OMG_MODELS_DATA_REFRESH` | `15m` | `30s`, `15m`, `1h`, `900`; minimum 10s |
| `--data-max-bytes` | `OMG_MODELS_DATA_MAX_BYTES` | `16777216` | archive cap; unpacked data may be up to 8x |
| | `OMG_MODELS_DATA_COMMIT` | image build arg | commit of the baked-in data |

For the public release:

```text
OMG_MODELS_DATA_URL=https://github.com/ncecere/omg-models/releases/download/data-latest/catalog.manifest.json
OMG_MODELS_DATA_REFRESH=15m
```

The archive is fetched from the same directory as the manifest URL, under the
name in `archive.name`.

### Fetch policy

- HTTPS only. (A hidden `--data-allow-http-loopback` flag accepts `http://`
  to `127.0.0.1`/`::1`/`localhost` for local testing; never set it in
  production.)
- Redirects are followed by hand (at most 5), each checked: only `https://`
  and only the configured URL's host or `github.com`,
  `objects.githubusercontent.com`, `release-assets.githubusercontent.com`.
  No credentials are sent anywhere.
- 60 s timeout per request; manifest capped at 1 MiB, archive at
  `--data-max-bytes` (checked against the manifest, `Content-Length` and the
  bytes read), unpacked data at 8x that, at most 20,000 files.
- The manifest request is conditional (`If-None-Match`); the ETag is kept
  only after a successful check, so a failed attempt is retried in full.
  When the manifest's `data_sha256` equals the served data's, the archive is
  not downloaded.
- Archive entries must be regular files under `data/` or `dist/` with safe
  relative paths, listed exactly once in the manifest with matching size and
  SHA-256; nothing is written to disk.
- Standard proxy variables (`HTTPS_PROXY`, `NO_PROXY`, ...) are honoured.

### Egress

The pod needs outbound TCP 443 to:

- `github.com` (release download URL, answers with a redirect)
- `release-assets.githubusercontent.com` and `objects.githubusercontent.com`
  (where GitHub redirects release assets)

`api.github.com` is not used by the server (only by the CI workflows). DNS
must resolve these names. Without egress the server keeps serving the
baked-in data and reports the failure.

### Swapping

A refresh builds a complete new snapshot (catalog, API files with their
ETags, page data) and replaces the old one with a single pointer swap. Each
request uses one snapshot from start to finish; in-flight requests finish on
the old one. API ETags are content hashes, so clients revalidate naturally.

The server serves its own build of the downloaded data. If the published
`dist/` differs (a different `omg-models` version built it), it logs that and
still serves its own build, so pages and API agree. If a newer data format
is published that this image cannot parse, validation fails and the server
keeps its current data: deploy a newer image.

A newly deployed image may briefly serve data newer than the release (the
publish run takes a few minutes after a push to main); the first refresh
after that may swap to the release's data, and the next publish run catches
up.

## Health and status

| Path | Meaning |
|---|---|
| `/healthz` | liveness: the process serves (`ok`) |
| `/readyz` | readiness: a validated catalog is loaded (`ready`; 503 if empty). A failed refresh does not make the server unready; it still serves the last good data. |
| `/api/status` | JSON: `data` (`source` `embedded`/`remote`, `commit`, `built_at`, `data_sha256`, counts) and `refresh` (`enabled`, `url`, `interval_seconds`, `last_attempt_at`, `last_result` `updated`/`unchanged`/`failed`, `last_error`, `last_success_at`, `last_update_at`, `consecutive_failures`) |

The footer shows "Data updated <time> (commit …)" for a remote snapshot, or
the baked-in commit before the first refresh. Logs (stderr): one line per
swap and per failure.

Suggested alert: `refresh.consecutive_failures` above 8 (two hours at 15m),
or `data.built_at` older than a day while the publish workflow is green.

## Kubernetes (Flux) sketch

```yaml
containers:
  - name: omg-models
    image: ghcr.io/ncecere/omg-models@sha256:...   # pinned; no image automation
    env:
      - name: OMG_MODELS_DATA_URL
        value: https://github.com/ncecere/omg-models/releases/download/data-latest/catalog.manifest.json
      - name: OMG_MODELS_DATA_REFRESH
        value: 15m
    securityContext:
      readOnlyRootFilesystem: true
      runAsNonRoot: true
      allowPrivilegeEscalation: false
      capabilities: { drop: [ALL] }
    livenessProbe: { httpGet: { path: /healthz, port: 8080 } }
    readinessProbe: { httpGet: { path: /readyz, port: 8080 } }
```

Each replica refreshes on its own; replicas may serve different snapshots for
up to one interval. A NetworkPolicy should allow egress to the hosts above
on 443 (plus DNS).

## Troubleshooting

- `last_error` says `host … is not allowed`: GitHub changed its asset host;
  update `GITHUB_RELEASE_HOSTS` in `crates/cli/src/live.rs` (and the
  NetworkPolicy).
- `does not match the manifest`: usually a publish in progress (the archive
  and manifest are uploaded one after the other); the next attempt
  succeeds.
- `does not validate`: the published data fails this image's validator
  (format change or a bad merge). Fix main or deploy a newer image; the
  server keeps its current data meanwhile.
- Force a republish: run `publish-data` by hand (Actions > publish-data >
  Run workflow on main). It still uploads only when the content changed.
