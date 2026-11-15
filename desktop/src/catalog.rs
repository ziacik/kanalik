use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
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
    pub source: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CatalogSource {
    #[default]
    Remote,
    Local(PathBuf),
}

fn source_config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("kanalik-gnome").join("catalog-source")
}

fn read_source_config(path: &Path) -> CatalogSource {
    match fs::read_to_string(path) {
        Ok(value) if !value.trim().is_empty() => {
            CatalogSource::Local(PathBuf::from(value.trim_end_matches(['\r', '\n'])))
        }
        _ => CatalogSource::Remote,
    }
}

fn write_source_config(path: &Path, source: &CatalogSource) -> Result<(), String> {
    match source {
        CatalogSource::Remote => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!("Cannot reset catalog preference: {err}")),
        },
        CatalogSource::Local(selected_path) => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|err| format!("Cannot create config directory: {err}"))?;
            }
            fs::write(path, selected_path.to_string_lossy().as_bytes())
                .map_err(|err| format!("Cannot save catalog preference: {err}"))
        }
    }
}

pub fn load_saved_source() -> CatalogSource {
    read_source_config(&source_config_path())
}

pub fn save_source(source: &CatalogSource) -> Result<(), String> {
    write_source_config(&source_config_path(), source)
}

pub fn load_catalog(source: &CatalogSource) -> Result<LoadedCatalog, String> {
    match source {
        CatalogSource::Remote => load_remote_catalog(),
        CatalogSource::Local(path) => {
            let body = fs::read_to_string(path).map_err(|err| {
                format!("Cannot read channels JSON from {}: {err}", path.display())
            })?;
            let channels = parse_catalog(&body)?;
            Ok(LoadedCatalog {
                channels,
                source: format!("Local JSON: {}", path.display()),
            })
        }
    }
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

fn load_remote_catalog() -> Result<LoadedCatalog, String> {
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
                source: "Live catalog".into(),
            })
        }
        Err(network_error) => {
            let cached = fs::read_to_string(cache_path())
                .map_err(|_| format!("Cannot fetch channel catalog: {network_error}"))?;
            let channels = parse_catalog(&cached).map_err(|err| {
                format!("Cannot fetch channel catalog: {network_error}; cache: {err}")
            })?;
            Ok(LoadedCatalog {
                channels,
                source: "Cached catalog (offline)".into(),
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

    #[test]
    fn loads_local_json_and_preserves_custom_headers() {
        let directory = std::env::temp_dir().join(format!(
            "kanalik-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let file = directory.join("channels.json");
        fs::write(
            &file,
            r#"{"channels":[{"id":"test","name":"My TV","url":"https://example.com/live.m3u8","headers":{"Referer":"https://example.com/"}}]}"#,
        )
        .unwrap();

        let source = CatalogSource::Local(file.clone());
        let loaded = load_catalog(&source).unwrap();
        assert_eq!(loaded.channels[0].name, "My TV");
        assert_eq!(
            loaded.channels[0].headers["Referer"],
            "https://example.com/"
        );
        assert!(loaded.source.contains("channels.json"));

        let config = directory.join("config").join("source");
        write_source_config(&config, &source).unwrap();
        assert_eq!(read_source_config(&config), source);
        write_source_config(&config, &CatalogSource::Remote).unwrap();
        assert_eq!(read_source_config(&config), CatalogSource::Remote);

        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn invalid_local_json_is_rejected() {
        let directory = std::env::temp_dir().join(format!(
            "kanalik-invalid-json-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        let file = directory.join("invalid.json");
        fs::write(
            &file,
            r#"{"channels":[{"name":"missing ID","url":"https://example.com/"}]}"#,
        )
        .unwrap();
        assert!(load_catalog(&CatalogSource::Local(file)).is_err());
        fs::remove_dir_all(&directory).unwrap();
    }
}
