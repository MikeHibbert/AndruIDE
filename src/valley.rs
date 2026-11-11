//! The valley: Andrew's work board, read live over his REST API.
//!
//! Andrew (the orchestrator behind Campfire Valley) publishes his board on a
//! Basic-auth'd HTTP API. This module is the client for it — the Valley page
//! in the UI fronts the board reads the way the Usage page fronts
//! fleet::usage, and the "Review with Andrew" conversations ride `chat()`.
//!
//! Configuration: `[settings.valley]` in config.toml (Settings › Valley)
//! first, then the environment, so no valley credentials are ever baked
//! into this public repo:
//! - config: `base_url` / `user` / `password`
//! - env:    `ANDRUIDE_VALLEY_URL` (base of Andrew's UI proxy, default
//!           `http://localhost:8001`), `ANDRUIDE_VALLEY_USER` (Basic-auth
//!           user — unset means "not configured"), `ANDRUIDE_VALLEY_PASSWORD`

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
    /// Builds a client: config.toml's [settings.valley] first, then the
    /// ANDRUIDE_VALLEY_* environment variables, then the defaults. An auth
    /// field unset means the page shows its "not configured" card instead
    /// of fetching.
    pub fn from_parts(cfg: crate::config::ValleySettings) -> Self {
        let pick = |cfg_v: &str, env_k: &str| {
            let c = cfg_v.trim();
            if !c.is_empty() {
                Some(c.to_string())
            } else {
                std::env::var(env_k).ok().filter(|v| !v.trim().is_empty())
            }
        };
        let base = pick(&cfg.base_url, "ANDRUIDE_VALLEY_URL").unwrap_or_else(|| "http://localhost:8001".into());
        let user = pick(&cfg.user, "ANDRUIDE_VALLEY_USER");
        let password = pick(&cfg.password, "ANDRUIDE_VALLEY_PASSWORD");
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

    /// One conversational turn with Andrew: POST /chat with
    /// `{session_id, message, mode}`. On an nginx-backed valley the base ends
    /// in `/api`, so this reaches his core's chat route either way.
    pub async fn chat(&self, session_id: &str, message: &str) -> Result<String, String> {
        if !self.configured() {
            return Err("The valley isn't configured — set it in Settings › Valley.".into());
        }
        let body = ChatIn { session_id: session_id.to_string(), message: message.to_string(), mode: "plan".into() };
        let url = format!("{}{}", self.base.trim_end_matches('/'), "/chat");
        let mut req = self.http.post(&url).json(&body);
        if let (Some(user), Some(password)) = (&self.user, &self.password) {
            req = req.basic_auth(user, Some(password));
        }
        let out: ChatOut = req
            .send()
            .await
            .map_err(|e| format!("Couldn't reach Andrew at {url}: {e}"))?
            .error_for_status()
            .map_err(|e| format!("The valley answered {e}"))?
            .json()
            .await
            .map_err(|e| format!("Andrew's reply isn't the shape we expect: {e}"))?;
        let reply = out.reply.trim().to_string();
        Ok(if reply.is_empty() { "Andrew didn't answer.".to_string() } else { reply })
    }
}

/// The body of a chat turn.
#[derive(serde::Serialize)]
pub struct ChatIn {
    pub session_id: String,
    pub message: String,
    pub mode: String,
}

/// Andrew's chat reply.
#[derive(Debug, Deserialize)]
pub struct ChatOut {
    #[serde(default)]
    pub reply: String,
}
