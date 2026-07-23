# 📊 OxideFeed — Alur Bisnis

**OxideFeed** adalah sistem yang otomatis mengambil berita dari RSS, menyaring berita yang relevan,
membersihkannya dari bias/clickbait, lalu mengirimkannya ke Telegram dalam format yang rapi.

Dokumen ini menjelaskan **alur proses dari awal sampai akhir** dalam bahasa bisnis — bukan teknis.

---

## 🔄 Ringkasan Alur

```
RSS Feed
    │
    ▼
┌────────────────────────┐
│  1. Ambil Berita       │  ← Setiap 60 menit (atau sesuai setting)
│     dari RSS           │
└────────┬───────────────┘
         │
         ▼
┌────────────────────────┐
│  2. Filter Lokal       │  ← Cek judul: boleh/tidak berdasarkan keyword
│  ┌──────┴──────┐       │
│  │  LOLOS ✅   │       │
│  └──────┬──────┘       │
│         │              │
│  ┌──────┴──────┐       │
│  │  DITOLAK ❌ │  → Skip (tidak akan diproses lagi)    │
│  └─────────────┘       │
└────────┬───────────────┘
         │ (Lolos filter)
         ▼
┌────────────────────────┐
│  3. Ambil Isi Berita   │  ← Buka websitenya, ambil teks lengkap
│     (Scrape)           │
└────────┬───────────────┘
         │
         ▼
┌────────────────────────┐
│  4. Cek Gemini         │  ← Tanya AI: ini berita penting atau tidak?
│  ┌──────┴──────┐       │
│  │  PENTING ✅ │       │  → Lanjut ke format AI
│  └──────┬──────┘       │
│         │              │
│  ┌──────┴──────┐       │
│  │  NGGAK ❌   │  → Skip, tandai sudah diproses          │
│  └─────────────┘       │
└────────┬───────────────┘
         │ (Penting)
         ▼
┌────────────────────────┐
│  5. Format Gemini      │  ← AI rapihin: fakta keras,
│     (Sanitasi)         │    kategorisasi, relevansi
└────────┬───────────────┘
         │
    ┌────┴────┐
    │         │
    ▼         ▼
  JSON ✅   GAGAL ❌
    │         │
    │         └──→ Kirim teks mentah
    │              [RAW BACKUP BUFFER]
    ▼
┌────────────────────────┐
│  6. Kirim ke Telegram  │  ← Format Markdown rapi
│  ┌──────┴──────┐       │
│  │  BERHASIL   │       │  → Simpan ke database
│  └─────────────┘       │  → Update posisi (watermark)
│  ┌──────┴──────┐       │
│  │  GAGAL      │       │  → JANGAN simpan ke database
│  └─────────────┘       │  → Akan diulang di cycle berikutnya
└────────────────────────┘
```

---

## 📋 Penjelasan Setiap Langkah

### Langkah 1: Ambil Berita dari RSS

Sistem membaca RSS feed yang sudah didaftarkan (default: CNBC Indonesia).

**Frekuensi:** Setiap 60 menit (bisa diubah via `POLL_INTERVAL_MINUTES`).

**Yang terjadi:**
- Sistem membaca posisi terakhir (watermark) dari database
- Ambil semua artikel dari RSS
- Bandingkan tanggal terbit artikel dengan posisi terakhir

### Langkah 2: Filter Lokal (Cek Judul)

Ini adalah **gerbang pertama**. Sistem mengecek judul artikel berdasarkan aturan sederhana:

**WHITELIST** — judul HARUS mengandung minimal satu kata ini:
```
regulasi, kebijakan, saham, ihsg, bi rate, tarif, bencana,
wabah, transjakarta, uu, pemerintah, ekonomi, emiten, rupiah, inflasi
```

**BLACKLIST** — judul TIDAK BOLEH mengandung kata ini sama sekali:
```
viral, netizen, hujat, menikah, selingkuh, pacar, artis,
gimmick, fakta menarik, rumor, putus
```

**Contoh:**
| Judul | Hasil | Alasan |
|---|---|---|
| "Pemerintah Terbitkan Aturan Baru" | ✅ LOLOS | Mengandung "pemerintah" |
| "Artis X Menikah Diam-diam" | ❌ DITOLAK | Blacklist "artis" & "menikah" |
| "IHSG Anjlok Akibat Perang Dagang" | ✅ LOLOS | Mengandung "ihsg" |

> **Yang ditolak langsung dicatat di database** dan tidak akan diproses lagi selamanya.

### Langkah 3: Ambil Isi Berita (Scrape)

Untuk artikel yang lolos filter judul, sistem akan:
1. Buka websitenya
2. Ambil teks lengkap artikelnya
3. Kalau gagal ambil teks (misal website error), pakai ringkasan dari RSS

> Jika teks yang didapat kurang dari 100 karakter, akan pakai ringkasan RSS saja.

### Langkah 4: Cek Gemini — Apakah Berita Ini Penting?

