# 🛠️ OxideFeed — Evaluasi Critical, Analisis Masalah & Rekomendasi Perbaikan

Dokumen ini berisi analisis kritis terhadap arsitektur dan alur bisnis **OxideFeed**, mengidentifikasi *blind spots* (titik buta) yang berpotensi menyebabkan berita penting terlewat (*false negatives*), berita sampah masuk (*false positives*), hingga potensi masalah operasional saat menggunakan Free Tier AI.

---

## 📌 Ringkasan Eksekutif

| Area Evaluasi | Status Saat Ini | Dampak / Risiko | Tingkat Urgensi |
|---|---|---|---|
| **1. Filter Lokal (Langkah 2)** | Hardcoded Whitelist & Blacklist kata tunggal pada Judul | Potensi berita penting terlewat tinggi (*False Negative*) & clickbait lolos (*False Positive*) | 🔴 High |
| **2. Efisiensi API Gemini** | 2 Call terpisah (Cek Penting → Format JSON) | Pemborosan kuota RPD hingga 50% pada Free Tier | 🔴 High |
| **3. Skenario Catch-Up (Laptop Mati)** | Memproses semua antrean sekaligus tanpa Throttling | Kuota RPD (20 req/hari) habis seketika, Telegram dibanjiri *RAW BACKUP* | 🟠 Medium |
| **4. Fallback Scraping (< 100 karakter)** | Mengirim ringkasan pendek RSS ke Gemini | Risiko AI Halusinasi tinggi karena kekurangan data konteks | 🟠 Medium |

---

## 🔍 Detail Masalah & Solusi Perbaikan

### 1. Filter Lokal (Judul) Terlalu Kaku & Rentan Leakage

#### ❌ Masalah
1. **Risiko *False Negative* (Berita Penting Terlewat):**
   - Istilah ekonomi/bisnis sangat bervariasi. Judul seperti *"Bank Indonesia Tahan Suku Bunga di 6%"* atau *"Penyebab Nilai Tukar Melemah"* akan **DITOLAK** karena tidak mengandung kata kunci eksak `bi rate` atau `rupiah`.
   - Berita penting seperti *"Rumor Akuisisi Bank X oleh Investor Asing Kian Menguat"* akan **DITOLAK** karena kata `rumor` ada di dalam **Blacklist**.
2. **Risiko *False Positive* (Berita Sampah Lolos):**
   - Media online sering menggabungkan istilah bisnis dengan sensasionalisme. 
   - *Contoh:* *"Anak Presiden Pakai Sepatu Rp50 Juta, Pemerintah Disorot"* → Mengandung kata `pemerintah` (Whitelist) dan **tidak** memuat kata dari Blacklist. Berita ini akan lolos ke tahap scraping & AI, membuang kuota/token.

#### ✅ Solusi & Perbaikan
* **Perluas Thesaurus Whitelist (Grup Kategoris):** Kelompokkan kata kunci berdasarkan domain konteks, bukan sekadar kata tunggal.
  * *Ekonomi Makro:* `suku bunga`, `inflasi`, `pajak`, `fiskal`, `moneter`, `devisa`, `cadangan devisa`, `apbn`, `pnbp`, `bi rate`, `rupiah`.
  * *Pasar Modal & Korporasi:* `saham`, `ihsg`, `emiten`, `dividen`, `akuisisi`, `merger`, `ipo`, `obligasi`, `kustodian`, `restrukturisasi`.
  * *Regulasi & Pemerintah:* `uu`, `perpu`, `perpres`, `permendag`, `permenkeu`, `kebijakan`, `tarif`, `regulasi`, `pemerintah`.
* **Gunakan Contextual Pair Matching (Kombinasi Kata):**
  - Jangan tolak kata `rumor` secara absolut jika berdampingan dengan `akuisisi`, `merger`, atau `investasi`.
* **Blacklist yang Lebih Spesifik:** Tambahkan kata-kata gaya hidup/gosip politik yang sering mengecoh: `biodata`, `profil`, `harta kekayaan`, `intip gaya`, `penampilan`, `viral di tiktok`, `netizen`.

---

### 2. Pemborosan Request API Gemini (Inefisiensi Pipeline)

#### ❌ Masalah
- **2 Call Per Artikel:** Alur saat ini melakukan Call #1 pada Langkah 4 (Cek Penting) lalu Call #2 pada Langkah 5 (Format JSON).
- **Dampak Ketat Free Tier:** Dengan batas **RPD (Request Per Day) = 20**, sistem ini hanya mampu memproses **maksimal 10 berita penting per hari** sebelum kuota habis dan sistem jatuh ke mode `RAW BACKUP BUFFER`.

