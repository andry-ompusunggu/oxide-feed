use crate::config::ContextualPair;
use crate::storage::Storage;
use feed_rs::parser;
use reqwest::Client;
use scraper::{Html, Selector};
use sha2::{Sha256, Digest};
use std::collections::HashSet;
use std::sync::Arc;

/// Minimum content length for scraped article body (chars)
/// Articles shorter than this will NOT go to Gemini (circuit breaker).
/// Instead, they are sent as a QUICK ALERT to Telegram.
const MIN_CONTENT_LENGTH: usize = 200;

/// A single processed news item
#[derive(Debug, Clone)]
pub struct NewsItem {
    pub hash: String,
    pub title: String,
    pub link: String,
    pub summary: String,
    pub pub_date: Option<String>,
    pub feed_url: String,
}

/// Compute SHA-256 hash for deduplication
pub fn compute_hash(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

/// Check if the title passes whitelist/blacklist filters.
/// `whitelist` and `blacklist` are loaded from config (env vars or defaults).
///
/// Now with contextual pair matching: jika blacklist word muncul bersamaan
/// dengan whitelist partner-nya (misal "rumor" + "akuisisi"), artikel LOLOS.
pub fn passes_local_filter(
    title: &str,
    whitelist: &[String],
    blacklist: &[String],
    contextual_pairs: &[ContextualPair],
) -> bool {
    let lower = title.to_lowercase();

    // Step 1: Check whitelist FIRST - apakah ada kata kunci yang cocok?
    let whitelist_matches: Vec<&str> = whitelist
        .iter()
        .filter(|w| lower.contains(w.as_str()))
        .map(|s| s.as_str())
        .collect();

    // Jika tidak ada whitelist match sama sekali, langsung reject
    if whitelist_matches.is_empty() {
        return false;
    }

    // Step 2: Check blacklist - tapi dengan contextual pair bypass
    'blacklist: for bad_word in blacklist {
        if !lower.contains(bad_word.as_str()) {
            continue;
        }

        // Blacklist word found! Cek apakah ada contextual partner yang cocok
        for pair in contextual_pairs {
            if bad_word.as_str() == pair.blacklist_word {
                for partner in &pair.whitelist_partners {
                    if whitelist_matches.contains(&partner.as_str()) || lower.contains(partner.as_str()) {
                        log::info!(
                            "Contextual bypass for '{}' paired with '{}'",
                            bad_word, partner
                        );
                        continue 'blacklist;
                    }
                }
            }
        }

        // Blacklist word found AND no contextual bypass -> reject
        return false;
    }

    true
}

/// Attempt to scrape full article body from the original URL.
/// Falls back to `<description>` / `<summary>` if scraped text < 100 chars.
pub async fn scrape_full_body(client: &Client, link: &str, fallback_summary: &str) -> String {
    // Try scraping
    match client.get(link).send().await {
        Ok(resp) => {
            if let Ok(html_text) = resp.text().await {
                let document = Html::parse_document(&html_text);

                // Try common selectors
                let selectors = [
                    "article",
                    "div.content",
                    "div.read__content",
                    ".article-content",
                    ".post-content",
                    "main",
                ];

                for sel_str in &selectors {
                    if let Ok(sel) = Selector::parse(sel_str) {
                        let extracted: String = document
                            .select(&sel)
                            .flat_map(|el| el.text())
                            .collect::<Vec<_>>()
                            .join(" ")
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ");

                        if extracted.len() >= MIN_CONTENT_LENGTH {
                            return extracted;
                        }
                    }
                }
            }
        }
        Err(e) => {
            log::warn!("Failed to scrape {}: {}", link, e);
        }
    }

    // Fallback: use RSS description/summary
    log::info!("Scraped content too short; falling back to RSS summary for {}", link);
    fallback_summary.to_string()
}

