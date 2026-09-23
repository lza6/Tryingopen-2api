# TryingOpen2API

TryingOpen2API reimplements [tryingopen.com](https://www.tryingopen.com) free-tier open-source models as an **OpenAI-compatible** and **Anthropic-compatible** local API gateway in Rust (axum). Single binary, zero external dependencies. Works with any OpenAI/Claude client (Claude Code, Codex, Cursor, LobeChat, NextChat).

**Fully anonymous**: no cookie / login / API key needed. The site rate-limits ~20 requests per hour per IP, so the gateway ships a **proxy pool with automatic failover** (residential proxy file + free-proxy fetcher dual source, 429 cooldown + rotation, exponential backoff, direct fallback).

- Upstream: https://www.tryingopen.com
- Default listen: http://127.0.0.1:47831
- Protocol reverse notes: docs/PROTOCOL.md
- Chinese README: README.md
