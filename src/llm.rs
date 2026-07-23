use crate::config::GeminiModelConfig;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// Structured output when Gemini approves the article
#[derive(Debug, Deserialize, Serialize)]
pub struct SanitizedOutput {
    pub topik: String,
    /// Nilai: regulasi | pasar | bencana | energi | korporasi | makro | lain
    pub kategori: String,
    pub fakta_keras: Vec<String>,
    /// Nilai: tinggi | sedang | rendah
    pub signifikansi: String,
    /// 1 kalimat relevansi — mengapa berita ini penting (tanpa spekulasi)
    pub relevansi: String,
}

/// Response from the combined process_article call (gate + sanitize in one)
#[derive(Debug, Deserialize)]
struct CombinedResponse {
    #[serde(default)]
    is_important: bool,
    #[serde(default)]
    reason_if_rejected: Option<String>,
    #[serde(default)]
    data: Option<SanitizedOutput>,
}

/// Result of processing a single article
pub enum ArticleResult {
    /// Gemini says NO — article is not relevant
    Rejected {
        reason: Option<String>,
    },
    /// Gemini approved and returned structured data
    Sanitized(SanitizedOutput),
    /// Gemini returned something we could not parse — use raw backup
    Fallback(String),
}

// ═══════════════════════════════════════════════════════
// Per-Model LlmClient
// ═══════════════════════════════════════════════════════

/// LLM client for a SINGLE Gemini model.
/// Each model has its own endpoint URL, rate-limit state, and RPD tracking.
pub struct LlmClient {
    pub config: GeminiModelConfig,
    client: Client,
    api_key: String,
    /// Timestamp of the last API call to this model (for RPM enforcement)
    last_call: std::sync::Mutex<Option<Instant>>,
}

impl LlmClient {
    pub fn new(client: Client, api_key: String, config: GeminiModelConfig) -> Self {
        LlmClient {
            config,
            client,
            api_key,
            last_call: std::sync::Mutex::new(None),
        }
    }

    /// Human-readable model identifier for logs
    pub fn model_label(&self) -> &str {
        &self.config.name
    }

    /// Await the minimum interval since the last call (per-model RPM throttle).
    /// Returns immediately if enough time has passed or this is the first call.
    pub async fn enforce_rate_limit(&self) {
        let interval = tokio::time::Duration::from_secs(self.config.min_interval_secs());

        let elapsed = {
            let guard = self.last_call.lock().unwrap_or_else(|e| e.into_inner());
            guard.map(|t| t.elapsed())
        };

        if let Some(elapsed) = elapsed {
            if elapsed < interval {
                let wait = interval - elapsed;
                log::info!(
                    "[{}] Rate limit: waiting {:.1}s (RPM ≤ {})",
                    self.config.name,
                    wait.as_secs_f64(),
                    self.config.rpm_limit
                );
                tokio::time::sleep(wait).await;
            }
        }

        // Update last call timestamp
        if let Ok(mut guard) = self.last_call.lock() {
            *guard = Some(Instant::now());
        }
    }