/// Fetch and parse all RSS feeds, returning new items that pass local filters.
///
/// Onboarding logic:
/// - If `onboarding_count > 0` and this is a first boot (no watermark), process
///   up to `onboarding_count` existing articles, then set watermark.
/// - BUGFIX: The onboarding limit is checked at the TOP of each URL loop to
///   ensure it works reliably (previously the break inside the entry loop
///   didn't always trigger correctly).
#[allow(clippy::too_many_arguments)]
pub async fn fetch_and_filter(
    client: &Client,
    storage: &Arc<Storage>,
    feed_urls: &[String],
    process_existing: bool,
    print_articles: bool,
    whitelist: &[String],
    blacklist: &[String],
    onboarding_count: usize,
    contextual_pairs: &[ContextualPair],
) -> Vec<NewsItem> {
    let mut new_items = Vec::new();
    // Cross-feed dedup: track title hashes to avoid processing duplicates across feeds
    let mut seen_titles: HashSet<String> = HashSet::new();
    let mut is_first_boot = false;

    for url in feed_urls {
        // ═══════════════════════════════════════════════════════
        // ONBOARDING LIMIT CHECK (EARLY GATE):
        // If we already reached the onboarding limit, skip remaining feeds.
        // This is placed BEFORE fetch_feed to avoid unnecessary network calls.
        // ═══════════════════════════════════════════════════════
        if is_first_boot && onboarding_count > 0 && new_items.len() >= onboarding_count {
            log::info!(
                "Onboarding: reached limit of {} articles. Skipping remaining feeds.",
                onboarding_count
            );
            continue;
        }

        log::info!("Fetching RSS feed: {}", url);

        match fetch_feed(client, url).await {
            Ok((feed, _latest_pub)) => {
                // Check watermark
                let watermark = storage.get_watermark(url).unwrap_or(None);

                // If no watermark exists, this is initial boot:
                if watermark.is_none() && !process_existing {
                    is_first_boot = true;

                    if onboarding_count > 0 {
                        // ONBOARDING MODE: process up to N articles, then set watermark
                        log::info!(
                            "First boot for feed {} with ONBOARDING mode (up to {} articles).",
                            url,
                            onboarding_count
                        );
                    } else {
                        // Normal first boot: capture current timestamp as watermark
                        // to prevent token drainage from historical items
                        let now_utc = chrono::Utc::now().to_rfc3339();
                        log::info!(
                            "First boot for feed {}. Setting watermark to current time {} and skipping all existing items.",
                            url,
                            now_utc
                        );
                        if let Err(e) = storage.set_watermark(url, &now_utc) {
                            log::error!("Failed to set watermark for {}: {}", url, e);
                        }
                        continue;
                    }
                }

                let total_entries = feed.entries.len();
                let mut passed = 0usize;
                let mut skipped_watermark = 0usize;
                let mut skipped_processed = 0usize;
                let mut skipped_filter = 0usize;
                let mut skipped_duplicate = 0usize;

                // Log mode
                if process_existing {
                    log::info!("TEST MODE: Processing articles regardless of watermark");
                }

                for entry in feed.entries {
                    // ═══════════════════════════════════════════════
                    // ONBOARDING LIMIT CHECK (ENTRY GATE):
                    // Check again before processing each entry, since the
                    // count may have been exceeded in a previous feed.
                    // ═══════════════════════════════════════════════
                    if is_first_boot && onboarding_count > 0 && new_items.len() >= onboarding_count {
                        break;
                    }

                    // Compute hash from URL or GUID
                    let link = entry
                        .links
                        .first()
                        .map(|l| l.href.clone())
                        .unwrap_or_default();

                    let guid = entry.id.clone();

                    let dedup_key = if !link.is_empty() { &link } else { &guid };
                    let hash = compute_hash(dedup_key);

                    let title = entry
                        .title
                        .map(|t| t.content)
                        .unwrap_or_default();

                    // Skip empty titles
                    if title.is_empty() {
                        continue;
                    }

                    // Skip if already processed (by URL hash)
                    if storage.is_processed(&hash).unwrap_or(false) {
                        skipped_processed += 1;
                        continue;
                    }

                    // Cross-feed dedup: skip if title already seen in another feed
                    let title_hash = compute_hash(&title.to_lowercase());
                    if !seen_titles.insert(title_hash) {
                        skipped_duplicate += 1;
                        continue;
                    }

                    // Check pub_date vs watermark (skip if not test mode and not onboarding)
                    if !process_existing && onboarding_count == 0 {
                        if let Some(ref wm_str) = watermark {
                            if let Some(pd) = &entry.published {
                                if let Ok(wm) = chrono::DateTime::parse_from_rfc3339(wm_str) {
                                    if *pd <= wm {
                                        skipped_watermark += 1;
                                        continue;
                                    }
                                } else {
                                    let pd_str = pd.to_rfc3339();
                                    if pd_str <= *wm_str {
                                        skipped_watermark += 1;
                                        continue;
                                    }
                                }
                            }
                        }
                    }

                    // Print article debug
                    if print_articles {
                        let pd_str = entry
                            .published
                            .map(|pd| pd.to_rfc3339())
                            .unwrap_or_default();
                        log::info!(
                            "[ARTICLE] title='{}' | date={} | url={}",
                            title, pd_str, link
                        );
                    }

                    // Apply whitelist/blacklist filter with contextual pair bypass
                    if !passes_local_filter(&title, whitelist, blacklist, contextual_pairs) {
                        if print_articles {
                            log::info!(
                                "[FILTER] '{}' → REJECTED by whitelist/blacklist",
                                title
                            );
                        }
                        // Mark as processed to skip indefinitely
                        if let Err(e) = storage.mark_processed(&hash, &title) {
                            log::error!("Failed to mark processed '{}': {}", title, e);
                        }
                        skipped_filter += 1;
                        continue;
                    }

                    if print_articles {
                        log::info!("[FILTER] '{}' → PASSED whitelist", title);
                    }

                    // Extract summary/description
                    let summary = entry
                        .summary
                        .map(|s| s.content)
                        .unwrap_or_default();

                    let summary_clean = summarize_text(&summary);

                    passed += 1;
                    new_items.push(NewsItem {
                        hash,
                        title,
                        link,
                        summary: summary_clean,
                        pub_date: entry.published.map(|pd| pd.to_rfc3339()),
                        feed_url: url.to_string(),
                    });
                }

                log::info!(
                    "Feed summary: {} total, {} skipped (watermark), {} skipped (processed), {} skipped (filter), {} skipped (duplicate), {} passed",
                    total_entries, skipped_watermark, skipped_processed, skipped_filter, skipped_duplicate, passed
                );

                // If onboarding mode and we processed articles, set watermark
                if is_first_boot && onboarding_count > 0 && !new_items.is_empty() {
                    let now_utc = chrono::Utc::now().to_rfc3339();
                    if let Err(e) = storage.set_watermark(url, &now_utc) {
                        log::error!("Failed to set onboarding watermark for {}: {}", url, e);
                    }
                }
            }
            Err(e) => {
                log::warn!("Failed to fetch/parse RSS feed {}: {}", url, e);
            }
        }
    }

    new_items
}

/// Fetch and parse a single RSS feed
async fn fetch_feed(
    client: &Client,
    url: &str,
) -> Result<(feed_rs::model::Feed, String), Box<dyn std::error::Error>> {
    let resp = client.get(url).send().await?;
    let bytes = resp.bytes().await?;
    let feed = parser::parse(bytes.as_ref())?;

    // Determine the latest publication date in the feed
    let latest = feed
        .entries
        .iter()
        .filter_map(|e| e.published)
        .max()
        .unwrap_or(chrono::Utc::now());

    let latest_str = latest.to_rfc3339();

    Ok((feed, latest_str))
}

/// Sort news items by pub_date descending (newest first)
pub fn sort_by_newest(items: &mut Vec<NewsItem>) {
    items.sort_by(|a, b| {
        match (&a.pub_date, &b.pub_date) {
            (Some(da), Some(db)) => db.cmp(da), // descending
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
}

/// Clean up summary: trim whitespace, limit length
fn summarize_text(text: &str) -> String {
    let cleaned: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    cleaned.chars().take(500).collect()
}
