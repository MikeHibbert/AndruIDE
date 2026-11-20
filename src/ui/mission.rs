//! The mission-control strip: Andrew's own answer to "is the valley working,
//! and what is it costing", drawn above the Valley board.
//!
//! WHY (Father, 20 Nov 2026 — AndruIDE mission control, step 3): every number
//! here is READ from the valley, never derived in this app. Two failures of
//! that day set the rule: a client-side rate priced a $2.92 day at $10.69, and
//! a run that looked busy for a whole day had produced nothing. So the strip
//! asks the server's own cheap reads — `GET /work/stats` (counts, and which
//! revision of the board they came from), `GET /work/live-capacity` (today's
//! burn, the plan's remaining credits, the worker slots) and
//! `GET /kumbayas/runs/{id}/health` (working / stalled / finished, with the
//! flags) — and shows what came back, basis and all.

use std::time::Instant;

use gpui::{AnyElement, Context, Div, FontWeight, Hsla, SharedString, div, prelude::*, px};

use super::github::Load;
use super::kit::{Button, card, spinner};
use super::root::Root;
use super::theme::{mono_font, Theme};
use crate::valley::{LiveCapacity, RunHealth, Valley, WorkStats};

/// What one pass of the strip reads. Both are cheap server reads: /work/stats
/// is 0.2 KB and ~17 ms, live-capacity a few KB (measured 20 Nov 2026).
pub(super) struct Mission {
    pub stats: WorkStats,
    pub capacity: LiveCapacity,
}

/// How often the strip re-reads. Short enough that the burn is the day's real
/// number while Father looks at it, long enough not to hammer the valley.
const STALE_SECS: u64 = 30;
/// A failed read retries sooner: a valley that blinked should come back.
const RETRY_SECS: u64 = 10;
/// A health read is per-run; re-read on the same clock.
const HEALTH_STALE_SECS: u64 = 30;

impl Root {
    /// (Re)reads the strip's two cheap reads in one job, so the counts and the
    /// burn always describe the same moment.
    pub(super) fn load_mission(&mut self, cx: &mut Context<Self>) {
        self.mission = Some(Load::Loading);
        self.mission_at = Some(Instant::now());
        let mut cfg = self.engine.settings().valley.clone();
        if let Some(base) = self.valley_view.clone() {
            cfg.base_url = base;
        }
        let valley = Valley::from_parts(cfg);
        if !valley.configured() {
            self.mission = Some(Load::Failed("not-configured".into()));
            cx.notify();
            return;
        }
        let job = self.engine.runtime().spawn(async move {
            let stats = valley.work_stats().await;
            let capacity = valley.live_capacity().await;
            (stats, capacity)
        });
        let task = cx.spawn(async move |this, cx| {
            let (stats, capacity) = job
                .await
                .unwrap_or_else(|e| (Err(e.to_string()), Err(e.to_string())));
            let result = match (stats, capacity) {
                (Ok(stats), Ok(capacity)) => Ok(Mission { stats, capacity }),
                (Err(e), _) => Err(format!("the stats: {e}")),
                (_, Err(e)) => Err(format!("the capacity: {e}")),
            };
            let _ = this.update(cx, |this, cx| {
                this.mission = Some(match result {
                    Ok(m) => Load::Ready(m),
                    Err(e) => Load::Failed(e),
                });
                cx.notify();
            });
        });
        self.track(task);
    }

    /// Loads when the strip is missing or stale, and never while loading.
    fn maybe_load_mission(&mut self, cx: &mut Context<Self>) {
        let loading = matches!(self.mission, Some(Load::Loading));
        let failed = matches!(self.mission, Some(Load::Failed(_)));
        let window = if failed { RETRY_SECS } else { STALE_SECS };
        let stale = self.mission.is_none() || self.mission_at.is_some_and(|t| t.elapsed().as_secs() > window);
        if !loading && stale {
            self.load_mission(cx);
        }
    }

