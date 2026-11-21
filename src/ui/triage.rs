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
use crate::valley::{Investigation, ItemDetail, RescueReceipts, RunSummary, WorkItem};

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
    /// An item the valley has blocked: not a failure, and not running. The
    /// action that helps is reading its evidence, then requeue or stop -
    /// retrying a blocked item blindly is how a loop is born.
    Blocked {
        item_id: String,
        title: String,
        project: String,
    },
    /// A rescue report no person has acknowledged: the agent's own account of
    /// a failure it triaged. Unacknowledged is still work waiting on a human.
    Rescue {
        wake_id: String,
        item_id: String,
        proposal: String,
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
        // WHY blocked is its own row (triage console, Father 21 Nov 2026): a
        // blocked item is not a failure - something stopped it on purpose - so
        // "Retry" is the wrong first suggestion. Its evidence decides between
        // requeue and stop.
        if item.status == "blocked" {
            out.push(TriageRow::Blocked {
                item_id: item.id.clone(),
                title: item.title.clone(),
                project: item.project.clone(),
            });
            continue;
        }
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

/// Rescue reports a person has not acknowledged yet. The feed is lite and
/// capped by the valley; an unacked one is exactly what the console is for.
pub(super) fn rescue_rows(receipts: &RescueReceipts) -> Vec<TriageRow> {
    receipts
        .receipts
        .iter()
        .filter(|r| r.acked != Some(true))
        .filter(|r| !r.wake_id.trim().is_empty())
        .map(|r| {
            let proposal = if r.proposal.trim().is_empty() { r.speak_to_andrew.trim() } else { r.proposal.trim() };
            TriageRow::Rescue {
                wake_id: r.wake_id.clone(),
                item_id: r.item_id.clone(),
                proposal: clipped(proposal, 220),
            }
        })
        .collect()
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
        // The console's two extra reads: the drain flag (it explains a whole
        // class of declined actions) and the rescue feed (its unacked rows are
        // work waiting on a person).
        let stale60 = |at: Option<Instant>| at.map_or(true, |t| t.elapsed().as_secs() > 60);
        if stale60(self.triage_drain.as_ref().map(|(_, at)| *at)) {
            self.load_triage_drain(cx);
        }
        if stale60(self.triage_receipts.as_ref().map(|(_, at)| *at)) {
            self.load_triage_receipts(cx);
        }
        let mut rows = triage_rows(&items, &runs);
        if let Some((Load::Ready(receipts), _)) = &self.triage_receipts {
            rows.extend(rescue_rows(receipts));
        }
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
        let blocked = rows.iter().filter(|r| matches!(r, TriageRow::Blocked { .. })).count();
        let failed = rows.iter().filter(|r| matches!(r, TriageRow::Failed { .. })).count();
        let rescue = rows.iter().filter(|r| matches!(r, TriageRow::Rescue { .. })).count();

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
                            .child("What needs you: runs stopped at a checkpoint, claims the board outlived, blocked items, failures worth another go, and rescue reports nobody has answered. Evidence first, then the action the evidence supports - and every write asks before it sends."),
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
                .child(chip(&theme, "blocked", blocked, theme.warning))
                .child(chip(&theme, "failed", failed, theme.text_muted))
                .child(chip(&theme, "to acknowledge", rescue, theme.text_muted)),
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

        // The drain flag, when it is on: the queue takes no new claims, which
        // is the honest context for every Retry on the page.
        if let Some((Load::Ready(drain), _)) = &self.triage_drain {
            if drain.draining {
                let since = drain.since.clone().unwrap_or_else(|| "an unrecorded time".into());
                body = body.child(
                    card(&theme)
                        .p(px(12.))
                        .flex_col()
                        .gap(px(3.))
                        .child(div().text_size(px(12.5)).font_weight(FontWeight::MEDIUM).text_color(theme.warning).child(format!("The valley is draining since {since}.")))
                        .child(div().text_size(px(11.5)).text_color(theme.text_muted).child(format!("No new claims are taken; {} item(s) are running and {} run(s) are in flight. A Retry now will be declined in the valley's own words.", drain.running_items, drain.active_runs))),
                );
            }
        } else if let Some((Load::Failed(e), _)) = &self.triage_drain {
            body = body.child(detail(&theme, format!("The drain state is out of reach: {e}")));
        }
        if let Some((Load::Failed(e), _)) = &self.triage_receipts {
            body = body.child(detail(&theme, format!("The rescue feed is out of reach: {e}")));
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
                    .child("Nothing needs you right now — no run is waiting on an answer, no claim outlived its run, nothing is blocked, and no rescue report is unacknowledged.")
            });
        } else {
            // Waiting, then stale claims, then failures: the order a person
            // should read them in, and only the groups that have rows.
            let groups: [(&'static str, fn(&TriageRow) -> bool); 5] = [
                ("Waiting on you", |r| matches!(r, TriageRow::Waiting { .. })),
                ("Claims that outlived their runs", |r| matches!(r, TriageRow::StaleClaim { .. })),
                ("Blocked, evidence first", |r| matches!(r, TriageRow::Blocked { .. })),
                ("Failed, worth another go", |r| matches!(r, TriageRow::Failed { .. })),
                ("Rescue reports to acknowledge", |r| matches!(r, TriageRow::Rescue { .. })),
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
                let mut card = card.child(self.item_actions(item_id, claim.as_deref(), cx));
                // Alert -> review -> steering in one press (Father, 21 Nov 2026:
                // "job fails -> user surface alerts the user -> user looks it
                // over and offers steering to help fix the issue"). Offered only
                // when a claim run exists: without it there is no conversation
                // to read, and a review button would promise evidence that is
                // not there.
                if let Some(claim_run) = claim.clone() {
                    let item = item_id.clone();
                    let title_for = title.clone();
                    card = card.child(
                        Button::new(
                            SharedString::from(format!("triage-teach-{item}")),
                            "Review with Andrew",
                        )
                        .style(ButtonStyle::Outlined)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.teach_failed_job(&item, &title_for, &claim_run, cx)
                        })),
                    );
                }
                for panel in self.triage_panels(item_id, theme) {
                    card = card.child(panel);
                }
                card.into_any_element()
            }
            TriageRow::StaleClaim { item_id, title, claim, run_status } => {
                let label = format!("claimed by run {claim}, which is {run_status}");
                let mut card = card(theme)
                    .p(px(14.))
                    .flex_col()
                    .gap(px(5.))
                    .child(row_head(theme, theme.warning, "stale claim", title, item_id))
                    .child(detail(theme, label))
                    .child(self.item_actions(item_id, Some(claim), cx));
                for panel in self.triage_panels(item_id, theme) {
                    card = card.child(panel);
                }
                card.into_any_element()
            }
            TriageRow::Blocked { item_id, title, project } => {
                let mut card = card(theme)
                    .p(px(14.))
                    .flex_col()
                    .gap(px(5.))
                    .child(row_head(theme, theme.warning, "blocked", title, item_id))
                    .child(detail(theme, "not failed and not running: something stopped it on purpose. Read its evidence before choosing requeue or stop."));
                if !project.is_empty() {
                    card = card.child(detail(theme, format!("project {project}")));
                }
                card = card.child(self.item_actions(item_id, None, cx));
                for panel in self.triage_panels(item_id, theme) {
                    card = card.child(panel);
                }
                card.into_any_element()
            }
            TriageRow::Rescue { wake_id, item_id, proposal } => {
                let mut card = card(theme)
                    .p(px(14.))
                    .flex_col()
                    .gap(px(5.))
                    .child(row_head(theme, theme.warning, "rescue report", "the rescue agent flagged a failure", wake_id));
                if !proposal.is_empty() {
                    card = card.child(detail(theme, proposal.clone()));
                }
                if !item_id.is_empty() {
                    card = card.child(detail(theme, format!("about {item_id}")));
                }
                let mut actions = div().flex().flex_wrap().items_center().gap(px(6.)).child(
                    Button::new(SharedString::from(format!("triage-ack-{wake_id}")), "Acknowledge")
                        .style(ButtonStyle::Primary)
                        .on_click(cx.listener({
                            let wake = wake_id.clone();
                            move |this, _, _, cx| this.triage_ack(wake.clone(), cx)
                        })),
                );
                if !item_id.is_empty() {
                    actions = actions.child(
                        Button::new(SharedString::from(format!("triage-open-{item_id}")), "Open")
                            .style(ButtonStyle::Ghost)
                            .on_click(cx.listener({
                                let item = item_id.clone();
                                move |this, _, window, cx| this.navigate(Route::ValleyItem(item.clone()), window, cx)
                            })),
                    );
                }
                card.child(actions).into_any_element()
            }
        }
    }

    /// The write actions an item row carries. Each write asks twice (arm, then
    /// send) through its OWN slot, so arming a kick can never authorise a
    /// requeue or a stop. Evidence and Investigate are reads and report the
    /// valley's own answer instead of a verdict of ours.
    fn item_actions(&self, item_id: &str, claim: Option<&str>, cx: &mut Context<Self>) -> Div {
        let mut actions = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .child(
                Button::new(SharedString::from(format!("triage-retry-{item_id}")), if self.armed_for(item_id) { "Send the kick" } else { "Retry" })
                    .style(if self.armed_for(item_id) { ButtonStyle::Primary } else { ButtonStyle::Outlined })
                    .on_click(cx.listener({
                        let item = item_id.to_string();
                        move |this, _, _, cx| this.triage_retry(item.clone(), cx)
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("triage-requeue-{item_id}")), if self.requeue_armed_for(item_id) { "Send the requeue" } else { "Requeue" })
                    .style(if self.requeue_armed_for(item_id) { ButtonStyle::Primary } else { ButtonStyle::Outlined })
                    .on_click(cx.listener({
                        let item = item_id.to_string();
                        move |this, _, _, cx| this.triage_requeue(item.clone(), cx)
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("triage-stop-item-{item_id}")), if self.stop_armed_for(item_id) { "Stop it now" } else { "Stop item" })
                    .style(if self.stop_armed_for(item_id) { ButtonStyle::Primary } else { ButtonStyle::Ghost })
                    .on_click(cx.listener({
                        let item = item_id.to_string();
                        move |this, _, _, cx| this.triage_stop_item(item.clone(), cx)
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("triage-evidence-{item_id}")), "Evidence")
                    .style(ButtonStyle::Ghost)
                    .on_click(cx.listener({
                        let item = item_id.to_string();
                        move |this, _, _, cx| this.triage_evidence(item.clone(), cx)
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("triage-investigate-{item_id}")), "Investigate")
                    .style(ButtonStyle::Ghost)
                    .on_click(cx.listener({
                        let item = item_id.to_string();
                        move |this, _, _, cx| this.triage_investigate(item.clone(), cx)
                    })),
            );
        if let Some(claim) = claim {
            actions = actions.child(
                Button::new(SharedString::from(format!("triage-why-{item_id}")), "Why")
                    .style(ButtonStyle::Ghost)
                    .on_click(cx.listener({
                        let run = claim.to_string();
                        move |this, _, _, cx| this.triage_why(run.clone(), cx)
                    })),
            );
            actions = actions.child(
                Button::new(SharedString::from(format!("triage-stop-{claim}")), "Stop the run")
                    .style(ButtonStyle::Ghost)
                    .on_click(cx.listener({
                        let run = claim.to_string();
                        move |this, _, _, cx| this.triage_stop(run.clone(), cx)
                    })),
            );
        }
        actions.child(
            Button::new(SharedString::from(format!("triage-open-{item_id}")), "Open")
                .style(ButtonStyle::Ghost)
                .on_click(cx.listener({
                    let item = item_id.to_string();
                    move |this, _, window, cx| this.navigate(Route::ValleyItem(item.clone()), window, cx)
                })),
        )
    }

    /// The evidence a row has asked for: the item's own log and, when asked,
    /// the judge's investigation. Both say what the valley said - this page
    /// never invents a cause.
    fn triage_panels(&self, item_id: &str, theme: &Theme) -> Vec<AnyElement> {
        let mut out = Vec::new();
        if let Some((load, _)) = self.triage_evidence.get(item_id) {
            out.push(evidence_panel(theme, load));
        }
        if let Some((load, _)) = self.triage_investigation.get(item_id) {
            out.push(investigation_panel(theme, load));
        }
        out
    }

    fn armed_for(&self, item_id: &str) -> bool {
        is_armed(&self.triage_armed, item_id)
    }

    fn requeue_armed_for(&self, item_id: &str) -> bool {
        is_armed(&self.triage_requeue_armed, item_id)
    }

    fn stop_armed_for(&self, item_id: &str) -> bool {
        is_armed(&self.triage_stop_armed, item_id)
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

    /// Evidence: the item's own detail read, shown in place. Reads only.
    fn triage_evidence(&mut self, item_id: String, cx: &mut Context<Self>) {
        self.triage_evidence.insert(item_id.clone(), (Load::Loading, Instant::now()));
        cx.notify();
        let valley = self.valley_client();
        let asked = item_id.clone();
        let job = self.engine.runtime().spawn(async move { valley.item(&item_id).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                let load = match result {
                    Ok(item) => Load::Ready(item),
                    Err(e) => Load::Failed(e),
                };
                this.triage_evidence.insert(asked, (load, Instant::now()));
                cx.notify();
            });
        });
        self.track(task);
    }

    /// Investigate: the valley reads its own telemetry and a judge answers
    /// healthy / stalled / dead. A read that writes a report on the valley side,
    /// so the page reports the verdict and nothing else.
    fn triage_investigate(&mut self, item_id: String, cx: &mut Context<Self>) {
        self.triage_investigation.insert(item_id.clone(), (Load::Loading, Instant::now()));
        cx.notify();
        let valley = self.valley_client();
        let asked = item_id.clone();
        let job = self.engine.runtime().spawn(async move { valley.investigate_item(&item_id).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(report) => {
                        let verdict = if report.verdict.choice.is_empty() {
                            "unjudged".to_string()
                        } else {
                            report.verdict.choice.clone()
                        };
                        this.triage_note(format!("The judge says {verdict} about {asked}."), verdict != "healthy", cx);
                        this.triage_investigation.insert(asked, (Load::Ready(report), Instant::now()));
                        // The investigation stamps a note on the board item.
                        this.valley_data = None;
                        this.load_valley(cx);
                    }
                    Err(e) => {
                        this.triage_note(format!("{asked}: {e}"), true, cx);
                        this.triage_investigation.insert(asked, (Load::Failed(e), Instant::now()));
                    }
                }
                cx.notify();
            });
        });
        self.track(task);
    }

    /// Stop the item itself: the board flags it and the engine halts at the
    /// next verse boundary. Terminal, unlike requeue, so it arms twice on its
    /// own slot and says so before the second press.
    fn triage_stop_item(&mut self, item_id: String, cx: &mut Context<Self>) {
        if self.stop_armed_for(&item_id) {
            self.triage_stop_armed = None;
            let valley = self.valley_client();
            let asked = item_id.clone();
            let reason = "stopped from the triage page (operator)";
            let job = self.engine.runtime().spawn(async move { valley.stop_item(&item_id, reason).await });
            let task = cx.spawn(async move |this, cx| {
                let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
                let _ = this.update(cx, |this, cx| {
                    match result {
                        Ok(out) if out.stop_requested => {
                            this.triage_note(format!("Stopped {asked} - the engine halts it at the next verse boundary."), false, cx);
                            this.valley_data = None;
                            this.load_valley(cx);
                            this.valley_runs = None;
                            this.maybe_load_runs(cx);
                        }
                        Ok(_) => this.triage_note(format!("The valley did not flag {asked} for a stop."), true, cx),
                        Err(e) => this.triage_note(format!("{asked}: {e}"), true, cx),
                    }
                    cx.notify();
                });
            });
            self.track(task);
        } else {
            self.triage_stop_armed = Some((item_id.clone(), Instant::now()));
            self.triage_note(
                format!("Stopping {item_id} is terminal - a stop is a full stop, not a requeue. Press Stop item again within {ARM_SECS}s to send it."),
                false,
                cx,
            );
        }
    }

    /// Requeue: PATCH status=pending with a note naming the operator. For a
    /// stuck item whose evidence says the work is still worth doing.
    fn triage_requeue(&mut self, item_id: String, cx: &mut Context<Self>) {
        if self.requeue_armed_for(&item_id) {
            self.triage_requeue_armed = None;
            let valley = self.valley_client();
            let asked = item_id.clone();
            let job = self
                .engine
                .runtime()
                .spawn(async move { valley.requeue_item(&item_id, "requeued from the triage page (operator)").await });
            let task = cx.spawn(async move |this, cx| {
                let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
                let _ = this.update(cx, |this, cx| {
                    match result {
                        Ok(item) => {
                            this.triage_note(format!("Requeued {asked} - it is {} now.", item.status), false, cx);
                            this.valley_data = None;
                            this.load_valley(cx);
                        }
                        Err(e) => this.triage_note(format!("{asked}: {e}"), true, cx),
                    }
                    cx.notify();
                });
            });
            self.track(task);
        } else {
            self.triage_requeue_armed = Some((item_id.clone(), Instant::now()));
            self.triage_note(
                format!("Requeue {item_id}? It goes back to pending and the queue may claim it. Press Requeue again within {ARM_SECS}s to send it."),
                false,
                cx,
            );
        }
    }

    /// Acknowledge one rescue report: the receipt is stamped on the valley, so
    /// the feed stops treating it as waiting on a person.
    fn triage_ack(&mut self, wake_id: String, cx: &mut Context<Self>) {
        let valley = self.valley_client();
        let asked = wake_id.clone();
        let job = self.engine.runtime().spawn(async move { valley.ack_rescue(&wake_id).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.triage_note(format!("Acknowledged {asked} - the valley stamped the receipt."), false, cx);
                        this.triage_receipts = None;
                        this.load_triage_receipts(cx);
                    }
                    Err(e) => this.triage_note(format!("{asked}: {e}"), true, cx),
                }
                cx.notify();
            });
        });
        self.track(task);
    }

    fn load_triage_drain(&mut self, cx: &mut Context<Self>) {
        self.triage_drain = Some((Load::Loading, Instant::now()));
        let valley = self.valley_client();
        let job = self.engine.runtime().spawn(async move { valley.drain_state().await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                let load = match result {
                    Ok(state) => Load::Ready(state),
                    Err(e) => Load::Failed(e),
                };
                this.triage_drain = Some((load, Instant::now()));
                cx.notify();
            });
        });
        self.track(task);
    }

    fn load_triage_receipts(&mut self, cx: &mut Context<Self>) {
        self.triage_receipts = Some((Load::Loading, Instant::now()));
        let valley = self.valley_client();
        let job = self.engine.runtime().spawn(async move { valley.rescue_receipts().await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                let load = match result {
                    Ok(receipts) => Load::Ready(receipts),
                    Err(e) => Load::Failed(e),
                };
                this.triage_receipts = Some((load, Instant::now()));
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

fn is_armed(slot: &Option<(String, Instant)>, item_id: &str) -> bool {
    slot.as_ref()
        .is_some_and(|(id, at)| id == item_id && at.elapsed().as_secs() < ARM_SECS)
}

/// Cut a long log line to something a card can hold, on a char boundary.
fn clipped(text: &str, max: usize) -> String {
    let t = text.trim();
    if t.chars().count() <= max {
        t.to_string()
    } else {
        let mut s: String = t.chars().take(max).collect();
        s.push('…');
        s
    }
}

/// The item's own evidence: why it failed (when it did), how many times it was
/// requeued, and the newest log lines. A requeue count over zero is the loop
/// signature the operator is looking for.
fn evidence_panel(theme: &Theme, load: &Load<ItemDetail>) -> AnyElement {
    let mut card = div()
        .mt(px(2.))
        .p(px(10.))
        .rounded(kit::corner(8.))
        .bg(theme.element)
        .flex_col()
        .gap(px(3.));
    match load {
        Load::Loading => card = card.child(detail(theme, "reading the item's own log…")),
        Load::Failed(e) => card = card.child(detail(theme, format!("Its log is out of reach: {e}"))),
        Load::Ready(item) => {
            if let Some(err) = item.failed_text() {
                card = card.child(detail(theme, format!("its last word: {}", clipped(err, 200))));
            }
            let requeues = item
                .progress_log
                .iter()
                .filter(|l| l.to_ascii_lowercase().contains("requeued"))
                .count();
            if requeues > 0 {
                card = card.child(detail(
                    theme,
                    format!("requeued {requeues} time(s) — that is a loop, not a slow job"),
                ));
            }
            if item.progress_log.is_empty() {
                card = card.child(detail(theme, "no log entries yet — nothing has worked it"));
            } else {
                for line in item.recent_log(3).into_iter().rev() {
                    card = card.child(detail(theme, clipped(line, 190)));
                }
            }
        }
    }
    card.into_any_element()
}

/// The judge's answer, when the operator pressed Investigate. Wording is the
/// valley's; the page only colours it.
fn investigation_panel(theme: &Theme, load: &Load<Investigation>) -> AnyElement {
    let mut card = div()
        .mt(px(2.))
        .p(px(10.))
        .rounded(kit::corner(8.))
        .bg(theme.element)
        .flex_col()
        .gap(px(3.));
    match load {
        Load::Loading => card = card.child(detail(
            theme,
            "asking the judge — it reads the run's telemetry and answers, so give it a moment…",
        )),
        Load::Failed(e) => card = card.child(detail(theme, format!("The investigation failed: {e}"))),
        Load::Ready(report) => {
            let choice = if report.verdict.choice.is_empty() { "unjudged" } else { report.verdict.choice.as_str() };
            let color = if choice == "healthy" { theme.text_muted } else { theme.warning };
            let mut line = format!("the judge says {choice}");
            if !report.item_id.is_empty() {
                line.push_str(&format!(" about {}", report.item_id));
            }
            if let Some(why) = report.verdict.reasons.first() {
                line.push_str(&format!(" — {}", clipped(why, 200)));
            }
            card = card.child(div().text_size(px(11.5)).text_color(color).child(line));
            // The context the verdict was reached on, so the operator can weigh
            // it: how long the item had waited, when it was looked at, and
            // whether the judge's reply actually parsed.
            let mut meta = Vec::new();
            if report.elapsed_hours > 0.0 {
                meta.push(format!("waited {:.1}h", report.elapsed_hours));
            }
            if !report.verdict.parsed {
                meta.push("the judge's reply did not parse, so read the reasons as raw".to_string());
            }
            if !report.investigated_at.is_empty() {
                meta.push(format!("looked at {}", report.investigated_at));
            }
            if !report.report_file.is_empty() {
                meta.push(format!("report {}", report.report_file));
            }
            if !meta.is_empty() {
                card = card.child(detail(theme, meta.join(" · ")));
            }
        }
    }
    card.into_any_element()
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

    #[test]
    fn a_blocked_item_is_its_own_row_not_a_failure() {
        // WHY (triage console, Father 21 Nov 2026): "Retry" is the wrong first
        // suggestion for a blocked item - the operator needs the evidence.
        let rows = triage_rows(&[item("work_b", "blocked", None)], &[]);
        assert_eq!(rows.len(), 1);
        assert!(matches!(&rows[0], TriageRow::Blocked { item_id, .. } if item_id == "work_b"));
    }

    #[test]
    fn only_unacknowledged_rescue_reports_need_a_person() {
        use crate::valley::{RescueReceipt, RescueReceipts};
        let feed = RescueReceipts {
            receipts: vec![
                RescueReceipt { wake_id: "w1".into(), item_id: "work_1".into(), proposal: "stalled".into(), ..Default::default() },
                RescueReceipt { wake_id: "w2".into(), acked: Some(true), proposal: "seen".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        let rows = rescue_rows(&feed);
        assert_eq!(rows.len(), 1);
        assert!(matches!(&rows[0], TriageRow::Rescue { wake_id, .. } if wake_id == "w1"));
    }
}
