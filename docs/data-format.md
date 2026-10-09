# Data format

The catalog is plain TOML under `data/`. `omg-models validate` checks it,
`omg-models build` turns it into the JSON API, and `omg-models schema` prints
a JSON Schema for both file types (also served at `/api/v1/schema.json`).

```text
data/
  providers/
    <provider>/
      provider.toml
      models/
        <model>.toml
```

- A **provider** is somewhere you call models: a first-party API (`openai`,
  `anthropic`), a cloud platform (`amazon-bedrock`) or an aggregator
  (`openrouter`).
- A **model** is one model, defined once, under its **home provider**
  (usually its maker's API; for open-weights models served only elsewhere,
  the provider that serves it). Its **offerings** list every provider that
  serves it, with the id that provider expects and that provider's prices.
- Dates are quoted strings: `"2026-10-09"`. Timestamps are UTC:
  `"2026-10-09T19:33:00Z"`. Unknown fields are errors.

## `provider.toml`

```toml
id = "amazon-bedrock"            # must equal the directory name; [a-z0-9-]
name = "Amazon Bedrock"
type = "cloud"                   # cloud | aggregator | self-hosted
website = "https://aws.amazon.com/bedrock/"
api_base = "https://bedrock-runtime.us-east-1.amazonaws.com"  # https (http only for self-hosted)
api_style = "bedrock-converse"   # openai-chat | openai-responses | anthropic-messages | bedrock-converse
auth = "AWS Signature Version 4 with IAM credentials, ..."    # prose, never a secret
env = ["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_REGION"]
docs = "https://docs.aws.amazon.com/bedrock/latest/userguide/"
pricing_page = "https://aws.amazon.com/bedrock/pricing/"

[sync]                           # optional; see "Sync" below
prices = "litellm"               # none | litellm | openrouter
litellm_providers = ["bedrock", "bedrock_converse"]
genai_prices_provider = "aws"    # cross-check source provider id
models_dev_provider = "amazon-bedrock"
```

`api_style` selects the API example on model pages.

## `models/<model>.toml`

```toml
id = "claude-haiku-5-5"          # must equal the file name; unique in the catalog; [a-z0-9._-]
name = "Claude Haiku 5.5"
vendor = "Anthropic"             # who made it
family = "claude-haiku"          # optional
description = "..."              # optional
release_date = "2026-10-07"      # optional
knowledge_cutoff = "2026-06"     # optional, YYYY-MM or YYYY-MM-DD
open_weights = false
license = "Apache-2.0"           # optional, for open-weights models
docs = "https://..."             # optional
reasoning = true                 # optional
tool_call = true                 # optional
status = "active"                # active | deprecated | retired
deprecation_date = "2027-01-01"  # required when deprecated
retirement_date = "2027-06-01"   # required when retired

[modalities]                     # text | image | audio | video | pdf | embedding
input = ["text", "image"]
output = ["text"]

[limits]                         # tokens; omit what you don't know (never 0)
context = 1000000
input = 922000                   # only when smaller than the context window
output = 128000

[[offerings]]
provider = "amazon-bedrock"
upstream_id = "global.anthropic.claude-haiku-5-5"   # exactly what you send
label = "Global cross-Region inference"             # optional qualifier
aliases = []                     # other ids for the same deployment and price
limits = { output = 64000 }      # optional per-offering override
sync = { litellm = "..." }       # optional source-key overrides (see "Sync")

[[offerings.prices]]
effective_from = "2026-10-09"
currency = "USD"
standard = { input_tokens = "0.1", output_tokens = "0.5", cache_read_tokens = "0.01", cache_write_5m_tokens = "0.125", cache_write_1h_tokens = "0.2" }
standard_tiers = [
  { above_prompt_tokens = 100000, rates = { input_tokens = "0.5", output_tokens = "2.5" } },
]
batch = { input_tokens = "0.05", output_tokens = "0.25" }
batch_tiers = [
  { above_prompt_tokens = 100000, rates = { input_tokens = "0.25", output_tokens = "1.25" } },
]
not_applicable = ["output_images"]   # optional: meters that cannot apply
source = { kind = "provider-price-list", url = "https://...", fetched_at = "2026-10-09T19:52:00Z", version = "20261008235637", sha256 = "c63d...", entry_key = "...", cites = "https://aws.amazon.com/bedrock/pricing/" }
notes = "..."
```