#### ✅ Solusi & Perbaikan
* **Konsolidasi Prompt (Single-Pass Classification & Extraction):**
  Gabungkan Langkah 4 dan Langkah 5 menjadi **1 kali panggil API**.
* Minta Gemini melakukan evaluasi kelayakan *sekaligus* menyusun JSON jika berita tersebut relevan.

**Contoh Response Schema (Single Prompt):**
```json
{
  "is_important": true,
  "reason_if_rejected": null,
  "data": {
    "topik": "TARIF PNBP NAIK",
    "kategori": "regulasi",
    "fakta_keras": [
      "Tarif PNBP naik 55%",
      "Berlaku mulai 16 Juli 2026"
    ],
    "signifikansi": "tinggi",
    "relevansi": "Menaikkan biaya pendaftaran merek bagi UMKM"
  }
}

```

> *Jika `is_important` bernilai `false`, field `data` cukup diisi `null`.* Dengan cara ini, efisiensi kuota API meningkat **100%** (1 artikel = 1 request).

---

### 3. Skenario Catch-Up Berisiko Menghabiskan Kuota Daily (RPD Exhaustion)

#### ❌ Masalah

* Saat laptop mati selama weekend (Skenario C), terdapat misal 45 artikel menumpuk di antrean RSS.
* Sistem memproses ke-45 artikel tersebut satu per satu secara sekuensial.
* Pada artikel **ke-21**, limit RPD Gemini (20 req/day) dipastikan **habis**.
* Artikel ke-21 hingga ke-45 akan di-*bypass* secara otomatis dan terkirim sebagai teks mentah (`[RAW BACKUP BUFFER]`) ke channel Telegram, menyebabkan *spam* teks tidak terformat.

#### ✅ Solusi & Perbaikan

* **Terapkan Daily Cap / Batch Processing Limit:**
* Batasi maksimal pemrosesan AI per siklus catch-up (misalnya max **5 artikel per run** atau **15 artikel per hari**).


* **Prioritisasi Berita Terbaru:**
* Saat catch-up, sortir artikel berdasarkan tanggal publikasi terbaru (`pubDate` descending), lalu ambil $N$ berita paling baru untuk dikirim ke AI. Berita lama yang sudah lewati batas kuota harian cukup dicatat di DB sebagai `skipped_over_limit` tanpa perlu di-spam ke Telegram.



---

### 4. Potensi Halusinasi AI pada Short Content (Fallback Scrape)

#### ❌ Masalah

* Jika scraping gagal atau isi artikel < 100 karakter, sistem menggunakan *summary* RSS.
* Mendorong Gemini untuk mengekstrak `fakta_keras`, `signifikansi`, dan `relevansi` dari teks 1-2 kalimat ringkasan yang minim konteks berisiko tinggi menyebabkan **AI Halusinasi** (mengarang analisis/dampak yang tidak ada di sumber asli).

#### ✅ Solusi & Perbaikan

* **Threshold & Circuit Breaker pada Scraping:**
* **Teks < 200 Karakter:** **Jangan kirim ke Gemini.**
* Kirimkan langsung ke Telegram sebagai pesan mini/alert ringkas:
> ⚡ **[QUICK NEWS]** *Pemerintah Terbitkan Aturan Baru Tarif PNBP.*
> *(Gagal memuat teks lengkap. Buka tautan untuk membaca lebih lanjut: [Link])*


* Langkah ini menjaga kredibilitas output AI dan menghemat request kuota Gemini untuk artikel bertipikal *long-form*.



---

## 🔄 Revisi Arsitektur Alur Bisnis (Proposed Flow)

