# 🏗️ OxideFeed — Architecture

**OxideFeed** is a lightweight, single-threaded news aggregator and LLM-powered sanitizer written in Rust. It fetches RSS feeds, filters articles via keyword rules (kategoris + contextual pair matching), validates relevance through **2 Gemini models (Opsi A)**, extracts structured facts, and delivers sanitized news to a Telegram channel.

**RAM Target:** < 30 MB RSS (50 MB cgroup limit recommended) | **Cost:** 100% Free Tier (Gemini × 2 models, Telegram Bot API, SQLite)

---

## Core Design Principles

- **Single-threaded async** (`current_thread` tokio) — minimal CPU context switching on a dev laptop
- **Multi-Model Fleet (Opsi A)** — 2 Gemini models: 3.5 Flash Lite (primary, 475 RPD) + 3.1 Flash Lite (backup, 150 RPD) — round-robin dengan prioritas
- **Per-Model RPD Guard** — Setiap model melacak pemakaian hariannya sendiri, skip model yang sudah cap
- **Per-Model RPM Throttle** — Rate limiter otomatis sesuai limit masing-masing model (15 RPM → 4s)
- **Embedded storage** (SQLite via `rusqlite`) — no external database daemon
- **Rustls TLS** — no OpenSSL dependency, lower memory footprint
- **Per-item watermark** — state commits only after successful Telegram delivery (HTTP 200 OK)
- **Graceful degradation** — Gemini failures or RPD exhaustion fall back to raw text buffer, never crash
- **Short Content Circuit Breaker** — artikel < 200 chars skip Gemini, kirim QUICK ALERT langsung

---

## System Architecture

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                         OxideFeed Engine                                     │
│              ┌──────────────────────────────────────────────┐                │
│              │         main.rs (Runtime)                      │               │
│              │  #[tokio::main(flavor="current_thread")]      │               │
│              │  [ModelRouter] [Per-Model RPD] [Prioritasi]  │               │
│              │        30-min loop cycle (default)            │               │
│              └──────────┬───────────────────────────────────┘                │
│                         │                                                    │
│         ┌───────────────┼───────────────┬────────────────┐                  │
│         ▼               ▼               ▼                ▼                  │
│   ┌──────────┐   ┌──────────┐   ┌────────────────┐   ┌──────────────┐      │
│   │ ingest   │   │ storage  │   │   llm.rs       │   │  telegram    │      │
│   │ .rs      │   │ .rs      │   │  ModelRouter   │   │  .rs         │      │
│   │ RSS+     │   │ SQLite   │   │  ┌────────────────┐│   │ Bot API      │      │
│   │ Scraper  │   │ State    │   │  │ 3.5 (PRIMARY)  ││   │ MarkdownV2   │      │
│   │ Filter   │   │ Per-Model│   │  ├────────────────┤│   │ Quick Alert  │      │
│   │          │   │ RPD      │   │  │ 3.1 (BACKUP)   ││   │              │      │
│   │          │   │ Tracker  │   │  └────────────────┘│   │              │      │
│   │          │   │          │   │                     │   │              │      │
│   │          │   │          │   │  └──────────┘  │   │              │      │
│   └──────────┘   └──────────┘   └────────────────┘   └──────────────┘      │
│                         │                                                    │
│         ┌───────────────┘                                                    │
│         ▼                                                                    │
│   ┌──────────────────┐                                                       │
│   │ config.rs        │  Environment variable loader + GeminiModelConfig[]    │
│   │                  │  ContextualPair cache built once at startup           │
│   └──────────────────┘                                                       │
└──────────────────────────────────────────────────────────────────────────────┘
```

---

## Data Pipeline Lifecycle

```
[Loop Triggered: Every N Minutes (default 30)]
│
▼
┌──────────────────────────────────────┐
│  1. Read Watermarks from SQLite      │  ← Get last checkpoint per feed
└──────────────┬───────────────────────┘
               ▼
┌──────────────────────────────────────┐
│  2. Fetch & Parse RSS Feeds          │  ← Parallel requests via reqwest
└──────────────┬───────────────────────┘
               ▼