Ini **gerbang kedua**. Sistem kirim judul + isi berita ke Google Gemini AI.
Gemini akan jawab:
- **"YA"** → berita ini penting, lanjut ke format
- **"TIDAK"** → berita ini tidak relevan, skip

**Kriteria penting:** Berita tentang perubahan regulasi besar, pergeseran ekonomi makro,
aksi korporasi, atau krisis publik di Indonesia.

> Butuh ~12 detik antar request ke Gemini (batas Free Tier: 5 request per menit).

### Langkah 5: Format Gemini (Sanitasi)

Kalau Gemini bilang "YA", dia akan otomatis merapikan berita menjadi format JSON:

```json
{
  "topik": "BLOK MASELA GROUNDBREAKING",
  "kategori": "energi",
  "fakta_keras": [
    "Proyek gas Blok Masela diresmikan hari ini",
    "Nilai investasi Rp355 triliun",
    "Dioperasikan Inpex, Pertamina, Petronas"
  ],
  "signifikansi": "tinggi",
  "relevansi": "Proyek ini akan memasok 40% kebutuhan gas nasional"
}
```

**Yang TIDAK dilakukan Gemini:**
- ❌ Tidak spekulasi dampak masa depan
- ❌ Tidak menghapus data penting
- ❌ Tidak menambahkan opini

**Kalau Gemini error/gagal:**
Sistem akan kirim teks mentah dengan label `[RAW BACKUP BUFFER]` — berita tetap sampai, hanya tidak diformat rapi.

### Langkah 6: Kirim ke Telegram

Hasil format dikirim ke channel/group Telegram.

**Kalau berhasil (HTTP 200 OK):**
- ✅ Catat artikel ini ke database (supaya tidak diproses ulang)
- ✅ Update posisi terakhir (watermark) dengan tanggal artikel ini

**Kalau gagal:**
- ❌ JANGAN catat apa-apa
- ❌ JANGAN update watermark
- ↻ Akan dicoba ulang di cycle berikutnya (1 jam lagi)

### Langkah 7: Notifikasi Startup & Error

**Startup:** Setiap app dijalankan, mengirim notifikasi:
```
🤖 OxideFeed v1.0 started
📰 1 feed(s), every 60 min
```

**Error:** Kalau satu siklus gagal total, mengirim peringatan:
```
⚠️ OxideFeed: Processing cycle failed
Error: ...
Check logs for details.
```

---

## 🎯 Skenario Lifecycle

### Skenario A: Pertama Kali Jalan — Normal (Skip Semua)

```
Watermark: BELUM ADA
Database: KOSONG

1. Ambil RSS
2. Set watermark = sekarang (misal: Senin 08:00 WIB)
3. Skip semua artikel existing (sudah terbit sebelum 08:00)
4. Tidur 60 menit
5. Cycle berikutnya: proses artikel BARU (terbit setelah 08:00 WIB)
```

> 🎯 **Kenapa skip semua?** Karena tidak ingin boros token AI untuk berita lama yang sudah basi.

### Skenario A2: Pertama Kali Jalan — Onboarding Mode (Proses N Artikel)

```
OXIDE_ONBOARDING_COUNT=5
Watermark: BELUM ADA
Database: KOSONG

1. Ambil RSS
2. Deteksi first boot + onboarding mode aktif
3. Proses 5 artikel terbaru (sesuai limit)
4. Set watermark = sekarang setelah selesai
5. Kirim 3-4 artikel (yang lolos Gemini) ke Telegram
6. Tidur 60 menit
7. Cycle berikutnya: hanya artikel BARU setelah onboarding
```

> 🎯 **Ini yang paling推荐 untuk fresh install.** Langsung lihat hasil tanpa nunggu, tanpa boros token.

### Skenario A3: Test Mode (OXIDE_PROCESS_EXISTING=1)

```
OXIDE_PROCESS_EXISTING=1
Watermark: DIABAIKAN
Database: BISA DIHAPUS

1. Ambil RSS
2. Abaikan watermark — proses SEMUA artikel
3. Kirim yang lolos filter ke Telegram
4. Kirim ringkasan log ke console
```

> ⚠️ **HATI-HATI:** Bisa menghabiskan RPD (20 request/hari) dalam 1 cycle.

```
Watermark: BELUM ADA
Database: KOSONG

1. Ambil RSS
2. Set watermark = sekarang (misal: Senin 08:00 WIB)
3. Skip semua artikel existing (sudah terbit sebelum 08:00)
4. Tidur 60 menit
5. Cycle berikutnya: proses artikel BARU (terbit setelah 08:00 WIB)
```

> 🎯 **Kenapa skip semua?** Karena tidak ingin boros token AI untuk berita lama yang sudah basi.

### Skenario B: Normal — Laptop Nyala Setiap Hari