```
RSS Feed
    │
    ▼
┌────────────────────────┐
│  1. Ambil Berita       │  ← Setiap 60 menit
└────────┬───────────────┘
         │
         ▼
┌────────────────────────┐
│  2. Filter Lokal       │  ← Cek judul: Expanded Whitelist & Blacklist
│  ┌──────┴──────┐       │
│  │  LOLOS ✅   │       │
│  └──────┬──────┘       │
│         │              │
│  ┌──────┴──────┐       │
│  │  DITOLAK ❌ │  → Skip & catat DB
│  └─────────────┘       │
└────────┬───────────────┘
         │
         ▼
┌────────────────────────┐
│  3. Ambil Isi (Scrape) │
└────────┬───────────────┘
         │
    ┌────┴────┐
    │         │
    ▼         ▼
> 200 Teks   < 200 Teks
  Char        Char
    │         │
    │         └──→ Kirim Telegram [QUICK ALERT] (Skip AI)
    ▼
┌────────────────────────┐
│  4. Process Gemini AI  │  ← SINGLE PROMPT (Evaluasi + Format JSON sekaligus)
│     (Max 5 req/cycle)  │  ← Memeriksa sisa kuota RPD
└────────┬───────────────┘
         │
    ┌────┴────┐
    │         │
    ▼         ▼
  PENTING   TIDAK PENTING / ERROR
    │         │
    │         └──→ Skip & Catat DB / Kirim Raw jika Error Limit
    ▼
┌────────────────────────┐
│  5. Kirim ke Telegram  │  ← Format Markdown Rapi
│     & Save Database    │
└────────────────────────┘

```

---

## 📋 Summary Checklist Action Plan

* [ ] **Refactor Filter Keyword:** Perluas daftar *whitelist* ke istilah-istilah makroekonomi/pasar modal dan perbaiki kriteria *blacklist*.
* [ ] **Combine Gemini Prompts:** Ubah *prompting logic* menjadi 1-step call (Kelayakan + JSON Extraction).
* [ ] **Implement Throttling & RPD Guard:** Tambahkan pembatasan jumlah artikel yang boleh diproses AI per cycle/hari untuk menghindari *rate limit spike*.
* [ ] **Add Length Threshold:** Batasi pemicuan AI hanya untuk teks hasil scrape di atas 200 karakter guna mencegah halusinasi.
"""

file_path = "OxideFeed_Evaluasi_dan_Perbaikan.md"
with open(file_path, "w", encoding="utf-8") as f:
f.write(md_content)

print(f"File successfully created: {file_path}")

```

```text?code_stdout&code_event_index=1
File successfully created: OxideFeed_Evaluasi_dan_Perbaikan.md


```

Your Markdown file is ready


Berikut adalah rangkuman dari dokumen evaluasi kritis dan rekomendasi perbaikan untuk **OxideFeed** yang telah dibuat dalam format Markdown (`.md`):

---

### 📌 Ringkasan Poin Evaluasi & Perbaikan

1. **Filter Lokal (Judul) Kaku & Rentan Leakage**
* **Masalah:** Kata kunci eksak (`bi rate`, `rupiah`) akan melewatkan berita penting seperti *"Bank Indonesia Tahan Suku Bunga"* atau *"Penyebab Nilai Tukar Melemah"*. Kata `rumor` pada blacklist juga membuang berita penting seperti *"Rumor Akuisisi Bank X"*.
* **Perbaikan:** Perluas whitelist berbasis **grup kategoris** (Ekonomi Makro, Pasar Modal, Regulasi) serta terapkan **contextual pair matching**.


2. **Innefisiensi Pemanggilan API Gemini (2 Call / Artikel)**
* **Masalah:** Memisah tahap "Cek Penting" dan "Format JSON" memakan 2 request per berita. Dengan batas Free Tier (20 RPD), sistem hanya sanggup mengolah maks. 10 berita penting/hari.
* **Perbaikan:** Konsolidasi menjadi **Single-Pass Prompt** (AI menilai kelayakan *sekaligus* menyusun JSON dalam 1 request). Hemat kuota hingga 50%.


3. **Risiko Catch-Up Habiskan Kuota Harian (RPD Exhaustion)**
* **Masalah:** Saat catch-up setelah laptop mati 3 hari, 45 artikel antrean diproses sekaligus. Memasuki artikel ke-21, limit RPD habis dan sisa artikel akan di-*spam* sebagai teks mentah (`RAW BACKUP BUFFER`) ke Telegram.
* **Perbaikan:** Terapkan **Daily Cap / Throttling Batching** (maksimal 5 artikel per siklus) dan dahulukan artikel yang paling baru (*pubDate descending*).


4. **Potensi Halusinasi AI pada Teks Pendek (< 100 Karakter)**
* **Masalah:** Memaksa AI mengekstrak *fakta keras* & *analisis relevansi* dari 1-2 kalimat ringkasan RSS (jika scraping gagal) berisiko memicu halusinasi.
* **Perbaikan:** Set threshold panjang teks (misal minimal 200 karakter). Jika di bawah itu, kirim sebagai **[QUICK ALERT]** ringkas tanpa diproses oleh AI.