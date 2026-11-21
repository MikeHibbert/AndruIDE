//! Triage: the valley's work that is waiting on a person, on one page.
//!
//! WHY (Father, 20 Nov 2026 — AndruIDE mission control, step 5): the board has
//! lanes and the strip has counts, but neither says "these are the three
//! things you have to decide". Andrew's own `/work/stats` counts what needs
//! you and stops there; the waiting runs are the runs the valley flags
//! `waiting_checkpoint`, the stale claims are the ones the strip already
//! exposes (a board row claiming a run the server says is over), and the
//! failures are the item's own word. This page gathers the three, with the
//! evidence each one came with, and one action each.
//!
//! Two things it deliberately does not do. It never calls a claim stale on
//! absence alone: the runs read is the last 50, and a long run created 60
//! runs ago would be missing from it while still working, so absence proves
//! nothing and the preflight answers when asked. And it never kicks without
//! asking twice: Retry reads the preflight, reports its verdict, and only
//! sends on a second press — the same discipline as the command bar, because
//! the same endpoint is behind both.

use std::time::Instant;

use gpui::{AnyElement, Context, Div, FontWeight, Hsla, SharedString, div, prelude::*, px};

use super::command::verdict_line;
use super::github::Load;
use super::input::TextInput;
use super::kit::{self, Button, ButtonStyle, card, section_title, spinner};
use super::overview::page;
use super::root::{Root, Route};
use super::theme::{ActiveTheme, Theme, mono_font};
use crate::valley::{RunSummary, WorkItem};

/// The page's sidebar tile colour: ember, not the campfire orange.
pub const TILE: (u32, u32) = (0xff8f7a, 0xd9503f);

/// How long an armed kick waits for its second press.
const ARM_SECS: u64 = 60;
/// How long the page's last note stays on screen.
const NOTE_SECS: u64 = 45;
/// The same board window the Valley page uses: a heavy read, refreshed on
/// visit at most every five minutes (that page's own rule, Father 12 Nov 2026).
const BOARD_STALE_SECS: u64 = 300;

/// One thing that needs a person, carrying the evidence it came with.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum TriageRow {
    /// A run stopped at a checkpoint: it will not move until it is answered.
    Waiting {
        run_id: String,
        kumbaya: String,
        camper: String,
    },
    /// An item the valley marked failed. Its claim run, when it names one, is
    /// evidence to show - not a verdict: the preflight says what is true now.
    Failed {
        item_id: String,
        title: String,
        project: String,
        claim: Option<String>,
    },
    /// A board row claiming a run the valley still lists as over: the board
    /// is describing work that no longer exists.
    StaleClaim {
        item_id: String,
        title: String,
        claim: String,
        run_status: String,
    },
}

/// The three kinds, in the order a person should read them.
pub(super) fn triage_rows(items: &[WorkItem], runs: &[RunSummary]) -> Vec<TriageRow> {
    let mut out = Vec::new();
    // What is blocking Andrew comes first: a checkpointed run does nothing
    // until someone answers it.
    for run in runs {
        if run.status == "waiting_checkpoint" {
            out.push(TriageRow::Waiting {
                run_id: run.id.clone(),
                kumbaya: run.kumbaya.clone(),
                camper: run.camper().to_string(),
            });
        }
    }
    for item in items {
        let claim = item.claim_run().map(str::to_string);
        if item.status == "failed" {
            out.push(TriageRow::Failed {
                item_id: item.id.clone(),
                title: item.title.clone(),
                project: item.project.clone(),
                claim,
            });
            continue;
        }
        let Some(claim) = claim else { continue };
        // Only a run still in the window can be judged here; a claim on a run
        // older than the last 50 is left to the preflight (see the module
        // note - absence is not proof).
        if let Some(run) = runs.iter().find(|r| r.id == claim) {
            if is_over(&run.status) {
                out.push(TriageRow::StaleClaim {
                    item_id: item.id.clone(),
                    title: item.title.clone(),
                    claim,
                    run_status: run.status.clone(),
                });
            }
        }
    }
    out
}

