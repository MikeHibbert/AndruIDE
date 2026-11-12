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
    /// The kumbaya run working this item when it's claimed — the drill
    /// down to the job's conversation (Father, 12 Nov 2026: three levels,
    /// listing detail conversation).
    #[serde(default)]
    pub claim_run_id: Option<String>,
}

impl WorkItem {
    /// The claim run when the item carries a real one (an empty string
    /// reads as none). Kept for the v2 running-since/claim views.
    #[allow(dead_code)]
    pub fn claim_run(&self) -> Option<&str> {
        self.claim_run_id.as_deref().map(str::trim).filter(|s| !s.is_empty())
    }
}

/// One work item in full, as `GET /work/{id}` reports it — what the item
/// page shows. Andrew's answer carries far more fields; serde keeps only
/// what the page reads.
#[derive(Debug, Clone, Deserialize)]
pub struct ItemDetail {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub priority: i64,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub claim_run_id: Option<String>,
    /// Why a claim failed, when it did. The server sends `""` for some
    /// items and `null` for others (the null form broke the item page,
    /// Father 12 Nov 2026); both read as none.
    #[serde(default)]
    pub error: Option<String>,
    /// The item's log as it was worked, oldest entries first.
    #[serde(default)]
    pub progress_log: Vec<String>,
}

impl ItemDetail {
    /// The claim run when the item carries a real one.
    pub fn claim_run(&self) -> Option<&str> {
        self.claim_run_id.as_deref().map(str::trim).filter(|s| !s.is_empty())
    }

    /// The failure text, when a real one is present. Null and `""` both read
    /// as none (the server sends null form in detail bodies, Father 12 Nov
    /// 2026).
    pub fn failed_text(&self) -> Option<&str> {
        self.error.as_deref().filter(|e| !e.trim().is_empty())
    }

    /// The log's newest entries, last first, capped for the page.
    pub fn recent_log(&self, n: usize) -> Vec<&str> {
        self.progress_log.iter().rev().take(n).map(String::as_str).collect()
    }
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
    /// The contract capabilities this valley advertises (contract v1:
    /// bare valleys say ["agent"], Andrew's tier adds the work board).
    /// A valley without the work tier gets the graceful "campers take
    /// work by handoff" card instead of an error (Father, 12 Nov 2026,
    /// on Valley A: "it doesn't have an API work endpoint... it's just
    /// a campfire valley").
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl ReadyOut {
    /// Does this valley run the work board (the /work surface)?
    pub fn has_work_tier(&self) -> bool {
        // Andrew's tier keeps serving the board regardless of how the
        // capability marker evolves, so an EMPTY list reads as Andrew
        // (backwards-compatible with builds that never sent the field).
        self.capabilities.is_empty()
            || self.capabilities.iter().any(|c| c == "planner" || c == "work")
    }
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

/// A valley invite, decoded from a paste or a QR (first run, Father 12 Nov
/// 2026): `andruide://join?base=…&user=…&password=…`, percent-encoded. A bare
/// http(s) URL joins with no credentials — the valley decides if it wants any.
#[derive(Debug, PartialEq)]
pub struct JoinParts {
    pub base: String,
    pub user: String,
    pub password: String,
}

/// Percent-decodes a query value. `+` stays `+` deliberately: invites carry
/// passwords, and only the generator's `%2B` form may produce one.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parses a pasted Valley invite; None says the text isn't one.
pub fn parse_join(input: &str) -> Option<JoinParts> {
    let s = input.trim();
    if s.starts_with("http://") || s.starts_with("https://") {
        // A plain base URL: a valley that needs no credentials.
        return Some(JoinParts {
            base: s.trim_end_matches('/').to_string(),
            user: String::new(),
            password: String::new(),
        });
    }
    let query = s.strip_prefix("andruide://join?")?;
    let mut base = None;
    let mut user = String::new();
    let mut password = String::new();
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=')?;
        match key.to_ascii_lowercase().as_str() {
            "base" => base = Some(percent_decode(value).trim_end_matches('/').to_string()),
            "user" => user = percent_decode(value),
            "password" | "pass" => password = percent_decode(value),
            // Unknown fields are ignored — the contract's tolerance rule.
            _ => {}
        }
    }
    let base = base?;
    if base.is_empty() {
        return None;
    }
    Some(JoinParts { base, user, password })
}

/// Read-only client for Andrew's valley API.
pub struct Valley {
    http: reqwest::Client,
    base: String,
    user: Option<String>,
    password: Option<String>,
    // Learned on the first read that comes back as the valley's HTML front
    // door: nginx-fronted valleys keep the API under /api, so a bare base
    // (a hand-typed Settings entry or a bare-URL join, Father's 13 Nov
    // 2026 screenshots) self-heals by retrying under /api. OnceLock keeps
    // the client Send+Sync while the prefix sticks for its lifetime.
    prefix: std::sync::OnceLock<String>,
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
        // Gzip on: the valley's heavy reads arrive MB-sized and the wire time is
        // most of the perceived slowness; the gzip transfer cuts them to a
        // fraction (Father's 13 Nov reports: slow reads + decode errors).
        // Timeout 120s: reads can legitimately take tens of seconds while campers
        // run; beyond that the read is a hang and should surface as an error the
        // page can retry, not block a render forever.
        let http = reqwest::Client::builder()
            .gzip(true)
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { http, base, user, password, prefix: std::sync::OnceLock::new() }
    }

    /// Whether credentials are configured; the page fetches only when true.
    pub fn configured(&self) -> bool {
        self.user.is_some() && self.password.is_some()
    }

    /// The URL for one API path under the current base and learned prefix.
    fn url_for(&self, path: &str) -> String {
        let prefix = self.prefix.get().map(String::as_str).unwrap_or("");
        format!("{}{}{}", self.base.trim_end_matches('/'), prefix, path)
    }

    /// Learning is available until the prefix is set — and never on a base
    /// that already carries the /api tail (learning there would double it).
    fn can_learn(&self) -> bool {
        self.prefix.get().is_none()
            && !self
                .base
                .trim_end_matches('/')
                .to_ascii_lowercase()
                .ends_with("/api")
    }

