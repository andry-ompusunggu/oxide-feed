# 🔧 OxideFeed — Setup Guide

## Prerequisites

- **Rust** (1.75+): Install via [rustup](https://rustup.rs/)
- **Linux / macOS / WSL2**: Target OS (tested on Linux Mint XFCE)
- **Telegram Account**: For bot creation
- **Google Account**: For Gemini API key (Free Tier)

---

## 1. Clone & Build

```bash
git clone <your-repo-url> oxide-feed
cd oxide-feed

# Build the project (release for production)
cargo build --release
```

> **Note:** The first build downloads ~10 crates and compiles SQLite from source (bundled). This takes 1–3 minutes.

---

## 2. Environment Variables

Copy the template and fill in your credentials:

```bash
cp .env.example .env
```

| Variable | Required | Default | Description |
|---|---|---|---|
| `TELEGRAM_BOT_TOKEN` | ✅ Yes | — | Bot token from [@BotFather](https://t.me/BotFather) |
| `TELEGRAM_CHAT_ID` | ✅ Yes | — | Target chat/channel ID (negative for groups) |
| `GEMINI_API_KEY` | ✅ Yes | — | API key from [Google AI Studio](https://aistudio.google.com/app/apikey). Satu key untuk 3 model. |
| `RSS_FEEDS` | ❌ No | Detik.com | Comma-separated RSS URLs |
| `POLL_INTERVAL_MINUTES` | ❌ No | `30` | Loop interval (semakin cepat = delay berita berkurang, tapi siklus lebih banyak) |
| `OXIDE_WHITELIST` | ❌ No | *(built-in, 91 keywords)* | Comma-separated keyword whitelist (6 kategori) |
| `OXIDE_BLACKLIST` | ❌ No | *(built-in, 30 keywords)* | Comma-separated keyword blacklist (3 kategori) |
| `OXIDE_ONBOARDING_COUNT` | ❌ No | `0` | Process N existing articles on first boot (0 = skip all) |
| `OXIDE_AUTO_VACUUM_DAYS` | ❌ No | `0` | Auto-delete processed_news older than N days (0 = disabled) |
| `OXIDE_MAX_ARTICLES_PER_CYCLE` | ❌ No | `40` | Maksimal artikel diproses AI per siklus. Dengan 3 model (total 918 RPD), aman di 40. |
| `OXIDE_GEMINI_MODELS` | ❌ No | *(3 model built-in)* | JSON array override untuk custom model fleet. Format: `[{"name":"...","rpd_limit":450,"rpm_limit":15}]` |
| `OXIDE_PROCESS_EXISTING` | ❌ No | `0` | Test mode: process ALL existing articles |
| `OXIDE_PRINT_ARTICLES` | ❌ No | `0` | Debug: log each article filter result |
| `RUST_LOG` | ❌ No | `info` | Log level (`error`, `warn`, `info`, `debug`, `trace`) |

### 🔐 Getting Each Credential

**Telegram Bot Token:**
1. Open Telegram, search for [@BotFather](https://t.me/BotFather)
2. Send `/newbot`, follow prompts
3. Copy the HTTP API token (e.g., `123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11`)

**Telegram Chat ID:**
1. Add your bot to a **private channel** as admin
2. Send any message to the channel
3. Visit: `https://api.telegram.org/bot<YOUR_TOKEN>/getUpdates`
4. Find `"chat":{"id":-1001234567890}` — use that negative number

**Gemini API Key:**
1. Go to [Google AI Studio](https://aistudio.google.com/app/apikey)
2. Click **"Create API Key"** → select a Google Cloud project
3. Copy the key (satu key untuk **3 model**: Flash Lite 3.1, Flash Lite 3.5, Flash 3.6)

> 🧠 **Multi-Model:** OxideFeed sekarang mendistribusikan request ke 3 model Gemini secara round-robin:
> - Gemini 3.1 Flash Lite: 15 RPM, 500 RPD (cap: 450)
> - Gemini 3.5 Flash Lite: 15 RPM, 500 RPD (cap: 450)
> - Gemini 3.6 Flash: 5 RPM, 20 RPD (cap: 18)
> - **Total fleet: 918 RPD** — dari sebelumnya hanya 15 RPD!

---

## 3. Running

### Normal Mode

App akan auto-load file `.env` dari folder yang sama, jadi cukup:

```bash
# Edit .env dengan kredensial Anda, lalu:
cargo run
```

Atau gunakan lingkungan tanpa file `.env` dengan export manual:

```bash
export TELEGRAM_BOT_TOKEN="123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11"
export TELEGRAM_CHAT_ID="-1001234567890"
export GEMINI_API_KEY="AIzaSy..."
export RSS_FEEDS="https://rss.detik.com/index.php"
export POLL_INTERVAL_MINUTES=60

cargo run
```

### 🏁 Onboarding Mode (For Fresh Install)

**Normal mode** di first boot akan skip semua artikel existing (untuk hemat token).
Akibatnya: user baru tidak melihat hasil apa-apa sampai ada artikel BARU di RSS
(bisa menunggu berjam-jam).

**Onboarding mode** memproses sejumlah N artikel terbaru di first boot, jadi kamu
langsung lihat hasilnya tanpa menunggu:

```bash
# Hapus database lama, lalu:
rm oxide_feed.db

# Onboarding: proses 3 artikel terbaru di first boot
OXIDE_ONBOARDING_COUNT=3 OXIDE_PRINT_ARTICLES=1 POLL_INTERVAL_MINUTES=1 cargo run
```

Log akan muncul seperti ini:
```
[2026-07-16T11:28:29+07:00 INFO  oxide_feed] ONBOARDING MODE: will process up to 3 existing articles
[2026-07-16T11:28:29+07:00 INFO  oxide_feed::ingest] First boot for feed ... with ONBOARDING mode (up to 3 articles).
[2026-07-16T11:28:29+07:00 INFO  oxide_feed::ingest] [ARTICLE] title='...' | date=... | url=...
[2026-07-16T11:28:29+07:00 INFO  oxide_feed::ingest] Feed summary: 100 total, ... 3 passed
[2026-07-16T11:28:29+07:00 INFO  oxide_feed] Found 3 new item(s) to process
```

> 🎯 **Onboarding aman untuk token** — hanya N artikel yang diproses, bukan semua.

### 🔬 Test Mode (Processing ALL Existing Articles)

**Test mode** memproses SEMUA artikel existing tanpa batas. Cocok untuk debugging
filter keyword, tapi **boros token Gemini** (setiap artikel = 1 API call):

```bash
# Hapus database lama, lalu:
rm oxide_feed.db

# Test mode: proses SEMUA artikel existing
# ⚠️ HATI-HATI: bisa menghabiskan RPD (20 request/hari) dalam 1 cycle!
OXIDE_PROCESS_EXISTING=1 OXIDE_PRINT_ARTICLES=1 POLL_INTERVAL_MINUTES=1 cargo run
```

Log:
```
[2026-07-16T11:28:29+07:00 INFO  oxide_feed::ingest] TEST MODE: Processing articles regardless of watermark
[2026-07-16T11:28:29+07:00 INFO  oxide_feed::ingest] [ARTICLE] title='...' | date=... | url=...
[2026-07-16T11:28:29+07:00 INFO  oxide_feed::ingest] [FILTER] '...' → PASSED whitelist
[2026-07-16T11:28:29+07:00 INFO  oxide_feed] Feed summary: 100 total, ... 10 passed
```

> **Catatan Timestamp:** Semua log display menggunakan WIB (UTC+7) agar mudah dibaca di Indonesia. Database tetap menyimpan watermark dalam UTC.

> **Multi-Model Efficiency:** Dengan 3 model Gemini didistribusi round-robin (total 918 RPD):
> - Maksimal **~40 artikel/siklus** (default `OXIDE_MAX_ARTICLES_PER_CYCLE=40`)
> - Hingga **~900 artikel/hari** tanpa kehabisan kuota
> - Setiap model punya rate limiter sendiri (RPM throttle)
> - Jika satu model kena RPD cap, request otomatis dialihkan ke model lain

## 4. Production Deployment

### 🚀 Quick Setup (1 Command)

```bash
cd oxide-feed
sudo ./deploy/setup.sh
```

Script ini akan:
1. Build release binary
2. Setup `/opt/oxide-feed/` directory
3. Copy binary & `.env`
4. Install systemd service
5. Enable auto-start on boot
6. Start service sekarang

### 📁 Manual Setup

#### 4.1 Build Production Binary

```bash
cd oxide-feed

# Build dengan optimasi penuh
cargo build --release

# Kecilkan ukuran binary (hapus symbol table)
strip target/release/oxide-feed

# Cek ukuran (≈ 9 MB setelah strip)
ls -lh target/release/oxide-feed
```

#### 4.2 Recommended Directory Structure

```
/opt/oxide-feed/
├── bin/
│   └── oxide-feed          # Production binary (9.2 MB stripped)
├── .env                     # Environment variables (credentials)
├── oxide_feed.db            # SQLite database (auto-created)
├── logs/                    # Log files (optional)
└── deploy/                  # Deployment scripts
    ├── setup.sh             # One-click setup script
    └── oxide-feed.service   # systemd service file
```

Setup manual:
```bash
sudo mkdir -p /opt/oxide-feed/bin
sudo cp target/release/oxide-feed /opt/oxide-feed/bin/
sudo cp .env /opt/oxide-feed/.env
```

> ⚠️ **Jangan lupa edit credentials** di `/opt/oxide-feed/.env` sebelum start!

#### 4.3 Install systemd Service

```bash
# Install service file
sudo cp deploy/oxide-feed.service /etc/systemd/system/
sudo systemctl daemon-reload

# Enable auto-start on boot
sudo systemctl enable oxide-feed

# Start sekarang
sudo systemctl start oxide-feed
```

#### 4.4 Auto-Start on Boot

```bash
# Aktifkan auto-start saat laptop dinyalakan
sudo systemctl enable oxide-feed

# Start sekarang
sudo systemctl start oxide-feed

# Cek status
sudo systemctl status oxide-feed
```

**Yang terjadi saat boot:**
1. Laptop menyala
2. systemd auto-start oxide-feed setelah network siap
3. App membaca watermark dari `oxide_feed.db`
4. **Langsung fetch RSS** — tidak nunggu 1 jam
5. Memproses artikel terbaru dengan prioritas (newest first), maksimal `OXIDE_MAX_ARTICLES_PER_CYCLE` artikel
6. Selesai → tidur 60 menit → cycle berikutnya

> **✅ Database tetap aman** — selama `oxide_feed.db` tidak dihapus, catch-up otomatis berjalan setiap restart.

### 4.4 Database Management

**File:** `/opt/oxide-feed/oxide_feed.db`

Database ini menyimpan:
- `processed_news` — hash artikel yang sudah diproses (untuk deduplikasi)
- `rss_states` — watermark per feed (posisi terakhir diproses)
- `daily_api_usage` — tracking jumlah panggilan Gemini per hari (RPD Guard)

**⚠️ JANGAN PERNAH menghapus database di production!**

| Tindakan | Akibat |
|---|---|
| `rm oxide_feed.db` | ❌ Semua watermark hilang → next boot skip semua artikel existing |
| Simpan DB | ✅ Watermark aman → restart langsung catch-up |

**Backup Database:**
```bash
# Cukup copy satu file saja
cp oxide_feed.db oxide_feed.db.backup-$(date +%Y%m%d)

# Atau dengan timestamp
cp /opt/oxide-feed/oxide_feed.db ~/backups/oxide-feed-20260720.db
```

**Restore Database:**
```bash
# Hentikan service dulu
sudo systemctl stop oxide-feed

# Restore backup
cp ~/backups/oxide-feed-20260720.db /opt/oxide-feed/oxide_feed.db

# Start lagi
sudo systemctl start oxide-feed
```

### 4.5 Monitoring Logs

```bash
# Live log
sudo journalctl -u oxide-feed -f

# Log 100 baris terakhir
sudo journalctl -u oxide-feed -n 100 --no-pager

# Log sejak hari ini
sudo journalctl -u oxide-feed --since today

# Log rentang tanggal tertentu
sudo journalctl -u oxide-feed --since "2026-07-20" --until "2026-07-21"

# Filter level tertentu (ERROR/WARN)
sudo journalctl -u oxide-feed -p err
```

**Log Levels (via `RUST_LOG`):**
| Level | Penggunaan |
|---|---|
| `error` | Hanya error — cocok untuk monitoring |
| `warn` | Error + warning — default produksi |
| `info` | Info lengkap — default |
| `debug` | Detail teknis — untuk troubleshooting |
| `trace` | Semua — sangat verbose |

Contoh penggunaan:
```bash
# Set di .env file
RUST_LOG=warn
```

### 4.6 Updating the Binary

```bash
# 1. Pull perubahan kode terbaru
git pull

# 2. Build binary baru
cargo build --release
strip target/release/oxide-feed

# 3. Hentikan service
sudo systemctl stop oxide-feed

# 4. Backup database (jaga-jaga)
cp /opt/oxide-feed/oxide_feed.db /opt/oxide-feed/oxide_feed.db.pre-update

# 5. Ganti binary
sudo cp target/release/oxide-feed /opt/oxide-feed/bin/oxide-feed

# 6. Start ulang
sudo systemctl start oxide-feed

# 7. Verifikasi logs
sudo journalctl -u oxide-feed -n 20 --no-pager
```

> **Catatan:** Database (`oxide_feed.db`) **tidak perlu diubah** saat update. Binary baru akan membaca watermark dari DB yang sama dan lanjut dari posisi terakhir.

### 4.7 Total Backup & Restore

**Backup (full):**
```bash
BACKUP_DIR=~/oxide-feed-backup-$(date +%Y%m%d)
mkdir -p $BACKUP_DIR

cp /opt/oxide-feed/oxide_feed.db $BACKUP_DIR/
cp /opt/oxide-feed/.env $BACKUP_DIR/
cp /opt/oxide-feed/bin/oxide-feed $BACKUP_DIR/

echo "Backup saved to $BACKUP_DIR"
```

**Restore (full):**
```bash
sudo systemctl stop oxide-feed

cp ~/oxide-feed-backup-20260720/oxide_feed.db /opt/oxide-feed/
cp ~/oxide-feed-backup-20260720/.env /opt/oxide-feed/
# binary juga bisa di-restore jika versi cocok

sudo systemctl start oxide-feed
```

### 4.8 Resource Monitoring

```bash
# Cek RAM & CPU
ps aux | grep oxide-feed

# Atau via systemd
sudo systemctl status oxide-feed

# Cek ukuran database
ls -lh /opt/oxide-feed/oxide_feed.db

# Cek disk usage keseluruhan
du -sh /opt/oxide-feed/
```

**Estimasi Resource:**
| Resource | Value |
|---|---|
| RAM | ~15–25 MB RSS |
| CPU (idle) | ~0% |
| CPU (processing) | ~5-15% (spike) |
| Disk (DB) | ~1 KB / 1000 artikel |
| Disk (binary) | ~4-6 MB |

### 4.9 Log Rotation (Opsional)

journald sudah handle rotasi otomatis. Tapi untuk file log custom:

```bash
# Buat konfigurasi logrotate
sudo tee /etc/logrotate.d/oxide-feed << 'EOF'
/opt/oxide-feed/logs/*.log {
    daily
    rotate 7
    compress
    delaycompress
    missingok
    notifempty
    postrotate
        systemctl restart oxide-feed
    endscript
}
EOF
```

---

## 5. Verifying It Works

### Quick Test (Test Mode)

Gunakan test mode untuk langsung melihat pipeline bekerja tanpa menunggu artikel baru:

```bash
rm oxide_feed.db
OXIDE_PROCESS_EXISTING=1 OXIDE_PRINT_ARTICLES=1 POLL_INTERVAL_MINUTES=1 cargo run 2>&1 | head -200
```

### Normal Mode (Menunggu Artikel Baru)

1. Run the app:
   ```bash
   rm oxide_feed.db
   POLL_INTERVAL_MINUTES=1 cargo run
   ```
2. Look for these log lines on first boot:
   ```
   First boot for feed ... Setting watermark to current time ... and skipping all existing items.
   ```
3. Tunggu hingga ada artikel BARU di sumber RSS — cycle berikutnya akan memprosesnya
4. Cek Telegram channel untuk notifikasi

---

## 5. Resource Usage

- **RAM**: ~15–25 MB RSS (single-threaded tokio, rustls TLS)
- **CPU**: Near 0% when idle, spikes during RSS/LLM calls
- **Disk**: `oxide_feed.db` grows slowly (~1 KB per 1000 articles)

---

## 6. Troubleshooting

| Symptom | Likely Cause | Fix |
|---|---|---|
| `TELEGRAM_BOT_TOKEN must be set` | `.env` file tidak ditemukan atau tidak lengkap | Pastikan `.env` ada di folder project dan kredensial terisi |
| `Gemini API call failed` | Invalid API key atau model tidak tersedia | Pastikan pakai `gemini-3.1-flash-lite`, regenerate key di [AI Studio](https://aistudio.google.com/) |
| `Telegram API error (400): ... can't parse entities` | Markdown escaping issue | Check article text for unsupported characters |
| No messages in channel | Tidak ada artikel baru sejak app berjalan (normal) | Gunakan `OXIDE_PROCESS_EXISTING=1` untuk test |
| `skipped (filter)` semua artikel di log | Whitelist default terlalu sempit untuk feed Anda | Tambah keyword di `OXIDE_WHITELIST` di `.env` (tidak perlu rebuild) — lihat daftar grup kategoris di ARCHITECTURE.md |
| `"QUICK NEWS"` muncul di channel | Artikel < 200 karakter — Gemini tidak dipanggil | Ini normal untuk artikel pendek. Buka tautan untuk baca selengkapnya. |
| `"RPD limit reached"` di channel | Kuota harian Gemini habis (default 15 calls/hari) | Naikkan `OXIDE_DAILY_GEMINI_CAP` atau tunggu reset besok |
| `No new items to process` terus | Ini normal — tidak ada artikel BARU di feed | Jalankan test mode: `OXIDE_PROCESS_EXISTING=1` |

---

## 7. Uninstalling

```bash
# Remove build artifacts
rm -rf target/

# Remove database (deletes all state)
rm oxide_feed.db

# Remove source
cd .. && rm -rf oxide-feed/
```
