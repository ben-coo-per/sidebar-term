//! Tailscale, through its CLI: is it installed and running, what is this Mac's name, and
//! Tailscale Serve, which publishes the Remote server (127.0.0.1 only) to the tailnet over
//! HTTPS with a real certificate. Serve is tailnet-only; Funnel, which would open it to the
//! internet, is never used.

use crate::model::TailscaleState;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where the CLI lives: inside the Tailscale app (App Store and standalone builds), or a
/// Homebrew install.
const CLI_CANDIDATES: &[&str] = &[
    "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
    "/opt/homebrew/bin/tailscale",
    "/usr/local/bin/tailscale",
];

pub fn find_cli() -> Option<PathBuf> {
    CLI_CANDIDATES
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .or_else(|| {
            std::env::var_os("PATH").and_then(|path| {
                std::env::split_paths(&path)
                    .map(|d| d.join("tailscale"))
                    .find(|p| p.is_file())
            })
        })
}

/// What `tailscale status --json` says that matters here.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub running: bool,
    pub dns_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct StatusJson {
    backend_state: Option<String>,
    #[serde(rename = "Self")]
    this: Option<SelfJson>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SelfJson {
    #[serde(rename = "DNSName")]
    dns_name: Option<String>,
}

pub fn parse_status(json: &str) -> Result<Status, String> {
    let s: StatusJson = serde_json::from_str(json).map_err(|e| format!("status JSON: {e}"))?;
    let running = s.backend_state.as_deref() == Some("Running");
    let dns_name = s
        .this
        .and_then(|t| t.dns_name)
        .map(|n| n.trim_end_matches('.').to_string())
        .filter(|n| !n.is_empty());
    Ok(Status {
        running,
        dns_name: dns_name.filter(|_| running),
    })
}

pub fn status(cli: &Path) -> Result<Status, String> {
    let out = run(cli, &["status", "--json", "--peers=false"])?;
    parse_status(&out)
}

/// Publish `http://127.0.0.1:port` at `https://<this Mac>/` on the tailnet. `--bg` keeps the
/// rule in Tailscale's config, so it survives the app restarting.
pub fn serve_on(cli: &Path, port: u16) -> Result<(), String> {
    let target = format!("http://127.0.0.1:{port}");
    run(cli, &["serve", "--bg", "--https=443", &target]).map(|_| ())
}

pub fn serve_off(cli: &Path) -> Result<(), String> {
    run(cli, &["serve", "--https=443", "off"]).map(|_| ())
}

/// The whole picture, for the Settings page.
pub fn state() -> TailscaleState {
    let Some(cli) = find_cli() else {
        return TailscaleState {
            installed: false,
            ..Default::default()
        };
    };
    match status(&cli) {
        Ok(s) => TailscaleState {
            installed: true,
            running: s.running,
            dns_name: s.dns_name,
            error: None,
        },
        Err(e) => TailscaleState {
            installed: true,
            running: false,
            dns_name: None,
            error: Some(e),
        },
    }
}

fn run(cli: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cli)
        .args(args)
        .output()
        .map_err(|e| format!("running {}: {e}", cli.display()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        Err(if err.is_empty() {
            format!("tailscale {} failed ({})", args[0], out.status)
        } else {
            err.lines().last().unwrap_or(err).to_string()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reads_the_backend_state_and_dns_name() {
        let json = r#"{"BackendState":"Running","Self":{"DNSName":"bens-mac.tail1234.ts.net.","Online":true},"Peer":{}}"#;
        assert_eq!(
            parse_status(json).unwrap(),
            Status {
                running: true,
                dns_name: Some("bens-mac.tail1234.ts.net".into())
            }
        );
        // Logged out: no usable name even if one is reported.
        let json = r#"{"BackendState":"NeedsLogin","Self":{"DNSName":"x.ts.net."}}"#;
        assert_eq!(
            parse_status(json).unwrap(),
            Status {
                running: false,
                dns_name: None
            }
        );
        assert_eq!(parse_status("{}").unwrap(), Status::default());
        assert!(parse_status("nope").is_err());
    }

    #[test]
    fn a_missing_cli_reports_not_installed() {
        let s = status(Path::new("/no/such/tailscale"));
        assert!(s.unwrap_err().contains("running /no/such/tailscale"));
        // `state()` never panics whatever the machine has.
        let st = state();
        assert!(st.installed || !st.running);
    }
}
