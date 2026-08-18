//! Weather for desktop widgets and the taskbar clock.
//!
//! Location order (Windows is first on purpose — that is the same
//! `Windows.Devices.Geolocation` stack the OS weather/widgets use):
//!   1. `?location=` typed by the user
//!   2. WinRT Geolocator (Wi‑Fi / GNSS via `lfsvc`)
//!   3. Settings → Privacy → Location → Default location
//!   4. Saved city in `.versailles/weather-location.txt`
//!
//! ISP IP geolocation is never used. On this machine it resolves to the
//! Vodafone POP (Hadamar), not the actual city. Forecast is Open-Meteo.

use serde::Serialize;
use serde_json::{json, Value};
use std::fs;
use std::sync::Mutex;
use std::time::{Duration, Instant};

static LIVE_FAIL_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

fn location_file() -> Option<std::path::PathBuf> {
    crate::registry::widgets_root()
        .ok()
        .map(|root| root.join(".versailles").join("weather-location.txt"))
}

fn read_saved_location() -> Option<String> {
    let path = location_file()?;
    let text = fs::read_to_string(path).ok()?;
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

pub fn write_saved_location(raw: &str) -> Result<(), String> {
    let path = location_file().ok_or("Widgets folder not found")?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        let _ = fs::remove_file(&path);
        return Ok(());
    }
    fs::write(path, trimmed).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeatherPayload {
    pub ok: bool,
    pub place: String,
    pub temp_c: i32,
    pub desc: String,
    pub high_c: i32,
    pub low_c: i32,
    pub glyph: String,
    pub lat: f64,
    pub lon: f64,
    pub source: String,
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("Versailles/0.1 (local weather widget; mark@localhost)")
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

#[cfg(windows)]
fn windows_live_coordinates() -> Result<(f64, f64), String> {
    use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator, PositionAccuracy};

    if let Ok(guard) = LIVE_FAIL_UNTIL.lock() {
        if let Some(until) = *guard {
            if Instant::now() < until {
                return Err("location-service-off".into());
            }
        }
    }

    let locator = Geolocator::new().map_err(|e| e.to_string())?;
    let _ = locator.AllowFallbackToConsentlessPositions();
    let _ = locator.SetDesiredAccuracy(PositionAccuracy::Default);

    let access = Geolocator::RequestAccessAsync()
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    if access != GeolocationAccessStatus::Allowed {
        return Err("location-denied".into());
    }

    let pos = locator
        .GetGeopositionAsync()
        .map_err(|e| e.to_string())?
        .get()
        .map_err(|e| e.to_string())?;
    let coord = pos.Coordinate().map_err(|e| e.to_string())?;
    let point = coord.Point().map_err(|e| e.to_string())?;
    let geo = point.Position().map_err(|e| e.to_string())?;
    Ok((geo.Latitude, geo.Longitude))
}

#[cfg(windows)]
fn remember_live_fail(err: &str) {
    if err.contains("0x80070422")
        || err.contains("deaktiviert")
        || err.contains("cannot be started")
        || err.contains("location-service-off")
    {
        if let Ok(mut guard) = LIVE_FAIL_UNTIL.lock() {
            *guard = Some(Instant::now() + Duration::from_secs(30 * 60));
        }
    }
}

#[cfg(windows)]
fn windows_default_coordinates() -> Result<(f64, f64), String> {
    use windows::Devices::Geolocation::Geolocator;

    let pref = Geolocator::DefaultGeoposition().map_err(|e| e.to_string())?;
    let geo = pref.Value().map_err(|_| "no-default".to_string())?;
    Ok((geo.Latitude, geo.Longitude))
}

/// Same OS location stack Windows weather uses. Live Wi‑Fi/GNSS first,
/// then the Settings default pin (works even when `lfsvc` is off).
#[cfg(windows)]
fn windows_coordinates() -> Result<(f64, f64, &'static str), String> {
    match windows_live_coordinates() {
        Ok((lat, lon)) => Ok((lat, lon, "windows")),
        Err(live_err) => {
            remember_live_fail(&live_err);
            match windows_default_coordinates() {
            Ok((lat, lon)) => Ok((lat, lon, "windows-default")),
            Err(_) => Err(live_err),
            }
        }
    }
}

#[cfg(not(windows))]
fn windows_coordinates() -> Result<(f64, f64, &'static str), String> {
    Err("Windows location is only available on Windows".into())
}

fn parse_coords(raw: &str) -> Option<(f64, f64)> {
    let s = raw.trim().trim_start_matches('~');
    let (a, b) = s.split_once(',')?;
    let lat = a.trim().parse().ok()?;
    let lon = b.trim().parse().ok()?;
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    Some((lat, lon))
}

async fn geocode_city(name: &str) -> Result<(f64, f64, String), String> {
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json",
        urlencoding_lite(name)
    );
    let body: Value = http()
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let hit = body
        .get("results")
        .and_then(|r| r.as_array())
        .and_then(|a| a.first())
        .ok_or_else(|| format!("No place named '{name}'"))?;
    let lat = hit.get("latitude").and_then(|v| v.as_f64()).ok_or("geocode")?;
    let lon = hit
        .get("longitude")
        .and_then(|v| v.as_f64())
        .ok_or("geocode")?;
    let label = hit
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(name)
        .to_string();
    Ok((lat, lon, label))
}