/// Anything the engine is no longer driving. `waiting_checkpoint` is still
/// Andrew's to finish, so it is not over - it is waiting.
fn is_over(status: &str) -> bool {
    !matches!(status, "running" | "waiting_checkpoint" | "pending")
}

impl Root {
    /// The Triage page.
    pub(super) fn render_triage(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        // The runs half refreshes on the Run page's clock (60s, 15s after a
        // failure); the board half on the Valley page's (5 min).
        self.maybe_load_runs(cx);
        if self.valley_data.is_none() || self.valley_at.is_some_and(|t| t.elapsed().as_secs() > BOARD_STALE_SECS)
        {
            self.load_valley(cx);
        }

        // Everything the rows need, owned, so rendering can create inputs and
        // read health without fighting the borrow of the two caches.
        let items: Vec<WorkItem> = match &self.valley_data {
            Some(Load::Ready(data)) => data.items.clone(),
            _ => Vec::new(),
        };
        let runs: Vec<RunSummary> = match &self.valley_runs {
            Some(Load::Ready(runs)) => runs.clone(),
            _ => Vec::new(),
        };
        let rows = triage_rows(&items, &runs);
        let board_problem: Option<String> = match &self.valley_data {
            Some(Load::Failed(e)) => Some(e.clone()),
            _ => None,
        };
        let runs_problem: Option<String> = match &self.valley_runs {
            Some(Load::Failed(e)) => Some(e.clone()),
            _ => None,
        };
        let loading = matches!(self.valley_data, None | Some(Load::Loading))
            || matches!(self.valley_runs, None | Some(Load::Loading));

        let waiting = rows.iter().filter(|r| matches!(r, TriageRow::Waiting { .. })).count();
        let stale = rows.iter().filter(|r| matches!(r, TriageRow::StaleClaim { .. })).count();
        let failed = rows.iter().filter(|r| matches!(r, TriageRow::Failed { .. })).count();

        let header = div()
            .flex()
            .flex_wrap()
            .items_end()
            .justify_between()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child("Triage"))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(theme.text_muted)
                            .child("What needs you: runs stopped at a checkpoint, claims the board outlived, and failures worth another go. Each action asks the valley before it acts."),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(Button::icon_only("triage-refresh", "refresh-cw").tooltip("Read the board and the runs again").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.valley_data = None;
                            this.valley_at = None;
                            this.valley_runs = None;
                            this.valley_runs_at = None;
                            this.triage_health.clear();
                            this.triage_armed = None;
                            this.load_valley(cx);
                            this.maybe_load_runs(cx);
                            cx.notify();
                        },
                    ))),
            );

        let mut body = page("triage-page", &self.page_scroll).child(header).child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(8.))
                .child(chip(&theme, "waiting on you", waiting, theme.accent))
                .child(chip(&theme, "stale claims", stale, theme.warning))
                .child(chip(&theme, "failed", failed, theme.text_muted)),
        );

        if let Some((text, error, at)) = &self.triage_note {
            if at.elapsed().as_secs() < NOTE_SECS {
                body = body.child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(if *error { theme.warning } else { theme.text })
                        .child(text.clone()),
                );
            }
        }

        if let Some(problem) = board_problem {
            body = body.child(
                card(&theme)
                    .p(px(14.))
                    .flex_col()
                    .gap(px(4.))
                    .child(div().text_size(px(13.)).text_color(theme.text).child("The board is out of reach."))
                    .child(div().text_size(px(12.)).text_color(theme.warning).child(problem)),
            );
        }
        if let Some(problem) = runs_problem {
            body = body.child(
                card(&theme)
                    .p(px(14.))
                    .flex_col()
                    .gap(px(4.))
                    .child(div().text_size(px(13.)).text_color(theme.text).child("The campfire is out of reach."))
                    .child(div().text_size(px(12.)).text_color(theme.warning).child(problem)),
            );
        }

        if rows.is_empty() {
            body = body.child(if loading {
                card(&theme)
                    .p(px(18.))
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .text_size(px(13.))
                    .text_color(theme.text_muted)
                    .child(spinner("triage-loading", 14., theme.text_muted))
                    .child("Reading the valley…")
            } else {
                card(&theme)
                    .p(px(18.))
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .text_size(px(13.))
                    .text_color(theme.text_muted)
                    .child("Nothing needs you right now — no run is waiting on an answer, and no claim outlived its run.")
            });
        } else {
            // Waiting, then stale claims, then failures: the order a person
            // should read them in, and only the groups that have rows.
            let groups: [(&'static str, fn(&TriageRow) -> bool); 3] = [
                ("Waiting on you", |r| matches!(r, TriageRow::Waiting { .. })),
                ("Claims that outlived their runs", |r| matches!(r, TriageRow::StaleClaim { .. })),
                ("Failed, worth another go", |r| matches!(r, TriageRow::Failed { .. })),
            ];
            for (title, kind) in groups {
                let in_group: Vec<&TriageRow> = rows.iter().filter(|r| kind(r)).collect();
                if in_group.is_empty() {
                    continue;
                }
                body = body.child(section_title(&theme, title, Some(in_group.len())));
                for row in in_group {
                    let card = self.render_triage_row(row, &theme, cx);
                    body = body.child(card);
                }
            }
            // The honest footnote: failures the window cannot judge are still
            // failures, and the Retry button asks the preflight when pressed.
            let unknown = rows
                .iter()
                .filter(|r| matches!(r, TriageRow::Failed { claim: Some(_), .. }))
                .count();
            if unknown > 0 {
                body = body.child(
                    div()
                        .pt(px(4.))
                        .text_size(px(11.))
                        .text_color(theme.text_placeholder)
                        .child(format!(
                            "{unknown} of these name a claim run. A claim is only called stale here when the run is still listed as over; otherwise Retry asks the preflight, which knows."
                        )),
                );
            }
        }
        body.into_any_element()
    }

    fn render_triage_row(&mut self, row: &TriageRow, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        match row {
            TriageRow::Waiting { run_id, kumbaya, camper } => {
                let input = self
                    .triage_answer
                    .entry(run_id.clone())
                    .or_insert_with(|| cx.new(|cx| TextInput::new(cx, "your answer…")))
                    .clone();
                let id = run_id.clone();
                let key = SharedString::from(format!("triage-answer-{id}"));
                let label = if kumbaya.is_empty() { "a run" } else { kumbaya.as_str() };
                card(theme)
                    .p(px(14.))
                    .flex_col()
                    .gap(px(6.))
                    .child(row_head(theme, theme.accent, "waiting on you", label, run_id))
                    .when(!camper.is_empty(), |d| d.child(detail(theme, format!("camper {camper}"))))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().flex_1().min_w(px(0.)).child(input))
                            .child(
                                Button::new(key, "Answer")
                                    .style(ButtonStyle::Primary)
                                    .on_click(cx.listener(move |this, _, _, cx| this.triage_answer(id.clone(), cx))),
                            ),
                    )
                    .into_any_element()
            }
            TriageRow::Failed { item_id, title, project, claim } => {
                let mut card = card(theme)
                    .p(px(14.))
                    .flex_col()
                    .gap(px(5.))
                    .child(row_head(theme, theme.error, "failed", title, item_id))
                    .when(!project.is_empty(), |d| d.child(detail(theme, format!("project {project}"))));
                if let Some(claim) = claim {
                    card = card.child(detail(theme, format!("claimed by run {claim}")));
                    if let Some((line, warn)) = self.triage_health_line(claim) {
                        card = card.child(div().text_size(px(11.5)).text_color(if warn { theme.warning } else { theme.text_muted }).child(line));
                    }
                }
                let mut actions = div().flex().flex_wrap().items_center().gap(px(6.)).child(
                    Button::new(SharedString::from(format!("triage-retry-{item_id}")), if self.armed_for(item_id) { "Send the kick" } else { "Retry" })
                        .style(if self.armed_for(item_id) { ButtonStyle::Primary } else { ButtonStyle::Outlined })
                        .on_click(cx.listener({
                            let item = item_id.clone();
                            move |this, _, _, cx| this.triage_retry(item.clone(), cx)
                        })),
                );
                if let Some(claim) = claim {
                    actions = actions.child(
                        Button::new(SharedString::from(format!("triage-why-{item_id}")), "Why")
                            .style(ButtonStyle::Ghost)
                            .on_click(cx.listener({
                                let run = claim.clone();
                                move |this, _, _, cx| this.triage_why(run.clone(), cx)
                            })),
                    );
                }
                actions = actions.child(
                    Button::new(SharedString::from(format!("triage-open-{item_id}")), "Open")
                        .style(ButtonStyle::Ghost)
                        .on_click(cx.listener({
                            let item = item_id.clone();
                            move |this, _, window, cx| this.navigate(Route::ValleyItem(item.clone()), window, cx)
                        })),
                );
                card.child(actions).into_any_element()
            }
            TriageRow::StaleClaim { item_id, title, claim, run_status } => {
                let label = format!("claimed by run {claim}, which is {run_status}");
                card(theme)
                    .p(px(14.))
                    .flex_col()
                    .gap(px(5.))
                    .child(row_head(theme, theme.warning, "stale claim", title, item_id))
                    .child(detail(theme, label))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                Button::new(SharedString::from(format!("triage-retry-{item_id}")), if self.armed_for(item_id) { "Send the kick" } else { "Retry" })
                                    .style(if self.armed_for(item_id) { ButtonStyle::Primary } else { ButtonStyle::Outlined })
                                    .on_click(cx.listener({
                                        let item = item_id.clone();
                                        move |this, _, _, cx| this.triage_retry(item.clone(), cx)
                                    })),
                            )
                            .child(
                                Button::new(SharedString::from(format!("triage-stop-{claim}")), "Stop the run")
                                    .style(ButtonStyle::Ghost)
                                    .on_click(cx.listener({
                                        let run = claim.clone();
                                        move |this, _, _, cx| this.triage_stop(run.clone(), cx)
                                    })),
                            )
                            .child(
                                Button::new(SharedString::from(format!("triage-open-{item_id}")), "Open")
                                    .style(ButtonStyle::Ghost)
                                    .on_click(cx.listener({
                                        let item = item_id.clone();
                                        move |this, _, window, cx| this.navigate(Route::ValleyItem(item.clone()), window, cx)
                                    })),
                            ),
                    )
                    .into_any_element()
            }
        }
    }

    fn armed_for(&self, item_id: &str) -> bool {
        self.triage_armed
            .as_ref()
            .is_some_and(|(id, at)| id == item_id && at.elapsed().as_secs() < ARM_SECS)
    }

    /// A claim run's health as one line, once it has been asked for.
    fn triage_health_line(&self, run_id: &str) -> Option<(String, bool)> {
        match self.triage_health.get(run_id) {
            Some((Load::Ready(h), _)) => {
                let mut line = format!(
                    "{} · {}/{} verses · {} did something · ${:.4} {}",
                    h.verdict, h.verses_done, h.verses_total, h.tool_verses, h.usage.cost_usd, h.usage.cost_basis
                );
                if !h.flags.is_empty() {
                    line.push_str(&format!(" · {}", h.flags.join(", ")));
                }
                Some((line, h.verdict == "stalled"))
            }
            Some((Load::Failed(e), _)) => Some((format!("its health is out of reach: {e}"), true)),
            Some((Load::Loading, _)) => Some(("reading its verses…".to_string(), false)),
            None => None,
        }
    }

    fn triage_why(&mut self, run_id: String, cx: &mut Context<Self>) {
        self.triage_health.insert(run_id.clone(), (Load::Loading, Instant::now()));
        cx.notify();
        let valley = self.valley_client();
        let asked = run_id.clone();
        let job = self.engine.runtime().spawn(async move { valley.run_health(&run_id).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                let load = match result {
                    Ok(health) => Load::Ready(health),
                    Err(e) => Load::Failed(e),
                };
                this.triage_health.insert(asked, (load, Instant::now()));
                cx.notify();
            });
        });
        self.track(task);
    }

    /// Retry asks the preflight, reports its verdict, and only sends on a
    /// second press inside a minute.
    fn triage_retry(&mut self, item_id: String, cx: &mut Context<Self>) {
        let valley = self.valley_client();
        let requested = item_id.clone();
        let job = self.engine.runtime().spawn(async move { valley.run_preflight(&item_id).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| match result {
                Err(e) => this.triage_note(e, true, cx),
                Ok(pre) if !pre.claimable => this.triage_note(format!("{} stays put — {}", pre.item_id, verdict_line(&pre)), true, cx),
                Ok(pre) => {
                    if this.armed_for(&requested) {
                        this.triage_armed = None;
                        this.triage_send_kick(requested.clone(), cx);
                    } else {
                        this.triage_armed = Some((requested.clone(), Instant::now()));
                        this.triage_note(
                            format!(
                                "{} is {}. Press Retry again within {ARM_SECS}s to send it{}.",
                                pre.item_id,
                                verdict_line(&pre),
                                if pre.stale_claim { " — its old claim is stale, so this is safe" } else { "" }
                            ),
                            false,
                            cx,
                        );
                    }
                }
            });
        });
        self.track(task);
    }

    fn triage_send_kick(&mut self, item: String, cx: &mut Context<Self>) {
        let valley = self.valley_client();
        let job = self.engine.runtime().spawn(async move { valley.kick(&item).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(outcome) => {
                        this.triage_note(outcome.message(), false, cx);
                        // The valley just changed: both reads are stale now.
                        this.valley_data = None;
                        this.load_valley(cx);
                        this.valley_runs = None;
                        this.maybe_load_runs(cx);
                    }
                    Err(e) => this.triage_note(e, true, cx),
                }
                cx.notify();
            });
        });
        self.track(task);
    }

    /// Stop the claim's run from a stale row: the board row names a run that
    /// is already over, so this only ever clears what is left behind.
    fn triage_stop(&mut self, run_id: String, cx: &mut Context<Self>) {
        let valley = self.valley_client();
        let asked = run_id.clone();
        let job = self.engine.runtime().spawn(async move { valley.cancel_run(&run_id).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.triage_note(format!("Asked the valley to stop {asked}."), false, cx);
                        this.valley_data = None;
                        this.load_valley(cx);
                        this.valley_runs = None;
                        this.maybe_load_runs(cx);
                    }
                    Err(e) => this.triage_note(format!("{asked}: {e}"), true, cx),
                }
                cx.notify();
            });
        });
        self.track(task);
    }

    fn triage_answer(&mut self, run_id: String, cx: &mut Context<Self>) {
        let Some(input) = self.triage_answer.get(&run_id).cloned() else {
            return;
        };
        let text = input.read(cx).text().trim().to_string();
        if text.is_empty() {
            self.triage_note(format!("Type an answer for {run_id} first."), true, cx);
            return;
        }
        let valley = self.valley_client();
        let asked = run_id.clone();
        let job = self.engine.runtime().spawn(async move { valley.resume(&run_id, &text).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        input.update(cx, |input, cx| input.set_text("", cx));
                        this.triage_note(format!("Answered {asked} — it carries on from there."), false, cx);
                        this.valley_runs = None;
                        this.maybe_load_runs(cx);
                    }
                    // The valley's own words, including a 409 on a run that
                    // has already moved on.
                    Err(e) => this.triage_note(format!("{asked}: {e}"), true, cx),
                }
                cx.notify();
            });
        });
        self.track(task);
    }

    fn triage_note(&mut self, text: impl Into<String>, error: bool, cx: &mut Context<Self>) {
        self.triage_note = Some((text.into(), error, Instant::now()));
        cx.notify();
    }

    /// The sidebar badge: how many runs are stopped waiting on a person.
    pub(super) fn triage_badge(&self) -> Option<usize> {
        if let Some(Load::Ready(runs)) = &self.valley_runs {
            let n = runs.iter().filter(|r| r.status == "waiting_checkpoint").count();
            if n > 0 {
                return Some(n);
            }
        }
        match &self.valley_stats {
            Some(Load::Ready(stats)) if stats.runs.needs_you > 0 => Some(stats.runs.needs_you),
            _ => None,
        }
    }
}