    /// Reads one run's health. Keyed by the run, so a new claim replaces the
    /// old answer instead of showing the wrong job's verdict.
    fn load_health(&mut self, run_id: String, cx: &mut Context<Self>) {
        self.mission_health = Some((run_id.clone(), Load::Loading));
        self.mission_health_at = Some(Instant::now());
        let mut cfg = self.engine.settings().valley.clone();
        if let Some(base) = self.valley_view.clone() {
            cfg.base_url = base;
        }
        let valley = Valley::from_parts(cfg);
        if !valley.configured() {
            self.mission_health = Some((run_id, Load::Failed("not-configured".into())));
            cx.notify();
            return;
        }
        let asked = run_id.clone();
        let job = self
            .engine
            .runtime()
            .spawn(async move { valley.run_health(&run_id).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                // Only answer for the run the strip is still asking about: a
                // read that came back after the claim moved is dropped rather
                // than shown against the new job.
                let still_asking = this.mission_health.as_ref().map(|(id, _)| id.as_str()) == Some(asked.as_str());
                if still_asking {
                    this.mission_health = Some((
                        asked,
                        match result {
                            Ok(health) => Load::Ready(health),
                            Err(e) => Load::Failed(e),
                        },
                    ));
                }
                cx.notify();
            });
        });
        self.track(task);
    }

    /// The strip: work counts, today's burn, and the active run's health.
    ///
    /// `active_run` is the claim of the first running item on the board (the
    /// page already holds the board). A stale claim shows up honestly here:
    /// the server answers `finished` for a run that is over, which is exactly
    /// the signal that the board's claim is stale.
    pub(super) fn mission_strip(
        &mut self,
        theme: &Theme,
        active_run: Option<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.maybe_load_mission(cx);
        if let Some(run_id) = &active_run {
            let keyed = self.mission_health.as_ref().map(|(id, _)| id.as_str());
            let loading = matches!(self.mission_health, Some((_, Load::Loading)));
            let stale = self
                .mission_health_at
                .is_none_or(|t| t.elapsed().as_secs() > HEALTH_STALE_SECS);
            if !loading && (keyed != Some(run_id.as_str()) || stale) {
                self.load_health(run_id.clone(), cx);
            }
        }

        let board: AnyElement = match &self.mission {
            Some(Load::Ready(m)) => board_card(theme, &m.stats),
            Some(Load::Loading) | None => {
                card(theme)
                    .p(px(12.))
                    .flex_1()
                    .min_w(px(190.))
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .text_color(theme.text_muted)
                    .child(spinner("mission-loading", 12., theme.text_muted))
                    .child("Asking the valley…")
                    .into_any_element()
            }
            Some(Load::Failed(e)) => failed_card(theme, "The board's counts", e),
        };
        let burn: AnyElement = match &self.mission {
            Some(Load::Ready(m)) => burn_card(theme, &m.capacity),
            _ => div().into_any_element(),
        };
        let health = self.health_card(theme, &active_run);
        div()
            .flex()
            .flex_wrap()
            .gap(px(8.))
            .child(board)
            .child(burn)
            .child(health)
            .child(
                div().flex_none().flex().items_center().child(
                    Button::icon_only("mission-refresh", "refresh-cw")
                        .tooltip("Read the valley's counts, burn and health again")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.mission = None;
                            this.mission_health = None;
                            this.mission_health_at = None;
                            this.load_mission(cx);
                        })),
                ),
            )
            .into_any_element()
    }

    fn health_card(&self, theme: &Theme, active_run: &Option<String>) -> AnyElement {
        let Some(run_id) = active_run else {
            return card(theme)
                .p(px(12.))
                .flex_1()
                .min_w(px(190.))
                .flex_col()
                .gap(px(3.))
                .child(label(theme, "The active run"))
                .child(div().text_size(px(13.)).text_color(theme.text_muted).child("Nothing is running — the board is quiet."))
                .into_any_element();
        };
        match &self.mission_health {
            Some((_, Load::Ready(h))) if h.run_id == *run_id || h.run_id.is_empty() => run_card(theme, h),
            Some((_, Load::Loading)) => card(theme)
                .p(px(12.))
                .flex_1()
                .min_w(px(190.))
                .flex_col()
                .gap(px(3.))
                .child(label(theme, "The active run"))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_size(px(12.5))
                        .text_color(theme.text_muted)
                        .child(spinner("mission-health-loading", 12., theme.text_muted))
                        .child("Reading its verses…"),
                )
                .into_any_element(),
            Some((_, Load::Failed(e))) if e.as_str() != "not-configured" => failed_card(theme, "The active run's health", e),
            _ => card(theme)
                .p(px(12.))
                .flex_1()
                .min_w(px(190.))
                .flex_col()
                .gap(px(3.))
                .child(label(theme, "The active run"))
                .child(div().text_size(px(13.)).text_color(theme.text_muted).child("Reading its verses…"))
                .into_any_element(),
        }
    }
}

