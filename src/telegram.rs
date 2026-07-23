use crate::llm::SanitizedOutput;
use reqwest::Client;
use serde::Deserialize;

/// Telegram Bot API base URL
const TELEGRAM_API_BASE: &str = "https://api.telegram.org/bot";

/// Response from Telegram API
#[derive(Debug, Deserialize)]
struct TelegramResponse {
    ok: bool,
    description: Option<String>,
}

/// Telegram client for sending messages
pub struct TelegramClient {
    client: Client,
    bot_token: String,
    chat_id: String,
}

impl TelegramClient {
    pub fn new(client: Client, bot_token: String, chat_id: String) -> Self {
        TelegramClient {
            client,
            bot_token,
            chat_id,
        }
    }

    /// Send a sanitized news item as a formatted Telegram message
    pub async fn send_news(
        &self,
        output: &SanitizedOutput,
        original_link: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let markdown = self.format_markdown(output, original_link);
        self.send_message(&markdown).await
    }

    /// Send a raw backup message when JSON sanitization fails
    pub async fn send_raw_backup(
        &self,
        title: &str,
        raw_text: &str,
        original_link: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let message = format!(
            "[RAW BACKUP BUFFER]\n\n*{}*\n\n{}\n\n🔗 {}",
            self.escape_markdown(title),
            self.escape_markdown(raw_text),
            self.escape_markdown(original_link),
        );
        self.send_message(&message).await
    }

    /// Send a quick alert for short content (bypasses Gemini).
    /// Formatted as a mini alert with link to read more.
    pub async fn send_quick_alert(
        &self,
        title: &str,
        original_link: &str,
        reason: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let message = format!(
            "⚡ [QUICK NEWS]\n\n*{}*\n\n_{}_\n\n🔗 {}",
            self.escape_markdown(title),
            self.escape_markdown(reason),
            self.escape_markdown(original_link),
        );
        self.send_message(&message).await
    }

    /// Send a plain-text notification message to the Telegram channel.
    /// Use this for errors, warnings, or startup notifications.
    /// No markdown formatting — plain text only.
    pub async fn send_notification(
        &self,
        text: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let url = format!("{}{}/sendMessage", TELEGRAM_API_BASE, self.bot_token);

        let body = serde_json::json!({
            "chat_id": self.chat_id,
            "text": text,
            "disable_web_page_preview": true,
        });

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let telegram_resp: TelegramResponse = resp.json().await?;

        if status.is_success() && telegram_resp.ok {
            log::info!("Telegram notification sent successfully");
            Ok(())
        } else {
            let err_msg = telegram_resp
                .description
                .unwrap_or_else(|| "Unknown Telegram error".to_string());
            log::error!("Telegram notification error ({}): {}", status, err_msg);
            Err(format!("Telegram notification error: {}", err_msg).into())
        }
    }

    /// Format structured output into Telegram Markdown V2
    fn format_markdown(&self, output: &SanitizedOutput, link: &str) -> String {
        let topik = self.escape_markdown(&output.topik);
        let kategori = self.escape_markdown(&output.kategori);
        let signifikansi = self.escape_markdown(&output.signifikansi);
        let fakta_keras: String = output
            .fakta_keras
            .iter()
            .map(|f| format!("• {}", self.escape_markdown(f)))
            .collect::<Vec<_>>()
            .join("\n");
        let relevansi = self.escape_markdown(&output.relevansi);
        let escaped_link = self.escape_markdown(link);

        // Pilih simbol signifikansi
        let sig_simbol = match output.signifikansi.as_str() {
            "tinggi" => "🔴",
            "sedang" => "🟡",
            _ => "🟢",
        };

        format!(
            "📢 *{}*\n\
             {} · {} {}\
             \n\n\
             *Fakta Keras:*\n\
             {}\n\n\
             *Relevansi:*\n\
             {}\n\n\
             🔗 {}",
            topik, kategori, sig_simbol, signifikansi, fakta_keras, relevansi, escaped_link,
        )
    }

    /// Escape special characters for Telegram Markdown V2
    fn escape_markdown(&self, text: &str) -> String {
        // Characters that need escaping in Markdown V2: _ * [ ] ( ) ~ ` > # + - = | { } . !
        let special_chars = [
            '_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!',
        ];
        let mut result = String::with_capacity(text.len() + 16);
        for c in text.chars() {
            if special_chars.contains(&c) {
                result.push('\\');
            }
            result.push(c);
        }
        result
    }

    /// Send a raw message to the Telegram chat (with Markdown V2)
    async fn send_message(&self, text: &str) -> Result<(), Box<dyn std::error::Error>> {
        let url = format!("{}{}/sendMessage", TELEGRAM_API_BASE, self.bot_token);

        let body = serde_json::json!({
            "chat_id": self.chat_id,
            "text": text,
            "parse_mode": "MarkdownV2",
            "disable_web_page_preview": false,
        });

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let telegram_resp: TelegramResponse = resp.json().await?;

        if status.is_success() && telegram_resp.ok {
            log::info!("Telegram message sent successfully");
            Ok(())
        } else {
            let err_msg = telegram_resp
                .description
                .unwrap_or_else(|| "Unknown Telegram error".to_string());
            log::error!("Telegram API error ({}): {}", status, err_msg);
            Err(format!("Telegram API error: {}", err_msg).into())
        }
    }
}
