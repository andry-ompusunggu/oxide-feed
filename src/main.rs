mod config;
mod ingest;
mod llm;
mod storage;
mod telegram;

use config::Config;
use ingest::{fetch_and_filter, scrape_full_body};
use llm::{ArticleResult, ModelRouter};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use storage::Storage;
use telegram::TelegramClient;

/// Counter for tracking cycles (used for periodic vacuum)
static CYCLE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Main entry point — single-threaded async runtime
#[tokio::main(flavor = "current_thread")]
async fn main() {
    // Auto-load .env file (if it exists). This allows env vars to be set
    // via a simple .env file without manual `source .env`.
    dotenvy::dotenv().ok();

    // Initialize logger with WIB (UTC+7) timestamps for local readability
    // Database storage still uses UTC internally — this is display-only.
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info"),
    )
    .format(|buf, record| {
        use chrono::Utc;
        let wib = chrono::FixedOffset::east_opt(7 * 3600)
            .expect("Invalid UTC+7 offset");
        let ts = Utc::now()
            .with_timezone(&wib)
            .format("%Y-%m-%dT%H:%M:%S%:z");
        writeln!(
            buf,
            "[{} {} {}] {}",
            ts,
            record.level(),
            record.target(),
            record.args()
        )
    })
    .init();

    log::info!("Starting OxideFeed v{}", env!("CARGO_PKG_VERSION"));

    // Load configuration from environment
    let config = Config::from_env();

    log::info!(
        "Monitoring {} RSS feed(s) every {} minute(s)",
        config.rss_feeds.len(),
        config.poll_interval_minutes
    );

    if config.onboarding_count > 0 {
        log::info!(
            "ONBOARDING MODE: will process up to {} existing articles on first boot",
            config.onboarding_count
        );
    }
    if config.auto_vacuum_days > 0 {
        log::info!(
            "Auto-vacuum: cleaning processed_news older than {} days",
            config.auto_vacuum_days
        );
    }
    if config.max_articles_per_cycle > 0 {
        let total_rpd: usize = config.gemini_models.iter().map(|m| m.rpd_limit).sum();
        log::info!(
            "Max articles per cycle: {} (multi-model fleet: {} models, total {} calls/day)",
            config.max_articles_per_cycle,
            config.gemini_models.len(),
            total_rpd,
        );
    }

    // Log per-model configuration
    for (i, model_cfg) in config.gemini_models.iter().enumerate() {
        log::info!(
            "  Model {}: {} | RPD cap: {} | RPM limit: {} (min interval: {}s)",
            i + 1,
            model_cfg.name,
            model_cfg.rpd_limit,
            model_cfg.rpm_limit,
            model_cfg.min_interval_secs(),
        );
    }

    // Log current whitelist/blacklist for debugging
    log::info!(
        "Whitelist ({} keywords): {:?}",
        config.whitelist.len(),
        config.whitelist
    );
    log::info!(
        "Blacklist ({} keywords): {:?}",
        config.blacklist.len(),
        config.blacklist
    );

    // Initialize SQLite storage
    let storage = Arc::new(
        Storage::open("oxide_feed.db")
            .expect("Failed to open SQLite database"),
    );

    log::info!("SQLite database initialized");

    // Create shared HTTP client with connection pooling
    let http_client = reqwest::Client::builder()
        .user_agent("OxideFeed/1.0")
        .build()
        .expect("Failed to build HTTP client");

    // Initialize multi-model router (distributes calls across 2 Gemini models — Opsi A)
    let model_router = Arc::new(ModelRouter::new(
        http_client.clone(),
        config.gemini_api_key.clone(),
        config.gemini_models.clone(),
    ));

    log::info!(
        "Model router initialized with {} model(s) (total RPD cap: {})",
        model_router.len(),
        model_router.total_rpd_limit(),
    );

    // Log which models are active
    for (name, rpd, rpm) in model_router.iter_models() {
        log::info!("  • {} — RPD: {}, RPM: {}", name, rpd, rpm);
    }

    // Initialize Telegram client
    let telegram_client = TelegramClient::new(
        http_client.clone(),
        config.telegram_bot_token.clone(),
        config.telegram_chat_id.clone(),
    );

    log::info!("Clients initialized. Entering main loop.");

    // Send startup notification
    if let Err(e) = telegram_client.send_notification(
        &format!(
            "🤖 OxideFeed v{} started\n\
             📰 {} feed(s), every {} min\n\
             🧠 {} model(s) — total {} RPD\n",
            env!("CARGO_PKG_VERSION"),
            config.rss_feeds.len(),
            config.poll_interval_minutes,
            model_router.len(),
            model_router.total_rpd_limit(),
        )
    ).await {
        log::warn!("Startup notification failed: {}", e);
    }

    // Main processing loop
    loop {
        log::info!("--- Starting processing cycle ---");

        if let Err(e) = process_cycle(
            &http_client,
            &storage,
            &model_router,
            &telegram_client,
            &config,
        )
        .await
        {
            log::error!("Processing cycle failed: {}", e);
            // Send error notification to Telegram
            let err_msg = format!(
                "⚠️ OxideFeed: Processing cycle failed\nError: {}\nCheck logs for details.",
                e
            );
            if let Err(notify_err) = telegram_client.send_notification(&err_msg).await {
                log::error!("Failed to send error notification: {}", notify_err);
            }
        }

        // Periodic database maintenance
        let cycle_count = CYCLE_COUNTER.fetch_add(1, Ordering::Relaxed);
        if config.auto_vacuum_days > 0 && cycle_count > 0 && cycle_count.is_multiple_of(10) {
            log::info!("Running periodic database maintenance...");
            if let Err(e) = storage.cleanup_old_processed(config.auto_vacuum_days) {
                log::error!("Database cleanup failed: {}", e);
            }
            if let Err(e) = storage.vacuum() {
                log::error!("Database vacuum failed: {}", e);
            }
        }

        log::info!(
            "--- Cycle complete. Sleeping for {} minute(s) ---",
            config.poll_interval_minutes
        );

        // Sleep for the configured interval
        tokio::time::sleep(tokio::time::Duration::from_secs(
            config.poll_interval_minutes * 60,
        ))
        .await;
    }
}

