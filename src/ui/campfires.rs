//! Campfires: look in on the valley — the campers, and what they're up to.
//! (Father, 12 Nov 2026: "add a Campfires option to view and drill down by
//! agent and activity"; "look in on the campfire and see what the campers
//! are upto"; and "add interuptions if the user wants to instuct the camper
//! (feedback/training)".) A camper is one session id on the valley's runs;
//! each of its runs is a torch. Opening a torch shows its verse trail, and
//! while it burns you can send the camper instruction mid-run — the message
//! rides the valley chat with interrupt set, so Andrew stops and listens.

use gpui::{AnyElement, Context, FontWeight, SharedString, Window, div, prelude::*, px};

use super::github::Load;
use super::input::TextInput;
use super::kit::{self, Button, ButtonStyle, card, section_title, spinner};
use super::overview::page;
use super::root::Root;
use super::theme::{ActiveTheme, Theme, mono_font};
use crate::valley::{self, CheckpointAsk, RunSummary};

/// The page's sidebar tile colour: campfire orange.
pub const TILE: (u32, u32) = (0xffa94d, 0xe0432d);

impl Root {
    /// The sidebar badge: a camper is asking for you.
    pub(super) fn campfires_badge(&self) -> Option<usize> {
        let runs = match self.valley_runs.as_ref() {
            Some(Load::Ready(runs)) => runs,
            _ => return None,
        };
        let n = valley::need_you(runs);
        (n > 0).then_some(n)
    }