┌──────────────────────────────────────┐
│  3. Local Filter (Stage 1)           │
│  • Skip if pub_date ≤ watermark      │
│  • Skip if hash in processed_news    │
│  • Whitelist / Blacklist match       │
│    (6 kategori + contextual bypass)  │
└──────┬───────────────────────────────┘
       │
  ┌────┴────┐
  │ FAIL    │ ←── Mark hash → Skip forever
  └─────────┘
       │ PASS
       ▼
┌──────────────────────────────────────┐
│  4. Scrape Full Article Body         │  ← CSS selectors
│      (or fallback to RSS summary)    │  ← Min. 200 chars
└──────┬───────────────────────────────┘
       │
  ┌────┴──────────────────────┐
  │  < 200 chars              │  ← Short Content Circuit Breaker
  │  → Kirim ⚡ QUICK ALERT   │
  │  → Mark hash → Lanjut     │
  └────┬──────────────────────┘
       │ ≥ 200 chars
       ▼
┌───────────────────────────────────────────────┐
│  5. Model Selection (Round-Robin)             │
│  • Router.select_model(|name| get_usage(name))│
│  • Skip model jika sudah RPD cap              │
│  • Jika SEMUA model cap → RAW BACKUP          │
└──────┬────────────────────────────────────────┘
       │ Ada model tersedia
       ▼
┌──────────────────────────────────────┐
│  6. Per-Model RPM Throttle           │
│  • enforce_rate_limit()              │
│  • Tunggu sesuai RPM masing-masing   │
└──────┬───────────────────────────────┘
       ▼
┌──────────────────────────────────────┐
│  7. LLM Processing (Stage 2)         │
│     Gating + sanitasi 1 prompt       │
│     {"is_important":false,...}        │ ← Reject
│     {"is_important":true,"data":{...}}│ ← Process
└──────┬───────────────────────────────┘
       │
  ┌────┴──────────────────────────┐
  │ false → Mark hash → Skip     │
  │ true  → Send to Telegram     │
  │ Parse fail → RAW BACKUP      │
  └────┬──────────────────────────┘
       ▼
┌──────────────────────────────────────┐
│  8. Telegram Dispatch                │
└──────┬───────────────────────────────┘
       │
  ┌────┴─────────────────────────┐
  │ HTTP 200 OK → commit state   │
  │ HTTP Error  → skip commit    │
  └────┬─────────────────────────┘
       ▼
  ┌──────────┐
  │  Done ✔  │
  └──────────┘
```

---

## Component Details

### 1. Core Runtime — `src/main.rs`

- **Runtime:** `#[tokio::main(flavor = "current_thread")]` — single OS thread
- **Loop:** Infinite `loop { process_cycle(); sleep(interval); }`
- **Error Handling:** Top-level `log::error!` — never panics during the processing loop
- **Log Timestamps:** WIB (UTC+7) di log, UTC di database
- **Startup Notification:** 🤖 OxideFeed vX started — now includes model count & total RPD
- **Error Notification:** Jika cycle gagal total, kirim `⚠️` ke Telegram
- **Model Selection:** `model_router.select_model(|name| get_usage(name))` — round-robin dengan skip RPD cap
- **Per-Model RPM:** Setiap model punya timer sendiri (`enforce_rate_limit()`), interval sesuai RPM masing-masing
- **Per-Model RPD Usage:** Dicatat di tabel `model_daily_usage(day, model_name)` — increment SEBELUM call
- **All-Model Cap:** Jika semua model mencapai RPD limit → RAW BACKUP dengan info total usage
- **Cycle Summary:** Setelah setiap siklus, log per-model usage + total fleet usage
- **Short Content Circuit Breaker:** < 200 chars → ⚡ QUICK ALERT
- **Artikel Prioritas:** Newest-first, dibatasi `max_articles_per_cycle`
- **State Guarantee:** Watermark & processed hashes hanya ditulis setelah HTTP 200 OK dari Telegram
- **Periodic DB Maintenance:** Every 10 cycles: VACUUM + cleanup old records

### 2. State Manager — `src/storage.rs`

**Database:** `oxide_feed.db` (SQLite, auto-created)

