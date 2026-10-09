# Security policy

## Reporting a vulnerability

Please report vulnerabilities **privately**. Don't open a public issue, pull request or discussion.

Use GitHub's private vulnerability reporting: **[Report a vulnerability](https://github.com/ncecere/omg-models/security/advisories/new)** on the repository's Security tab. Only the maintainers see the report, and we work on the fix with you in a private advisory.

Include the affected version or commit, what an attacker can do and what they need first, steps or a proof of concept, and any suggested fix.

The Open Model Catalog has one maintainer. We aim to reply within 5 business days and to agree a disclosure date with you once we understand the problem. We credit reporters in the release notes unless you'd rather we didn't.

## Supported versions

The catalog is pre-1.0. Fixes are made on `main` and deployed from it; only the latest image is supported.

## Dependency and code scanning

Every push and pull request runs, in CI:
- **cargo-deny** (`deny.toml`): RustSec advisories (vulnerable, unmaintained, unsound and yanked crates fail), a licence allow-list compatible with MIT distribution, and crates.io as the only source.
- **clippy** with warnings as errors, the test suite, and **actionlint** on the workflows.
- the container contract (`scripts/container-test.sh`): distroless image without a shell, UID 10001, read-only root filesystem, all capabilities dropped.

Dependabot opens weekly grouped updates for Cargo, GitHub Actions and the `Dockerfile` base images. Release images are scanned by Trivy (fixable HIGH and CRITICAL findings block publishing), carry an SBOM and provenance, and are signed with cosign (keyless). Workflow actions are pinned by commit SHA.

## Scope

In scope:
- the `omg-models` binary (`validate`, `build`, `sync`, `serve`, `healthcheck`) and its web app;
- the container image built from the `Dockerfile`;
- the workflows in `.github/workflows/`, especially `sync.yml`, which fetches third-party data and opens pull requests with `GITHUB_TOKEN`.

Areas we care most about:
- **served content:** script injection or markup injection through catalog data or query parameters, CSP bypasses, open redirects;
- **sync integrity:** a source response that makes `sync` write outside `data/`, corrupt or rewrite existing price history, or get a large price change auto-merged without review;
- **supply chain:** workflow token permissions, unpinned actions, image provenance.

Out of scope:
- the accuracy of a price (report wrong data as a normal issue or pull request; prices are list-price estimates, not invoices);
- the availability or content of upstream sources (LiteLLM, genai-prices, OpenRouter, models.dev, provider pages);
- denial of service by volume alone, and findings that need a compromised host or repository admin access.