fn label(theme: &Theme, text: &'static str) -> Div {
    div().text_size(px(11.)).font_weight(FontWeight::MEDIUM).text_color(theme.text_muted).child(text)
}

fn strip_card(theme: &Theme) -> Div {
    card(theme).p(px(12.)).flex_col().gap(px(3.)).flex_1().min_w(px(190.))
}

fn detail(theme: &Theme, text: impl Into<SharedString>) -> Div {
    div().text_size(px(11.5)).text_color(theme.text_muted).child(text.into())
}

fn failed_card(theme: &Theme, title: &'static str, error: &str) -> AnyElement {
    card(theme)
        .p(px(12.))
        .flex_1()
        .min_w(px(190.))
        .flex_col()
        .gap(px(3.))
        .child(label(theme, title))
        .child(div().text_size(px(12.)).text_color(theme.warning).child(format!("Out of reach: {error}")))
        .into_any_element()
}

fn board_card(theme: &Theme, stats: &WorkStats) -> AnyElement {
    strip_card(theme)
        .child(label(theme, "Work board"))
        .child(div().text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).child(format!("{} items", stats.work.total)))
        .child(detail(
            theme,
            format!(
                "{} waiting · {} running · {} blocked",
                stats.work.pending, stats.work.running, stats.work.blocked
            ),
        ))
        .child(detail(
            theme,
            format!(
                "{} done today · {} failed · {} needs you · {} campfires lit",
                stats.work.done_today, stats.work.failed, stats.runs.needs_you, stats.runs.running
            ),
        ))
        // The revision the counts came from: the same fingerprint the list
        // gate uses, so "the board changed" has one authority.
        .child(
            div()
                .font_family(mono_font())
                .text_size(px(10.5))
                .text_color(theme.text_placeholder)
                .child(format!("board rev {}", fingerprint_tail(&stats.fingerprint))),
        )
        .into_any_element()
}

fn burn_card(theme: &Theme, capacity: &LiveCapacity) -> AnyElement {
    let burn = &capacity.burn;
    if burn.cost_usd <= 0.0 && burn.cost_basis.is_none() {
        return strip_card(theme)
            .child(label(theme, "Today's burn"))
            .child(div().text_size(px(13.)).text_color(theme.text_muted).child("Andrew didn't report today's burn."))
            .into_any_element();
    }
    // The basis is shown, not implied: 'provider' is the provider's own bill,
    // 'rate table' is an estimate from published rates (Father, 20 Nov 2026).
    let basis = basis_word(burn.cost_basis.as_deref());
    let mut money = div().flex().items_end().gap(px(4.));
    money = money
        .child(div().text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).font_family(mono_font()).child(format!("${:.2}", burn.cost_usd)))
        .child(div().text_size(px(10.5)).text_color(theme.text_placeholder).child(basis));
    let mut card = strip_card(theme)
        .child(label(theme, "Today's burn"))
        .child(money);
    if let Some(cash) = burn.cash_usd {
        card = card.child(detail(theme, format!("${cash:.2} cash at the plan's rate")));
    }
    if let Some(allowance) = &burn.allowance {
        if let (Some(left), Some(total)) = (allowance.included_balance_usd, allowance.included_allowance_usd) {
            card = card.child(detail(theme, format!("${left:.2} of ${total:.0} credits left")));
        }
    }
    let calls = burn.total_calls.unwrap_or(0);
    let tokens = burn.provider_tokens.as_ref().map(|t| t.total).unwrap_or(0);
    card.child(detail(
        theme,
        format!(
            "{calls} calls · {} tokens · {}/{} worker slots busy",
            tokens_short(tokens),
            capacity.busy_slots(),
            capacity.pool_size
        ),
    ))
    .into_any_element()
}