```sql
-- Deduplication & historical skip
CREATE TABLE processed_news (
    id          TEXT PRIMARY KEY,           -- SHA-256 of URL or RSS GUID
    title       TEXT NOT NULL,
    processed_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))  -- ISO 8601 UTC
);

CREATE INDEX IF NOT EXISTS idx_processed_at ON processed_news(processed_at);

-- Per-feed checkpoint for catch-up
CREATE TABLE rss_states (
    feed_url               TEXT PRIMARY KEY,
    last_fetched_pub_date  TEXT NOT NULL   -- ISO 8601 UTC
);

-- Per-model daily API usage tracking (RPD Guard — multi-model)
CREATE TABLE model_daily_usage (
    date        TEXT NOT NULL,              -- 'YYYY-MM-DD'
    model_name  TEXT NOT NULL,              -- 'gemini-3.1-flash-lite', etc.
    call_count  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (date, model_name)
);
```

**Key behavior:**
- On **first boot**: depends on config:
  - `ONBOARDING_COUNT > 0`: process up to N existing articles, then set watermark (instant results)
  - `ONBOARDING_COUNT = 0` (default): watermark = now, skip all existing items (prevents token drain)
- On **catch-up** (after offline period): processes items from watermark to now, **newest first**, max `MAX_ARTICLES_PER_CYCLE`
- Watermark updates **per-item**, not per-batch — undelivered items are retried next cycle
- **Auto-vacuum:** `cleanup_old_processed(days)` deletes records older than N days; `VACUUM` reclaims disk
- **Per-Model RPD Tracking:** `get_today_api_usage(model_name)` + `increment_api_usage(model_name)` — setiap model punya counter sendiri
- **Total Fleet Usage:** `get_total_today_api_usage()` — SUM semua model untuk logging cycle summary
- **Cleanup:** `cleanup_old_usage_records()` — hapus record > 30 hari (dari tabel `model_daily_usage`)

### 3. Ingestion — `src/ingest.rs`

**RSS Parsing:** `feed-rs` crate — handles RSS 2.0, Atom, JSON Feed

**Whitelist Kategoris** (must match ≥ 1, loaded from `OXIDE_WHITELIST` env var):

| Grup | Keywords |
|---|---|
| **Makro & Moneter** | `suku bunga`, `bi rate`, `inflasi`, `rupiah`, `nilai tukar`, `pajak`, `fiskal`, `moneter`, `devisa`, `cadangan devisa`, `apbn`, `pnbp`, `defisit`, `utang negara`, `pertumbuhan ekonomi`, `gdp`, `pdb`, `konsumsi rumah tangga`, `daya beli` |
| **Pasar Modal & Korporasi** | `saham`, `ihsg`, `emiten`, `dividen`, `akuisisi`, `merger`, `ipo`, `obligasi`, `restrukturisasi`, `right issue`, `bei`, `bursa`, `kustodian`, `reksadana`, `sukuk` |
| **Regulasi & Pemerintah** | `uu`, `perpu`, `perpres`, `permen`, `permendag`, `permenkeu`, `kebijakan`, `tarif`, `regulasi`, `pemerintah`, `dpr`, `kpk`, `mk`, `mahkamah konstitusi`, `omnibus law`, `cipta kerja` |
| **Energi & SDA** | `energi`, `bbm`, `pertamina`, `pln`, `minyak`, `gas bumi`, `batubara`, `nikel`, `timah`, `sawit`, `harga energi`, `tarif listrik`, `subsidi energi` |
| **Bencana & Krisis** | `bencana`, `gempa`, `tsunami`, `banjir`, `letusan`, `wabah`, `pandemi`, `covid`, `darurat`, `evakuasi`, `korban jiwa`, `kerusakan`, `longsor`, `kekeringan` |
| **Transportasi & Infra** | `transjakarta`, `mrt`, `lrt`, `krl`, `tol`, `infrastruktur`, `bandara`, `pelabuhan`, `jalan`, `kereta cepat`, `whoosh`, `ikn`, `ibu kota negara` |

**Blacklist** (must match 0, loaded from `OXIDE_BLACKLIST` env var):

| Grup | Keywords |
|---|---|
| **Clickbait** | `viral`, `viral di media sosial`, `viral di tiktok`, `viral di twitter` |
| **Lifestyle & Gosip** | `menikah`, `selingkuh`, `pacar`, `artis`, `pacaran`, `putus`, `gimmick`, `fakta menarik`, `biodata`, `profil`, `harta kekayaan`, `intip gaya`, `penampilan`, `transformasi`, `fashion`, `makeup`, `outfit`, `ootd` |
| **Netizen & Ujaran** | `netizen`, `hujat`, `komen`, `komentar`, `warganet`, `medsos`, `media sosial`, `ramai di` |

