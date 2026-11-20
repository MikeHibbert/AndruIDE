//! The command bar: one line, plain verbs, against Andrew's own answers.
//!
//! WHY (Father, 20 Nov 2026 — AndruIDE mission control, step 4): the strip
//! shows what the valley is doing; the bar is how the operator *asks* and
//! *acts* without hunting through pages. Two rules from the day it came from:
//! a write must ask first and act second (the preflight exists for exactly
//! that — `POST /work/{id}/run` used to mutate the board before deciding), and
//! a verb must report what the server said rather than a blanket success.
//!
//! No keybinding on purpose: Father's Ctrl+Space is speech to text, so the bar
//! is a field on the Valley page that takes the keyboard when clicked.

use std::time::Instant;

use gpui::{AnyElement, Context, FontWeight, div, prelude::*, px};

use super::github::Load;
use super::kit::{Button, card};
use super::root::Root;
use super::theme::{mono_font, Theme};
use crate::valley::Valley;

/// How long an armed write waits for its second word. Long enough to read the
/// preflight's answer, short enough that a forgotten arm cannot fire later.
const ARM_SECS: u64 = 60;
/// How long a reply stays under the bar before it fades from view.
const REPLY_SECS: u64 = 45;

const HINT: &str = "kick <item> · stop <run> · answer <run> <text> · why <run> · item <item> · cost · status · help";

/// What the bar last said, and whether it was bad news.
pub(super) struct CommandReply {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

/// One typed line, understood. Pure so it can be tested without a window.
#[derive(Debug, PartialEq)]
pub(super) enum Verb {
    Help,
    Status,
    Cost,
    Why(String),
    Item(String),
    Kick(String),
    Stop(String),
    Answer(String, String),
    Unknown(String),
}

pub(super) fn parse(line: &str) -> Verb {
    let line = line.trim();
    let (verb, rest) = match line.split_once(char::is_whitespace) {
        Some((verb, rest)) => (verb, rest.trim()),
        None => (line, ""),
    };
    match verb.to_ascii_lowercase().as_str() {
        "help" | "?" => Verb::Help,
        "status" | "board" => Verb::Status,
        "cost" | "burn" => Verb::Cost,
        "why" | "health" => Verb::Why(rest.to_string()),
        "item" | "preflight" => Verb::Item(rest.to_string()),
        "kick" => Verb::Kick(rest.to_string()),
        "stop" => Verb::Stop(rest.to_string()),
        "answer" | "reply" => match rest.split_once(char::is_whitespace) {
            Some((run, text)) => Verb::Answer(run.trim().to_string(), text.trim().to_string()),
            None => Verb::Answer(rest.to_string(), String::new()),
        },
        _ => Verb::Unknown(verb.to_string()),
    }
}

impl Root {
    /// The valley client the bar uses: the network valley in view, else the
    /// configured one — the same rule the page's own reads follow.
    fn valley_client(&self) -> Valley {
        let mut cfg = self.engine.settings().valley.clone();
        if let Some(base) = self.valley_view.clone() {
            cfg.base_url = base;
        }
        Valley::from_parts(cfg)
    }

    /// Runs whatever is in the bar (the Run button and Return both land here).
    pub(super) fn submit_command(&mut self, cx: &mut Context<Self>) {
        let line = self.command_input.read(cx).text().trim().to_string();
        self.command_input.update(cx, |input, cx| input.set_text("", cx));
        if line.is_empty() {
            return;
        }
        self.run_command_line(line, cx);
    }

    pub(super) fn run_command_line(&mut self, line: String, cx: &mut Context<Self>) {
        match parse(&line) {
            Verb::Help => self.say(help_text(), false, cx),
            Verb::Unknown(verb) => self.say(format!("I don't know \"{verb}\". Say help."), true, cx),
            Verb::Status => self.command_status(cx),
            Verb::Cost => self.command_cost(cx),
            Verb::Why(run) if run.is_empty() => self.say("why needs a run id (or its first 8 characters).", true, cx),
            Verb::Why(run) => self.command_why(run, cx),
            Verb::Item(item) if item.is_empty() => self.say("item needs an item id (work_…).", true, cx),
            Verb::Item(item) => self.command_item(item, cx),
            Verb::Kick(item) if item.is_empty() => self.say("kick needs an item id (work_…).", true, cx),
            Verb::Kick(item) => self.command_kick(item, cx),
            Verb::Stop(run) if run.is_empty() => self.say("stop needs a run id (or its first 8 characters).", true, cx),
            Verb::Stop(run) => self.command_stop(run, cx),
            Verb::Answer(run, _) if run.is_empty() => self.say("answer needs a run id, then the answer.", true, cx),
            Verb::Answer(run, text) if text.is_empty() => {
                self.say(format!("answer {run} needs words after the run id."), true, cx)
            }
            Verb::Answer(run, text) => self.command_answer(run, text, cx),
        }
    }

