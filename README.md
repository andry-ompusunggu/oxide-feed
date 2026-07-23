# 🦀 OxideFeed

**News aggregator & LLM-powered sanitizer.** Fetches RSS feeds, filters articles via keyword rules, validates relevance through **3 Gemini models** distributed round-robin, extracts structured facts, and delivers sanitized news to a Telegram channel.

- **RAM:** < 30 MB RSS (single-threaded tokio + rustls)
- **Cost:** 100% Free Tier (Gemini × 3 models, Telegram Bot API, SQLite)
- **Language:** Rust
- **Throughput:** ~900+ articles/day (vs ~15 with single model)

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
| 🧠 **Multi-Model LLM** | 3 Gemini models round-robin: 3.1 Flash Lite, 3.5 Flash Lite, 3.6 Flash |
| 🔄 **Per-Model RPD Guard** | Each model tracks & respects its own daily quota |
| ⏱️ **Per-Model RPM Throttle** | Auto-spacing per model's rate limit (4s–12s) |
| 📰 **3 RSS Feeds** | Antaranews Ekonomi, Ekonomi-Finansial, Ekonomi-Bursa |
| 🔍 **Keyword Filtering** | 91 whitelist + 30 blacklist keywords + contextual bypass |
| 📎 **Circuit Breaker** | Artikel < 200 chars → skip Gemini, kirim QUICK ALERT |
| ⏱️ **Poll 30 Menit** | Delay maks 29 menit, 48 siklus/hari |
| 📊 **Cycle Summary** | Per-model usage log setelah setiap siklus |
| 💾 **SQLite State** | Watermark, dedup, RPD tracking — all embedded |

## License

MIT