/// Execute one full processing pipeline cycle.
///
/// Multi-model distribution (Opsi A — Best Quality):
/// - Round-robin: gemini-3.5-flash-lite (primary, 475 RPD) + gemini-3.1-flash-lite (backup, 150 RPD)
/// - Per-model RPD guard: skips model if it has reached its daily cap
/// - Per-model RPM throttle: respects each model's rate limit
/// - Circuit breaker: artikel < 200 chars skip Gemini, kirim QUICK ALERT
async fn process_cycle(
    client: &reqwest::Client,
    storage: &Arc<Storage>,
    model_router: &Arc<ModelRouter>,
    telegram_client: &TelegramClient,
    config: &Config,
) -> Result<(), Box<dyn std::error::Error>> {
    // Step 1: Fetch RSS feeds and apply local filters
    log::info!("Step 1: Fetching and filtering RSS feeds");
    let mut new_items = fetch_and_filter(
        client,
        storage,
        &config.rss_feeds,
        config.process_existing,
        config.print_articles,
        &config.whitelist,
        &config.blacklist,
        config.onboarding_count,
        &config.contextual_pairs,
    ).await;

    if new_items.is_empty() {
        log::info!("No new items to process");
        return Ok(());
    }

    log::info!("Found {} new item(s) to process", new_items.len());

    // Step 1b: Prioritasi — sortir artikel terbaru dulu
    ingest::sort_by_newest(&mut new_items);

    // Step 1c: Batasi jumlah artikel per cycle
    let max_items = if config.max_articles_per_cycle > 0 {
        std::cmp::min(config.max_articles_per_cycle, new_items.len())
    } else {
        new_items.len()
    };

    if max_items < new_items.len() {
        log::info!(
            "Limiting to {} newest article(s) out of {} total (max_articles_per_cycle)",
            max_items,
            new_items.len()
        );
        // Mark skipped items as processed so they don't get re-fetched
        for item in new_items.iter().skip(max_items) {
            if let Err(e) = storage.mark_processed(&item.hash, &item.title) {
                log::error!("Failed to mark skipped item '{}': {}", item.title, e);
            }
        }
        new_items.truncate(max_items);
    }

    // Track how many Gemini calls we've made this cycle (per-model for logs)
    let mut gemini_calls_this_cycle = 0usize;

    // Process each item through the pipeline
    for item in &new_items {
        log::info!("Processing: {}", item.title);

        // Step 2: Scrape full article body
        log::info!("Step 2: Scraping full article");
        let full_body = scrape_full_body(client, &item.link, &item.summary).await;

        // ═══════════════════════════════════════════════════════
        // SHORT CONTENT CIRCUIT BREAKER:
        // Jika teks < 200 chars, skip Gemini — kirim QUICK ALERT
        // ═══════════════════════════════════════════════════════
        if full_body.len() < 200 {
            log::info!(
                "Step 3: Content too short ({} chars) — skipping Gemini. Sending QUICK ALERT.",
                full_body.len()
            );
            match telegram_client.send_quick_alert(
                &item.title,
                &item.link,
                "Gagal memuat teks lengkap. Buka tautan untuk membaca lebih lanjut.",
            ).await {
                Ok(()) => {
                    log::info!("Quick alert sent successfully. Committing state.");
                    if let Err(e) = storage.mark_processed(&item.hash, &item.title) {
                        log::error!("Failed to mark processed '{}': {}", item.title, e);
                    }
                    if let Some(ref pub_date) = item.pub_date {
                        if let Err(e) = storage.set_watermark(&item.feed_url, pub_date) {
                            log::error!("Failed to update watermark for {}: {}", item.feed_url, e);
                        }
                    }
                }
                Err(e) => {
                    log::error!(
                        "Quick alert delivery failed for '{}': {}. NOT committing state.",
                        item.title, e
                    );
                }
            }
            continue;
        }

        // ═══════════════════════════════════════════════════════
        // MODEL SELECTION: Round-robin with per-model RPD guard
        // ═══════════════════════════════════════════════════════
        let model = model_router.select_model(|model_name| {
            storage.get_today_api_usage(model_name).unwrap_or(0)
        });

        let model = match model {
            Some(m) => m,
            None => {
                // All models at RPD cap — send RAW BACKUP
                log::warn!(
                    "Step 3: All models at RPD cap. Sending '{}' as raw backup.",
                    item.title
                );
                let total_usage = storage.get_total_today_api_usage().unwrap_or(0);
                match telegram_client.send_raw_backup(
                    &item.title,
                    &format!(
                        "[All models at daily cap — {} total calls today. Max RPD: {}]",
                        total_usage,
                        model_router.total_rpd_limit(),
                    ),
                    &item.link,
                ).await {
                    Ok(()) => {
                        if let Err(e) = storage.mark_processed(&item.hash, &item.title) {
                            log::error!("Failed to mark processed '{}': {}", item.title, e);
                        }
                        if let Some(ref pub_date) = item.pub_date {
                            if let Err(e) = storage.set_watermark(&item.feed_url, pub_date) {
                                log::error!("Failed to update watermark for {}: {}", item.feed_url, e);
                            }
                        }
                    }
                    Err(e) => {
                        log::error!(
                            "Raw backup (all capped) failed for '{}': {}. NOT committing.",
                            item.title, e
                        );
                    }
                }
                continue;
            }
        };

        // Step 3a: Per-model rate limit (RPM throttle)
        log::info!(
            "Step 3: LLM processing via {} (call #{})",
            model.model_label(),
            gemini_calls_this_cycle + 1
        );
        model.enforce_rate_limit().await;
        gemini_calls_this_cycle += 1;

        // Increment usage BEFORE the call (conservative: prevent quota overrun)
        if let Err(e) = storage.increment_api_usage(model.model_label()) {
            log::error!("Failed to increment API usage for {}: {}", model.model_label(), e);
        }

        match model.process_article(&item.title, &full_body).await {
            ArticleResult::Rejected { reason } => {
                let reason_str = reason.as_deref().unwrap_or("no reason");
                log::info!(
                    "[{}] REJECTED '{}' — {}",
                    model.model_label(),
                    item.title,
                    reason_str
                );
                if let Err(e) = storage.mark_processed(&item.hash, &item.title) {
                    log::error!("Failed to mark processed '{}': {}", item.title, e);
                }
            }
            ArticleResult::Sanitized(sanitized) => {
                // Step 4: Send formatted news to Telegram
                log::info!("Step 4: Sending to Telegram");
                match telegram_client.send_news(&sanitized, &item.link).await {
                    Ok(()) => {
                        // Transaction commit: only save state after successful delivery
                        log::info!("Telegram delivery successful. Committing state.");
                        if let Err(e) = storage.mark_processed(&item.hash, &item.title) {
                            log::error!("Failed to mark processed '{}': {}", item.title, e);
                        }

                        if let Some(ref pub_date) = item.pub_date {
                            if let Err(e) = storage.set_watermark(&item.feed_url, pub_date) {
                                log::error!(
                                    "Failed to update watermark for {}: {}",
                                    item.feed_url, e
                                );
                            }
                        }
                    }
                    Err(e) => {
                        log::error!(
                            "Telegram delivery failed for '{}': {}. NOT committing state.",
                            item.title, e
                        );
                    }
                }
            }
            ArticleResult::Fallback(raw_text) => {
                // Fallback: send raw backup to Telegram
                log::warn!(
                    "[{}] FALLBACK for '{}'. Sending raw backup.",
                    model.model_label(),
                    item.title
                );
                match telegram_client.send_raw_backup(&item.title, &raw_text, &item.link).await {
                    Ok(()) => {
                        if let Err(e) = storage.mark_processed(&item.hash, &item.title) {
                            log::error!("Failed to mark processed '{}': {}", item.title, e);
                        }
                        if let Some(ref pub_date) = item.pub_date {
                            if let Err(e) = storage.set_watermark(&item.feed_url, pub_date) {
                                log::error!(
                                    "Failed to update watermark for {}: {}",
                                    item.feed_url, e
                                );
                            }
                        }
                    }
                    Err(e) => {
                        log::error!(
                            "Raw backup delivery failed for '{}': {}. NOT committing state.",
                            item.title, e
                        );
                    }
                }
            }
        }
    }

    // Log cycle summary with per-model usage
    log_cycle_summary(storage, model_router);

    // Cleanup old API usage records (once per cycle)
    if let Err(e) = storage.cleanup_old_usage_records() {
        log::error!("Failed to cleanup old usage records: {}", e);
    }

    Ok(())
}

/// Log per-model and total usage after each cycle
fn log_cycle_summary(storage: &Arc<Storage>, model_router: &ModelRouter) {
    for (name, rpd, _rpm) in model_router.iter_models() {
        let usage = storage.get_today_api_usage(name).unwrap_or(0);
        if usage > 0 {
            log::info!("Usage [{}]: {} calls today (cap: {})", name, usage, rpd);
        }
    }

    let total = storage.get_total_today_api_usage().unwrap_or(0);
    log::info!(
        "Total API usage today: {} calls (fleet cap: {})",
        total,
        model_router.total_rpd_limit()
    );
}