    /// The valley's nginx answers unknown bare-host paths with the HTML
    /// front door (verified live 13 Nov 2026: GET /kumbayas/runs on
    /// localhost:8001 → 200 text/html). HTML says we hit the door, not the
    /// API. (Father: "still not working" on item detail + campfires.)
    fn looks_like_html(resp: &reqwest::Response) -> bool {
        resp.headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.trim().to_ascii_lowercase().starts_with("text/html"))
            .unwrap_or(false)
    }

    fn auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let (Some(user), Some(password)) = (&self.user, &self.password) {
            req.basic_auth(user, Some(password))
        } else {
            req
        }
    }

    /// One GET against the valley, with Basic auth when configured.
    async fn get(&self, path: &str) -> Result<reqwest::Response, String> {
        let url = self.url_for(path);
        let mut resp = self
            .auth(self.http.get(&url))
            .send()
            .await
            .map_err(|e| format!("Couldn't reach the valley at {url}: {e}"))?;
        if self.can_learn() && Self::looks_like_html(&resp) {
            let _ = self.prefix.set("/api".to_string());
            let url = self.url_for(path);
            resp = self
                .auth(self.http.get(&url))
                .send()
                .await
                .map_err(|e| format!("Couldn't reach the valley at {url}: {e}"))?;
        }
        resp.error_for_status()
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
    /// `{session_id, message, mode, interrupt}`. With `interrupt` set
    /// (AndruIDE's Campfires instruct line) Andrew stops a running turn to
    /// listen — feedback or training mid-run, at Father's direction.
    /// On an nginx-backed valley the base ends in `/api`, so this reaches
    /// his core's chat route either way.
    pub async fn chat(&self, session_id: &str, message: &str, interrupt: bool) -> Result<String, String> {
        if !self.configured() {
            return Err("The valley isn't configured — set it in Settings › Valley.".into());
        }
        let body = ChatIn { session_id: session_id.to_string(), message: message.to_string(), mode: "plan".into(), interrupt };
        let mut url = self.url_for("/chat");
        let mut resp = self
            .auth(self.http.post(&url).json(&body))
            .send()
            .await
            .map_err(|e| format!("Couldn't reach Andrew at {url}: {e}"))?;
        if self.can_learn() && Self::looks_like_html(&resp) {
            let _ = self.prefix.set("/api".to_string());
            url = self.url_for("/chat");
            resp = self
                .auth(self.http.post(&url).json(&body))
                .send()
                .await
                .map_err(|e| format!("Couldn't reach Andrew at {url}: {e}"))?;
        }
        let out: ChatOut = resp
            .error_for_status()
            .map_err(|e| format!("The valley answered {e}"))?
            .json()
            .await
            .map_err(|e| format!("Andrew's reply isn't the shape we expect: {e}"))?;
        let reply = out.reply.trim().to_string();
        Ok(if reply.is_empty() { "Andrew didn't answer.".to_string() } else { reply })
    }

    /// The valley's runs live: every kumbaya run (worker kicks, schedules,
    /// chats) with its verses — what the campfire is doing right now.
    pub async fn runs(&self) -> Result<Vec<RunSummary>, String> {
        if !self.configured() {
            return Err("The valley isn't configured — set it in Settings › Valley.".into());
        }
        // lite=1 asks the valley to strip the verse result/intent and
        // report/inputs from the list read (depth stays on /kumbayas/runs/{id}).
        // Measured live 13 Nov 2026: the plain list shipped 3.98MB and took
        // 14.3s under pool load -- what made the board unreadable in this app.
        // Backends that don't know the param ignore it, so this stays safe
        // against every valley. (Father 13 Nov 2026: "you can't even look to
        // see what jobs are in the valley now.")
        let resp = self
            .get("/kumbayas/runs?lite=1")
            .await
            .map_err(|e| format!("The runs: {e}"))?;
        resp.json::<Vec<RunSummary>>()
            .await
            .map_err(|e| format!("The runs aren't the shape we expect: {e}"))
    }

    /// One work item in full (the item page's detail level).
    pub async fn item(&self, id: &str) -> Result<ItemDetail, String> {
        if !self.configured() {
            return Err("The valley isn't configured — set it in Settings › Valley.".into());
        }
        let resp = self
            .get(&format!("/work/{id}"))
            .await
            .map_err(|e| format!("The item: {e}"))?;
        resp.json::<ItemDetail>()
            .await
            .map_err(|e| format!("The item isn't the shape we expect: {e}"))
    }

    /// One run's conversation: every verse with what the camper said and
    /// did on the job (the item page's third level).
    pub async fn talk(&self, run_id: &str) -> Result<RunTalk, String> {
        if !self.configured() {
            return Err("The valley isn't configured — set it in Settings › Valley.".into());
        }
        let resp = self
            .get(&format!("/kumbayas/runs/{run_id}"))
            .await
            .map_err(|e| format!("The conversation: {e}"))?;
        resp.json::<RunTalk>()
            .await
            .map_err(|e| format!("The conversation isn't the shape we expect: {e}"))
    }

    /// Answers a waiting checkpoint: POST /kumbayas/runs/{id}/resume with
    /// `{"answer": …}`. (Father, 13 Nov 2026: "what a shame I can't respond
    /// to that needs you work_day_review" — the responder card on the
    /// Campfires and item pages rides this.)
    pub async fn resume(&self, run_id: &str, answer: &str) -> Result<(), String> {
        if !self.configured() {
            return Err("The valley isn't configured — set it in Settings › Valley.".into());
        }
        let body = serde_json::json!({ "answer": answer });
        let mut url = self.url_for(&format!("/kumbayas/runs/{run_id}/resume"));
        let mut resp = self
            .auth(self.http.post(&url).json(&body))
            .send()
            .await
            .map_err(|e| format!("Couldn't reach the valley at {url}: {e}"))?;
        if self.can_learn() && Self::looks_like_html(&resp) {
            let _ = self.prefix.set("/api".to_string());
            url = self.url_for(&format!("/kumbayas/runs/{run_id}/resume"));
            resp = self
                .auth(self.http.post(&url).json(&body))
                .send()
                .await
                .map_err(|e| format!("Couldn't reach the valley at {url}: {e}"))?;
        }
        resp.error_for_status()
            .map(|_| ())
            .map_err(|e| format!("The valley answered {e}"))
    }

    /// Stops a running job: POST /kumbayas/runs/{id}/cancel. The server
    /// cancels the run and requeues its work item if this run owns one
    /// (Father, 12 Nov 2026: "we'll be able to control there those from
    /// the valley too" — the stop rides the Valley board's inline button).
    pub async fn cancel_run(&self, run_id: &str) -> Result<(), String> {
        if !self.configured() {
            return Err("The valley isn't configured — set it in Settings › Valley.".into());
        }
        let mut url = self.url_for(&format!("/kumbayas/runs/{run_id}/cancel"));
        let mut resp = self
            .auth(self.http.post(&url))
            .send()
            .await
            .map_err(|e| format!("Couldn't reach the valley at {url}: {e}"))?;
        if self.can_learn() && Self::looks_like_html(&resp) {
            let _ = self.prefix.set("/api".to_string());
            url = self.url_for(&format!("/kumbayas/runs/{run_id}/cancel"));
            resp = self
                .auth(self.http.post(&url))
                .send()
                .await
                .map_err(|e| format!("Couldn't reach the valley at {url}: {e}"))?;
        }
        resp.error_for_status()
            .map(|_| ())
            .map_err(|e| format!("The valley answered {e}"))
    }

    /// Andrew's scheduler: the kicks (cron and interval) that keep the
    /// valley breathing (worker kicks, the eight-hour kick, briefs,
    /// patrols). (Father, 13 Nov 2026: under Schedules, "schedules can be
    /// jobs or can be actual scheduled things like cron jobs and the eight
    /// hour kick and all that sort of thing.")
    pub async fn schedules(&self) -> Result<Vec<ScheduleEntry>, String> {
        if !self.configured() {
            return Err("The valley isn't configured — set it in Settings › Valley.".into());
        }
        let resp = self
            .get("/schedules")
            .await
            .map_err(|e| format!("The schedules: {e}"))?;
        resp.json::<Vec<ScheduleEntry>>()
            .await
            .map_err(|e| format!("The schedules aren't the shape we expect: {e}"))
    }

    /// Andrew's ceremonies: the kumbayas the songbook carries (GET
    /// /kumbayas; 108 of them live, full detail even in the list). (Father,
    /// 13 Nov 2026: "On the settings prompts page where it says session
    /// actions, we can put our kumbays there. And then they can be observed
    /// and modified if needs be from there.")
    pub async fn kumbayas(&self) -> Result<Vec<KumbayaSummary>, String> {
        if !self.configured() {
            return Err("The valley isn't configured — set it in Settings › Valley.".into());
        }
        let resp = self
            .get("/kumbayas")
            .await
            .map_err(|e| format!("The ceremonies: {e}"))?;
        resp.json::<Vec<KumbayaSummary>>()
            .await
            .map_err(|e| format!("The ceremonies aren't the shape we expect: {e}"))
    }

    /// One ceremony in full detail (GET /kumbayas/{name}; names are slugs).
    pub async fn kumbaya_raw(&self, name: &str) -> Result<serde_json::Value, String> {
        if !self.configured() {
            return Err("The valley isn't configured — set it in Settings › Valley.".into());
        }
        let resp = self
            .get(&format!("/kumbayas/{name}"))
            .await
            .map_err(|e| format!("The ceremony: {e}"))?;
        resp.json::<serde_json::Value>()
            .await
            .map_err(|e| format!("The ceremony isn't the shape we expect: {e}"))
    }

    /// Save a ceremony: POST /kumbayas is Andrew's songbook save, a proper
    /// upsert (it bumps the version when the definition changes and flips
    /// dependents to outdated). The whole value goes back, unknown fields
    /// and all, so an edit is lossless.
    pub async fn save_kumbaya(&self, value: serde_json::Value) -> Result<(), String> {
        if !self.configured() {
            return Err("The valley isn't configured — set it in Settings › Valley.".into());
        }
        let mut url = self.url_for("/kumbayas");
        let mut resp = self
            .auth(self.http.post(&url).json(&value))
            .send()
            .await
            .map_err(|e| format!("Couldn't reach the valley at {url}: {e}"))?;
        if self.can_learn() && Self::looks_like_html(&resp) {
            let _ = self.prefix.set("/api".to_string());
            url = self.url_for("/kumbayas");
            resp = self
                .auth(self.http.post(&url).json(&value))
                .send()
                .await
                .map_err(|e| format!("Couldn't reach the valley at {url}: {e}"))?;
        }
        resp.error_for_status()
            .map(|_| ())
            .map_err(|e| format!("The valley answered {e}"))
    }

    /// Retire a ceremony.
    pub async fn delete_kumbaya(&self, name: &str) -> Result<(), String> {
        if !self.configured() {
            return Err("The valley isn't configured — set it in Settings › Valley.".into());
        }
        let mut url = self.url_for(&format!("/kumbayas/{name}"));
        let mut resp = self
            .auth(self.http.delete(&url))
            .send()
            .await
            .map_err(|e| format!("Couldn't reach the valley at {url}: {e}"))?;
        if self.can_learn() && Self::looks_like_html(&resp) {
            let _ = self.prefix.set("/api".to_string());
            url = self.url_for(&format!("/kumbayas/{name}"));
            resp = self
                .auth(self.http.delete(&url))
                .send()
                .await
                .map_err(|e| format!("Couldn't reach the valley at {url}: {e}"))?;
        }
        resp.error_for_status()
            .map(|_| ())
            .map_err(|e| format!("The valley answered {e}"))
    }

    /// Andrew's conversations, his side of the chat queue (GET /sessions
    /// returns a bare list; probed live 13 Nov 2026). (Father, 13 Nov 2026:
    /// run Andrew from the machine page like an IDE of his own - a
    /// conversation, and sessions you can manage.)
    pub async fn sessions(&self) -> Result<Vec<SessionRow>, String> {
        if !self.configured() {
            return Err("The valley isn't configured — set it in Settings › Valley.".into());
        }
        let resp = self
            .get("/sessions")
            .await
            .map_err(|e| format!("The sessions: {e}"))?;
        resp.json::<Vec<SessionRow>>()
            .await
            .map_err(|e| format!("The sessions aren't the shape we expect: {e}"))
    }

    /// One conversation's transcript: Andrew's turns and Dad's.
    pub async fn read_session(&self, id: &str) -> Result<SessionRead, String> {
        if !self.configured() {
            return Err("The valley isn't configured — set it in Settings › Valley.".into());
        }
        let resp = self
            .get(&format!("/sessions/{}", id.replace(' ', "%20")))
            .await
            .map_err(|e| format!("The conversation: {e}"))?;
        resp.json::<SessionRead>()
            .await
            .map_err(|e| format!("The transcript isn't the shape we expect: {e}"))
    }
}