**Contextual Pair Matching:** Blacklist word yang biasanya memblock artikel, bisa di-bypass jika muncul bersama whitelist partner:

| Blacklist Word | Partner Whitelist | Contoh Judul |
|---|---|---|
| `rumor` | `akuisisi`, `merger`, `investasi`, `saham`, `emiten`, `bank`, `go public`, `buyback` | *"Rumor Akuisisi Bank X oleh Investor Asing"* ✅ LOLOS |
| `isyaratkan` | `suku bunga`, `bi rate`, `inflasi`, `pemerintah` | *"BI Isyaratkan Suku Bunga Naik"* ✅ LOLOS |
| `sinyal` | `ekonomi`, `pasar`, `saham`, `ihsg`, `regulasi` | *"Sinyal Ekonomi Positif dari Pemerintah"* ✅ LOLOS |

> Contextual pairs di-cache di `Config` struct saat startup (zero-cost, tidak dialokasi ulang per artikel).

**Scraper Fallback:**
1. Try CSS selectors: `article`, `div.content`, `div.read__content`, `.article-content`, `.post-content`, `main`
2. If extracted text < **200 chars** → **Circuit Breaker aktif**: Skip Gemini, kirim ⚡ QUICK ALERT ke Telegram
3. Scrape gagal total → gunakan RSS `<description>` / `<summary>` sebagai fallback

**Deduplication:**
- SHA-256 hash of article URL (or RSS GUID as fallback)
- **Cross-feed dedup:** title hash tracked across all feeds — artikel sama dari feed berbeda hanya diproses sekali
- **Mark & skip:** artikel yang ditolak filter langsung dicatat di DB agar tidak diproses ulang

### 4. LLM Client & ModelRouter — `src/llm.rs`

**Model Fleet (Free Tier):**

| # | Model (Opsi A) | RPM | RPD | Cap (95%) | Interval |
|---|-------|:---:|:---:|:---------:|:--------:|
| 1 | Gemini 3.5 Flash Lite **(PRIMARY)** | 15 | 500 | **475** | 4s |
| 2 | Gemini 3.1 Flash Lite **(BACKUP)** | 15 | 500 | **150** | 4s |
| | **Total Fleet** | **30** | **1000** | **625** | — |

**ModelRouter — Round-Robin Distribution:**

```rust
pub struct ModelRouter {
    models: Vec<LlmClient>,
    current_index: AtomicUsize,
}

impl ModelRouter {
    // Round-robin, skip yang sudah RPD cap
    pub fn select_model<F>(&self, daily_usage_fn: F) -> Option<&LlmClient>
    where F: Fn(&str) -> usize;

    // Query semua model, skip yang usage >= rpd_limit
    pub fn total_rpd_limit(&self) -> usize;
}
```

**Per-Model LlmClient:**
Setiap instance punya:
- `config.api_base` — URL spesifik model (berbeda tiap model)
- `last_call: Mutex<Option<Instant>>` — timer RPM sendiri
- `process_article()` — gating + sanitasi dalam 1 call

**Per-Model Rate Limiting (`enforce_rate_limit`):**
```rust
// Gemini 3.5 Flash Lite (15 RPM, PRIMARY): interval 4s — mendapat mayoritas artikel
// Gemini 3.1 Flash Lite (15 RPM, BACKUP):  interval 4s — hanya jika 3.5 RPD cap
```

**Combined Pipeline (1 API call per article per model):**

```json
// Tidak relevan:
{ "is_important": false, "reason_if_rejected": "..." }

// Relevan:
{
  "is_important": true,
  "data": {
    "topik": "TARIF PNBP NAIK",
    "kategori": "regulasi",
    "fakta_keras": ["..."],
    "signifikansi": "tinggi",
    "relevansi": "..."
  }
}
```

**Resilience:**
- Satu model down → router coba model berikutnya
- Semua model down → RAW BACKUP
- Satu model cap RPD → lewati, coba model lain
- Semua model cap RPD → RAW BACKUP dengan info total fleet usage

