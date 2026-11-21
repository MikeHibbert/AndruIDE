//! Teach: point at a failure, say how it should have gone, and let Andrew
//! work out why he didn't match before he offers a belief to change.
//!
//! WHY (Father, 21 Nov 2026: "the teach lane should be where its possible to
//! provide feedback on an agents system prompt/beliefs and we should be able to
//! point at failures and offer suggestions of how we think it should have been
//! handled then andrew can introspect to see why he didn't match and offer to
//! update his beliefs to ensure that outcome in future. like a review/debug of
//! kumbayas and systems behaviour"): this page is not a notes box. It drives
//! Andrew's own `examine_belief` ceremony - behaviour in, belief + origin probe
//! + revised belief out - and stops at his apply / note_only / refine
//! checkpoint, so the belief change is his proposal and the operator's decision.
//!
//! Three things it deliberately does not do. It never writes a lesson itself:
//! the ceremony's own completion path mints it, gated on `apply` (see
//! `_on_kumbaya_completed`), because a page writing beliefs directly would
//! bypass every guard in `LessonsStore`. It never claims the evidence it does
//! not have: a run list row carries the verse name, and the verse's words come
//! from the run's own detail read. And it never starts a run on one press: the
//! Teach button arms first, because a ceremony run is LLM spend.
//!
//! The lessons card is what Andrew actually carries into his next claim - the
//! store `format_context()` injects into his prompt. There is no per-lesson
//! delete over HTTP, so the id is shown and the rollback gap is stated rather
//! than papered over.

use std::time::{Duration, Instant};

use gpui::{AnyElement, Context, Div, FontWeight, Hsla, SharedString, div, prelude::*, px};

use super::github::Load;
use super::input::TextInput;
use super::kit::{self, Button, ButtonStyle, card, section_title, spinner};
use super::overview::page;
use super::root::Root;
use super::theme::{ActiveTheme, Theme, mono_font};
use crate::valley::{RunSummary, RunTalk};

/// The page's sidebar tile: a slate-teal, distinct from the board's ember and
/// the triage console's coral (belief work is its own lane).
pub const TILE: (u32, u32) = (0x9be7c4, 0x2f9e7e);

/// How long an armed Teach press waits for its second press.
const ARM_SECS: u64 = 60;
/// How long the page's last note stays on screen.
const NOTE_SECS: u64 = 45;
/// A failure's evidence is a detail read; refresh it on visit, not per frame.
const EVIDENCE_STALE_SECS: u64 = 60;
/// How often the beliefs card re-reads while the page is open.
const LESSONS_STALE_SECS: u64 = 30;

/// One failure worth teaching, carrying the little the run list knows: the
/// verse that is flagged and its kind. The verse's words live in the detail
/// read, never here.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Failure {
    pub run_id: String,
    pub kumbaya: String,
    pub verse: String,
    pub kind: String,
    pub created_at: String,
    /// The run itself failed, not just one verse (the row says which).
    pub run_failed: bool,
}

/// The runs a person can point at: a failed run, or a run with a failed verse.
/// Newest first, because the runs read already is.
pub(super) fn failure_rows(runs: &[RunSummary]) -> Vec<Failure> {
    let mut out = Vec::new();
    for run in runs {
        let failed_verse = run.verses.iter().find(|v| v.status == "failed");
        let run_failed = run.status == "failed";
        if !run_failed && failed_verse.is_none() {
            continue;
        }
        out.push(Failure {
            run_id: run.id.clone(),
            kumbaya: run.kumbaya.clone(),
            verse: failed_verse.map(|v| v.verse.clone()).unwrap_or_default(),
            kind: failed_verse.map(|v| v.kind.clone()).unwrap_or_default(),
            created_at: run.created_at.clone(),
            run_failed,
        });
    }
    out
}