/// A checkpoint's ask: the question Andrew holds a run on, and the answers
/// it will take ("accept" / "refine" / "skip" on the day review, captured
/// live 13 Nov 2026).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CheckpointAsk {
    #[serde(default)]
    pub question: String,
    #[serde(default)]
    pub options: Vec<String>,
}

/// One of Andrew's conversations in the list (GET /sessions; live shape,
/// probed 13 Nov 2026: session_id, message_count, created_at, last_active,
/// preview, phase, mode).
#[derive(Clone, Debug, Deserialize)]
pub struct SessionRow {
    pub session_id: String,
    #[serde(default)]
    pub message_count: Option<i64>,
    /// Parsed but not shown (the list leads with preview + phase + count).
    #[serde(default)]
    #[allow(dead_code)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub last_active: Option<String>,
    #[serde(default)]
    pub preview: Option<String>,
    #[serde(default)]
    pub phase: Option<String>,
    /// Parsed but not shown (phase carries the plan/execute split already).
    #[serde(default)]
    #[allow(dead_code)]
    pub mode: Option<String>,
}

/// One turn of a transcript (GET /sessions/{id}; role is user/assistant,
/// content is the text).
#[derive(Clone, Debug, Deserialize)]
pub struct SessionTurn {
    pub role: String,
    pub content: String,
    /// Parsed but not shown (turns read in list order, no clock per turn).
    #[serde(default)]
    #[allow(dead_code)]
    pub timestamp: Option<String>,
}