    /// Says one thing under the bar. Errors are the valley's own words.
    fn say(&mut self, text: impl Into<String>, error: bool, cx: &mut Context<Self>) {
        self.command_reply = Some(CommandReply { text: text.into(), error, at: Instant::now() });
        cx.notify();
    }

    fn command_status(&mut self, cx: &mut Context<Self>) {
        let valley = self.valley_client();
        let job = self.engine.runtime().spawn(async move { valley.work_stats().await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| match result {
                Ok(stats) => {
                    let w = &stats.work;
                    let line = format!(
                        "{} items ({} waiting · {} running · {} blocked · {} failed · {} done today) · {} need you · board rev {}",
                        w.total,
                        w.pending,
                        w.running,
                        w.blocked,
                        w.failed,
                        w.done_today,
                        stats.runs.needs_you,
                        tail(&stats.fingerprint)
                    );
                    this.say(line, false, cx)
                }
                Err(e) => this.say(e, true, cx),
            });
        });
        self.track(task);
    }

    fn command_cost(&mut self, cx: &mut Context<Self>) {
        let valley = self.valley_client();
        let job = self.engine.runtime().spawn(async move { valley.live_capacity().await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| match result {
                Ok(capacity) => {
                    let burn = &capacity.burn;
                    let mut line = format!("${:.2} today", burn.cost_usd);
                    if let Some(basis) = burn.cost_basis.as_deref() {
                        line.push_str(&format!(" ({})", basis));
                    }
                    if let Some(cash) = burn.cash_usd {
                        line.push_str(&format!(" · ${cash:.2} cash"));
                    }
                    if let Some(allowance) = &burn.allowance {
                        if let (Some(left), Some(total)) =
                            (allowance.included_balance_usd, allowance.included_allowance_usd)
                        {
                            line.push_str(&format!(" · ${left:.2} of ${total:.0} credits left"));
                        }
                    }
                    line.push_str(&format!(
                        " · {} calls · {}/{} worker slots busy",
                        burn.total_calls.unwrap_or(0),
                        capacity.busy_slots(),
                        capacity.pool_size
                    ));
                    this.say(line, false, cx)
                }
                Err(e) => this.say(e, true, cx),
            });
        });
        self.track(task);
    }

    fn command_why(&mut self, run: String, cx: &mut Context<Self>) {
        let run = self.resolve_run(&run);
        let valley = self.valley_client();
        let asked = run.clone();
        let job = self.engine.runtime().spawn(async move { valley.run_health(&run).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| match result {
                Ok(health) => {
                    let mut line = format!(
                        "{} — {} ({} of {} verses, {} did something, {} LLM calls)",
                        health.kumbaya,
                        health.verdict,
                        health.verses_done,
                        health.verses_total,
                        health.tool_verses,
                        health.llm_calls
                    );
                    if let Some(secs) = health.since_last_verse_seconds {
                        line.push_str(&format!(" · last verse {}s ago", secs.round() as i64));
                    }
                    if !health.flags.is_empty() {
                        line.push_str(&format!(" · {}", health.flags.join(", ")));
                    }
                    line.push_str(&format!(
                        " · ${:.4} {} · {} tokens",
                        health.usage.cost_usd, health.usage.cost_basis, health.usage.total_tokens
                    ));
                    this.say(line, false, cx)
                }
                // A 404 is the honest answer to an unknown run; say which one.
                Err(e) => this.say(format!("{asked}: {e}"), true, cx),
            });
        });
        self.track(task);
    }

    fn command_item(&mut self, item: String, cx: &mut Context<Self>) {
        let valley = self.valley_client();
        let job = self.engine.runtime().spawn(async move { valley.run_preflight(&item).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| match result {
                Ok(pre) => this.say(format!("{} · {}", pre.item_id, verdict_line(&pre)), false, cx),
                Err(e) => this.say(e, true, cx),
            });
        });
        self.track(task);
    }

    /// `kick` asks the preflight first, then asks the operator, then acts.
    fn command_kick(&mut self, item: String, cx: &mut Context<Self>) {
        let valley = self.valley_client();
        let requested = item.clone();
        let job = self.engine.runtime().spawn(async move { valley.run_preflight(&item).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| match result {
                Err(e) => this.say(e, true, cx),
                Ok(pre) if !pre.claimable => this.say(format!("Not kicked — {}", verdict_line(&pre)), true, cx),
                Ok(pre) => {
                    let key = format!("kick:{requested}");
                    let armed = this
                        .command_armed
                        .as_ref()
                        .is_some_and(|(k, at)| k == &key && at.elapsed().as_secs() < ARM_SECS);
                    if armed {
                        this.command_armed = None;
                        this.send_kick(requested, cx);
                    } else {
                        this.command_armed = Some((key, Instant::now()));
                        this.say(
                            format!(
                                "{} is claimable{}. Say `kick {}` again within {ARM_SECS}s to send it.",
                                pre.item_id,
                                if pre.stale_claim { " (its old claim is stale, so this is safe)" } else { "" },
                                pre.item_id
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

    fn send_kick(&mut self, item: String, cx: &mut Context<Self>) {
        let valley = self.valley_client();
        let job = self.engine.runtime().spawn(async move { valley.kick(&item).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(outcome) => {
                        this.say(outcome.message(), false, cx);
                        // The board and the strip both describe a valley that
                        // just changed: re-read them rather than show the old.
                        this.valley_data = None;
                        this.load_valley(cx);
                        this.mission = None;
                        this.mission_health = None;
                    }
                    Err(e) => this.say(e, true, cx),
                }
                cx.notify();
            });
        });
        self.track(task);
    }

    fn command_stop(&mut self, run: String, cx: &mut Context<Self>) {
        let run = self.resolve_run(&run);
        let key = format!("stop:{run}");
        let armed = self
            .command_armed
            .as_ref()
            .is_some_and(|(k, at)| k == &key && at.elapsed().as_secs() < ARM_SECS);
        if !armed {
            self.command_armed = Some((key, Instant::now()));
            self.say(format!("Say `stop {run}` again within {ARM_SECS}s to stop it."), false, cx);
            return;
        }
        self.command_armed = None;
        let valley = self.valley_client();
        let asked = run.clone();
        let job = self.engine.runtime().spawn(async move { valley.cancel_run(&run).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.say(format!("Stopped {asked}. Its item goes back to the board."), false, cx);
                        this.valley_data = None;
                        this.load_valley(cx);
                        this.mission_health = None;
                    }
                    Err(e) => this.say(format!("{asked}: {e}"), true, cx),
                }
                cx.notify();
            });
        });
        self.track(task);
    }

    /// Answering a checkpoint is what the responder cards already do, so it
    /// goes straight out rather than arming: reading it twice helps nobody.
    fn command_answer(&mut self, run: String, text: String, cx: &mut Context<Self>) {
        let run = self.resolve_run(&run);
        let valley = self.valley_client();
        let asked = run.clone();
        let job = self.engine.runtime().spawn(async move { valley.resume(&run, &text).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.say(format!("Answered {asked} — it carries on from there."), false, cx);
                        this.mission_health = None;
                    }
                    Err(e) => this.say(format!("{asked}: {e}"), true, cx),
                }
                cx.notify();
            });
        });
        self.track(task);
    }

    /// A run id as the operator typed it: the full id, or a prefix that only
    /// one known run matches. Anything else goes out as typed and the server
    /// says 404 — no guessing at ids.
    fn resolve_run(&self, given: &str) -> String {
        let given = given.trim();
        if given.is_empty() {
            return String::new();
        }
        if let Some(Load::Ready(runs)) = &self.valley_runs {
            if let Some(exact) = runs.iter().find(|r| r.id == given) {
                return exact.id.clone();
            }
            let matches: Vec<&str> = runs
                .iter()
                .map(|r| r.id.as_str())
                .filter(|id| id.starts_with(given))
                .collect();
            if matches.len() == 1 {
                return matches[0].to_string();
            }
        }
        given.to_string()
    }

    pub(super) fn render_command_bar(&mut self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let input = self.command_input.clone();
        let reply = self
            .command_reply
            .as_ref()
            .filter(|r| r.at.elapsed().as_secs() < REPLY_SECS)
            .map(|r| (r.text.clone(), r.error));
        let mut bar = card(theme)
            .p(px(10.))
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .font_family(mono_font())
                            .text_color(theme.text_placeholder)
                            .child(">"),
                    )
                    .child(div().flex_1().min_w(px(0.)).child(input))
                    .child(
                        Button::new("command-run", "Run")
                            .on_click(cx.listener(|this, _, _, cx| this.submit_command(cx))),
                    ),
            )
            .child(div().text_size(px(11.)).text_color(theme.text_placeholder).child(HINT));
        if let Some((text, error)) = reply {
            bar = bar.child(
                div()
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if error { theme.warning } else { theme.text })
                    .child(text),
            );
        }
        bar.into_any_element()
    }
}

