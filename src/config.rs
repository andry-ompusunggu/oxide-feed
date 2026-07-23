use serde::Deserialize;
use std::env;

// ═══════════════════════════════════════════════════════
// Expanded Whitelist — Grup Kategoris
// ═══════════════════════════════════════════════════════

/// Ekonomi Makro & Moneter
const MACRO_KEYWORDS: &[&str] = &[
    "suku bunga", "bi rate", "inflasi", "rupiah", "nilai tukar",
    "pajak", "fiskal", "moneter", "devisa", "cadangan devisa",
    "apbn", "pnbp", "defisit", "utang negara", "pertumbuhan ekonomi",
    "gdp", "pdb", "konsumsi rumah tangga", "daya beli",
];

/// Pasar Modal & Korporasi
const MARKET_KEYWORDS: &[&str] = &[
    "saham", "ihsg", "emiten", "dividen", "akuisisi",
    "merger", "ipo", "obligasi", "restrukturisasi", "right issue",
    "bei", "bursa", "kustodian", "reksadana", "sukuk",
];

/// Regulasi, Pemerintah & Kebijakan
const REGULATION_KEYWORDS: &[&str] = &[
    "uu", "perpu", "perpres", "permen", "permendag", "permenkeu",
    "kebijakan", "tarif", "regulasi", "pemerintah", "dpr", "kpk",
    "mk", "mahkamah konstitusi", "omnibus law", "cipta kerja",
];

/// Energi & Sumber Daya Alam
const ENERGY_KEYWORDS: &[&str] = &[
    "energi", "bbm", "bbm", "pertamina", "pln", "minyak",
    "gas bumi", "batubara", "nikel", "timah", "sawit",
    "harga energi", "tarif listrik", "subsidi energi",
];

/// Bencana & Krisis
const CRISIS_KEYWORDS: &[&str] = &[
    "bencana", "gempa", "tsunami", "banjir", "letusan",
    "wabah", "pandemi", "covid", "darurat", "evakuasi",
    "korban jiwa", "kerusakan", "longsor", "kekeringan",
];

/// Transportasi & Infrastruktur
const INFRASTRUCTURE_KEYWORDS: &[&str] = &[
    "transjakarta", "mrt", "lrt", "krl", "tol",
    "infrastruktur", "bandara", "pelabuhan", "jalan",
    "kereta cepat", "whoosh", "ikn", "ibu kota negara",
];

// ═══════════════════════════════════════════════════════
// Expanded Blacklist
// ═══════════════════════════════════════════════════════

/// Sensasionalisme & Clickbait
const CLICKBAIT_BLACKLIST: &[&str] = &[
    "viral", "viral di media sosial", "viral di tiktok", "viral di twitter",
];

/// Gosip & Gaya Hidup
const LIFESTYLE_BLACKLIST: &[&str] = &[
    "menikah", "selingkuh", "pacar", "artis", "pacaran", "putus",
    "gimmick", "fakta menarik", "biodata", "profil",
    "harta kekayaan", "intip gaya", "penampilan", "transformasi",
    "fashion", "makeup", "outfit", "ootd",
];

/// Netizen & Ujaran
const TOXIC_BLACKLIST: &[&str] = &[
    "netizen", "hujat", "komen", "komentar", "warganet",
    "medsos", "media sosial", "ramai di",
];

// ═══════════════════════════════════════════════════════
// Contextual Pairs
// ═══════════════════════════════════════════════════════
/// Jika blacklist word muncul BERSAMA dengan whitelist word,
/// artikel TETAP lolos filter. Misal: "rumor" + "akuisisi" = lolos.
/// Format: (blacklist_word, whitelist_partner)
const CONTEXTUAL_PAIRS: &[(&str, &[&str])] = &[
    ("rumor", &["akuisisi", "merger", "investasi", "saham", "emiten", "bank", "go public", "buyback"]),
    ("isyaratkan", &["suku bunga", "bi rate", "inflasi", "pemerintah"]),
    ("sinyal", &["ekonomi", "pasar", "saham", "ihsg", "regulasi"]),
];