/// A transcript: the conversation under one session id.
#[derive(Clone, Debug, Deserialize)]
pub struct SessionRead {
    /// Parsed but not shown (the page is already keyed by the session id).
    #[allow(dead_code)]
    pub session_id: String,
    pub messages: Vec<SessionTurn>,
}

/// The body of a chat turn.
#[derive(serde::Serialize)]
pub struct ChatIn {
    pub session_id: String,
    pub message: String,
    pub mode: String,
    /// Instructing a camper mid-run (Campfires, Father 12 Nov 2026): Andrew
    /// stops the running turn and listens. False on ordinary chat, so the
    /// field doesn't even go on the wire there.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub interrupt: bool,
}

/// How many runs want a person right now (a checkpoint holds them).
pub fn need_you(runs: &[RunSummary]) -> usize {
    runs.iter().filter(|r| r.status == "waiting_checkpoint").count()
}

/// The age of a run in seconds, when its stamp parses — feeds the
/// last-hour activity summary (Father, 13 Nov 2026: Activity as "a
/// summary of activity over the last hour for Andrew").
pub fn age_seconds(rfc3339: &str) -> Option<u64> {
    let t = chrono::DateTime::parse_from_rfc3339(rfc3339).ok()?;
    let secs = (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_seconds();
    Some(secs.max(0) as u64)
}

/// One entry of Andrew's scheduler (GET /schedules, read live 13 Nov
/// 2026): the kicks that make the valley breathe. The kick history rides
/// `result_history` — skipped at parse time, the Schedules page shows the
/// last one's words only.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ScheduleEntry {
    pub name: String,
    #[serde(default)]
    pub cron: Option<String>,
    #[serde(default)]
    pub interval_seconds: Option<i64>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub next_run: Option<String>,
    #[serde(default)]
    pub last_run_at: Option<String>,
    #[serde(default)]
    pub last_result: Option<ScheduleResult>,
    #[serde(default)]
    pub status: Option<String>,
}

/// `last_result` is an object carrying the kick's outcome in `summary`
/// ("kick accepted: run started on the main loop", read live).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ScheduleResult {
    #[serde(default)]
    pub summary: Option<String>,
}

/// A ceremony in the songbook's list (GET /kumbayas): its name and the
/// one-liner, with extras tolerated. The full detail (steps, prompts) is
/// fetched per ceremony.
#[derive(Clone, Debug, Deserialize)]
pub struct KumbayaSummary {
    pub name: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub version: Option<i64>,
    #[serde(default)]
    pub status: Option<String>,
    /// Parsed for completeness (the ceremony list carries them); shown
    /// later in the settings card.
    #[serde(default)]
    #[allow(dead_code)]
    pub kind: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub updated_at: Option<String>,
}

impl ScheduleEntry {
    /// The cadence, human-readable: the cron line, else the interval.
    pub fn cadence(&self) -> Option<String> {
        if let Some(c) = self.cron.as_deref().filter(|c| !c.trim().is_empty()) {
            return Some(c.to_string());
        }
        self.interval_seconds.map(|s| match s {
            n if n % 3600 == 0 => format!("every {}h", n / 3600),
            n if n % 60 == 0 => format!("every {}m", n / 60),
            n => format!("every {n}s"),
        })
    }

    /// What the last kick said (accepted / declined / its own words).
    pub fn last_words(&self) -> Option<&str> {
        self.last_result
            .as_ref()
            .and_then(|r| r.summary.as_deref())
            .filter(|s| !s.trim().is_empty())
    }
}

/// One verse inside a run, trimmed to what the Run page shows. The heavy
/// fields Andrew sends with each verse (`result`, `intent`) are skipped at
/// parse time, so a 50-run list reads cheap; Torches will read them later.
#[derive(Debug, Clone, Deserialize)]
pub struct VerseLite {
    pub verse: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub tool_name: String,
    /// The ask a checkpoint verse holds the run on; the lite list read
    /// keeps it (the server strips result/intent, not checkpoint), so the
    /// Needs-you card can show the question without a deep read. (Father,
    /// 13 Nov 2026: needs-you should pop up on the Overview.)
    #[serde(default)]
    pub checkpoint: Option<CheckpointAsk>,
}