```
Watermark: Kemarin 16:00 WIB
Database: ADA

1. Ambil RSS
2. Bandingkan: artikel dengan tanggal > kemarin 16:00 WIB
3. Proses artikel baru yang muncul sejak kemarin
4. Kirim ke Telegram
5. Update watermark ke artikel terbaru
6. Tidur 60 menit
```

### Skenario C: Catch-Up — Laptop Mati 3 Hari (Weekend)

```
Watermark: Jumat 16:00 WIB
Database: ADA
Laptop mati: Sabtu, Minggu

Senin pagi 08:00 WIB — laptop dinyalakan:
1. Ambil RSS
2. Bandingkan: ambil semua artikel sejak Jumat 16:00 → 45 artikel baru!
3. Proses 1 per 1 (jeda 12 detik antar artikel karena rate limit AI)
4. ≈ 15 menit untuk selesai
5. Kirim ke Telegram
6. Update watermark
7. Tidur 60 menit
```

> ✅ **Tidak ada berita yang terlewat.** Semua artikel selama weekend akan terproses.



---

## 🗄️ State & Database

Database (`oxide_feed.db`) menyimpan 2 hal:

### Tabel 1: `processed_news` — Riwayat Artikel

| Kolom | Contoh |
|---|---|
| `id` (hash artikel) | `a1b2c3d4e5f6...` |
| `title` | "Pemerintah Terbitkan Aturan Baru" |
| `processed_at` | `2026-07-20 08:00:00` |

**Guna:** Supaya artikel yang sudah diproses (atau ditolak) tidak diproses ulang.

### Tabel 2: `rss_states` — Posisi Terakhir per Feed

| Kolom | Contoh |
|---|---|
| `feed_url` | `https://www.cnbcindonesia.com/news/rss` |
| `last_fetched_pub_date` | `2026-07-20T01:00:00+00:00` (UTC) |

**Guna:** Tahu posisi terakhir untuk catch-up. Disimpan dalam UTC agar konsisten
meskipun laptop pindah zona waktu.

---

## ⚡ Batasan Free Tier (Gemini 3.1 Flash-Lite)

| Metrik | Batas | Dampak ke Alur |
|---|---|---|
| **RPM** (Request per menit) | 5 | Maksimal 5 artikel diproses per menit → jeda 12 detik antar artikel |
| **RPD** (Request per hari) | 20 | Maksimal 20 artikel per hari → kalau lagi banyak berita, yang ke-21 dikirim mentah |

**Estimasi Harian:**
| Skenario | Artikel dari RSS | Lolos Filter Judul | Request Gemini | Status |
|---|---|---|---|---|
| Hari biasa | ~100 artikel | 2-5 artikel | 2-5 ✅ | Aman |
| Hari ramai | ~100 artikel | 10-15 artikel | 10-15 ✅ | Masih aman |
| Hari super ramai | ~100 artikel | 20+ artikel | 20 ❌ | Kelebihan RPD |

> Kalau kelebihan RPD, artikel sisanya tetap dikirim ke Telegram sebagai teks mentah
> (raw backup), bukan dihilangkan.

---

## 📝 Contoh Alur Lengkap (1 Artikel)

**Berita:** *"Pemerintah Resmi Naikkan Tarif PNBP 55%, Ini Daftar Lengkapnya"*

```
Step 1: RSS Feed → ditemukan
Step 2: Filter judul → "pemerintah" ada di whitelist ✅
Step 3: Scrape artikel → ambil teks lengkap ✅
Step 4: Cek Gemini → "YA, ini penting" ✅
Step 5: Format Gemini →
    topik: "TARIF PNBP NAIK"
    kategori: "regulasi"
    fakta: "Tarif PNBP naik 55%", "Berlaku 16 Juli 2026"
    signifikansi: "tinggi"
    relevansi: "Menaikkan biaya pendaftaran merek bagi UMKM"
Step 6: Kirim ke Telegram → BERHASIL ✅
    → Catat ke database
    → Update watermark
    → Selesai ✔️
```

---

## ❓ FAQ

### Q: Apakah artikel yang sudah dikirim bisa dikirim ulang?
Tidak. Setiap artikel dicatat hash-nya di database. Artikel dengan hash yang sama
akan langsung di-skip.

### Q: Kalau Telegram error (server down), beritanya hilang?
Tidak. Karena state TIDAK dicatat, artikel akan diproses ulang di cycle berikutnya
(60 menit kemudian). Berita tetap aman.

### Q: Berapa lama catch-up kalau libur seminggu?
Estimasi: jika ada ~70 artikel baru, dengan jeda 12 detik antar request + ~8 detik
waktu proses per artikel = ~20 detik × 70 = ~23 menit.

### Q: Apakah bisa lihat log real-time?
Ya, lihat bagian [Monitoring Logs di SETUP.md](./SETUP.md#45-monitoring-logs).

### Q: Apakah data aman kalau laptop mati mendadak?
Aman. Database SQLite menggunakan transaksi atomic. Kalau proses terputus di tengah,
state tidak dicatat sebagian. Tidak ada data korup.
