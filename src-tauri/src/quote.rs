//! Daily quote for the desktop card.
//!
//! Primary: [ZenQuotes](https://zenquotes.io/api/today) — one line per UTC day.
//! Fallback: [FixQuotes QOTD RSS](https://fixquotes.com/feeds/qotd.rss).
//! Cached in memory until the UTC date rolls so we do not hammer free tiers.

use serde::Serialize;
use serde_json::Value;
use std::sync::Mutex;
use std::time::Duration;

const ZEN_TODAY: &str = "https://zenquotes.io/api/today";
const FIX_RSS: &str = "https://fixquotes.com/feeds/qotd.rss";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotePayload {
    pub ok: bool,
    pub text: String,
    pub author: String,
    pub source: String,
}

struct Cache {
    day: String,
    payload: QuotePayload,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

fn utc_day() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("Deck/0.1 (local quote widget; https://zenquotes.io)")
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

fn cached() -> Option<QuotePayload> {
    let guard = CACHE.lock().ok()?;
    let hit = guard.as_ref()?;
    if hit.day == utc_day() {
        Some(hit.payload.clone())
    } else {
        None
    }
}

fn store(payload: QuotePayload) {
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some(Cache {
            day: utc_day(),
            payload,
        });
    }
}

fn decode_xml(raw: &str) -> String {
    raw.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .trim()
        .to_string()
}

fn tag_between(hay: &str, open: &str, close: &str) -> Option<String> {
    let start = hay.find(open)? + open.len();
    let rest = hay.get(start..)?;
    let end = rest.find(close)?;
    Some(decode_xml(&rest[..end]))
}

fn parse_zen(body: &Value) -> Option<QuotePayload> {
    let row = body.as_array()?.first()?;
    let text = row.get("q")?.as_str()?.trim();
    if text.is_empty() {
        return None;
    }
    let author = row
        .get("a")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .trim();
    Some(QuotePayload {
        ok: true,
        text: text.to_string(),
        author: if author.is_empty() {
            "Unknown".into()
        } else {
            author.to_string()
        },
        source: "zenquotes".into(),
    })
}

fn parse_fix_rss(xml: &str) -> Option<QuotePayload> {
    let item_start = xml.find("<item>")?;
    let item_end = xml[item_start..].find("</item>")?;
    let item = &xml[item_start..item_start + item_end];
    let text = tag_between(item, "<description>", "</description>")?;
    if text.is_empty() {
        return None;
    }
    let author = tag_between(item, "<dc:creator>", "</dc:creator>")
        .or_else(|| tag_between(item, "<title>", "</title>"))
        .unwrap_or_else(|| "Unknown".into());
    Some(QuotePayload {
        ok: true,
        text,
        author,
        source: "fixquotes".into(),
    })
}

async fn fetch_zen(client: &reqwest::Client) -> Result<QuotePayload, String> {
    let body: Value = client
        .get(ZEN_TODAY)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    parse_zen(&body).ok_or_else(|| "ZenQuotes returned an empty quote".into())
}

async fn fetch_fix(client: &reqwest::Client) -> Result<QuotePayload, String> {
    let xml = client
        .get(FIX_RSS)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    parse_fix_rss(&xml).ok_or_else(|| "FixQuotes RSS had no item".into())
}

pub async fn lookup() -> Result<QuotePayload, String> {
    if let Some(hit) = cached() {
        return Ok(hit);
    }
    let client = http();
    let payload = match fetch_zen(&client).await {
        Ok(p) => p,
        Err(zen_err) => {
            tracing::warn!("ZenQuotes today failed: {zen_err}");
            fetch_fix(&client).await.map_err(|rss_err| {
                format!("quote feeds unavailable ({zen_err}; rss: {rss_err})")
            })?
        }
    };
    store(payload.clone());
    Ok(payload)
}

pub fn error_json(err: impl ToString) -> Value {
    serde_json::json!({ "ok": false, "error": err.to_string() })
}