    /// Single combined call: gating check + deep sanitization in ONE API request.
    ///
    /// Menggunakan response schema baru:
    /// - is_important: false + reason_if_rejected -> artikel tidak relevan
    /// - is_important: true + data -> artikel relevan + data terstruktur
    /// - Jika is_important = false, data = null untuk hemat token
    ///
    /// Optimasi: 1 artikel = 1 request
    pub async fn process_article(&self, title: &str, full_body: &str) -> ArticleResult {
        let prompt = self.build_prompt(title, full_body);

        match self.call_gemini(&prompt).await {
            Ok(response) => {
                // Try to parse the structured JSON response
                let json_str = extract_json(&response);

                match serde_json::from_str::<CombinedResponse>(&json_str) {
                    Ok(combined) => {
                        if combined.is_important {
                            // Artikel relevan — extract structured data
                            match combined.data {
                                Some(data) => {
                                    log::info!(
                                        "[{}] APPROVED '{title}'",
                                        self.config.name
                                    );
                                    ArticleResult::Sanitized(data)
                                }
                                None => {
                                    // is_important = true tapi data null → fallback
                                    log::warn!(
                                        "[{}] marked '{title}' as important but data was null. Using raw.",
                                        self.config.name
                                    );
                                    ArticleResult::Fallback(response)
                                }
                            }
                        } else {
                            // Artikel tidak relevan
                            let reason = combined
                                .reason_if_rejected
                                .unwrap_or_else(|| "No reason given".to_string());
                            log::info!(
                                "[{}] REJECTED '{title}' — {reason}",
                                self.config.name
                            );
                            ArticleResult::Rejected {
                                reason: Some(reason),
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to parse [{}] response for '{title}': {e}. Using raw backup.",
                            self.config.name
                        );
                        ArticleResult::Fallback(response)
                    }
                }
            }
            Err(e) => {
                log::warn!("[{}] API call failed for '{title}': {e}", self.config.name);
                // On failure, send as raw backup so no news is lost
                ArticleResult::Fallback(format!("[{} unavailable: {e}]", self.config.name))
            }
        }
    }

    /// Build the Gemini prompt with proper escaping.
    ///
    /// Includes explicit rejection rules based on production evaluation:
    /// - REJECT daily market tickers (IHSG/Rupiah daily fluctuations without structural trigger)
    /// - REJECT local government CSR/socialization events (Pemkot/Pemkab level)
    /// - STRICT ALLOW for systemic regulatory changes, macro fiscal/monetary data,
    ///   major geopolitical trade shifts, and material corporate actions.
    fn build_prompt(&self, title: &str, full_body: &str) -> String {
        // Use a plain String builder to avoid format! escaping issues with JSON braces
        let mut p = String::new();
        p.push_str("System: You are an objective news analyzer for Indonesia. ");
        p.push_str("Determine if this article reports a major regulatory amendment, ");
        p.push_str("a macroeconomic structural shift, a corporate market action, ");
        p.push_str("or an immediate public crisis in Indonesia.\n\n");

        p.push_str("Respond with one of these exact JSON structures:\n\n");

        p.push_str("1. If NOT relevant (no speculation, no forced analysis):\n");
        p.push_str("{\"is_important\": false, \"reason_if_rejected\": \"Brief reason\"}\n\n");

        p.push_str("2. If relevant:\n");
        p.push_str("{\"is_important\": true, \"data\": {");
        p.push_str("\"topik\": \"Brief Topic (Max 5 Words CAPS)\", ");
        p.push_str("\"kategori\": \"choose one: regulasi, pasar, bencana, energi, korporasi, makro, or lain\", ");
        p.push_str("\"fakta_keras\": [\"Factual bullet points\"], ");
        p.push_str("\"signifikansi\": \"choose one: tinggi, sedang, rendah\", ");
        p.push_str("\"relevansi\": \"Why this matters (1-2 sentences)\"");
        p.push_str("}}\n\n");

        p.push_str("IMPORTANT RULES:\n");
        p.push_str("- If is_important = false, set data = null (do not fill data).\n");
        p.push_str("- Only fill in what the article actually supports. ");
        p.push_str("Never fabricate or speculate about impacts.\n");
        p.push_str("- REJECT articles that merely report daily stock index movements ");
        p.push_str("(IHSG up/down by X points) or daily currency fluctuation ");
        p.push_str("without a major policy or structural trigger.\n");
        p.push_str("- REJECT regional/municipal level socialization or CSR events ");
        p.push_str("(e.g., Pemkot/Pemkab financial literacy sessions, local CSR ");
        p.push_str("programs without material impact on company valuation).\n");
        p.push_str("- REJECT lifestyle, clickbait, automotive tips, consumer guides, ");
        p.push_str("and celebrity gossip.\n");
        p.push_str("\n");
        p.push_str("STRICT ALLOW for:\n");
        p.push_str("- Systemic regulatory changes (UU/Perpu/PP/RUU, government regulations)\n");
        p.push_str("- Macro fiscal/monetary data (BI Rate, APBN Deficit, Inflation, trade balance)\n");
        p.push_str("- Major geopolitical trade shifts (tariffs, trade wars, sanctions affecting Indonesia)\n");
        p.push_str("- Material corporate actions (Debt default, Bond redemption, M&A, IPO, restructuring)\n");
        p.push_str("\n");

        p.push_str("Title: ");
        p.push_str(title);
        p.push_str("\nBody: ");
        p.push_str(full_body);

        p
    }

    /// Make a request to the Gemini API
    async fn call_gemini(&self, prompt: &str) -> Result<String, Box<dyn std::error::Error>> {
        let url = format!("{}?key={}", self.config.api_base, self.api_key);

        let body = serde_json::json!({
            "contents": [{
                "parts": [{
                    "text": prompt
                }]
            }]
        });

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await?;

        // Extract text from Gemini response structure
        let text = json["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .ok_or_else(|| {
                format!(
                    "Unexpected Gemini response structure: {}",
                    serde_json::to_string(&json).unwrap_or_default()
                )
            })?;

        Ok(text.to_string())
    }
}

