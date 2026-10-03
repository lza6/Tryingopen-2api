# TryingOpen2API

TryingOpen2API reimplements [tryingopen.com](https://www.tryingopen.com) free-tier open-source models as an **OpenAI-compatible** and **Anthropic-compatible** local API gateway in Rust (axum). Single binary, zero external dependencies. Works with any OpenAI/Claude client (Claude Code, Codex, Cursor, LobeChat, NextChat).

**Fully anonymous**: no cookie / login / API key needed. The site rate-limits ~20 requests per IP per 24h UTC day, so the gateway ships a **proxy pool with automatic failover** (residential proxy file + free-proxy fetcher dual source, 429 cooldown + rotation, exponential backoff, direct fallback with quota).

**v0.1.17 highlights**
- Panel polling merged into one parallel refresh (refreshPanel + Promise.allSettled, no more double-refresh flicker)
- Mobile adaptation: sticky table header, ≥44px touch targets at ≤640px, colspan fixes
- Dynamic catalog refresh preserves static `message_limit` / `cheaper_fallback` fallback meta (kimi messageLimit=5 no longer wiped)
- Security: panel key-injection XSS fix (character whitelist + `<`/`>` JSON escaping), rate-limit attribution anti-spoofing (`x-api-key` wins when both headers present), log-masking regex now covers `sk-to-<uuid>` hyphen keys
- Benchmark baseline refreshed (3 endpoints 200/200, p50 10-12ms) + lto thin/fat comparison (kept thin)
- Earlier (≤v0.1.16): model capability passthrough (`reasoning`/`message_limit`/`cheaper_fallback`), 429 auto-downgrade to upstream's suggested cheaper model, `GET /api/usage`, structured JSON request logs (key-masked), `config.local.json` deep-merge, reasoning badge / downgrade chips / cumulative request count, Anthropic `thinking` parameter mapping, actionable 502 errors

Endpoints: /v1/chat/completions, /v1/messages, /v1/responses, /v1/models, /healthz, /metrics (auth), /api/proxies, /api/proxies/refresh-free, /api/catalog/refresh, /api/guide, /api/usage, /api/config/api-key, /ui. Tests: 65 (fmt/clippy/65 tests green).

- Upstream: https://www.tryingopen.com
- Default listen: http://127.0.0.1:47831
- Protocol reverse notes: docs/PROTOCOL.md
- Chinese README: README.md
