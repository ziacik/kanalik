use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use wait_timeout::ChildExt;

use crate::catalog::Channel;

// Reading ten frames via the same player backend is less fragile than
// decoding two seconds of HLS via ffmpeg and parsing its progress output.
const PROCESS_TIMEOUT: Duration = Duration::from_secs(25);

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

pub fn configure_mpv(command: &mut Command, channel: &Channel) {
    let mut extra_headers = Vec::new();
    for (key, value) in &channel.headers {
        if key.eq_ignore_ascii_case("referer") {
            command.arg(format!("--referrer={value}"));
        } else if key.eq_ignore_ascii_case("user-agent") {
            command.arg(format!("--user-agent={value}"));
        } else {
            extra_headers.push(format!("{key}: {value}"));
        }
    }
    if !extra_headers.is_empty() {
        extra_headers.sort();
        command.arg(format!("--http-header-fields={}", extra_headers.join(",")));
    }
    command.arg(&channel.url);
}

/// Check a short playback through mpv, which is also the app's fallback player.
/// Timeout means inconclusive, not a broken stream.
pub fn check(channel: &Channel) -> CheckResult {
    let started = Instant::now();
    let mut command = Command::new("mpv");
    command.args([
        "--no-config",
        "--no-input-terminal",
        "--vo=null",
        "--ao=null",
        "--frames=12",
        "--msg-level=all=error",
    ]);
    configure_mpv(&mut command, channel);
    command.stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return CheckResult::new(
                CheckStatus::Failed,
                started,
                format!("Cannot start mpv: {err}"),
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
            return CheckResult::new(CheckStatus::Failed, started, format!("mpv error: {err}"));
        }
    };

    let output = match child.wait_with_output() {
        Ok(output) => output,
        Err(err) => {
            return CheckResult::new(CheckStatus::Failed, started, format!("mpv error: {err}"));
        }
    };

    if timed_out {
        return CheckResult::new(
            CheckStatus::TimedOut,
            started,
            "Timed out waiting for video frames; availability is unknown",
        );
    }

    if output.status.success() {
        return CheckResult::new(
            CheckStatus::Online,
            started,
            "mpv finished playback of the video sample",
        );
    }

    let error = String::from_utf8_lossy(&output.stderr);
    let details = error
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("mpv could not play the stream");
    CheckResult::new(CheckStatus::Failed, started, details.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn mpv_receives_catalog_headers() {
        let channel = Channel {
            id: "test".into(),
            name: "Test".into(),
            url: "https://example.com/index.m3u8".into(),
            headers: HashMap::from([("Referer".into(), "https://example.com/".into())]),
        };
        let mut command = Command::new("mpv");
        configure_mpv(&mut command, &channel);
        let args: Vec<_> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert!(args.contains(&"--referrer=https://example.com/".to_string()));
        assert_eq!(args.last().map(String::as_str), Some(channel.url.as_str()));
    }
}