/// The behaviour input the ceremony reads: the failure as it happened, the
/// verse's own last word when the evidence has it, then the operator's account
/// of what should have happened instead.
///
/// WHY the operator's note rides in `behavior` and no new input was invented:
/// the live Songbook's `examine_belief` declares exactly behaviour + belief, and
/// a seeded-input change does not reach an existing saved ceremony. Naming the
/// expectation inside the behaviour is honest - it is the behaviour the
/// operator wanted, stated as such - and Andrew's origin probe still has to
/// find the belief that produced the actual one.
pub(super) fn compose_behaviour(failure: &Failure, evidence: Option<&RunTalk>, expected: &str) -> String {
    let said = evidence
        .and_then(|talk| talk.verses.iter().find(|v| v.verse == failure.verse))
        .and_then(|v| v.said())
        .map(|s| clipped(s, 400));
    let mut out = if failure.verse.trim().is_empty() {
        format!(
            "The ceremony '{}' failed as a run (run {}); no single verse is flagged. ",
            failure.kumbaya, failure.run_id
        )
    } else {
        format!(
            "The ceremony '{}' failed at verse '{}' ({}) in run {}. ",
            failure.kumbaya, failure.verse, failure.kind, failure.run_id
        )
    };
    if let Some(said) = said {
        out.push_str(&format!("The verse's own last word was: \"{said}\". "));
    }
    out.push_str("This is how the operator believes it should have gone instead:\n");
    out.push_str(expected.trim());
    out
}