fn run_card(theme: &Theme, health: &RunHealth) -> AnyElement {
    let (color, word) = verdict_style(theme, &health.verdict);
    let mut card = strip_card(theme)
        .child(label(theme, "The active run"))
        .child(
            div()
                .flex()
                .items_end()
                .gap(px(6.))
                .child(div().text_size(px(17.)).font_weight(FontWeight::SEMIBOLD).text_color(color).child(word))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme.text_placeholder)
                        .child(if health.kumbaya.is_empty() { "a run".to_string() } else { health.kumbaya.clone() }),
                ),
        )
        .child(detail(
            theme,
            format!(
                "{}/{} verses · {} did something · {} LLM calls",
                health.verses_done, health.verses_total, health.tool_verses, health.llm_calls
            ),
        ))
        .when(!health.current_verse.is_empty(), |d| {
            d.child(detail(theme, format!("on {}", health.current_verse)))
        });
    if let Some(secs) = health.since_last_verse_seconds {
        let note = if health.verdict == "stalled" {
            format!("nothing new for {}", duration_text(secs))
        } else {
            format!("last verse {}", duration_text(secs))
        };
        card = card.child(detail(theme, note));
    }
    let words = flag_words(health);
    if !words.is_empty() {
        card = card.child(div().text_size(px(11.5)).text_color(theme.warning).child(words.join(" · ")));
    }
    let usage = format!(
        "${:.4} {} · {} tokens",
        health.usage.cost_usd,
        basis_word(Some(health.usage.cost_basis.as_str())),
        tokens_short(health.usage.total_tokens)
    );
    let shown = match health.steps_done_of_estimate.as_deref() {
        Some(steps) => format!("{usage} · {steps} steps of the estimate"),
        None => usage,
    };
    card.child(div().font_family(mono_font()).text_size(px(10.5)).text_color(theme.text_placeholder).child(shown))
        .into_any_element()
}

/// The cost's provenance in a word — `provider` is the provider's own bill,
/// `rate table` an estimate from published rates. Never invented here; an
/// unknown basis is shown verbatim rather than rounded to one of ours.
fn basis_word(basis: Option<&str>) -> String {
    match basis {
        Some("provider_usage_api") => "provider".to_string(),
        Some("rate_table") => "rate table".to_string(),
        Some(other) if !other.is_empty() => other.to_string(),
        _ => "no basis".to_string(),
    }
}

fn verdict_style(theme: &Theme, verdict: &str) -> (Hsla, &'static str) {
    match verdict {
        "working" => (theme.accent, "Working"),
        "stalled" => (theme.warning, "Stalled"),
        "finished" => (theme.text_muted, "Finished"),
        _ => (theme.text_muted, "Unknown"),
    }
}

/// The server's flags, in words a person reads without a decoder ring.
fn flag_words(health: &RunHealth) -> Vec<String> {
    health
        .flags
        .iter()
        .map(|flag| match flag.as_str() {
            "no_tool_calls" => "not one tool call".to_string(),
            "llm_only_tail" => format!("{} verses of talk since it last did anything", health.verses_since_tool),
            "paid_for_degenerate_replies" => "paid for empty replies".to_string(),
            other => other.replace('_', " "),
        })
        .collect()
}

/// The board fingerprint's tail: it is a file hash, so only its tail is worth
/// drawing; an empty one says so.
fn fingerprint_tail(fingerprint: &str) -> String {
    let trimmed = fingerprint.trim();
    if trimmed.is_empty() {
        return "—".to_string();
    }
    let tail: String = trimmed.chars().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect();
    tail
}

fn tokens_short(n: i64) -> String {
    let n = n.max(0) as f64;
    if n >= 1_000_000.0 {
        format!("{:.1}M", n / 1_000_000.0)
    } else if n >= 1_000.0 {
        format!("{:.1}k", n / 1_000.0)
    } else {
        format!("{n:.0}")
    }
}

