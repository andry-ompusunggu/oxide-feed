# 🦀 OxideFeed

**News aggregator & LLM-powered sanitizer.** Fetches RSS feeds, filters articles via keyword rules, validates relevance through **2 Gemini models** (Opsi A — Best Quality), extracts structured facts, and delivers sanitized news to a Telegram channel.

- **RAM:** < 30 MB RSS (single-threaded tokio + rustls)
- **Cost:** 100% Free Tier (Gemini × 2 models, Telegram Bot API, SQLite)
- **Language:** Rust
- **Throughput:** ~625 articles/day (Opsi A: 3.5 primary 475 RPD + 3.1 backup 150 RPD)

---

## Quick Start

```bash
cp .env.example .env   # Fill in your API keys
source .env && cargo run
```

> 📖 See **[SETUP.md](./SETUP.md)** for detailed installation and configuration guide.
>
> 🏗️ See **[ARCHITECTURE.md](./ARCHITECTURE.md)** for system design and pipeline documentation.

## Key Features

| Feature | Description |
|---|---|
| 🧠 **Multi-Model LLM** | 2 Gemini models: 3.5 Flash Lite (primary) + 3.1 Flash Lite (backup) — Opsi A |
| 🔄 **Per-Model RPD Guard** | Each model tracks & respects its own daily quota |
| ⏱️ **Per-Model RPM Throttle** | Auto-spacing per model's rate limit |
| 📰 **RSS Feeds** | Configurable via `RSS_FEEDS` env var |
| 🔍 **Keyword Filtering** | Whitelist + blacklist + contextual bypass + daily noise filter |
| 📎 **Circuit Breaker** | Artikel < 200 chars → skip Gemini, kirim QUICK ALERT |
| ⏱️ **Poll 30 Menit** | Delay maks 29 menit, 48 siklus/hari |
| 📊 **Cycle Summary** | Per-model usage log setelah setiap siklus |
| 💾 **SQLite State** | Watermark, dedup, RPD tracking — all embedded |

## License

MIT