// ═══════════════════════════════════════════════════════
// Default Aggregate Whitelist (all categories combined)
// ═══════════════════════════════════════════════════════

fn default_whitelist() -> Vec<String> {
    let mut all: Vec<&str> = Vec::new();
    all.extend_from_slice(MACRO_KEYWORDS);
    all.extend_from_slice(MARKET_KEYWORDS);
    all.extend_from_slice(REGULATION_KEYWORDS);
    all.extend_from_slice(ENERGY_KEYWORDS);
    all.extend_from_slice(CRISIS_KEYWORDS);
    all.extend_from_slice(INFRASTRUCTURE_KEYWORDS);
    all.iter().map(|s| s.to_string()).collect()
}

fn default_blacklist() -> Vec<String> {
    let mut all: Vec<&str> = Vec::new();
    all.extend_from_slice(CLICKBAIT_BLACKLIST);
    all.extend_from_slice(LIFESTYLE_BLACKLIST);
    all.extend_from_slice(TOXIC_BLACKLIST);
    all.iter().map(|s| s.to_string()).collect()
}

/// Contextual pairs stored in Config for zero-cost caching
#[derive(Debug, Clone)]
pub struct ContextualPair {
    pub blacklist_word: String,
    pub whitelist_partners: Vec<String>,
}

/// Get contextual pairs as owned data for storage in Config.
/// Build once at startup, then pass by reference.
pub fn build_contextual_pairs() -> Vec<ContextualPair> {
    CONTEXTUAL_PAIRS
        .iter()
        .map(|(bw, partners)| ContextualPair {
            blacklist_word: bw.to_string(),
            whitelist_partners: partners.iter().map(|s| s.to_string()).collect(),
        })
        .collect()
}

// ═══════════════════════════════════════════════════════
// Gemini Model Configuration
// ═══════════════════════════════════════════════════════
/// Configuration for a single Gemini model endpoint.
/// Each model has its own RPD (Requests Per Day) cap,
/// RPM (Requests Per Minute) limit, and API base URL.
#[derive(Debug, Clone, Deserialize)]
pub struct GeminiModelConfig {
    /// Human-readable name (e.g. "gemini-3.1-flash-lite")
    pub name: String,
    /// Full API endpoint URL
    pub api_base: String,
    /// Daily request cap for this model (free tier headroom)
    pub rpd_limit: usize,
    /// Requests per minute limit (for inter-call spacing)
    pub rpm_limit: usize,
}

impl GeminiModelConfig {
    /// Build the default 2-model fleet (Gemini 3.6 Flash removed due to 503 instability).
    ///
    /// Rate limits from Google AI Studio (Free Tier):
    /// - Gemini 3.1 Flash Lite: 15 RPM, 500 RPD
    /// - Gemini 3.5 Flash Lite: 15 RPM, 500 RPD
    ///
    /// We use 95% of RPD (up from 90% since we no longer have the 3rd model).
    pub fn defaults() -> Vec<Self> {
        vec![
            GeminiModelConfig {
                name: "gemini-3.1-flash-lite".to_string(),
                api_base: "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.1-flash-lite:generateContent".to_string(),
                rpd_limit: 475,  // 95% of 500 RPD
                rpm_limit: 15,
            },
            GeminiModelConfig {
                name: "gemini-3.5-flash-lite".to_string(),
                api_base: "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.5-flash-lite:generateContent".to_string(),
                rpd_limit: 475,  // 95% of 500 RPD
                rpm_limit: 15,
            },
        ]
    }

    /// Compute the minimum interval (seconds) between calls to this model.
    /// e.g. 15 RPM → 60/15 = 4s.  5 RPM → 60/5 = 12s.
    pub fn min_interval_secs(&self) -> u64 {
        if self.rpm_limit == 0 {
            12 // sensible default
        } else {
            (60.0 / self.rpm_limit as f64).ceil() as u64
        }
    }
}

