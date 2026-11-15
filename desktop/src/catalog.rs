use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

pub const CATALOG_URL: &str = "https://plainraw.com/json/cbc833011422";

#[derive(Clone, Debug, Deserialize)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct Catalog {
    pub channels: Vec<Channel>,
}

pub struct LoadedCatalog {
    pub channels: Vec<Channel>,
    pub source: &'static str,
}

pub fn parse_catalog(body: &str) -> Result<Vec<Channel>, String> {
    let catalog: Catalog =
        serde_json::from_str(body).map_err(|err| format!("Invalid channels JSON: {err}"))?;

    if catalog.channels.is_empty() {
        return Err("The channel catalog is empty".into());
    }

    let mut ids = HashSet::new();
    for channel in &catalog.channels {
        if channel.id.trim().is_empty() || channel.name.trim().is_empty() {
            return Err("A channel is missing its id or name".into());
        }
        if !channel.url.starts_with("https://") && !channel.url.starts_with("http://") {
            return Err(format!("Invalid stream URL for {}", channel.name));
        }
        if !ids.insert(&channel.id) {
            return Err(format!("Duplicate channel ID: {}", channel.id));
        }
    }
    Ok(catalog.channels)
}

fn cache_path() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("kanalik-gnome").join("channels.json")
}

pub fn load_catalog() -> Result<LoadedCatalog, String> {
    let online = (|| {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(12))
            .build()
            .map_err(|err| err.to_string())?;
        let body = client
            .get(CATALOG_URL)
            .send()
            .and_then(|response| response.error_for_status())
            .and_then(|response| response.text())
            .map_err(|err| err.to_string())?;
        let channels = parse_catalog(&body)?;
        Ok::<_, String>((channels, body))
    })();

    match online {
        Ok((channels, body)) => {
            let path = cache_path();
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(path, body);
            Ok(LoadedCatalog {
                channels,
                source: "Live catalog",
            })
        }
        Err(network_error) => {
            let cached = fs::read_to_string(cache_path())
                .map_err(|_| format!("Cannot fetch channel catalog: {network_error}"))?;
            let channels = parse_catalog(&cached)
                .map_err(|err| format!("Cannot fetch channel catalog: {network_error}; cache: {err}"))?;
            Ok(LoadedCatalog {
                channels,
                source: "Cached catalog (offline)",
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_catalog_shape_and_headers() {
        let channels = parse_catalog(
            r#"{"version":14,"channels":[{"id":"joj-24","name":"JOJ 24","url":"https://example.org/playlist.m3u8","headers":{"Referer":"https://example.org/"}, "epg":{"openEpg":"JOJ 24"}}]}"#,
        )
        .unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].headers["Referer"], "https://example.org/");
    }

    #[test]
    fn rejects_duplicate_ids() {
        let json = r#"{"channels":[{"id":"x","name":"X","url":"https://example.org/a"},{"id":"x","name":"Y","url":"https://example.org/b"}]}"#;
        assert!(parse_catalog(json).unwrap_err().contains("Duplicate"));
    }

    #[test]
    fn rejects_non_http_urls() {
        let json = r#"{"channels":[{"id":"x","name":"X","url":"file:///etc/passwd"}]}"#;
        assert!(parse_catalog(json).is_err());
    }
}
