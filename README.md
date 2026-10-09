# Open Model Catalog (omg-models)

An open catalog of AI models and what they cost, served at
**<https://models.omg.bitop.dev>**: list prices per provider with cache,
batch and prompt-size tiers, limits, modalities, upstream model ids and
append-only price history, every price linked to its source. It is part of
the [Open Model Gateway](https://omg.bitop.dev) family (docs:
<https://docs.omg.bitop.dev>) and publishes exact price lines the gateway can
import (`/api/v1/omg-prices.json`).

Everything is Rust: a data crate, a CLI, and a server-rendered website built
with [Topcoat](https://github.com/tokio-rs/topcoat) 0.10 and Topcoat UI. The
site runs without JavaScript, cookies, tracking or third-party requests.

## Repository

```text
crates/catalog   data model, loader, validator, JSON exports (no I/O beyond reading data/)
crates/cli       omg-models: validate | build | schema | sync | serve | healthcheck
crates/web       Topcoat app (pages, static JSON API, security headers), Topcoat UI components
data/            providers/<provider>/provider.toml, providers/<provider>/models/<model>.toml
docs/            data-format.md
```

## Data

Prices are exact decimal strings in USD per unit (per 1M tokens for token
meters), never floats. Unknown prices are left out, never shown as zero.
History is append-only: a change is a new entry with a later
`effective_from`. Each entry records where it came from (URL, fetch time and,
for automated sources, commit, SHA-256 and entry key). The full format,
meters, tiers, batch rules and validation checks are in
[docs/data-format.md](docs/data-format.md).

Seed data (fetched 2026-10-09 from the official Anthropic and OpenAI pricing
and model pages, the AWS Price List for Amazon Bedrock, and OpenRouter's
models API): 4 providers (OpenAI, Anthropic, Amazon Bedrock, OpenRouter),
8 models (Claude Opus/Sonnet/Haiku 5.5, GPT-6.1 Sol, GPT-6 Luna, GPT-5.4
Mini, gpt-oss-120b, Llama 4 Maverick), 19 offerings.

## Endpoints

| Path | Contents |
|---|---|
| `/` | model table with search, filters (provider, modality, max input price, context, open weights) and sorting |
| `/models/{model}` | prices per provider with history and provenance, limits, ids, API examples |
| `/providers`, `/providers/{provider}` | providers and their models |
| `/compare?m1=&m2=&m3=&m4=` | two to four models side by side |
| `/api`, `/about` | API documentation; licences and attribution |
| `/api.json` | models.dev-compatible catalog (deviations in the data-format doc) |
| `/api/v1/index.json` | counts, last update, licence, endpoints |
| `/api/v1/providers.json`, `/api/v1/providers/{id}.json` | providers |
| `/api/v1/models.json`, `/api/v1/models/{id}.json` | models, offerings, current prices; per model the full history |
| `/api/v1/omg-prices.json` | OMG Pricing v3 lines: integer micro-USD per batch (exact; non-integer values rounded up and flagged) |
| `/api/v1/history.json` | every price entry, newest first |
| `/api/v1/schema.json` | JSON Schema of the data files |
| `/healthz` | liveness |

API files are the exact bytes `omg-models build` writes, served with
`Access-Control-Allow-Origin: *`, a strong `ETag` (304 on `If-None-Match`)
and `Cache-Control: public, max-age=300, stale-while-revalidate=3600`. Every
response carries a strict CSP (`default-src 'none'`; same-origin styles,
fonts and images; no scripts), `X-Frame-Options: DENY`, `nosniff`,
`Referrer-Policy`, `Permissions-Policy` and HSTS. Pages are `no-cache`
because they reference content-hashed assets.

## Usage

Requirements: Rust 1.98.1 (`rust-toolchain.toml`) and the Topcoat CLI for
the asset bundle: `cargo install topcoat-cli --version =0.10.0 --locked`.
The web crate's build script downloads Topcoat's pinned Tailwind CLI once
(set `TAILWIND_CLI` to use a local binary).

```sh
cargo run -p omg-models -- validate            # check data/
cargo run -p omg-models -- build --out dist    # write the JSON API
cargo run -p omg-models -- schema              # JSON Schema of the data files
topcoat asset bundle --bin omg-models          # CSS, fonts, logo -> target/debug/assets
cargo run -p omg-models -- serve --listen 127.0.0.1:8941
cargo run -p omg-models -- sync --dry-run      # compare with upstream sources
```

`serve` validates the data, builds the JSON in memory and refuses to start on
errors. Settings: `OMG_MODELS_DATA` (default `data`), `OMG_MODELS_LISTEN`
(default `127.0.0.1:8080`), `OMG_MODELS_ASSETS` (default `assets/` next to the
binary). Use `topcoat dev` for live reload while editing the site.

Checks (CI runs the same):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
topcoat asset bundle --bin omg-models && OMG_MODELS_REQUIRE_ASSETS=1 cargo test --workspace
cargo deny check
actionlint
docker build -t omg-models:local . && scripts/container-test.sh omg-models:local
```

The web HTTP tests render pages through the real router and need the asset
bundle from the step above; without one they are skipped unless
`OMG_MODELS_REQUIRE_ASSETS=1`.

## Contributing data

1. Add or edit TOML under `data/` (see [docs/data-format.md](docs/data-format.md)).
   Take prices from the provider's own pricing page or price list and record
   the URL and fetch time. Leave out what you cannot verify.
2. To change a price, append a new `[[offerings.prices]]` entry; do not edit
   old ones.
3. Run `omg-models validate` and `omg-models build`, and open a pull request.

## Sync policy

`.github/workflows/sync.yml` runs **hourly** (and on demand). It runs
`omg-models sync` against:

- **LiteLLM** `model_prices_and_context_window.json` (MIT): primary source
  for `openai`, `anthropic` and `amazon-bedrock` (`[sync] prices = "litellm"`).
  It is the only free source with batch, 5-minute and 1-hour cache writes,
  `_above_Nk_tokens` tiers (strictly greater; xAI's are inclusive and are
  converted) and per-image/second/character/request rates. Keys are exact
  wire ids, Bedrock geo prefixes included. The file is pinned to the commit
  that last changed it; provenance records the commit, SHA-256 and key.
- **OpenRouter** `/api/v1/models`: primary for `openrouter` (batch from the
  `:batch` ids).
- **pydantic genai-prices v2** (MIT): cross-check only. Its values are never
  written; disagreements are listed in the PR and block auto-merge for the
  affected change.
- **models.dev** `api.json` (MIT): fills missing release dates, knowledge
  cutoffs and limits; differences are listed, never overwritten. Not a price
  source (no batch, no 1-hour writes).

Source numbers are parsed from their JSON text as exact decimals (never via
`f64`). The parser is an allowlist; unmapped keys, service tiers
(priority/flex) and time-of-day prices are reported, not guessed. Requests
identify themselves with a User-Agent, use ETags/commit pins to avoid
re-downloading, and run once an hour.

Changes go to one rolling pull request from `bot/price-updates` with a
readable summary. It is labelled:

- **`auto-merge`** when every change is small: a first price for an
  unpriced offering, filled metadata, or rates that moved within **±25%**,
  with no cross-check disagreement on a changed offering. GitHub auto-merge
  is enabled and merges once CI passes.
- **`needs-review`** for anything else: larger moves, newly listed meters on
  an existing price, tier changes, values that are not a whole micro-USD,
  cross-source disagreements, a model or price that disappeared from its
  source, or an unavailable cross-check source.

Removals never delete data. A standing disagreement on an unchanged offering
is listed in the PR but does not block unrelated changes. When the sources
match `main` again, the bot PR is closed.

**Repository settings needed** (once): allow auto-merge; allow GitHub
Actions to create pull requests; protect `main` with the CI checks (`rust`,
`cargo-deny`, `actionlint`, `container`). The workflow uses `GITHUB_TOKEN`
with `contents`, `pull-requests` and `actions` write only. Because events
from `GITHUB_TOKEN` do not start other workflows, it dispatches `ci.yml` on
the bot branch, and merges it makes do not rebuild the image; set a
`PRICE_SYNC_TOKEN` secret (GitHub App or fine-grained token) to have bot
merges trigger the normal push pipeline.

## Deploy

`Dockerfile` builds a distroless image (`gcr.io/distroless/cc-debian12`,
pinned by digest) with the binary as entrypoint (`serve` by default), the
asset bundle in `/app/assets` and the data baked into `/app/data` at build
time. It runs as UID/GID 10001, writes nothing (use a read-only root
filesystem), listens on `0.0.0.0:8080`, and has an exec-form
`HEALTHCHECK` (`omg-models healthcheck`). Put it behind a TLS-terminating
proxy for `models.omg.bitop.dev`.

`image.yml` (from CI on `main` and `v*` tags) builds linux/amd64 and
linux/arm64 on native runners, pushes by digest, checks the version, runs
the container contract and Trivy (fixable HIGH/CRITICAL fail), then
publishes a multi-arch index to `ghcr.io/ncecere/omg-models` with SBOM and
provenance and signs it with cosign. Deploy by digest; data changes ship
with the next image.

## Licence

Code: MIT ([LICENSE](LICENSE)). Data: MIT with attribution to its sources;
see [NOTICE.md](NOTICE.md) (the data licence is a pending maintainer
decision, MIT or CC BY 4.0). Inter is under the SIL OFL 1.1.