Cloud platforms often price the same model differently per endpoint type
(Bedrock `global.` versus `us.` inference profiles differ by 10%). Those are
separate offerings: never strip a geo prefix.

### Meters and units

Every rate is an **exact decimal string** in USD per the meter's unit. Floats
(`2.5` without quotes), signs, exponents and separators are rejected.

| Meter | Unit | OMG v3 meter, batch |
|---|---|---|
| `input_tokens` | per 1M tokens (uncached input) | `input_tokens`, 1,000,000 |
| `output_tokens` | per 1M tokens (reasoning included) | `output_tokens`, 1,000,000 |
| `cache_read_tokens` | per 1M tokens | `cache_read_tokens`, 1,000,000 |
| `cache_write_tokens` | per 1M tokens, provider default TTL | `cache_write_tokens`, 1,000,000 |
| `cache_write_5m_tokens` | per 1M tokens, 5-minute TTL | `cache_write_5m_tokens`, 1,000,000 |
| `cache_write_1h_tokens` | per 1M tokens, 1-hour TTL | `cache_write_1h_tokens`, 1,000,000 |
| `input_characters` | per 1M characters | `input_characters`, 1,000,000 |
| `output_images` | per image | `output_images`, 1 |
| `input_audio_seconds` | per second | `input_audio_seconds_ms`, 1,000 |
| `output_audio_seconds` | per second | `output_audio_seconds_ms`, 1,000 |
| `search_units` | per search (e.g. $10/1,000 searches = `"0.01"`) | `search_units`, 1 |
| `requests` | per request | `requests`, 1 |

Claude-style caches price 5-minute and 1-hour writes separately: use
`cache_write_5m_tokens`/`cache_write_1h_tokens`. Use `cache_write_tokens`
when the provider has one write price.

**Unknown is not zero.** Leave out a meter (or a whole price entry) you
cannot verify. `"0"` means free. `not_applicable` means the meter cannot apply
to this offering.

### Tiers

`standard_tiers`/`batch_tiers` replace the base rates for the **whole
request** when the prompt (all input tokens, cache reads and writes included)
is **strictly greater** than `above_prompt_tokens`; the highest exceeded tier
wins. Thresholds must ascend, tiers apply to token meters only, and every
tier meter needs a base rate. This is OMG's `min_prompt_tokens`.

### Batch

`batch` holds the rates a provider publishes for its batch API. They are
never derived from standard rates. A batch card may cover fewer meters than
`standard` (some providers publish batch input/output only); the OMG export
then reports `batch_complete: false`, because the gateway requires batch
lines to cover the same meters.

### Provenance and history

`source.kind` is one of `manual`, `provider-page`, `provider-price-list`,
`litellm`, `genai-prices`, `openrouter`, `models.dev`.

- `url` (https) and `fetched_at` are always required; `manual` also needs
  `notes`.
- Dataset kinds (`litellm`, `genai-prices`, `openrouter`, `models.dev`)
  need `entry_key`; `litellm` and `genai-prices` also need `version` (git
  commit) and `sha256` (of the fetched file).
- `cites` is the provider page the source itself cites.
- `carried_over` lists meters copied unchanged from the previous entry
  because the source did not list them.

Price history is **append-only**: to change a price, add a new
`[[offerings.prices]]` entry with a later (or equal, with a later
`fetched_at`) `effective_from`. Never edit or delete an old entry; the
validator rejects out-of-order history. The last entry is the current
price. `effective_from` is the date the price was first observed unless the
provider states a start date.

## Validation

