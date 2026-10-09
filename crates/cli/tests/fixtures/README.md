# Recorded source fixtures

Trimmed excerpts of the real upstream files, fetched on 2026-10-09 (UTC),
used by `crates/cli/tests/sync.rs`. Tests never touch the network.

| File | Source | Version at recording |
|---|---|---|
| `litellm.json` | `https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json` (MIT) | commit `de6a0717b7e2fa1ba5b6565d8a9e3ec2223719bc`; entries copied byte-for-byte, including their number text |
| `genai-prices.json` | `https://raw.githubusercontent.com/pydantic/genai-prices/main/prices/new_data/v2/data.json` (MIT) | commit `0f22c2688bc2b1db090eb9df0ab778384fd70b40`; `anthropic`, `aws`, `openai`, `openrouter` providers, relevant models only, `extractors` dropped, re-serialised |
| `openrouter.json` | `https://openrouter.ai/api/v1/models` | fetched 2026-10-09T19:37Z (SHA-256 of the full response `bdc08489…`); relevant models only, `description` dropped |
| `models-dev.json` | `https://models.dev/api.json` (MIT) | ETag `fe1b9957f174077434ba42e396f6a4a4`; four providers, relevant models only |

`litellm.json` also keeps two edge cases: an `xai/…` entry (inclusive
`_above_Nk_tokens` threshold) and `azure/gpt-realtime-whisper` (a per-second
rate that is not a whole micro-USD).

To refresh: run `omg-models sync --dry-run` (it caches the full files under
`.sync/cache/`) and copy the entries you need. Keep the number text exactly as
the source writes it.