/// One kumbaya run as `GET /kumbayas/runs` reports it, trimmed like its
/// verses. `session_id` names the camper driving it ("chat:worker_kick"),
/// the verses say where the run has got to.
#[derive(Debug, Clone, Deserialize)]
pub struct RunSummary {
    pub id: String,
    #[serde(default)]
    pub kumbaya: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub created_at: String,
    // Kept for the drill-down once Torches reads verses' results too; until
    // then reading it trips dead-code, hence the allow.
    #[allow(dead_code)]
    #[serde(default)]
    pub finished_at: Option<String>,
    #[serde(default)]
    pub verses: Vec<VerseLite>,
}

impl RunSummary {
    /// The camper driving this run: the first segment of the session id
    /// ("chat:worker_kick" → "worker_kick"); the page reads a blank one as
    /// "valley".
    pub fn camper(&self) -> &str {
        let id = self.session_id.trim();
        let after = id.split_once(':').map(|(_, tail)| tail.trim()).unwrap_or("");
        if after.is_empty() { id } else { after }
    }

    /// The verse in progress: the first one running (a checkpoint verse
    /// runs while it waits for the operator, so this covers that too).
    pub fn current_verse(&self) -> Option<&VerseLite> {
        self.verses.iter().find(|v| v.status == "running")
    }

    /// How far the run has got: the verses passed over or finished.
    pub fn settled_verses(&self) -> usize {
        self.verses.iter().filter(|v| matches!(v.status.as_str(), "completed" | "skipped" | "failed")).count()
    }

    /// The ask this run waits on (its newest checkpoint verse that carries
    /// one) — the Needs-you cards read the question from here.
    pub fn waiting_ask(&self) -> Option<&CheckpointAsk> {
        self.verses.iter().rev().find_map(|v| v.checkpoint.as_ref())
    }
}

/// The order rows show in: what needs the operator first, then what's
/// running, then failed, then everything else — each band newest first.
pub fn order_runs(runs: &mut [RunSummary]) {
    runs.sort_by(|a, b| {
        band(a.status.as_str())
            .cmp(&band(b.status.as_str()))
            .then_with(|| b.created_at.cmp(&a.created_at))
    });
}

fn band(status: &str) -> u8 {
    match status {
        "waiting_checkpoint" => 0,
        "running" => 1,
        "failed" => 2,
        _ => 3,
    }
}

/// Runs grouped by camper, keeping the order `order_runs` left them in; a
/// camper sits where its best-ranked run sits.
pub fn group_by_camper(runs: &[RunSummary]) -> Vec<(String, Vec<&RunSummary>)> {
    let mut groups: Vec<(String, Vec<&RunSummary>)> = Vec::new();
    for run in runs {
        let camper = run.camper();
        let camper = if camper.is_empty() { "valley" } else { camper };
        if let Some(group) = groups.iter_mut().find(|(name, _)| name == camper) {
            group.1.push(run);
        } else {
            groups.push((camper.to_string(), vec![run]));
        }
    }
    groups
}

/// "just now" / "3m" / "1h 04m" since an RFC-3339 timestamp.
pub fn age_text(rfc3339: &str) -> Option<String> {
    let at = chrono::DateTime::parse_from_rfc3339(rfc3339).ok()?;
    let secs = chrono::Utc::now().signed_duration_since(at.with_timezone(&chrono::Utc)).num_seconds().max(0);
    Some(if secs < 60 {
        "just now".into()
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
    })
}

/// Andrew's chat reply.
#[derive(Debug, Deserialize)]
pub struct ChatOut {
    #[serde(default)]
    pub reply: String,
}

/// One verse of a run's conversation: its name and what the camper said
/// and did on it (Andrew sends the full `result` per verse; the item
/// page caps how much of it it draws).
#[derive(Debug, Clone, Deserialize)]
pub struct VerseTalk {
    pub verse: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub result: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub checkpoint: Option<CheckpointAsk>,
}

impl VerseTalk {
    /// What the camper said on this verse: its result, or its error, or
    /// nothing (a pending verse hasn't spoken yet). Kept whole; the page
    /// decides how much to draw.
    pub fn said(&self) -> Option<&str> {
        self.result.as_deref().filter(|s| !s.is_empty()).or(self.error.as_deref().filter(|s| !s.is_empty()))
    }
}

/// One run's conversation in full, as `GET /kumbayas/runs/{id}` reports it.
#[derive(Debug, Clone, Deserialize)]
pub struct RunTalk {
    // Parsed for completeness; the page shows status, kumbaya and verses
    // (created_at joins the header when a running-since line is wanted).
    #[serde(default)]
    #[allow(dead_code)]
    pub id: String,
    #[serde(default)]
    pub kumbaya: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub created_at: String,
    #[serde(default)]
    pub verses: Vec<VerseTalk>,
}

