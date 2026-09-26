# TryingOpen2API

TryingOpen2API reimplements [tryingopen.com](https://www.tryingopen.com) free-tier open-source models as an **OpenAI-compatible** and **Anthropic-compatible** local API gateway in Rust (axum). Single binary, zero external dependencies. Works with any OpenAI/Claude client (Claude Code, Codex, Cursor, LobeChat, NextChat).

**Fully anonymous**: no cookie / login / API key needed. The site rate-limits ~20 requests per IP per 24h UTC day, so the gateway ships a **proxy pool with automatic failover** (residential proxy file + free-proxy fetcher dual source, 429 cooldown + rotation, exponential backoff, direct fallback with quota).

**v0.1.13 highlights**
- Model capability passthrough: `reasoning` / `message_limit` / `cheaper_fallback` in /v1/models (reflects upstream catalog)
- 429 auto-downgrade to upstream's suggested cheaper model (rotates egress)
- Per-API-key usage tracking: `GET /api/usage` (bounded in-memory)
- Structured JSON request logs (key-masked)
- `config.local.json` deep-merge override
- Web panel: reasoning badge / downgrade chips / cumulative request count

Endpoints: /v1/chat/completions, /v1/messages, /v1/responses, /v1/models, /healthz, /metrics (auth), /api/proxies, /api/usage, /ui. Tests: 57 (fmt/clippy/57 tests green).

- Upstream: https://www.tryingopen.com
- Default listen: http://127.0.0.1:47831
- Protocol reverse notes: docs/PROTOCOL.md
- Chinese README: README.md