### 5. Telegram Dispatcher — `src/telegram.rs`

**Message Types:**

| Method | Format | Trigger |
|---|---|---|
| `send_news` | 📢 MarkdownV2 terstruktur | Gemini approved + valid JSON |
| `send_raw_backup` | 📄 Teks mentah | Gemini invalid JSON / RPD limit |
| `send_quick_alert` | ⚡ Mini alert + link | Short content circuit breaker (< 200 chars) |
| `send_notification` | 🤖 Plain text | Startup / error notification |

**News Template (MarkdownV2):**
```
📢 *[TOPIK]*
[kategori] · [🔴/🟡/🟢] [signifikansi]

*Fakta Keras:*
• [fakta_keras item 1]
• [fakta_keras item 2]

*Relevansi:*
[relevansi]

🔗 [Original URL]
```

**Quick Alert Template:**
```
⚡ [QUICK NEWS]

*[Judul]*
_(Gagal memuat teks lengkap. Buka tautan untuk membaca lebih lanjut.)_

🔗 [Link]
```

**Error Handling:** Returns `Err` on non-200 → caller skips state commit

---

## Multi-Model RPD Guard System (Opsi A)

### Cara Kerja

1. **Per-model daily tracking:** Setiap panggilan Gemini dicatat di tabel `model_daily_usage` dengan composite key `(date, model_name)`
2. **Model Selection:** `ModelRouter::select_model(|name| get_usage(name))` — round-robin, skip model yang sudah ≥ `rpd_limit`
3. **Prioritas:** 3.5-flash-lite (RPD 475) mendapat mayoritas artikel. 3.1-flash-lite (RPD 150) sebagai backup jika 3.5 cap.
4. **Jika semua model cap:** Artikel dikirim sebagai RAW BACKUP dengan info total fleet usage. Tidak ada berita hilang.
5. **Increment:** `increment_api_usage(model_name)` dipanggil SEBELUM request (konservatif — prevent quota overrun)
6. **Cycle Summary:** Setelah siklus selesai, log per-model usage + total fleet

### Fleet Capacity

| Model | RPD Cap | RPM | Per-Cycle (48 siklus) |
|---|---|---:|---:|
| Gemini 3.5 Flash Lite **(PRIMARY)** | 475 | 15 | ~10 |
| Gemini 3.1 Flash Lite **(BACKUP)** | 150 | 15 | ~3 |
| **Total Fleet** | **625** | **30** | **~13** |

### Skenario Catch-Up (Opsi A)

```
Laptop mati 3 hari → 45 artikel menumpuk
→ Artikel 1: 3.5 (PRIMARY) → approve
→ Artikel 2: 3.1 (BACKUP) → approve
→ Artikel 3: 3.5 (PRIMARY) → reject
...
→ 3.1 mencapai 150 RPD cap → hanya 3.5 yang jalan
→ 3.5 mencapai 475 RPD cap → RAW BACKUP untuk sisanya
→ Mayoritas artikel diproses 3.5 (filter lebih presisi)
```

**Perbandingan dengan single model:**
| Metrik | Single Model (sebelum) | Multi-Model Opsi A (sekarang) |
|---|---|---|
| RPD Total | 15 (cap) | **625** (fleet) |
| Artikel/cycle | ~5 | **~15** |
| Artikel/hari | ~15 | **~625** |
| Model digunakan | 1 | **2** (3.5 primary + 3.1 backup) |
| RPM aggregate | 5 | **30** |

---

## Short Content Circuit Breaker

### Masalah
Jika scraping gagal dan fallback ke summary RSS (< 200 karakter), memaksa Gemini mengekstrak `fakta_keras` + `relevansi` dari teks 1-2 kalimat berisiko **AI halusinasi**.

### Solusi
1. Threshold dinaikkan dari **100 → 200 karakter**
2. Jika teks hasil scrape < 200 karakter:
   - **Skip Gemini** (hemat kuota RPD)
   - Kirim ⚡ **QUICK ALERT** ke Telegram
   - Tandai sebagai processed
   - Lanjut ke artikel berikutnya

---

## Dependency Map