fn chip(theme: &Theme, label: &'static str, n: usize, color: Hsla) -> Div {
    div()
        .px(px(10.))
        .py(px(6.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(kit::corner(8.))
        .bg(theme.element)
        .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).font_family(mono_font()).text_color(if n > 0 { color } else { theme.text_muted }).child(n.to_string()))
        .child(div().text_size(px(11.5)).text_color(theme.text_muted).child(label))
}

fn row_head(theme: &Theme, color: Hsla, badge: &str, title: &str, id: &str) -> Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(8.))
        .child(
            div()
                .px(px(6.))
                .h(px(20.))
                .flex()
                .items_center()
                .rounded(kit::corner(6.))
                .bg(theme.element)
                .text_size(px(11.))
                .font_family(mono_font())
                .text_color(color)
                .child(badge.to_string()),
        )
        .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(title.to_string()))
        .child(
            div()
                .text_size(px(11.5))
                .font_family(mono_font())
                .text_color(theme.text_placeholder)
                .child(id.to_string()),
        )
}

fn detail(theme: &Theme, text: impl Into<SharedString>) -> Div {
    div().text_size(px(11.5)).text_color(theme.text_muted).child(text.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, status: &str, claim: Option<&str>) -> WorkItem {
        WorkItem {
            id: id.into(),
            title: format!("{id} title"),
            status: status.into(),
            priority: 2,
            project: "andrew".into(),
            created_at: String::new(),
            started_at: None,
            claim_run_id: claim.map(str::to_string),
        }
    }

    fn run(id: &str, status: &str) -> RunSummary {
        RunSummary {
            id: id.into(),
            kumbaya: "work_board_worker".into(),
            status: status.into(),
            session_id: Some("chat:worker_kick".into()),
            created_at: String::new(),
            finished_at: None,
            verses: Vec::new(),
        }
    }

    #[test]
    fn what_blocks_andrew_is_listed_first() {
        let rows = triage_rows(&[item("work_1", "failed", None)], &[run("r-1", "waiting_checkpoint")]);
        assert_eq!(rows.len(), 2);
        assert!(matches!(rows[0], TriageRow::Waiting { .. }));
        assert!(matches!(rows[1], TriageRow::Failed { .. }));
    }

    #[test]
    fn a_failed_item_keeps_its_story_and_is_listed_once() {
        // The claim is dead, but the row is about the failure: retrying it is
        // the action either way, and the preflight judges the claim.
        let rows = triage_rows(&[item("work_1", "failed", Some("r-dead"))], &[run("r-dead", "completed")]);
        assert_eq!(rows.len(), 1);
        match &rows[0] {
            TriageRow::Failed { claim, .. } => assert_eq!(claim.as_deref(), Some("r-dead")),
            other => panic!("expected the failure, got {other:?}"),
        }
    }

    #[test]
    fn a_claim_on_a_run_the_valley_lists_as_over_is_stale() {
        let rows = triage_rows(&[item("work_2", "running", Some("r-done"))], &[run("r-done", "completed")]);
        assert_eq!(rows.len(), 1);
        match &rows[0] {
            TriageRow::StaleClaim { claim, run_status, .. } => {
                assert_eq!(claim, "r-done");
                assert_eq!(run_status, "completed");
            }
            other => panic!("expected a stale claim, got {other:?}"),
        }
    }

    #[test]
    fn a_claim_on_a_run_still_working_is_not_stale() {
        for status in ["running", "waiting_checkpoint", "pending"] {
            let rows = triage_rows(&[item("work_3", "running", Some("r-live"))], &[run("r-live", status)]);
            // A checkpointed run is a Waiting row of its own; the point here
            // is only that its claim is not called stale.
            assert!(
                !rows.iter().any(|r| matches!(r, TriageRow::StaleClaim { .. })),
                "{status} must not read as stale"
            );
        }
    }

    #[test]
    fn a_claim_outside_the_window_is_not_called_stale() {
        // The runs read is the last 50: a long run created 60 runs ago is
        // missing from it while still working, so absence proves nothing.
        let rows = triage_rows(&[item("work_4", "running", Some("r-old"))], &[run("r-other", "running")]);
        assert!(rows.is_empty());
    }
}