fn help_text() -> String {
    [
        "status — the board's counts and which revision they came from",
        "cost — today's burn, its basis, cash and remaining credits",
        "why <run> — is that run working, stalled or finished, and what it cost",
        "item <item> — would a kick work right now, and if not, why",
        "kick <item> — asks the preflight, then asks you again before sending",
        "stop <run> — asks once, then stops it",
        "answer <run> <text> — answers its checkpoint",
    ]
    .join("\n")
}

/// The preflight's verdict in one line: what it is, and the reason or warning.
fn verdict_line(pre: &crate::valley::RunPreflight) -> String {
    let mut line = if pre.claimable {
        format!("claimable ({})", pre.state)
    } else if pre.reason.trim().is_empty() {
        format!("not kickable ({})", pre.state)
    } else {
        format!("not kickable — {}", pre.reason.trim())
    };
    if let Some(warning) = pre.warning.as_deref().map(str::trim).filter(|w| !w.is_empty()) {
        line.push_str(&format!(" · {warning}"));
    }
    line
}

fn tail(fingerprint: &str) -> String {
    let trimmed = fingerprint.trim();
    if trimmed.is_empty() {
        return "—".to_string();
    }
    trimmed.chars().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbs_split_on_the_first_space() {
        assert_eq!(parse("status"), Verb::Status);
        assert_eq!(parse("  COST  "), Verb::Cost);
        assert_eq!(parse("kick work_caf976e921"), Verb::Kick("work_caf976e921".into()));
        assert_eq!(parse("why db8d80e4"), Verb::Why("db8d80e4".into()));
        assert_eq!(parse("item work_x"), Verb::Item("work_x".into()));
    }

    #[test]
    fn an_answer_keeps_its_words_whole() {
        assert_eq!(
            parse("answer db8d80e4 yes, ship it now"),
            Verb::Answer("db8d80e4".into(), "yes, ship it now".into())
        );
        // No answer yet: the bar says so rather than sending an empty one.
        assert_eq!(parse("answer db8d80e4"), Verb::Answer("db8d80e4".into(), String::new()));
    }

    #[test]
    fn a_verb_nobody_knows_is_named_not_guessed() {
        assert_eq!(parse("deploy everything"), Verb::Unknown("deploy".into()));
        assert_eq!(parse(""), Verb::Unknown(String::new()));
    }

    #[test]
    fn the_preflight_verdict_reads_honestly() {
        let claimable = crate::valley::RunPreflight {
            item_id: "work_x".into(),
            state: "claimable".into(),
            claimable: true,
            stale_claim: true,
            warning: Some("item still carries a claim from finished run 0ee1309f; the claim is stale, kicking it is safe".into()),
            ..Default::default()
        };
        assert!(verdict_line(&claimable).starts_with("claimable (claimable)"));
        assert!(verdict_line(&claimable).contains("the claim is stale"));

        let refused = crate::valley::RunPreflight {
            state: "paused".into(),
            reason: "project 'andrew' is paused (queue pause); run-now deferred".into(),
            ..Default::default()
        };
        assert_eq!(
            verdict_line(&refused),
            "not kickable — project 'andrew' is paused (queue pause); run-now deferred"
        );
    }
}
