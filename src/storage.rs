use rusqlite::{Connection, params, Result as SqlResult};
use std::sync::Mutex;

/// Thread-safe wrapper around SQLite connection
pub struct Storage {
    conn: Mutex<Connection>,

}

impl Storage {
    /// Open (or create) the SQLite database and initialize tables
    pub fn open(path: &str) -> SqlResult<Self> {
        let conn = Connection::open(path)?;
        // Enable WAL mode for better concurrent performance
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        let storage = Storage {
            conn: Mutex::new(conn),
        };
        storage.initialize_tables()?;
        Ok(storage)
    }

    /// Create tables if they do not exist
    fn initialize_tables(&self) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS processed_news (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                processed_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            );

            CREATE TABLE IF NOT EXISTS rss_states (
                feed_url TEXT PRIMARY KEY,
                last_fetched_pub_date TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS daily_api_usage (
                date TEXT NOT NULL,
                model_name TEXT NOT NULL,
                call_count INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (date, model_name)
            );

            CREATE TABLE IF NOT EXISTS model_daily_usage (
                date TEXT NOT NULL,
                model_name TEXT NOT NULL,
                call_count INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (date, model_name)
            );

            CREATE INDEX IF NOT EXISTS idx_processed_at ON processed_news(processed_at);
            "
        )?;
        Ok(())
    }

    /// Check if a news item hash has already been processed
    pub fn is_processed(&self, hash: &str) -> SqlResult<bool> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM processed_news WHERE id = ?1",
            params![hash],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Mark a news item as processed by inserting its hash
    pub fn mark_processed(&self, hash: &str, title: &str) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT OR IGNORE INTO processed_news (id, title) VALUES (?1, ?2)",
            params![hash, title],
        )?;
        Ok(())
    }

    /// Get the last fetched publication date for a feed
    pub fn get_watermark(&self, feed_url: &str) -> SqlResult<Option<String>> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let result = conn.query_row(
            "SELECT last_fetched_pub_date FROM rss_states WHERE feed_url = ?1",
            params![feed_url],
            |row| row.get(0),
        );
        match result {
            Ok(date) => Ok(Some(date)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Set (insert or update) the watermark for a feed
    pub fn set_watermark(&self, feed_url: &str, pub_date: &str) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT OR REPLACE INTO rss_states (feed_url, last_fetched_pub_date) VALUES (?1, ?2)",
            params![feed_url, pub_date],
        )?;
        Ok(())
    }

    /// Vacuum the database to reclaim disk space.
    /// Should be called periodically (e.g., once a day).
    pub fn vacuum(&self) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute_batch("VACUUM;")?;
        log::info!("Database vacuum completed");
        Ok(())
    }

    /// Delete processed_news records older than `days` days.
    /// Returns the number of deleted rows.
    pub fn cleanup_old_processed(&self, days: u64) -> SqlResult<usize> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let deleted = conn.execute(
            "DELETE FROM processed_news WHERE processed_at < strftime('%Y-%m-%dT%H:%M:%SZ', 'now', ?1)",
            params![format!("-{} days", days)],
        )?;
        if deleted > 0 {
            log::info!("Cleaned up {} old processed_news records (> {} days)", deleted, days);
        }
        Ok(deleted)
    }

    // ═══════════════════════════════════════════════════════════
    // Daily API Usage Tracking (RPD Guard)
    // ═══════════════════════════════════════════════════════════

    // ═══════════════════════════════════════════════════════════
    // Per-Model Daily API Usage Tracking (RPD Guard)
    // ═══════════════════════════════════════════════════════════

    /// Get today's Gemini API call count for a specific model.
    /// Returns 0 if no record for today yet.
    pub fn get_today_api_usage(&self, model_name: &str) -> SqlResult<usize> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let result = conn.query_row(
            "SELECT COALESCE(call_count, 0) FROM model_daily_usage WHERE date = ?1 AND model_name = ?2",
            params![today, model_name],
            |row| row.get::<_, i64>(0),
        );
        match result {
            Ok(count) => Ok(count as usize),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
            Err(e) => Err(e),
        }
    }

    /// Get combined API usage across ALL models for today.
    /// Useful for global logging / diagnostics.
    pub fn get_total_today_api_usage(&self) -> SqlResult<usize> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let result = conn.query_row(
            "SELECT COALESCE(SUM(call_count), 0) FROM model_daily_usage WHERE date = ?1",
            params![today],
            |row| row.get::<_, i64>(0),
        );
        match result {
            Ok(count) => Ok(count as usize),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
            Err(e) => Err(e),
        }
    }

    /// Increment today's API usage for a specific model.
    pub fn increment_api_usage(&self, model_name: &str) -> SqlResult<()> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        conn.execute(
            "INSERT INTO model_daily_usage (date, model_name, call_count) VALUES (?1, ?2, 1)
             ON CONFLICT(date, model_name) DO UPDATE SET call_count = call_count + 1",
            params![today, model_name],
        )?;
        Ok(())
    }

    /// Clean up old model usage records (older than 30 days).
    pub fn cleanup_old_usage_records(&self) -> SqlResult<usize> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let deleted = conn.execute(
            "DELETE FROM model_daily_usage WHERE date < strftime('%Y-%m-%d', 'now', '-30 days')",
            [],
        )?;
        if deleted > 0 {
            log::info!("Cleaned up {} old model API usage records", deleted);
        }
        Ok(deleted)
    }
}
