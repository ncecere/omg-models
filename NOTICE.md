# Notices and attribution

## Licences

- **Code** (everything outside `data/`): MIT, see [LICENSE](LICENSE).
- **Catalog data** (`data/`, and the JSON API built from it): MIT, the same
  licence and copyright notice as the code, with the attribution below.
  Prices and limits are facts published by the providers; the automated
  sources the catalog draws on publish their data under MIT. Keep this
  notice (or a link to <https://models.omg.bitop.dev/about>) when you
  redistribute the data.

> **Decision pending:** the data licence is proposed as MIT (all automated
> inputs are MIT). CC BY 4.0 with source attribution is the alternative the
> maintainer may choose instead. Update this file, `README.md`, the `/about`
> page and `api/v1/index.json` (`license.data`) together.

## Data sources

| Source | Licence | Use |
|---|---|---|
| [BerriAI/litellm](https://github.com/BerriAI/litellm) `model_prices_and_context_window.json` | MIT, Copyright (c) 2023 Berri AI | Primary price source for `openai`, `anthropic`, `amazon-bedrock` offerings. Entries record the commit, SHA-256 and entry key. |
| [pydantic/genai-prices](https://github.com/pydantic/genai-prices) v2 data | MIT, Copyright (c) Pydantic Services Inc. 2025 to present | Cross-check only; never written to the catalog. |
| [OpenRouter models API](https://openrouter.ai/api/v1/models) | OpenRouter terms of service | Prices for the `openrouter` provider only. |
| [models.dev](https://github.com/anomalyco/models.dev) `api.json` | MIT, Copyright (c) 2025 models.dev | Missing model metadata only (release dates, knowledge cutoffs, limits). |
| Provider pricing pages and price lists (Anthropic, OpenAI, AWS Price List) | Provider terms | Seed prices and verification links; each price entry links its page. |

Not used: Bifrost's datasheet (no stated data licence), Helicone (stale and
unavailable), tokencost (a stale LiteLLM copy) and Artificial Analysis
(requires a key and a commercial agreement for redistribution).

## Bundled assets

- Inter Variable 4 (`crates/web/assets/fonts/`): SIL Open Font License 1.1,
  Copyright 2016 The Inter Project Authors; see `crates/web/assets/fonts/OFL.txt`.
- The Portal logo and favicons (`crates/web/assets/brand/`): from the Open
  Model Gateway project (`omg-assets/logo/portal/`), used unmodified.
- Topcoat UI components (`crates/web/src/components/`) and theme
  (`crates/web/styles.css`): copied from [tokio-rs/topcoat](https://github.com/tokio-rs/topcoat)
  0.10.0, MIT; edited locally (see the file headers and `components.toml`).
- `crates/web/brand.css`: the Open Model Gateway brand tokens, a verbatim
  copy of `omg-website/app/brand.css`.