impl Root {
    pub(super) fn render_teach(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.maybe_load_runs(cx);
        self.maybe_load_teach_lessons(cx);
        if let Some(pick) = self.teach_pick.clone() {
            self.maybe_load_teach_evidence(&pick.run_id, cx);
        }
        self.teach_expected
            .get_or_insert_with(|| cx.new(|cx| TextInput::new(cx, "What Andrew should have done here — in your words").mono()));
        self.teach_belief
            .get_or_insert_with(|| cx.new(|cx| TextInput::new(cx, "Optional: the belief you think drove it").mono()));

        let theme = cx.theme();
        let runs: Vec<RunSummary> = match &self.valley_runs {
            Some(Load::Ready(runs)) => runs.clone(),
            _ => Vec::new(),
        };
        let failures = failure_rows(&runs);
        let runs_problem: Option<String> = match &self.valley_runs {
            Some(Load::Failed(e)) => Some(e.clone()),
            Some(Load::Loading) | Some(Load::Ready(_)) => None,
            None => Some("the runs have not been read yet".into()),
        };

        let header = card(&theme)
            .p(px(16.))
            .flex_col()
            .gap(px(7.))
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::MEDIUM)
                    .child("Steer the belief, not just the queue"),
            )
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(theme.text_muted)
                    .child("Pick a failure, say how it should have gone. Andrew examines the belief that produced the behaviour, tells you where it came from, and proposes a revision — the belief changes only if you press Apply."),
            )
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(theme.text_placeholder)
                    .child("Nothing is written on this page. The ceremony writes its own lesson, and only on Apply; Note only leaves his store untouched."),
            );

        let mut body = page("teach-page", &self.page_scroll).child(header);
        if let Some((text, error, at)) = &self.teach_note {
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

        let mut failures_card = div().flex().flex_col().gap(px(8.));
        failures_card = failures_card.child(section_title(&theme, "A failure to teach", Some(failures.len())));
        if failures.is_empty() {
            let empty = if runs_problem.is_some() {
                kit::empty_state(&theme, "book-open", "The campfire's runs are out of reach, so there is nothing to point at yet.")
            } else {
                kit::empty_state(&theme, "book-open", "No failed runs or failed verses in the last fifty. Pick something older from the Run page when one appears.")
            };
            failures_card = failures_card.child(card(&theme).child(empty));
        }
        for failure in &failures {
            let picked = self.teach_pick.as_ref().is_some_and(|p| p.run_id == failure.run_id);
            let badge = if failure.run_failed { "failed run" } else { "failed verse" };
            let label = if failure.kumbaya.trim().is_empty() { "a run".to_string() } else { failure.kumbaya.clone() };
            let mut row = card(&theme)
                .p(px(14.))
                .flex_col()
                .gap(px(5.))
                .child(row_head(&theme, theme.error, badge, &label, &failure.run_id));
            let mut detail_line = Vec::new();
            if !failure.verse.trim().is_empty() {
                detail_line.push(format!("verse {}", failure.verse));
            }
            if !failure.kind.trim().is_empty() {
                detail_line.push(failure.kind.clone());
            }
            if !failure.created_at.trim().is_empty() {
                detail_line.push(failure.created_at.clone());
            }
            if !detail_line.is_empty() {
                row = row.child(detail(&theme, detail_line.join(" · ")));
            }
            let for_click = failure.run_id.clone();
            row = row.child(
                Button::new(
                    SharedString::from(format!("teach-pick-{}", failure.run_id)),
                    if picked { "Reading its evidence" } else { "Teach this failure" },
                )
                .style(if picked { ButtonStyle::Primary } else { ButtonStyle::Outlined })
                .on_click(cx.listener(move |this, _, _, cx| this.teach_pick_failure(for_click.clone(), cx))),
            );
            failures_card = failures_card.child(row);
        }
        body = body.child(failures_card);
        if runs_problem.is_some() && !failures.is_empty() {
            body = body.child(detail(&theme, "The run list is stale; the rows above are the last read."));
        }

        if let Some(pick) = self.teach_pick.clone() {
            let workbench = self.render_teach_workbench(&pick, &theme, cx);
            body = body.child(workbench);
        }

        body.into_any_element()
    }

    /// Evidence and the three things a person does here: read what happened,
    /// say what should have happened, decide the belief.
    fn render_teach_workbench(&self, pick: &Failure, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let mut out = div().flex().flex_col().gap(px(16.));

        // 1. The evidence: the run's own verses. A failed verse's words are the
        // detail read's, not the list's.
        let mut evidence_card = card(theme).p(px(14.)).flex_col().gap(px(6.)).child(section_title(theme, "What happened", None));
        match self.teach_evidence.as_ref() {
            Some((Load::Ready(talk), _)) if talk_matches(talk, pick) => {
                let mut trail = div().flex().flex_col().gap(px(4.));
                for verse in &talk.verses {
                    let mut line = div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_family(mono_font())
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(verse.verse.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(10.5))
                                        .text_color(status_colour(theme, &verse.status))
                                        .child(format!("{} · {}", verse.kind, verse.status)),
                                ),
                        );
                    if let Some(said) = verse.said() {
                        line = line.child(
                            div()
                                .text_size(px(11.5))
                                .text_color(theme.text_muted)
                                .whitespace_normal()
                                .child(clipped(said, 240)),
                        );
                    }
                    trail = trail.child(line);
                }
                evidence_card = evidence_card.child(trail);
            }
            Some((Load::Loading, _)) => {
                evidence_card = evidence_card.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(spinner(SharedString::from("teach-evidence-spin"), 12., theme.text_muted))
                        .child(detail(theme, "reading the run's verses…")),
                );
            }
            Some((Load::Failed(e), _)) => {
                evidence_card = evidence_card.child(detail(theme, format!("The run's verses are out of reach: {e}")));
            }
            _ => {
                evidence_card = evidence_card.child(detail(theme, "Reading the run's verses…"));
            }
        }
        out = out.child(evidence_card);

        // 2. The operator's account, and the spend verb behind two presses.
        let expected = self.teach_expected.clone();
        let belief = self.teach_belief.clone();
        let armed = self.teach_armed_for(&pick.run_id);
        let mut form = card(theme)
            .p(px(14.))
            .flex_col()
            .gap(px(8.))
            .child(section_title(theme, "How should it have gone?", None))
            .child(detail(theme, "Andrew reads this as the outcome to aim for, then examines why he produced a different one. Be concrete — the action, the check, or the order of work you wanted."));
        if let Some(input) = expected {
            form = form.child(
                div()
                    .w_full()
                    .rounded(kit::corner(8.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.element)
                    .p(px(8.))
                    .child(input),
            );
        }
        form = form.child(detail(theme, "Optional: if you already know the belief that drove it, name it and Andrew will examine that one instead of deriving it."));
        if let Some(input) = belief {
            form = form.child(
                div()
                    .w_full()
                    .rounded(kit::corner(8.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.element)
                    .p(px(8.))
                    .child(input),
            );
        }
        form = form.child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(6.))
                .child(
                    Button::new(
                        SharedString::from(format!("teach-start-{}", pick.run_id)),
                        if armed { "Send it — the run starts now" } else { "Teach this failure" },
                    )
                    .style(if armed { ButtonStyle::Primary } else { ButtonStyle::Outlined })
                    .on_click(cx.listener(|this, _, _, cx| this.teach_start(cx))),
                )
                .when(armed, |d| d.child(detail(theme, format!("This starts an examine_belief run (LLM spend). Press again within {ARM_SECS}s."))))
                .child(
                    Button::new(SharedString::from("teach-clear"), "Clear")
                        .style(ButtonStyle::Ghost)
                        .on_click(cx.listener(|this, _, _, cx| this.teach_clear(cx))),
                ),
        );
        out = out.child(form);

        // 3. The ceremony's own conversation: verses as they land, and the
        // checkpoint where a belief may change.
        if let Some((load, _)) = &self.teach_run {
            let mut run_card = card(theme).p(px(14.)).flex_col().gap(px(6.)).child(section_title(theme, "Andrew examining it", None));
            match load {
                Load::Loading => {
                    run_card = run_card.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(spinner(SharedString::from("teach-run-spin"), 12., theme.text_muted))
                            .child(detail(theme, "the ceremony is singing; its verses will appear here…")),
                    );
                }
                Load::Failed(e) => {
                    run_card = run_card.child(detail(theme, format!("The run is out of reach: {e}")));
                }
                Load::Ready(talk) => {
                    run_card = run_card.child(
                        div()
                            .text_size(px(11.5))
                            .font_family(mono_font())
                            .text_color(match talk.status.as_str() {
                                "running" | "waiting_checkpoint" => theme.accent,
                                "completed" => theme.success,
                                "failed" => theme.error,
                                _ => theme.text_muted,
                            })
                            .child(format!("{} · {} verses · {}", talk.status, talk.verses.len(), talk.kumbaya)),
                    );
                    let mut stack = div().flex().flex_col().gap(px(4.));
                    for verse in &talk.verses {
                        if let Some(said) = verse.said() {
                            stack = stack.child(
                                div()
                                    .flex_col()
                                    .gap(px(1.))
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .font_family(mono_font())
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(verse.verse.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(theme.text_muted)
                                            .whitespace_normal()
                                            .child(clipped(said, 500)),
                                    ),
                            );
                        }
                    }
                    run_card = run_card.child(stack);
                    if talk.status == "waiting_checkpoint" {
                        if let Some(ask) = talk.waiting_ask() {
                            run_card = run_card
                                .child(
                                    div()
                                        .mt(px(4.))
                                        .p(px(10.))
                                        .rounded(kit::corner(8.))
                                        .bg(theme.element)
                                        .text_size(px(12.))
                                        .text_color(theme.text)
                                        .whitespace_normal()
                                        .child(ask.question.clone()),
                                )
                                .child(detail(theme, "Apply records the revised belief as a lesson he carries from now on. Note only records nothing. Refine sends it back for another pass."))
                                .child(
                                    div()
                                        .flex()
                                        .flex_wrap()
                                        .items_center()
                                        .gap(px(6.))
                                        .child(
                                            Button::new(SharedString::from("teach-apply"), "Apply the belief")
                                                .style(ButtonStyle::Primary)
                                                .on_click(cx.listener(|this, _, _, cx| this.teach_resume("apply", cx))),
                                        )
                                        .child(
                                            Button::new(SharedString::from("teach-note"), "Note only")
                                                .style(ButtonStyle::Outlined)
                                                .on_click(cx.listener(|this, _, _, cx| this.teach_resume("note_only", cx))),
                                        )
                                        .child(
                                            Button::new(SharedString::from("teach-refine"), "Refine")
                                                .style(ButtonStyle::Ghost)
                                                .on_click(cx.listener(|this, _, _, cx| this.teach_resume("refine", cx))),
                                        ),
                                );
                        }
                    }
                }
            }
            out = out.child(run_card);
        }

        out.child(self.render_teach_beliefs(theme, cx)).into_any_element()
    }

    /// What Andrew carries now: his lessons store, newest first. The id is
    /// shown because that is what the valley's retirement machinery takes.
    fn render_teach_beliefs(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let mut card_el = card(theme)
            .p(px(14.))
            .flex_col()
            .gap(px(6.))
            .child(section_title(theme, "What he carries", None))
            .child(detail(theme, "The lessons store his prompt injects. A belief you apply lands here; his own selection then ranks it against everything else he has learned."));
        match &self.teach_lessons {
            Some((Load::Ready(lessons), _)) if !lessons.is_empty() => {
                let mut stack = div().flex().flex_col().gap(px(7.));
                for lesson in lessons.iter().take(12) {
                    stack = stack.child(
                        div()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme.text)
                                    .whitespace_normal()
                                    .child(clipped(&lesson.text, 240)),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .font_family(mono_font())
                                    .text_color(theme.text_placeholder)
                                    .child(lesson.provenance()),
                            ),
                    );
                }
                card_el = card_el.child(stack);
            }
            Some((Load::Ready(_), _)) => {
                card_el = card_el.child(detail(theme, "The store is empty — nothing has been applied yet."));
            }
            Some((Load::Loading, _)) => {
                card_el = card_el.child(detail(theme, "reading his beliefs…"));
            }
            Some((Load::Failed(e), _)) => {
                card_el = card_el.child(detail(theme, format!("His beliefs are out of reach: {e}")));
            }
            None => {
                card_el = card_el.child(detail(theme, "reading his beliefs…"));
            }
        }
        card_el
            .child(detail(theme, "No per-lesson delete exists over HTTP; retiring a lesson still uses the valley's own machinery, with the id above as the handle."))
            .child(
                Button::new(SharedString::from("teach-lessons-refresh"), "Read again")
                    .style(ButtonStyle::Ghost)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.teach_lessons = None;
                        this.load_teach_lessons(cx);
                    })),
            )
            .into_any_element()
    }

    fn teach_armed_for(&self, run_id: &str) -> bool {
        self.teach_armed
            .as_ref()
            .is_some_and(|(id, at)| id == run_id && at.elapsed().as_secs() < ARM_SECS)
    }

    fn teach_pick_failure(&mut self, run_id: String, cx: &mut Context<Self>) {
        let pick = self
            .valley_runs
            .as_ref()
            .and_then(|load| match load {
                Load::Ready(runs) => failure_rows(runs).into_iter().find(|f| f.run_id == run_id),
                _ => None,
            });
        match pick {
            Some(pick) => {
                self.teach_pick = Some(pick);
                self.teach_armed = None;
                self.load_teach_evidence(&run_id, cx);
                cx.notify();
            }
            None => self.teach_note("That failure is no longer in the runs read; refresh and pick again.".into(), true, cx),
        }
    }

    fn teach_clear(&mut self, cx: &mut Context<Self>) {
        self.teach_pick = None;
        self.teach_armed = None;
        if let Some(input) = &self.teach_expected {
            input.update(cx, |input, cx| input.set_text("", cx));
        }
        if let Some(input) = &self.teach_belief {
            input.update(cx, |input, cx| input.set_text("", cx));
        }
        cx.notify();
    }

    fn teach_note(&mut self, text: String, error: bool, cx: &mut Context<Self>) {
        self.teach_note = Some((text, error, Instant::now()));
        cx.notify();
    }

    fn teach_expected_text(&self, cx: &Context<Self>) -> String {
        self.teach_expected
            .as_ref()
            .map(|i| i.read(cx).text().trim().to_string())
            .unwrap_or_default()
    }

    fn teach_belief_text(&self, cx: &Context<Self>) -> String {
        self.teach_belief
            .as_ref()
            .map(|i| i.read(cx).text().trim().to_string())
            .unwrap_or_default()
    }

    /// Starts the ceremony. First press only arms: a run is LLM spend, and the
    /// same discipline as every other spend verb in this app.
    fn teach_start(&mut self, cx: &mut Context<Self>) {
        let Some(pick) = self.teach_pick.clone() else { return };
        let expected = self.teach_expected_text(cx);
        if expected.is_empty() {
            self.teach_note("Write how it should have gone before teaching — that is the outcome Andrew aims at.".into(), true, cx);
            return;
        }
        if !self.teach_armed_for(&pick.run_id) {
            self.teach_armed = Some((pick.run_id.clone(), Instant::now()));
            self.teach_note(
                format!("This starts an examine_belief run on Andrew (LLM spend). Press Teach this failure again within {ARM_SECS}s to send it."),
                false,
                cx,
            );
            return;
        }
        self.teach_armed = None;
        let belief = self.teach_belief_text(cx);
        let evidence = self.teach_evidence_talk(&pick.run_id);
        let behaviour = compose_behaviour(&pick, evidence, &expected);
        let inputs = serde_json::json!({ "behavior": behaviour, "belief": belief });
        let valley = self.valley_client();
        let task = cx.spawn(async move |this, cx| {
            let result = valley.run_kumbaya("examine_belief", inputs).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(started) if !started.id.is_empty() => {
                        this.teach_run_id = Some(started.id.clone());
                        this.teach_run = Some((Load::Loading, Instant::now()));
                        let ceremony = if started.kumbaya.trim().is_empty() { "a ceremony".to_string() } else { started.kumbaya.clone() };
                        this.teach_note(
                            format!("Andrew is examining it — {ceremony} run {} ({}).", started.id, started.status),
                            false,
                            cx,
                        );
                        this.teach_watch(started.id, cx);
                    }
                    Ok(_) => this.teach_note("The valley returned no run for it.".into(), true, cx),
                    Err(e) => this.teach_note(e, true, cx),
                }
                cx.notify();
            });
        });
        self.track(task);
    }

    /// Answers the ceremony's checkpoint and keeps watching the tail.
    fn teach_resume(&mut self, answer: &str, cx: &mut Context<Self>) {
        let Some(run_id) = self.teach_run_id.clone() else { return };
        self.teach_run = Some((Load::Loading, Instant::now()));
        let valley = self.valley_client();
        let said = answer.to_string();
        let task = cx.spawn(async move |this, cx| {
            let result = valley.resume(&run_id, &said).await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        let line = match said.as_str() {
                            "apply" => "Applied — the ceremony is writing the lesson now.",
                            "note_only" => "Noted only; his store will not change.",
                            "refine" => "Sent back for another pass.",
                            _ => "Answered.",
                        };
                        this.teach_note(line.to_string(), false, cx);
                        this.teach_watch(run_id, cx);
                    }
                    Err(e) => {
                        this.teach_note(e, true, cx);
                        this.load_teach_lessons(cx);
                    }
                }
                cx.notify();
            });
        });
        self.track(task);
    }

    /// Watches one ceremony run to its next stop. Polls the run's own detail
    /// read, stops at a checkpoint (a person decides) or a terminal status,
    /// and re-reads the belief store when it finishes.
    fn teach_watch(&mut self, run_id: String, cx: &mut Context<Self>) {
        let valley = self.valley_client();
        let watched = run_id.clone();
        let task = cx.spawn(async move |this, cx| {
            loop {
                let result = valley.talk(&watched).await;
                let done = match &result {
                    Ok(talk) => talk.status != "running",
                    Err(_) => true,
                };
                let keep = this
                    .update(cx, |this, cx| {
                        if this.teach_run_id.as_deref() != Some(watched.as_str()) {
                            return false;
                        }
                        this.teach_run = Some((
                            match result {
                                Ok(talk) => Load::Ready(talk),
                                Err(e) => Load::Failed(e),
                            },
                            Instant::now(),
                        ));
                        if done {
                            this.load_teach_lessons(cx);
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if done || !keep {
                    break;
                }
                cx.background_executor().timer(Duration::from_secs(3)).await;
            }
        });
        self.track(task);
    }

    fn teach_evidence_talk(&self, run_id: &str) -> Option<&RunTalk> {
        self.teach_evidence.as_ref().and_then(|(load, _)| match load {
            Load::Ready(talk) if talk_matches(talk, &Failure { run_id: run_id.to_string(), ..Default::default() }) => Some(talk),
            _ => None,
        })
    }

    fn maybe_load_teach_evidence(&mut self, run_id: &str, cx: &mut Context<Self>) {
        let loading = matches!(self.teach_evidence, Some((Load::Loading, _)));
        let fresh = self
            .teach_evidence
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed().as_secs() < EVIDENCE_STALE_SECS)
            && self
                .teach_evidence
                .as_ref()
                .is_some_and(|(load, _)| matches!(load, Load::Ready(t) if talk_matches(t, &Failure { run_id: run_id.to_string(), ..Default::default() })));
        if !loading && !fresh {
            self.load_teach_evidence(run_id, cx);
        }
    }

    fn load_teach_evidence(&mut self, run_id: &str, cx: &mut Context<Self>) {
        self.teach_evidence = Some((Load::Loading, Instant::now()));
        let valley = self.valley_client();
        let asked = run_id.to_string();
        let job = self.engine.runtime().spawn(async move { valley.talk(&asked).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                this.teach_evidence = Some((
                    match result {
                        Ok(talk) => Load::Ready(talk),
                        Err(e) => Load::Failed(e),
                    },
                    Instant::now(),
                ));
                cx.notify();
            });
        });
        self.track(task);
    }

    fn maybe_load_teach_lessons(&mut self, cx: &mut Context<Self>) {
        let loading = matches!(self.teach_lessons, Some((Load::Loading, _)));
        let stale = self
            .teach_lessons
            .as_ref()
            .map_or(true, |(_, at)| at.elapsed().as_secs() > LESSONS_STALE_SECS);
        if !loading && stale {
            self.load_teach_lessons(cx);
        }
    }

    fn load_teach_lessons(&mut self, cx: &mut Context<Self>) {
        self.teach_lessons = Some((Load::Loading, Instant::now()));
        let valley = self.valley_client();
        let job = self.engine.runtime().spawn(async move { valley.lessons(30).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                this.teach_lessons = Some((
                    match result {
                        Ok(lessons) => Load::Ready(lessons),
                        Err(e) => Load::Failed(e),
                    },
                    Instant::now(),
                ));
                cx.notify();
            });
        });
        self.track(task);
    }
}

