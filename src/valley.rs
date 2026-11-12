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

    /// The valley's runs live: every kumbaya run (worker kicks, schedules,
    /// chats) with its verses — what the campfire is doing right now.
    pub async fn runs(&self) -> Result<Vec<RunSummary>, String> {
        if !self.configured() {
            return Err("The valley isn't configured — set it in Settings › Valley.".into());
        }
        let resp = self.get("/kumbayas/runs").await.map_err(|e| format!("The runs: {e}"))?;
        resp.json::<Vec<RunSummary>>()
            .await
            .map_err(|e| format!("The runs aren't the shape we expect: {e}"))
    }
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
}
