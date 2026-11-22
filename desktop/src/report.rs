//! Exportable per-channel availability reports.
//! CSV stays in catalog order so results are easy to compare across runs.

use std::collections::HashMap;
use std::time::Duration;

use crate::catalog::Channel;
use crate::checker::CheckStatus;

#[derive(Debug)]
pub struct ReportEntry {
    pub status: CheckStatus,
    pub elapsed: Duration,
    pub checked_at: String,
    pub details: String,
}

pub fn to_csv(
    channels: &[Channel],
    results: &HashMap<String, ReportEntry>,
    running: bool,
) -> String {
    let mut csv = String::from("ID,Channel,URL,Status,Duration (s),Checked at,Details\r\n");

    for channel in channels {
        let mut duration = String::new();
        let (status, checked_at, details) = match results.get(&channel.id) {
            Some(entry) => {
                duration = format!("{:.2}", entry.elapsed.as_secs_f64());
                let status = match entry.status {
                    CheckStatus::Online => "Online",
                    CheckStatus::Failed => "Failed",
                    CheckStatus::TimedOut => "Unknown (timeout)",
                };
                (status, entry.checked_at.as_str(), entry.details.as_str())
            }
            None if running => ("Pending", "", ""),
            None => ("Not checked", "", ""),
        };

        let cells = [
            channel.id.as_str(),
            channel.name.as_str(),
            channel.url.as_str(),
            status,
            duration.as_str(),
            checked_at,
            details,
        ];
        csv.push_str(
            &cells
                .iter()
                .map(|value| csv_cell(value))
                .collect::<Vec<_>>()
                .join(","),
        );
        csv.push_str("\r\n");
    }

    csv
}

/// Always quote fields, including embedded commas, quotation marks and newlines.
/// Guard against CSV formula injection when a report is opened in a spreadsheet.
fn csv_cell(value: &str) -> String {
    let dangerous = value.trim_start().starts_with(['=', '+', '-', '@']);
    let safe = if dangerous {
        format!("'{value}")
    } else {
        value.to_owned()
    };
    format!("\"{}\"", safe.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn channel(id: &str, name: &str) -> Channel {
        Channel {
            id: id.to_owned(),
            name: name.to_owned(),
            url: "https://example.com/stream.m3u8".to_owned(),
            headers: HashMap::new(),
        }
    }

    #[test]
    fn exports_every_channel_in_catalog_order() {
        let channels = vec![channel("a", "Alpha"), channel("b", "Beta")];
        let results = HashMap::from([(
            "b".to_owned(),
            ReportEntry {
                status: CheckStatus::TimedOut,
                elapsed: Duration::from_millis(12500),
                checked_at: "2026-11-15 20:58:00 +0100".to_owned(),
                details: "No frames".to_owned(),
            },
        )]);
        let csv = to_csv(&channels, &results, false);
        assert!(csv.starts_with("ID,Channel,URL,Status,Duration (s),Checked at,Details\r\n"));
        assert!(
            csv.contains("\"a\",\"Alpha\",\"https://example.com/stream.m3u8\",\"Not checked\"")
        );
        assert!(csv.contains("\"b\",\"Beta\",\"https://example.com/stream.m3u8\",\"Unknown (timeout)\",\"12.50\",\"2026-11-15 20:58:00 +0100\",\"No frames\""));
        assert!(csv.find("\"a\"").unwrap() < csv.find("\"b\"").unwrap());
    }

    #[test]
    fn escapes_csv_and_neutralizes_formula_cells() {
        let channels = vec![
            channel("x", "Name, \"special\"\n=SUM(1,2)"),
            channel("y", "=2+3"),
        ];
        let results = HashMap::from([(
            "x".to_owned(),
            ReportEntry {
                status: CheckStatus::Failed,
                elapsed: Duration::from_millis(525),
                checked_at: String::new(),
                details: "HTTP \"404\"\nUnavailable".to_owned(),
            },
        )]);
        let csv = to_csv(&channels, &results, true);
        assert!(csv.contains("\"Name, \"\"special\"\"\n=SUM(1,2)\""));
        assert!(csv.contains("\"'=2+3\""));
        assert!(csv.contains("\"HTTP \"\"404\"\"\nUnavailable\""));
        assert!(csv.contains("\"Pending\""));
        assert!(csv.contains("\"0.53\""));
    }
}