fn urlencoding_lite(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

async fn reverse_place(lat: f64, lon: f64) -> String {
    let url = format!(
        "https://nominatim.openstreetmap.org/reverse?lat={lat:.5}&lon={lon:.5}&format=json"
    );
    let Ok(body) = http().get(url).send().await else {
        return "Local".into();
    };
    let Ok(body) = body.json::<Value>().await else {
        return "Local".into();
    };
    let addr = body.get("address");
    for key in ["city", "town", "village", "municipality", "suburb", "county"] {
        if let Some(name) = addr.and_then(|a| a.get(key)).and_then(|v| v.as_str()) {
            if !name.is_empty() {
                return name.to_string();
            }
        }
    }
    "Local".into()
}

fn wmo_desc_glyph(code: i64) -> (&'static str, &'static str) {
    match code {
        0 => ("Clear", "sun"),
        1 => ("Mostly clear", "sun"),
        2 => ("Partly cloudy", "cloud"),
        3 => ("Overcast", "cloud"),
        45 | 48 => ("Fog", "fog"),
        51 | 53 | 55 | 56 | 57 => ("Drizzle", "rain"),
        61 | 63 | 65 | 66 | 67 | 80 | 81 | 82 => ("Rain", "rain"),
        71 | 73 | 75 | 77 | 85 | 86 => ("Snow", "snow"),
        95 | 96 | 99 => ("Thunderstorm", "storm"),
        _ => ("Clouds", "cloud"),
    }
}

async fn forecast(lat: f64, lon: f64) -> Result<(i32, i32, i32, &'static str, &'static str), String> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat:.5}&longitude={lon:.5}&current=temperature_2m,weather_code&daily=temperature_2m_max,temperature_2m_min&timezone=auto"
    );
    let body: Value = http()
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let current = body.get("current").ok_or("forecast")?;
    let temp = current
        .get("temperature_2m")
        .and_then(|v| v.as_f64())
        .ok_or("temp")? as i32;
    let code = current
        .get("weather_code")
        .and_then(|v| v.as_i64())
        .unwrap_or(2);
    let daily = body.get("daily");
    let high = daily
        .and_then(|d| d.get("temperature_2m_max"))
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_f64())
        .unwrap_or(temp as f64) as i32;
    let low = daily
        .and_then(|d| d.get("temperature_2m_min"))
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_f64())
        .unwrap_or(temp as f64) as i32;
    let (desc, glyph) = wmo_desc_glyph(code);
    Ok((temp, high, low, desc, glyph))
}

async fn resolve_named(raw: &str, source: &str) -> Result<(f64, f64, String, String), String> {
    if let Some((lat, lon)) = parse_coords(raw) {
        Ok((lat, lon, String::new(), source.to_string()))
    } else {
        let (lat, lon, label) = geocode_city(raw).await?;
        Ok((lat, lon, label, source.to_string()))
    }
}

pub struct WeatherQuery {
    pub location: Option<String>,
    pub save: bool,
}

pub async fn lookup(q: WeatherQuery) -> Result<WeatherPayload, String> {
    let explicit = q
        .location
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    if q.save {
        write_saved_location(explicit.as_deref().unwrap_or(""))?;
    }

    let (lat, lon, mut place, source) = if let Some(raw) = explicit {
        resolve_named(&raw, "manual").await?
    } else {
        match tokio::task::spawn_blocking(windows_coordinates)
            .await
            .map_err(|e| e.to_string())?
        {
            Ok((lat, lon, source)) => (lat, lon, String::new(), source.to_string()),
            Err(err) => {
                if let Some(saved) = read_saved_location() {
                    resolve_named(&saved, "saved").await?
                } else {
                    let hint = if err.contains("0x80070422")
                        || err.contains("deaktiviert")
                        || err.contains("cannot be started")
                    {
                        "Windows location is off. Type your city and press Enter."
                    } else if err.contains("location-denied") {
                        "Allow location for desktop apps, or type your city and press Enter."
                    } else {
                        "Type your city and press Enter."
                    };
                    return Err(hint.into());
                }
            }
        }
    };

    if place.is_empty() {
        place = reverse_place(lat, lon).await;
    }

    let (temp_c, high_c, low_c, desc, glyph) = forecast(lat, lon).await?;
    Ok(WeatherPayload {
        ok: true,
        place,
        temp_c,
        desc: desc.into(),
        high_c,
        low_c,
        glyph: glyph.into(),
        lat,
        lon,
        source,
    })
}

pub fn error_json(msg: impl Into<String>) -> Value {
    json!({ "ok": false, "error": msg.into() })
}

pub fn plain_text(payload: &WeatherPayload) -> String {
    format!("{} {}°", payload.desc, payload.temp_c)
}