`omg-models validate` reports every problem with its file and location:

- schema: required fields, types, unknown fields, enum values;
- ids: format, file/directory name match, unique model ids, one upstream id
  (or alias) per provider across the catalog;
- references: offering providers exist; each model offers through its home
  provider and has at least one offering;
- decimals: exact, non-negative; values that are not a whole micro-USD per
  OMG batch are **warnings** (the OMG export rounds them up and flags them);
- dates: valid calendar dates, history in order, lifecycle dates
  consistent with `status`;
- tiers: ascending, token meters only, base rates present;
- limits: positive, input/output not above context;
- provenance: present, https, kind-specific fields.

`--strict` turns warnings into errors.

## JSON outputs

`omg-models build --out dist` writes (deterministic: sorted keys, stable
order, no build timestamps):

| File | Contents |
|---|---|
| `api.json` | models.dev shape: `{provider: {id, name, env, api, doc, models: {<upstream id>: {id, name, catalog_id, family, cost, limit, modalities, release_date, knowledge, open_weights, ...}}}}` |
| `api/v1/index.json` | counts, `last_updated`, licence, endpoint list |
| `api/v1/providers.json`, `api/v1/providers/<id>.json` | providers; per provider its offerings with current prices |
| `api/v1/models.json`, `api/v1/models/<id>.json` | models with offerings; per model the full price history and provenance |
| `api/v1/omg-prices.json` | OMG Pricing v3 lines per offering (current price) |
| `api/v1/history.json` | every price entry, newest first, `superseded` flag |
| `api/v1/schema.json` | the JSON Schema of the data files |

### Deviations from models.dev in `api.json`

- Costs are JSON numbers written with the exact decimal text (no float
  round trip); unknown costs are omitted rather than zero.
- `cost.cache_write` is the 5-minute write when an offering prices TTLs
  separately (models.dev's convention for Claude); 1-hour writes, batch
  rates and non-token meters are only in `/api/v1/`.
- `cost.tiers[].tier.size` means "prompt strictly greater than size".
- Models are keyed by the provider's wire id (Bedrock profile ids keep their
  prefix); `catalog_id` links back to `/api/v1/models/<id>.json`. Offerings
  with a `label` get it appended to `name`.
- `last_updated` is the current price's `effective_from`. There are no
  `npm`, `attachment`, `temperature` or `structured_output` fields.

### OMG Pricing v3 export

`omg-prices.json` has one item per offering with a current price:

```json
{
  "model": "claude-haiku-5-5", "provider": "anthropic", "upstream_id": "claude-haiku-5-5",
  "effective_from": "2026-10-09",
  "price_lines": [
    {"meter": "input_tokens", "microusd_per_batch": "100000", "batch": 1000000,
     "unit_label": "/M tokens", "sku_label": "Input", "source_usd_per_unit": "0.1"},
    {"meter": "input_tokens", "microusd_per_batch": "500000", "batch": 1000000,
     "unit_label": "/M tokens", "sku_label": "Input", "source_usd_per_unit": "0.5",
     "min_prompt_tokens": 100000}
  ],
  "batch_price_lines": [...], "batch_complete": false, "rounded_up": false,
  "limits": {"context": 1000000, "input": null, "output": 128000},
  "provenance": {...}
}
```

`microusd_per_batch` = USD-per-unit rate × 10^6, computed with integer
decimal arithmetic. When the result is not an integer it is rounded **up**
and the line carries `"rounded_up": true`. Not-applicable meters export as
`{"meter": ..., "not_applicable": true}`.

## Sync

`omg-models sync` appends prices from upstream sources (see the README's
"Sync policy"). Per provider, `[sync] prices` picks the primary source;
offerings are looked up by `upstream_id` unless `[offerings.sync]` overrides
the key (`litellm`, `genai_prices`, `openrouter`, `models_dev`) or sets
`disabled = true`. Meters the primary source does not list are carried over
into the new entry and named in `source.carried_over`.
