//! Startup reachability probe — advisory only, nothing gates on it.
//!
//! LTBox pulls EDL loaders, root providers, driver packages and its own
//! updates from GitHub, but the binary itself travels through other
//! channels (mirrors, direct hand-off) into regions where GitHub is
//! blocked while the rest of the internet works. Probing the link and
//! GitHub *separately* lets the startup notice name which of the two is
//! missing instead of collapsing both into a flat "offline".
//!
//! Distinct from [`ltbox_device::driver::probe_connectivity`], which is
//! the GitHub-only gate for the Qualcomm driver install/update buttons.

use std::time::Duration;

use crate::downloader::USER_AGENT;

/// Fixed-body link check. Microsoft's NCSI host backs the Windows
/// connectivity indicator, so it stays reachable in the regions that block
/// GitHub, and matching the exact body rejects a captive portal that
/// answers 200 with a login page.
const NCSI_URL: &str = "http://www.msftconnecttest.com/connecttest.txt";
const NCSI_BODY: &str = "Microsoft Connect Test";
/// Second opinion for networks that block Microsoft — Firefox's
/// portal-detection host, same fixed-body contract.
const PORTAL_URL: &str = "http://detectportal.firefox.com/success.txt";
const PORTAL_BODY: &str = "success";
/// GitHub canary: the host every LTBox release / artifact lookup starts
/// from, so its reachability is what actually predicts feature failure.
const GITHUB_URL: &str = "https://api.github.com/";

/// Short per-probe budget. Three sequential probes at worst, and this runs
/// off the UI thread, so the whole check is bounded well under the time a
/// user spends on the dashboard before starting anything.
const PROBE_TIMEOUT: Duration = Duration::from_secs(6);

/// Outcome of the two startup reachability checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectivityReport {
    /// The link itself works — a fixed-body probe a captive portal fails.
    pub internet: bool,
    /// `api.github.com` answered. Can be `false` while [`Self::internet`]
    /// is `true` wherever GitHub specifically is blocked.
    pub github: bool,
}

impl ConnectivityReport {
    /// Nothing to tell the user about.
    pub fn all_reachable(&self) -> bool {
        self.internet && self.github
    }
}

fn probe_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(USER_AGENT)
        .timeout_global(Some(PROBE_TIMEOUT))
        .build()
        .new_agent()
}

/// Whether `url` answered with a body starting with `expected`. A captive
/// portal that intercepts the request answers 200 with its own HTML, which
/// fails the body match — that is the whole point of the fixed-body hosts.
fn body_matches(agent: &ureq::Agent, url: &str, expected: &str) -> bool {
    agent
        .get(url)
        .call()
        .ok()
        .and_then(|mut resp| resp.body_mut().read_to_string().ok())
        .is_some_and(|body| body.trim_start().starts_with(expected))
}

/// Run both checks. Never fails: every transport error is simply a
/// `false`, because the caller only turns this into a log line.
pub fn probe() -> ConnectivityReport {
    let agent = probe_agent();
    let internet =
        body_matches(&agent, NCSI_URL, NCSI_BODY) || body_matches(&agent, PORTAL_URL, PORTAL_BODY);
    let github = agent.get(GITHUB_URL).call().is_ok();
    // A GitHub hit proves the link even when both fixed-body hosts are
    // blocked, so never report "no internet" alongside a reachable GitHub.
    ConnectivityReport {
        internet: internet || github,
        github,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_reachable_requires_both() {
        assert!(
            ConnectivityReport {
                internet: true,
                github: true
            }
            .all_reachable()
        );
        assert!(
            !ConnectivityReport {
                internet: true,
                github: false
            }
            .all_reachable()
        );
        assert!(
            !ConnectivityReport {
                internet: false,
                github: false
            }
            .all_reachable()
        );
    }
}
