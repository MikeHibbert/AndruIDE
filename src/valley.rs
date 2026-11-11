//! The valley: Andrew's work board, read live over his REST API.
//!
//! Andrew (the orchestrator behind Campfire Valley) publishes his board on a
//! Basic-auth'd HTTP API. This module is the read-only client for it — the
//! Valley page in the UI fronts this the way the Usage page fronts
//! fleet::usage.
//!
//! Configuration comes from the environment, so no valley credentials are
//! ever baked into this public repo:
//! - `ANDRUIDE_VALLEY_URL`      base of Andrew's UI proxy (default `http://localhost:8001`)
//! - `ANDRUIDE_VALLEY_USER`     Basic-auth user (no default: unset means "not configured")
//! - `ANDRUIDE_VALLEY_PASSWORD` Basic-auth password

use std::collections::BTreeMap;

use serde::Deserialize;

/// One item on Andrew's work board, as `GET /work` reports it.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkItem {
    pub id: String,
    pub title: String,
    pub status: String,
    #[serde(default)]
    pub priority: i64,
    #[serde(default)]
    pub project: String,
    // Kept for the planned "running since" column on the running cards (v2);
    // reading them before then trips dead-code, hence the allow.
    #[allow(dead_code)]
    #[serde(default)]
    pub created_at: String,
    #[allow(dead_code)]
    #[serde(default)]
    pub started_at: Option<String>,
}

/// Andrew's `/ready` payload — service health at a glance.
#[derive(Debug, Clone, Deserialize)]
pub struct ReadyOut {
    #[serde(default)]
    pub service: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub draining: bool,
}

/// One read of the valley: the whole board plus the service pill.
#[derive(Debug, Clone)]
pub struct ValleyStatus {
    pub items: Vec<WorkItem>,
    pub ready: Option<ReadyOut>,
}

/// Per-status item counts across the board.
pub fn counts(items: &[WorkItem]) -> BTreeMap<String, usize> {
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for item in items {
        *out.entry(item.status.clone()).or_default() += 1;
    }
    out
}

/// The board lanes in the order the page shows them; any other status is
/// appended after these.
pub const LANES: [&str; 6] = ["running", "blocked", "pending", "completed", "stopped", "failed"];

/// Read-only client for Andrew's valley API.
pub struct Valley {
    http: reqwest::Client,
    base: String,
    user: Option<String>,
    password: Option<String>,
}

impl Valley {
    /// Builds a client from the environment. `user` unset means the page
    /// should show its "not configured" card instead of fetching.
    pub fn from_env() -> Self {
        let base = std::env::var("ANDRUIDE_VALLEY_URL")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "http://localhost:8001".into());
        let user = std::env::var("ANDRUIDE_VALLEY_USER").ok().filter(|v| !v.trim().is_empty());
        let password = std::env::var("ANDRUIDE_VALLEY_PASSWORD").ok().filter(|v| !v.trim().is_empty());
        Self { http: reqwest::Client::new(), base, user, password }
    }

    /// Whether credentials are configured; the page fetches only when true.
    pub fn configured(&self) -> bool {
        self.user.is_some() && self.password.is_some()
    }

    /// One GET against the valley, with Basic auth when configured.
    async fn get(&self, path: &str) -> Result<reqwest::Response, String> {
        let url = format!("{}{}", self.base.trim_end_matches('/'), path);
        let mut req = self.http.get(&url);
        if let (Some(user), Some(password)) = (&self.user, &self.password) {
            req = req.basic_auth(user, Some(password));
        }
        req.send()
            .await
            .map_err(|e| format!("Couldn't reach the valley at {url}: {e}"))?
            .error_for_status()
            .map_err(|e| format!("The valley answered {e}"))
    }

    /// Reads the whole board and the service pill in one go.
    pub async fn status(&self) -> Result<ValleyStatus, String> {
        let resp = self.get("/work").await.map_err(|e| format!("The board: {e}"))?;
        // The 200 body is a bare JSON list of items.
        let items: Vec<WorkItem> = resp
            .json()
            .await
            .map_err(|e| format!("The board isn't the shape we expect: {e}"))?;
        // The pill is best-effort: a service hiccup only hides it, the board still shows.
        let ready: Option<ReadyOut> = match self.get("/ready").await {
            Ok(r) => r.json::<ReadyOut>().await.ok(),
            Err(_) => None,
        };
        Ok(ValleyStatus { items, ready })
    }
}