    pub(super)     fn render_campfires(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.maybe_load_runs(cx);
        // Pre-create the instruct input before the runs loop: the loop borrows
        // self.valley_runs for the whole page pass, so the row below must not
        // need &mut self mid-render (rustc E0502, caught on first compile).
        if self.campfire_input.is_none() {
            self.campfire_input = Some(
                cx.new(|cx| TextInput::new(cx, "Instruct the camper — feedback, training, a change of course…").mono()),
            );
        }
        // cx.theme() takes no generics in this tree (theme.rs:105); run.rs binds it the same way.
        let theme = cx.theme();
        // Same ordering as the Run page: needs-you first, then running,
        // then the settled runs, newest within each band.
        if let Some(Load::Ready(runs)) = self.valley_runs.as_mut() {
            valley::order_runs(runs);
        }
        let count = match self.valley_runs.as_ref() {
            Some(Load::Ready(runs)) if !runs.is_empty() => Some(runs.len()),
            _ => None,
        };
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
                    .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child("Campfires"))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(theme.text_muted)
                            .child("Look in on the campfire — the campers, and what they're up to. Open a torch to watch its verses, or talk to a camper while it burns."),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(section_title(&theme, "At the campfire", count))
                    .child(
                        Button::icon_only("campfires-refresh", "refresh-cw")
                            .tooltip("Listen for the runs again")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.valley_runs = None;
                                this.maybe_load_runs(cx);
                            })),
                    ),
            );
        let mut body = page("campfires-page", &self.page_scroll).child(header);
        let groups = match self.valley_runs.as_ref() {
            None | Some(Load::Loading) => {
                return body
                    .child(card(&theme).p(px(18.)).flex_row().items_center().gap(px(10.)).text_size(px(13.)).text_color(theme.text_muted).child(spinner("campfires-loading", 14., theme.text_muted)).child("Looking in on the campfire…"))
                    .into_any_element();
            }
            Some(Load::Failed(e)) if e == "not-configured" => {
                return body
                    .child(card(&theme).p(px(18.)).flex_col().gap(px(8.)).text_size(px(13.)).text_color(theme.text_muted)
                        .child("The valley isn't configured in this app yet — Settings › Valley."))
                    .into_any_element();
            }
            Some(Load::Failed(e)) => {
                return body
                    .child(card(&theme).p(px(18.)).flex_row().items_center().gap(px(10.)).text_size(px(13.)).text_color(theme.text_muted).child(e.clone()))
                    .into_any_element();
            }
            Some(Load::Ready(runs)) if runs.is_empty() => {
                return body
                    .child(card(&theme).p(px(18.)).text_size(px(13.)).text_color(theme.text_muted).child("No campers at the fire."))
                    .into_any_element();
            }
            Some(Load::Ready(runs)) => valley::group_by_camper(runs),
        };
        for (camper, group) in &groups {
            let open = self.campfire_open.as_deref() == Some(camper.as_str());
            let mut area = div().flex().flex_col().gap(px(2.));
            area = area.child(self.render_campfire_row(&theme, camper, group, open, cx));
            if open {
                for run in group {
                    let torch_open = self.campfire_torch.as_deref() == Some(run.id.as_str());
                    area = area.child(self.render_torch_row(&theme, run, torch_open, cx));
                    if torch_open {
                        area = area.child(self.render_valley_run_verses(&theme, run));
                        if run.status == "waiting_checkpoint" {
                            // The responder card rides only owned clones: the
                            // loop holds the runs borrow, and the card's
                            // buttons must outlive it (rustc E0502 rule).
                            if let Some(ask) = run.waiting_ask() {
                                let run_id = run.id.clone();
                                area = area.child(self.render_checkpoint_card(&theme, run_id, ask.clone(), cx));
                            }
                        }
                        if run.status == "running" {
                            area = area.child(self.render_instruct_row(run, cx));
                        }
                    }
                }
            }
            body = body.child(area);
        }
        body.into_any_element()
    }

    /// One camper: what their torches are doing together.
    fn render_campfire_row(
        &self,
        theme: &Theme,
        camper: &str,
        // group is &[&RunSummary]: we borrow the runs we sorted in place above.
        group: &[&RunSummary],
        open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let need = group.iter().filter(|r| r.status == "waiting_checkpoint").count();
        let running = group.iter().filter(|r| r.status == "running").count();
        let chip: AnyElement = if need > 0 {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(kit::icon("flame", 12., theme.accent))
                .child(div().text_size(px(10.5)).font_weight(FontWeight::MEDIUM).text_color(theme.accent).child("NEEDS YOU"))
                .into_any_element()
        } else if running > 0 {
            spinner(("campfire-spin", camper.len()), 11., theme.text_muted).into_any_element()
        } else {
            div().w(px(12.)).into_any_element()
        };
        let click = {
            let camper = camper.to_string();
            cx.listener(move |this: &mut Root, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Root>| {
                this.campfire_open = if this.campfire_open.as_deref() == Some(camper.as_str()) { None } else { Some(camper.clone()) };
                this.campfire_torch = None;
                cx.notify();
            })
        };
        div()
            .id(SharedString::from(format!("campfire-{}", camper)))
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(28.))
            .px(px(10.))
            .pl(px(4.))
            .mx(px(6.))
            .rounded(kit::corner(7.))
            .cursor_pointer()
            .hover(|st| st.bg(theme.element_hover))
            .when(open, |d| d.bg(theme.element))
            .child(chip)
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .child(camper.to_string()),
            )
            .child(
                div()
                    .flex_none()
                    .font_family(mono_font())
                    .text_size(px(11.))
                    .text_color(theme.text_placeholder)
                    .child(SharedString::from(format!(
                        "{} run{} · {} verses",
                        group.len(),
                        if group.len() == 1 { "" } else { "s" },
                        group.iter().map(|r| r.verses.len()).sum::<usize>(),
                    ))),
            )
            .on_click(click)
            .into_any_element()
    }

    /// One torch (a camper's run): where it is, and whether we're watching it.
    fn render_torch_row(
        &self,
        theme: &Theme,
        run: &RunSummary,
        open: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let chip: AnyElement = match run.status.as_str() {
            "running" => spinner(("torch-spin", run.id.len()), 10., theme.text_muted).into_any_element(),
            "waiting_checkpoint" => kit::icon("flame", 11., theme.accent).into_any_element(),
            "completed" => kit::icon("check", 11., theme.success).into_any_element(),
            "failed" => kit::icon("x", 11., theme.error).into_any_element(),
            other => div().text_size(px(10.)).text_color(theme.text_placeholder).child(other.to_string()).into_any_element(),
        };
        let at = run
            .current_verse()
            .map(|v| v.verse.clone())
            .unwrap_or_else(|| format!("{} of {} verses", run.settled_verses(), run.verses.len()));
        let age = valley::age_text(&run.created_at).unwrap_or_default();
        let click = {
            let id = run.id.clone();
            cx.listener(move |this: &mut Root, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Root>| {
                this.campfire_torch = if this.campfire_torch.as_deref() == Some(id.as_str()) { None } else { Some(id.clone()) };
                cx.notify();
            })
        };
        div()
            .id(SharedString::from(format!("torch-{}", run.id)))
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(26.))
            .px(px(10.))
            .pl(px(14.))
            .mx(px(6.))
            .rounded(kit::corner(7.))
            .cursor_pointer()
            .hover(|st| st.bg(theme.element_hover))
            .when(open, |d| d.bg(theme.element))
            .child(chip)
            .child(
                div()
                    .flex_none()
                    .w(px(170.))
                    .truncate()
                    .text_size(px(12.))
                    .child(run.kumbaya.clone())
                    .when(run.status == "waiting_checkpoint", |d| d.text_color(theme.accent)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .font_family(mono_font())
                    .text_size(px(11.))
                    .text_color(theme.text_muted)
                    .child(at),
            )
            .when(!age.is_empty(), |d| {
                d.child(div().flex_none().text_size(px(10.5)).text_color(theme.text_placeholder).child(age))
            })
            .on_click(click)
            .into_any_element()
    }

    /// The instruct line: feedback or training for a camper mid-burn.
    /// (Father, 12 Nov 2026: "add interuptions if the user wants to instuct
    /// the camper".) The message rides the valley chat with interrupt set.
    fn render_instruct_row(&self, run: &RunSummary, cx: &mut Context<Self>) -> AnyElement {
        let input = self
            .campfire_input
            .as_ref()
            .map(|i| i.clone())
            .unwrap_or_else(|| cx.new(|cx| TextInput::new(cx, "Instruct the camper").mono()));
        let session = run.session_id.clone();
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .mx(px(16.))
            .mb(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .child(input),
            )
            .child(
                Button::new("campfire-instruct", "Instruct")
                    .style(ButtonStyle::Primary)
                    .tooltip("Send, interrupting the camper mid-run")
                    .disabled(run.session_id.is_empty())
                    .on_click(cx.listener(move |this: &mut Root, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Root>| {
                        this.campfire_instruct(session.clone(), cx);
                    })),
            )
            .into_any_element()
    }

    /// The responder card: a checkpoint holds this run, and here are the
    /// answers it will take. (Father, 13 Nov 2026: "what a shame I can't
    /// respond to that needs you work_day_review" — and "Those needs need
    /// you could also be pop up on the overview page".) `run_id` and `ask`
    /// arrive owned (cloned by the caller) so the card's buttons don't
    /// borrow the runs list the page is iterating.
    pub(super) fn render_checkpoint_card(
        &self,
        theme: &Theme,
        run_id: String,
        ask: CheckpointAsk,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let question = ask.question.trim();
        let buttons = ask
            .options
            .iter()
            .filter(|o| !o.trim().is_empty())
            .map(|option| {
                let run_id = run_id.clone();
                let answer = option.clone();
                Button::new(SharedString::from(format!("cp-{run_id}-{option}")), option.clone())
                    .style(ButtonStyle::Primary)
                    .on_click(cx.listener(move |this: &mut Root, _: &gpui::ClickEvent, _: &mut Window, cx: &mut Context<Root>| {
                        this.resume_checkpoint(run_id.clone(), answer.clone(), cx);
                    }))
            })
            .collect::<Vec<_>>();
        card(&theme)
            .mx(px(16.))
            .mb(px(8.))
            .p(px(12.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .border_1()
            .border_color(theme.accent)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(kit::icon("flame", 12., theme.accent))
                    .child(
                        div()
                            .text_size(px(11.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.accent)
                            .child("NEEDS YOU — ANDREW IS WAITING"),
                    ),
            )
            .when(!question.is_empty(), |d| {
                d.child(div().text_size(px(12.5)).text_color(theme.text).whitespace_normal().child(super::valley_item::clipped(
                    question,
                    600,
                )))
            })
            .when(buttons.is_empty(), |d| {
                d.child(div().text_size(px(11.5)).text_color(theme.text_muted).child(
                    "No answer options came with the ask — instruct the camper instead.",
                ))
            })
            .child(div().flex().flex_wrap().items_center().gap(px(8.)).children(buttons))
            .into_any_element()
    }

    /// Answers the checkpoint: the run resumes on the valley, and the page's
    /// caches are dropped so the next paint reads the run's new state.
    pub(super) fn resume_checkpoint(&mut self, run_id: String, answer: String, cx: &mut Context<Self>) {
        let valley = crate::valley::Valley::from_parts(self.engine.settings().valley.clone());
        let answer_for_job = answer.clone();
        let job = self.engine.runtime().spawn(async move {
            valley.resume(&run_id, &answer_for_job).await
        });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                let (msg, err) = match &result {
                    Ok(_) => (format!("Answered with \"{}\" — the camper carries on.", answer), false),
                    Err(e) => (e.clone(), true),
                };
                this.notify_user(msg, err, cx);
                if result.is_ok() {
                    // Drop the caches: the run left the checkpoint, so both
                    // the torch trail and the runs list are stale now.
                    this.valley_talks.clear();
                    this.valley_runs = None;
                    this.maybe_load_runs(cx);
                }
            });
        });
        self.track(task);
    }

    /// Sends the instruction: interrupt the camper mid-run.
    fn campfire_instruct(&mut self, session_id: String, cx: &mut Context<Self>) {        let text = self
            .campfire_input
            .as_ref()
            .map(|i| i.read(cx).text().trim().to_string())
            .unwrap_or_default();
        if text.is_empty() {
            self.notify_user("Write the instruction first.", true, cx);
            return;
        }
        if let Some(i) = &self.campfire_input {
            i.update(cx, |i, cx| i.set_text("", cx));
        }
        let valley = crate::valley::Valley::from_parts(self.engine.settings().valley.clone());
        let job = self.engine.runtime().spawn(async move {
            valley.chat(&session_id, &text, true).await
        });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                this.notify_user(
                    match &result {
                        Ok(_) => "The camper stopped to listen.".into(),
                        Err(e) => SharedString::from(e.clone()),
                    },
                    match &result {
                        Ok(_) => false,
                        Err(_) => true,
                    },
                    cx,
                );
            });
        });
        self.track(task);
    }
}