// ═══════════════════════════════════════════════════════
// ModelRouter — Round-Robin Distribution Across Models
// ═══════════════════════════════════════════════════════

/// Distributes API calls across multiple Gemini models in round-robin fashion.
/// Skips models that have reached their daily RPD cap.
pub struct ModelRouter {
    /// All available model clients
    models: Vec<LlmClient>,
    /// Round-robin index (atomic for interior mutability)
    current_index: AtomicUsize,
}

impl ModelRouter {
    pub fn new(client: Client, api_key: String, model_configs: Vec<GeminiModelConfig>) -> Self {
        let models = model_configs
            .into_iter()
            .map(|cfg| LlmClient::new(client.clone(), api_key.clone(), cfg))
            .collect();

        ModelRouter {
            models,
            current_index: AtomicUsize::new(0),
        }
    }

    /// Number of models in the fleet
    pub fn len(&self) -> usize {
        self.models.len()
    }

    /// Select the next available model using round-robin (skip models at RPD limit).
    /// Returns `None` if **all** models have reached their daily cap.
    ///
    /// The `daily_usage_fn` closure should return the current call-count for a given model name.
    pub fn select_model<F>(&self, daily_usage_fn: F) -> Option<&LlmClient>
    where
        F: Fn(&str) -> usize,
    {
        let start = self.current_index.fetch_add(1, Ordering::Relaxed) % self.models.len();

        for i in 0..self.models.len() {
            let idx = (start + i) % self.models.len();
            let model = &self.models[idx];
            let usage = daily_usage_fn(&model.config.name);

            if usage < model.config.rpd_limit {
                log::info!(
                    "Router → {} (usage {}/{})",
                    model.config.name,
                    usage,
                    model.config.rpd_limit
                );
                return Some(model);
            } else {
                log::info!(
                    "Router skip {} (usage {}/{}, at RPD cap)",
                    model.config.name,
                    usage,
                    model.config.rpd_limit
                );
            }
        }

        log::warn!("All {} models at RPD cap. No model available.", self.models.len());
        None
    }

    /// Get combined RPD limit across all models (for logging / diagnostics)
    pub fn total_rpd_limit(&self) -> usize {
        self.models.iter().map(|m| m.config.rpd_limit).sum()
    }

    /// Iterate over all model names + configs (for startup logging)
    pub fn iter_models(&self) -> impl Iterator<Item = (&str, usize, usize)> {
        self.models.iter().map(|m| {
            (m.config.name.as_str(), m.config.rpd_limit, m.config.rpm_limit)
        })
    }
}

/// Extract JSON from a text that may contain markdown code fences
fn extract_json(text: &str) -> String {
    // Try to find JSON inside ```json ... ``` fences
    if let Some(start) = text.find("```json") {
        let after_start = &text[start + 7..];
        if let Some(end) = after_start.find("```") {
            return after_start[..end].trim().to_string();
        }
    }

    // Try to find JSON inside ``` ... ``` fences
    if let Some(start) = text.find("```") {
        let after_start = &text[start + 3..];
        if let Some(end) = after_start.find("```") {
            let candidate = after_start[..end].trim();
            if candidate.starts_with('{') {
                return candidate.to_string();
            }
        }
    }

    // Fallback: return the whole text trimmed
    text.trim().to_string()
}