| Crate | Purpose | Memory Note |
|---|---|---|
| `tokio` | Async runtime | `current_thread` flavor |
| `reqwest` | HTTP client | `rustls-tls` (no OpenSSL) |
| `feed-rs` | RSS/Atom parser | Streaming parser |
| `scraper` | HTML CSS selector | Uses `html5ever` internally |
| `rusqlite` | Embedded SQLite | `bundled` feature |
| `serde` / `serde_json` | JSON serialization (also for GeminiModelConfig env override) | Derive macros |
| `sha2` / `hex` | SHA-256 hashing | Stack-allocated |
| `chrono` | Timestamp handling | RFC 3339 formatting |
| `log` / `env_logger` | Structured logging | Timestamps in WIB (UTC+7), filter via `RUST_LOG` |

---

## Resilience Matrix

| Failure Mode | Behavior |
|---|---|
| **Gemini invalid JSON** | Fallback to `[RAW BACKUP BUFFER]` mode |
| **Gemini rate limit** | Log warning, default gating to YES |
| **RPD quota exhausted** | Kirim RAW BACKUP dengan info sisa kuota (bukan spam RAW BACKUP berantakan) |
| **Short content (< 200 chars)** | Skip Gemini, kirim ⚡ QUICK ALERT |
| **Telegram API down** | Skip state commit → auto-retry next cycle |
| **RSS XML parse error** | Log warning, `continue` to next feed |
| **Network timeout** | Log warning, skip feed this cycle |
| **Scrape failure** | Use RSS `<description>` as fallback |

---

## Configuration Reference

| Env Var | Default | Purpose |
|---|---|---|
| `TELEGRAM_BOT_TOKEN` | **(required)** | Bot authentication |
| `TELEGRAM_CHAT_ID` | **(required)** | Destination chat/channel |
| `GEMINI_API_KEY` | **(required)** | Gemini API key (satu key untuk 2 model — Opsi A) |
| `RSS_FEEDS` | `https://rss.detik.com/index.php` | Comma-separated URLs |
| `POLL_INTERVAL_MINUTES` | `30` | Loop delay |
| `OXIDE_WHITELIST` | *(built-in, ~91 keywords)* | Comma-separated keyword whitelist (6 kategori) |
| `OXIDE_BLACKLIST` | *(built-in, ~38 keywords)* | Comma-separated keyword blacklist (termasuk daily noise filter) |
| `OXIDE_ONBOARDING_COUNT` | `0` | Process N articles on first boot (0 = skip all) |
| `OXIDE_AUTO_VACUUM_DAYS` | `0` | Delete processed_news older than N days (0 = disabled) |
| `OXIDE_MAX_ARTICLES_PER_CYCLE` | `15` | Max articles per cycle (Opsi A: total ~625 RPD / 48 siklus ≈ 13/siklus) |
| `OXIDE_GEMINI_MODELS` | *(Opsi A built-in)* | JSON array override untuk custom model fleet |
| `OXIDE_PROCESS_EXISTING` | `0` | Test mode: process ALL existing articles |
| `OXIDE_PRINT_ARTICLES` | `0` | Debug: log each article filter result |
| `RUST_LOG` | `info` | Log level (`error`, `warn`, `info`, `debug`, `trace`) |

---

## File Tree

```
oxide-feed/
├── Cargo.toml              # Dependencies & metadata
├── .env.example            # Environment variable template
├── SETUP.md                # Installation guide
├── ARCHITECTURE.md         # This file
├── deploy/
│   ├── setup.sh            # Production deployment script
│   └── oxide-feed.service  # systemd service file
├── src/
│   ├── main.rs             # Runtime engine, pipeline orchestrator, ModelRouter integration
│   ├── config.rs           # Env var loader + GeminiModelConfig + keyword kategoris
│   ├── storage.rs          # SQLite state manager + per-model RPD tracking
│   ├── ingest.rs           # RSS fetch, filter kategoris, scrape, contextual bypass
│   ├── llm.rs              # ModelRouter + per-model LlmClient (Opsi A: 3.5 primary + 3.1 backup)
│   └── telegram.rs         # Telegram dispatcher (news, raw backup, quick alert)
└── oxide_feed.db           # Auto-created SQLite database
```

---

## License

OxideFeed is released under the MIT License. See `LICENSE` for details.