/// Application configuration loaded from environment variables
pub struct Config {
    /// Telegram Bot API token
    pub telegram_bot_token: String,
    /// Telegram Chat ID (private channel) to send messages to
    pub telegram_chat_id: String,
    /// Gemini API key (Google AI Studio Free Tier) — reused across all 3 models
    pub gemini_api_key: String,
    /// List of RSS feed URLs to monitor
    pub rss_feeds: Vec<String>,
    /// Check interval in minutes
    pub poll_interval_minutes: u64,
    /// Test mode: process existing articles (bypass watermark)
    pub process_existing: bool,
    /// Print each article and filter result to stdout
    pub print_articles: bool,
    /// Whitelist keywords — at least one must match (lowercased)
    pub whitelist: Vec<String>,
    /// Blacklist keywords — none may match (lowercased)
    pub blacklist: Vec<String>,
    /// Onboarding mode: process up to N existing articles on first boot (0 = disabled)
    pub onboarding_count: usize,
    /// Auto-vacuum: delete processed_news older than N days (0 = disabled)
    pub auto_vacuum_days: u64,
    /// Max articles to process per cycle (0 = unlimited)
    pub max_articles_per_cycle: usize,
    /// Contextual pairs for blacklist bypass (cached at startup)
    pub contextual_pairs: Vec<ContextualPair>,
    /// Modele Gemini yang akan digunakan dengan konfigurasi masing-masing
    pub gemini_models: Vec<GeminiModelConfig>,
}

impl Config {
    /// Load configuration from environment variables.
    /// Panics if required variables are missing.
    pub fn from_env() -> Self {
        let telegram_bot_token = env::var("TELEGRAM_BOT_TOKEN")
            .expect("TELEGRAM_BOT_TOKEN must be set");
        let telegram_chat_id = env::var("TELEGRAM_CHAT_ID")
            .expect("TELEGRAM_CHAT_ID must be set");
        let gemini_api_key = env::var("GEMINI_API_KEY")
            .expect("GEMINI_API_KEY must be set");

        let rss_feeds_raw = env::var("RSS_FEEDS")
            .unwrap_or_else(|_| {
                "https://rss.detik.com/index.php".to_string()
            });

        let rss_feeds: Vec<String> = rss_feeds_raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let poll_interval_minutes = env::var("POLL_INTERVAL_MINUTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);

        let process_existing = env::var("OXIDE_PROCESS_EXISTING")
            .ok()
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        let print_articles = env::var("OXIDE_PRINT_ARTICLES")
            .ok()
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        // Whitelist: from env or default
        let whitelist = env::var("OXIDE_WHITELIST")
            .ok()
            .map(|raw| {
                raw.split(',')
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_else(default_whitelist);

        // Blacklist: from env or default
        let blacklist = env::var("OXIDE_BLACKLIST")
            .ok()
            .map(|raw| {
                raw.split(',')
                    .map(|s| s.trim().to_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_else(default_blacklist);

        // Onboarding count: process N existing articles on first boot
        let onboarding_count = env::var("OXIDE_ONBOARDING_COUNT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        // Auto-vacuum: days threshold
        let auto_vacuum_days = env::var("OXIDE_AUTO_VACUUM_DAYS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);

        // Max articles per cycle (default 20 — dengan 2 model, total ~950 RPD / 48 siklus ≈ 20/cycle)
        // 0 = unlimited (proceed as fast as models allow)
        let max_articles_per_cycle = env::var("OXIDE_MAX_ARTICLES_PER_CYCLE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(20);

        // Build model fleet — defaults for free tier, or override via env JSON
        // Format: JSON array of {name, rpd_limit, rpm_limit}
        // Example:
        // OXIDE_GEMINI_MODELS='[{"name":"gemini-3.1-flash-lite","rpd_limit":450,"rpm_limit":15}]'
        let gemini_models: Vec<GeminiModelConfig> = env::var("OXIDE_GEMINI_MODELS")
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_else(GeminiModelConfig::defaults);

        Config {
            telegram_bot_token,
            telegram_chat_id,
            gemini_api_key,
            rss_feeds,
            poll_interval_minutes,
            process_existing,
            print_articles,
            whitelist,
            blacklist,
            onboarding_count,
            auto_vacuum_days,
            max_articles_per_cycle,
            contextual_pairs: build_contextual_pairs(),
            gemini_models,
        }
    }
}