fn duration_text(secs: f64) -> String {
    let secs = secs.max(0.0) as u64;
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else {
        format!("{}h {:02}m ago", secs / 3600, (secs % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::valley::{CapacitySlot, RunCounts, StatCounts};

    /// Captured live from `GET /work/stats`, 20 Nov 2026 (AndruIDE step 3).
    const STATS: &str = r#"{"fingerprint":"1795197725473553144:22403329","work":{"total":261,"pending":2,"running":0,"blocked":2,"failed":30,"completed":64,"done_today":2},"runs":{"needs_you":0,"running":0,"completed":35,"failed":15}}"#;

    /// Captured live from `GET /work/live-capacity`, 20 Nov 2026, with the
    /// slots and projects trimmed (the burn block is verbatim).
    const CAPACITY: &str = r#"{
        "pool_size": 3, "pool_source": "env", "running_total": 0,
        "projects": [],
        "slots": [
            {"slot": 1, "name": "work_board_worker", "enabled": true, "state": "idle", "running": null, "project": null, "idle_reason": null},
            {"slot": 2, "name": "work_board_worker_2", "enabled": true, "state": "running", "running": "work_caf976e921", "project": "andrew", "idle_reason": null}
        ],
        "burn": {
            "day": "2026-11-20", "total_calls": 609, "total_tokens": 14285793,
            "cost_usd": 3.62081, "currency": "USD", "cost_display": 3.62081,
            "cost_basis": "provider_usage_api", "recorded_cost_usd": 1.006572,
            "cached_prompt_tokens": 11357056, "unpriced_calls": 0,
            "cash_usd": 0.965551,
            "plan": {"credits_usd": 300.0, "cash_usd": 80.0, "effective_rate": 0.266667, "source": "env"},
            "allowance": {"included_balance_usd": 281.83181, "included_allowance_usd": 300.0,
                          "purchased_balance_usd": 0.0, "period_from": "2026-11-15T18:45:46Z",
                          "period_until": "2026-12-15T18:45:46Z", "source": "ollama_balance_api"},
            "provider_tokens": {"input": 64672914, "cached_input": 55411152, "output": 599817, "total": 65272731},
            "ceiling": 10.0, "camper_num_predict": 8192
        }
    }"#;

    /// Captured live from `GET /kumbayas/runs/db8d80e4-…/health`, 20 Nov 2026.
    const HEALTH: &str = r#"{
        "run_id": "db8d80e4-1c62-4beb-b9fa-bf358ea4fbf5", "kumbaya": "startup_watch",
        "status": "completed", "verdict": "finished", "flags": [],
        "verses_total": 10, "verses_done": 6, "verses_failed": 0, "verses_skipped": 4,
        "current_verse": "harmony_check", "current_verse_status": "completed",
        "tool_verses": 2, "verses_since_tool": 4, "llm_calls": 2, "degenerate_replies": 0,
        "elapsed_seconds": 3.5, "since_last_verse_seconds": 3187.6,
        "estimate_steps": null, "steps_done_of_estimate": null, "item_id": "",
        "usage": {"calls": 2, "prompt_tokens": 8075, "cached_prompt_tokens": 1024,
                  "completion_tokens": 450, "total_tokens": 8525,
                  "cost_usd": 0.00133072, "cost_basis": "rate_table"}
    }"#;

    #[test]
    fn the_stats_are_the_servers_counts_and_its_board_revision() {
        let stats: WorkStats = serde_json::from_str(STATS).expect("the live /work/stats answer parses");
        assert_eq!(stats.work.total, 261);
        assert_eq!(stats.work.pending, 2);
        assert_eq!(stats.work.done_today, 2);
        assert_eq!(stats.work.failed, 30);
        assert_eq!(stats.runs.needs_you, 0);
        assert!(stats.fingerprint.starts_with("1795197725473553144:"));
    }

    #[test]
    fn the_burn_keeps_the_valleys_basis_and_the_plans_credits() {
        let capacity: LiveCapacity = serde_json::from_str(CAPACITY).expect("the live /work/live-capacity answer parses");
        let burn = &capacity.burn;
        // The number and where it came from, both as reported: never re-derived.
        assert_eq!(burn.cost_usd, 3.62081);
        assert_eq!(basis_word(burn.cost_basis.as_deref()), "provider");
        assert_eq!(burn.cash_usd, Some(0.965551));
        assert_eq!(burn.provider_tokens.as_ref().map(|t| t.total), Some(65_272_731));
        let allowance = burn.allowance.as_ref().expect("the allowance came with it");
        assert_eq!(allowance.included_balance_usd, Some(281.83181));
        assert_eq!(allowance.included_allowance_usd, Some(300.0));
        assert_eq!(capacity.pool_size, 3);
        assert_eq!(capacity.busy_slots(), 1);
    }

    #[test]
    fn a_valley_that_could_not_read_its_burn_says_nothing_rather_than_zero_dollars() {
        let capacity: LiveCapacity = serde_json::from_str(
            r#"{"pool_size": 2, "running_total": 0, "projects": [], "slots": [], "burn": {}}"#,
        )
        .expect("an empty burn block parses");
        assert_eq!(capacity.burn.cost_usd, 0.0);
        assert_eq!(capacity.burn.cost_basis, None);
        assert!(capacity.burn.allowance.is_none());
        assert_eq!(capacity.busy_slots(), 0);
    }

    #[test]
    fn the_health_read_keeps_the_valleys_verdict_and_the_runs_price() {
        let health: RunHealth = serde_json::from_str(HEALTH).expect("the live health answer parses");
        assert_eq!(health.verdict, "finished");
        assert_eq!(health.verses_total, 10);
        assert_eq!(health.verses_done, 6);
        assert_eq!(health.tool_verses, 2);
        assert_eq!(health.since_last_verse_seconds, Some(3187.6));
        // The run's cost and the basis it was credited on, as the valley says.
        assert_eq!(health.usage.total_tokens, 8525);
        assert_eq!(health.usage.cost_usd, 0.00133072);
        assert_eq!(basis_word(Some(health.usage.cost_basis.as_str())), "rate table");
        assert!(health.steps_done_of_estimate.is_none());
        assert_eq!(basis_word(None), "no basis");
        // An unfamiliar basis is shown as the valley named it, not guessed at.
        assert_eq!(basis_word(Some("invoice_import")), "invoice_import");
    }

    fn stats() -> WorkStats {
        WorkStats {
            fingerprint: "1795197725473553144:22403329".into(),
            work: StatCounts { total: 261, pending: 2, running: 0, blocked: 2, failed: 30, done_today: 2, ..Default::default() },
            runs: RunCounts { needs_you: 0, running: 0, ..Default::default() },
        }
    }

    #[test]
    fn the_fingerprint_shows_its_tail_only() {
        assert_eq!(fingerprint_tail(&stats().fingerprint), "22403329");
        assert_eq!(fingerprint_tail("  "), "—");
        assert_eq!(fingerprint_tail("abc"), "abc");
    }

    #[test]
    fn the_strips_flags_are_translated_not_dumped() {
        let health = RunHealth {
            flags: vec!["llm_only_tail".into(), "paid_for_degenerate_replies".into(), "no_tool_calls".into()],
            verses_since_tool: 4,
            ..Default::default()
        };
        let words = flag_words(&health);
        assert_eq!(words[0], "4 verses of talk since it last did anything");
        assert_eq!(words[1], "paid for empty replies");
        assert_eq!(words[2], "not one tool call");
        // A flag this app has never seen still reads as words, not snake_case.
        let unknown = RunHealth { flags: vec!["some_new_flag".into()], ..Default::default() };
        assert_eq!(flag_words(&unknown), vec!["some new flag".to_string()]);
    }

    #[test]
    fn tokens_and_durations_read_like_a_person_wrote_them() {
        assert_eq!(tokens_short(65_272_731), "65.3M");
        assert_eq!(tokens_short(8_525), "8.5k");
        assert_eq!(tokens_short(42), "42");
        assert_eq!(duration_text(12.0), "12s ago");
        assert_eq!(duration_text(605.0), "10m ago");
        assert_eq!(duration_text(3_900.0), "1h 05m ago");
    }

    #[test]
    fn busy_slots_are_the_ones_holding_work() {
        let capacity = LiveCapacity {
            pool_size: 3,
            slots: vec![
                CapacitySlot { running: Some("work_a".into()), ..Default::default() },
                CapacitySlot { running: None, ..Default::default() },
                CapacitySlot { running: Some("work_b".into()), ..Default::default() },
            ],
            ..Default::default()
        };
        assert_eq!(capacity.busy_slots(), 2);
    }
}