impl Default for Failure {
    fn default() -> Self {
        Failure {
            run_id: String::new(),
            kumbaya: String::new(),
            verse: String::new(),
            kind: String::new(),
            created_at: String::new(),
            run_failed: false,
        }
    }
}

fn talk_matches(talk: &RunTalk, failure: &Failure) -> bool {
    talk.id == failure.run_id
}

fn status_colour(theme: &Theme, status: &str) -> Hsla {
    match status {
        "failed" => theme.error,
        "completed" => theme.success,
        "running" | "waiting_checkpoint" => theme.accent,
        _ => theme.text_placeholder,
    }
}

fn row_head(theme: &Theme, colour: Hsla, badge: &str, title: &str, id: &str) -> Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(7.))
        .child(
            div()
                .flex_none()
                .rounded_full()
                .px(px(7.))
                .py(px(1.))
                .bg(colour.opacity(0.15))
                .text_size(px(10.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(colour)
                .child(badge.to_string()),
        )
        .child(div().min_w(px(0.)).flex_1().truncate().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(theme.text).child(title.to_string()))
        .child(div().flex_none().text_size(px(10.5)).font_family(mono_font()).text_color(theme.text_placeholder).child(id.to_string()))
}

fn detail(theme: &Theme, text: impl Into<SharedString>) -> Div {
    div().text_size(px(11.5)).text_color(theme.text_muted).whitespace_normal().child(text.into())
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::valley::{RunSummary, VerseLite};

    fn verse(name: &str, status: &str) -> VerseLite {
        VerseLite {
            verse: name.into(),
            kind: "llm".into(),
            status: status.into(),
            tool_name: String::new(),
            checkpoint: None,
        }
    }

    fn run(id: &str, status: &str, verses: Vec<VerseLite>) -> RunSummary {
        RunSummary {
            id: id.into(),
            kumbaya: "work_board_worker".into(),
            status: status.into(),
            session_id: None,
            created_at: "2026-11-21T09:00:00Z".into(),
            finished_at: None,
            verses,
        }
    }

    #[test]
    fn only_failures_and_failed_verses_are_offered() {
        let rows = failure_rows(&[
            run("ok", "completed", vec![verse("implement", "completed")]),
            run("bad-tree", "failed", vec![verse("implement", "completed")]),
            run("bad-leaf", "completed", vec![verse("verify", "failed")]),
            run("busy", "running", vec![verse("plan", "running")]),
        ]);
        let ids: Vec<&str> = rows.iter().map(|r| r.run_id.as_str()).collect();
        assert_eq!(ids, vec!["bad-tree", "bad-leaf"]);
        assert!(rows[0].run_failed && rows[0].verse.is_empty());
        assert_eq!(rows[1].verse, "verify");
    }

    #[test]
    fn the_operator_words_reach_the_ceremony_verbatim() {
        let failure = Failure {
            run_id: "run-1".into(),
            kumbaya: "work_board_worker".into(),
            verse: "verify".into(),
            kind: "llm".into(),
            created_at: String::new(),
            run_failed: false,
        };
        let behaviour = compose_behaviour(&failure, None, "run the focused test before claiming done");
        assert!(behaviour.contains("run the focused test before claiming done"));
        assert!(behaviour.contains("verse 'verify'"));
        assert!(behaviour.contains("should have gone instead"));
    }
}