impl RunTalk {
    /// The ask this conversation's run waits on (its newest checkpoint
    /// verse that carries one) — the item page's responder card reads it
    /// from here.
    pub fn waiting_ask(&self) -> Option<&CheckpointAsk> {
        self.verses.iter().rev().find_map(|v| v.checkpoint.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed slice of the real `GET /kumbayas/runs` answer, captured
    /// live on 11 Nov 2026 (heavy fields included on purpose: they must be
    /// skipped at parse time, and a verse may report `pending`).
    const FIXTURE: &str = r##"[
        {"id": "r-empty", "kumbaya": "work_board_worker", "version": 6,
         "inputs": {"_drift_contract": "Ceremony: work_board_worker", "no_work": true},
         "status": "completed",
         "verses": [
             {"verse": "pick_item", "kind": "mcp_tool", "status": "completed",
              "result": "(no pending work items)", "checkpoint": null, "error": "",
              "attempts": 1, "started_at": "2026-11-11T23:02:11.499331Z",
              "finished_at": "2026-11-11T23:02:14.337430Z", "intent": "",
              "tool_name": "work_next", "process_name": ""},
             {"verse": "gate", "kind": "mcp_tool", "status": "completed",
              "result": "EMPTY_QUEUE", "tool_name": "work_next"},
             {"verse": "all_done", "kind": "report", "status": "completed",
              "result": "# Work board worker run complete", "tool_name": ""}
         ],
         "report": "Per-verse contract review: pick_item in tune, gate in tune.",
         "scheduled_job_id": null, "autonomous": true, "session_id": "chat:worker_kick",
         "created_at": "2026-11-11T23:02:11.498243Z",
         "finished_at": "2026-11-11T23:02:36.733484Z",
         "rating": null, "drift_streak": 0, "waste_llm_calls": 1},
        {"id": "r-waiting", "kumbaya": "work_board_worker", "status": "waiting_checkpoint",
         "verses": [
             {"verse": "execute_build.implement", "kind": "agent", "status": "completed", "tool_name": ""},
             {"verse": "checkpoint_review", "kind": "checkpoint", "status": "running", "tool_name": ""}
         ],
         "session_id": "chat:worker_kick", "created_at": "2026-11-11T23:05:00.000000Z"},
        {"id": "r-running", "kumbaya": "self_wake_watchdog", "status": "running",
         "verses": [
             {"verse": "look", "kind": "agent", "status": "completed", "tool_name": ""},
             {"verse": "act", "kind": "mcp_tool", "status": "running", "tool_name": "work_next"},
             {"verse": "tell", "kind": "report", "status": "pending", "tool_name": ""}
         ],
         "session_id": "scheduler:tell", "created_at": "2026-11-11T23:04:00.000000Z"}
    ]"##;

    fn parsed() -> Vec<RunSummary> {
        serde_json::from_str(FIXTURE).expect("the live run list shape parses")
    }

    #[test]
    fn parses_live_list_and_skips_heavy_fields() {
        let runs = parsed();
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].id, "r-empty");
        assert_eq!(runs[0].kumbaya, "work_board_worker");
        assert_eq!(runs[0].session_id, "chat:worker_kick");
        assert_eq!(runs[0].status, "completed");
        assert_eq!(runs[0].verses.len(), 3);
        assert_eq!(runs[0].verses[1].tool_name, "work_next");
    }

    #[test]
    fn current_verse_finds_the_running_one() {
        let runs = parsed();
        assert_eq!(runs[2].current_verse().expect("running verse").verse, "act");
        assert!(runs[0].current_verse().is_none(), "a finished run has none");
    }

    #[test]
    fn settled_verses_leaves_running_and_pending_out() {
        let runs = parsed();
        assert_eq!(runs[2].settled_verses(), 1);
        assert_eq!(runs[0].settled_verses(), 3);
    }

    #[test]
    fn campers_split_from_the_session_id() {
        assert_eq!(runs_camper("chat:worker_kick"), "worker_kick");
        assert_eq!(runs_camper("scheduler:tell"), "tell");
        assert_eq!(runs_camper("plain-id"), "plain-id");
        assert_eq!(runs_camper("  "), "");
    }

    fn runs_camper(session_id: &str) -> String {
        let run = RunSummary {
            id: "x".into(),
            kumbaya: "k".into(),
            status: "running".into(),
            session_id: session_id.into(),
            created_at: String::new(),
            finished_at: None,
            verses: Vec::new(),
        };
        run.camper().to_string()
    }

    #[test]
    fn order_puts_needs_you_then_running_then_the_rest_newest_first() {
        let mut runs = parsed();
        runs.reverse();
        order_runs(&mut runs);
        let ids: Vec<&str> = runs.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["r-waiting", "r-running", "r-empty"]);
    }

    #[test]
    fn groups_by_camper_keeping_the_order() {
        let mut runs = parsed();
        order_runs(&mut runs);
        let groups = group_by_camper(&runs);
        let names: Vec<&str> = groups.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["worker_kick", "tell"]);
        assert_eq!(groups[0].1.len(), 2, "worker_kick owns its waiting and finished runs");
        assert_eq!(groups[0].1[0].id, "r-waiting", "a camper's own rows stay in urgency order");
    }

    #[test]
    fn need_you_counts_the_checkpoints() {
        assert_eq!(need_you(&parsed()), 1);
        assert_eq!(need_you(&[]), 0);
    }

    #[test]
    fn age_text_formats_and_refuses_junk() {
        assert_eq!(age_text("not a date"), None);
        assert!(age_text("2020-01-01T00:00:00Z").is_some_and(|t| t.ends_with('m') || t.ends_with("now")));
    }

    #[test]
    fn join_parses_the_scheme_form() {
        let invite = "andruide://join?base=http%3A%2F%2F172.18.144.1%3A8001%2Fapi&user=mykibbert&password=op%20en%2Bz";
        let join = parse_join(invite).expect("a well-formed invite parses");
        assert_eq!(join.base, "http://172.18.144.1:8001/api");
        assert_eq!(join.user, "mykibbert");
        assert_eq!(join.password, "op en+z", "space and plus decode, trailing slash trimmed off base");
    }

    #[test]
    fn join_takes_a_bare_base_url() {
        let join = parse_join("http://192.168.1.50:8105/").expect("a bare base joins with no credentials");
        assert_eq!(join.base, "http://192.168.1.50:8105");
        assert!(join.user.is_empty() && join.password.is_empty());
    }

    #[test]
    fn join_refuses_what_is_not_an_invite() {
        assert!(parse_join("").is_none());
        assert!(parse_join("junk").is_none());
        assert!(parse_join("andruide://join").is_none(), "no query, no base");
        assert!(parse_join("andruide://join?user=x").is_none(), "a base is required");
    }

    /// A trimmed slice of the real `GET /work/{id}` answer, captured live
    /// on 12 Nov 2026 (extra fields included on purpose: serde must ignore
    /// them, and empty strings AND nulls read as none).
    const ITEM_FIXTURE: &str = r##"{
        "id": "work_ab12cd34ef", "title": "One-list routine taxonomy",
        "description": "Render schedules, todos and ceremonies through ONE presentation function.\n\nSecond paragraph of the spec.",
        "project": "andrew", "item_class": "work", "status": "running",
        "priority": 20, "run_now": false, "start_after": null,
        "deliverable_paths": [],
        "progress_log": ["[2026-11-11 21:00] claimed by worker", "[2026-11-11 21:02] plan landed"],
        "created_at": "2026-11-11T20:41:00.000000Z",
        "started_at": "2026-11-11T21:00:59.000000Z",
        "completed_at": null, "work_seconds": 40, "work_started_at": "2026-11-11",
        "error": "",
        "claim_run_id": "f7f19b49-3878-47c9-a0f3-bf635c13c10d",
        "fail_count": 0, "mint_count": 0, "research_findings": "",
        "worktree_path": "", "estimate_steps": 0, "staged": [], "stages": [],
        "stage_index": 0, "repair_passes": 0, "judge_stop": false,
        "stop_requested": false, "guidance": [], "guidance_read_index": 0
    }"##;

    #[test]
    fn parses_item_detail_and_ignores_the_rest() {
        let item: ItemDetail = serde_json::from_str(ITEM_FIXTURE).expect("the live item shape parses");
        assert_eq!(item.title, "One-list routine taxonomy");
        assert_eq!(item.status, "running");
        assert_eq!(item.priority, 20);
        assert!(item.progress_log.len() == 2 && item.failed_text().is_none());
        assert_eq!(item.claim_run(), Some("f7f19b49-3878-47c9-a0f3-bf635c13c10d"));
        assert_eq!(item.recent_log(1), ["[2026-11-11 21:02] plan landed"]);
    }

    /// The null form the detail bodies actually send (7 of 8 probed on
    /// 12 Nov 2026, Father m1593): one null field must not kill the decode.
    #[test]
    fn error_null_reads_as_none() {
        let item: ItemDetail = serde_json::from_str(
            r#"{"id":"work_null","title":"t","error":null,"priority":3}"#,
        )
        .expect("a null error parses");
        assert!(item.failed_text().is_none());
        assert_eq!(item.description, "");
        assert_eq!(item.claim_run(), None);
    }

    #[test]
    fn empty_claim_and_error_read_as_none() {
        let item: ItemDetail = serde_json::from_str("{\"id\":\"x\",\"title\":\"t\"}")
            .expect("a bare item object parses");
        assert_eq!(item.claim_run(), None);
        assert!(item.failed_text().is_none());
        assert!(item.progress_log.is_empty());
    }

    /// A trimmed slice of the real `GET /kumbayas/runs/{id}` answer, captured
    /// live on 12 Nov 2026 (a verse may say nothing yet, and its result can
    /// be long: the page caps the drawing, the client keeps it whole).
    const TALK_FIXTURE: &str = r##"{
        "id": "f7f19b49-3878-47c9-a0f3-bf635c13c10d",
        "kumbaya": "work_board_worker", "version": 6,
        "inputs": {"_claim_run_id": "f7f19b49"},
        "status": "running",
        "verses": [
            {"verse": "pick_item", "kind": "mcp_tool", "status": "completed",
             "result": "{\"id\": \"work_ab12cd34ef\", \"title\": \"One-list routine taxonomy\"}",
             "error": "", "tool_name": "work_next"},
            {"verse": "execute_build.implement", "kind": "agent", "status": "running",
             "result": null, "error": ""},
            {"verse": "checkpoint_review", "kind": "checkpoint", "status": "pending",
             "result": null}
        ],
        "report": "", "autonomous": true, "session_id": "chat:worker_kick",
        "created_at": "2026-11-11T21:00:59.000000Z",
        "updated_at": "2026-11-11T21:04:00.000000Z", "finished_at": null,
        "rating": null, "drift_streak": 0
    }"##;

    #[test]
    fn parses_the_conversation_and_finds_what_was_said() {
        let talk: RunTalk = serde_json::from_str(TALK_FIXTURE).expect("the live run shape parses");
        assert_eq!(talk.id, "f7f19b49-3878-47c9-a0f3-bf635c13c10d");
        assert_eq!(talk.kumbaya, "work_board_worker");
        assert_eq!(talk.verses.len(), 3);
        let said = talk.verses[0].said().expect("the pick verse said something");
        assert!(said.contains("\"title\": \"One-list routine taxonomy\""));
        assert!(talk.verses[1].said().is_none(), "a running verse hasn't spoken yet");
        assert!(talk.verses[2].said().is_none(), "a pending verse hasn't spoken yet");
    }

    /// The checkpoint ask travels on both the lite list read and the full
    /// detail read (the day review's ask, captured live 13 Nov 2026:
    /// question plus accept/refine/skip). A checkpoint verse without an
    /// ask still parses, as none.
    #[test]
    fn waiting_ask_carries_the_question_and_options() {
        let runs: Vec<RunSummary> = serde_json::from_str(
            r#"[{"id":"r-ask","kumbaya":"work_day_review","status":"waiting_checkpoint",
                 "session_id":"scheduled:work_day_review","created_at":"2026-11-13T15:30:22Z",
                 "verses":[{"verse":"review_checkpoint","kind":"checkpoint","status":"running",
                            "checkpoint":{"question":"Accept the scorecard, request a refine pass, or skip?",
                                          "options":["accept","refine","skip"]}}]}]"#,
        )
        .expect("a run with a checkpoint ask parses");
        let ask = runs[0].waiting_ask().expect("the ask is on the newest checkpoint verse");
        assert_eq!(ask.options, ["accept", "refine", "skip"]);
        assert!(ask.question.contains("Accept the scorecard"));
        let bare: RunSummary = serde_json::from_str(
            r#"{"id":"r-bare","status":"waiting_checkpoint",
                "verses":[{"verse":"review_checkpoint","kind":"checkpoint","status":"running"}]}"#,
        )
        .expect("a checkpoint verse without an ask parses (serde default)");
        assert_eq!(bare.waiting_ask(), None, "missing checkpoint reads as none");
    }

    #[test]
    fn the_detail_conversation_carries_the_ask_too() {
        let talk: RunTalk = serde_json::from_str(
            r#"{"id":"f7f19b49","kumbaya":"work_board_worker","status":"waiting_checkpoint",
                "verses":[{"verse":"review_checkpoint","kind":"checkpoint","status":"pending",
                           "checkpoint":{"question":"Accept the scorecard?","options":["accept"]}}]}"#,
        )
        .expect("the detail run shape parses");
        assert_eq!(talk.waiting_ask().expect("the ask").question, "Accept the scorecard?");
    }

    /// A trimmed slice of the real `GET /schedules` answer, captured live
    /// on 13 Nov 2026 (the history array rides through untouched: serde
    /// must ignore it). A null `interval_seconds` and a missing
    /// `last_result` both parse, and the kicker cadence renders.
    #[test]
    fn parses_the_scheduler_kicks() {
        let scheds: Vec<ScheduleEntry> = serde_json::from_str(
            r#"[
                {"id":"4a53fd29","name":"work_board_worker_11",
                 "prompt":"Process the next pending item.",
                 "target_campfire":"Andrew","cron":"40-59/20 * * * *",
                 "interval_seconds":null,"run_once":false,"run_at":null,
                 "enabled":true,"skill_to_save":null,"kumbaya_name":"work_board_worker",
                 "kumbaya_inputs":{},"kumbaya_autonomous":true,
                 "created_at":"2026-10-19T14:22:17.964975Z",
                 "last_run_at":"2026-11-12T17:40:44.608050Z",
                 "last_result":{"at":"2026-11-12T17:40:44.607914+00:00",
                                "summary":"(work_board_worker kick accepted: run started on the main loop)"},
                 "result_history":[{"at":"x","summary":"older word that must be ignored"}],
                 "next_run":"2026-11-12T18:40:00Z","status":"active"},
                {"name":"m8_hourly_kick","interval_seconds":28800,"enabled":false,
                 "cron":null}
            ]"#,
        )
        .expect("the live schedules shape parses");
        assert_eq!(scheds.len(), 2);
        assert_eq!(scheds[0].cadence().as_deref(), Some("40-59/20 * * * *"));
        assert_eq!(scheds[0].last_result.as_ref().and_then(|r| r.summary.as_deref()).map(|s| s.contains("kick accepted")), Some(true));
        assert_eq!(scheds[0].last_words().map(|s| s.contains("accepted")), Some(true));
        assert_eq!(scheds[0].next_run.as_deref(), Some("2026-11-12T18:40:00Z"));
        assert_eq!(scheds[1].cadence().as_deref(), Some("every 8h"), "the eight-hour kick reads from its interval");
        assert!(scheds[1].last_words().is_none());
    }

    #[test]
    fn age_seconds_reads_the_stamp_or_admits_it_cannot() {
        assert!(age_seconds("2026-01-01T00:00:00.000000Z").is_some(), "a past stamp parses");
        assert!(age_seconds("").is_none());
        assert!(age_seconds("not a stamp").is_none());
    }

    const KUMBAYA_FIXTURE: &str = r##"[
  {
    "name": "work_day_review",
    "summary": "The day review: score the day's work.",
    "version": 3,
    "status": "current",
    "kind": "mcp_tool",
    "updated_at": "2026-11-13T09:00:00Z",
    "inputs": {"day": "string"},
    "steps": []
  },
  {
    "name": "self_wake_watchdog",
    "summary": null,
    "version": 1
  }
]"##;

    #[test]
    fn parses_the_ceremony_list() {
        let rows: Vec<KumbayaSummary> = serde_json::from_str(KUMBAYA_FIXTURE).expect("parses");
        assert_eq!(rows.len(), 2, "both ceremonies land");
        assert_eq!(rows[0].name, "work_day_review");
        assert_eq!(rows[0].version, Some(3), "the version reads (int, live probed)");
        assert_eq!(rows[0].status.as_deref(), Some("current"));
        // Sparse rows: only the name is promised.
        assert_eq!(rows[1].name, "self_wake_watchdog");
        assert!(rows[1].summary.is_none());
        assert!(rows[1].kind.is_none());
    }

    const SESSIONS_FIXTURE: &str = r##"[
  {
    "session_id": "scheduled:self_improve (kumbaya)",
    "message_count": 4,
    "created_at": "2026-11-13T09:00:00Z",
    "last_active": "2026-11-13T09:12:00Z",
    "preview": "you returned no result",
    "phase": "discuss",
    "mode": "plan",
    "result_history": [{"at": "2026-11-13T09:12:00Z", "summary": "ignored"}]
  },
  {
    "session_id": "chat:andruide-machine-beelink",
    "message_count": 0,
    "preview": "Andrew is listening."
  }
]"##;

    #[test]
    fn parses_the_conversation_list() {
        let rows: Vec<SessionRow> = serde_json::from_str(SESSIONS_FIXTURE).expect("parses");
        assert_eq!(rows.len(), 2, "both rows land");
        assert_eq!(rows[0].session_id, "scheduled:self_improve (kumbaya)");
        assert_eq!(rows[0].message_count, Some(4));
        assert_eq!(rows[0].phase.as_deref(), Some("discuss"));
        // Extra fields the server carries are ignored, not fatal.
        assert!(rows[0].mode.is_some());
        assert!(rows[1].phase.is_none(), "a sparse row still lands");
        assert_eq!(rows[1].message_count, Some(0));
    }

    const TRANSCRIPT_FIXTURE: &str = r##"{
  "session_id": "chat:andruide-machine-beelink",
  "messages": [
    {
      "role": "user",
      "content": "Morning Andrew - what is on the board?",
      "images": [],
      "timestamp": "2026-11-13T09:00:00Z"
    },
    {
      "role": "assistant",
      "content": "Morning Dad. Eleven things pending, one is waiting on you.",
      "timestamp": "2026-11-13T09:00:04Z",
      "meta": {"duration_ms": 812}
    }
  ]
}"##;

    #[test]
    fn parses_a_transcript_of_his_turns() {
        let read: SessionRead = serde_json::from_str(TRANSCRIPT_FIXTURE).expect("parses");
        assert_eq!(read.session_id, "chat:andruide-machine-beelink");
        assert_eq!(read.messages.len(), 2, "both turns land");
        assert_eq!(read.messages[0].role, "user");
        assert_eq!(read.messages[1].role, "assistant");
        assert!(read.messages[1].content.contains("waiting on you"));
        assert!(read.messages[1].timestamp.is_some(), "the timestamp rides along");
    }
}
