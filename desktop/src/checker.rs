use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use wait_timeout::ChildExt;

use crate::catalog::Channel;

const PROCESS_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckStatus {
    Online,
    Failed,
    TimedOut,
}

#[derive(Debug)]
pub struct CheckResult {
    pub status: CheckStatus,
    pub elapsed: Duration,
    pub details: String,
}

impl CheckResult {
    fn new(status: CheckStatus, started: Instant, details: impl Into<String>) -> Self {
        Self {
            status,
            elapsed: started.elapsed(),
            details: details.into(),
        }
    }
}

/// Decodes a short sample rather than trusting a successful playlist request.
/// A stream is considered online only if ffmpeg reports actual output timestamps.
pub fn check(channel: &Channel) -> CheckResult {
    let started = Instant::now();
    let mut command = Command::new("ffmpeg");
    command
        .args(["-nostdin", "-hide_banner", "-nostats", "-loglevel", "error"])
        .args(["-rw_timeout", "4000000"]);

    if !channel.headers.is_empty() {
        let mut headers: Vec<_> = channel.headers.iter().collect();
        headers.sort_by_key(|(key, _)| *key);
        let fields = headers
            .iter()
            .map(|(key, value)| format!("{key}: {value}\r\n"))
            .collect::<String>();
        command.arg("-headers").arg(fields);
    }

    command
        .arg("-i")
        .arg(&channel.url)
        .args(["-t", "2", "-map", "0:v:0?", "-map", "0:a:0?"])
        .args(["-progress", "pipe:1", "-f", "null", "-"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return CheckResult::new(
                CheckStatus::Failed,
                started,
                format!("Cannot launch ffmpeg: {err}. Install ffmpeg first."),
            );
        }
    };

    let timed_out = match child.wait_timeout(PROCESS_TIMEOUT) {
        Ok(Some(_)) => false,
        Ok(None) => {
            let _ = child.kill();
            true
        }
        Err(err) => {
            let _ = child.kill();
            let _ = child.wait();
            return CheckResult::new(
                CheckStatus::Failed,
                started,
                format!("Error waiting for ffmpeg: {err}"),
            );
        }
    };

    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(err) => {
            return CheckResult::new(
                CheckStatus::Failed,
                started,
                format!("Cannot read ffmpeg output: {err}"),
            );
        }
    };

    if timed_out {
        return CheckResult::new(
            CheckStatus::TimedOut,
            started,
            "No playable media before timeout",
        );
    }

    if output.status.success() && has_decoded_media(&String::from_utf8_lossy(&output.stdout)) {
        return CheckResult::new(CheckStatus::Online, started, "Decoded a live media sample");
    }

    let error = String::from_utf8_lossy(&output.stderr);
    let detail = error
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("No decoded media received from the stream");
    CheckResult::new(CheckStatus::Failed, started, detail.to_owned())
}

fn has_decoded_media(progress: &str) -> bool {
    progress.lines().any(|line| {
        line.strip_prefix("out_time_us=")
            .or_else(|| line.strip_prefix("out_time_ms="))
            .and_then(|value| value.parse::<u64>().ok())
            .is_some_and(|timestamp| timestamp > 0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirms_real_decoding_progress() {
        assert!(has_decoded_media(
            "frame=25\nout_time_us=1500000\nprogress=end\n"
        ));
    }

    #[test]
    fn zero_progress_is_not_a_success() {
        assert!(!has_decoded_media("frame=0\nout_time_us=0\nprogress=end\n"));
        assert!(!has_decoded_media("HTTP/1.1 200 OK"));
    }
}
