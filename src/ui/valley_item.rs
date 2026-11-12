//! One board item at three levels, Father 12 Nov 2026: the listing (the
//! Valley page) opens this detail page — what the item is about, its log —
//! and the conversation of the job working it (the verses of its claim
//! run), with a line to instruct a camper that is still running.

use gpui::{AnyElement, ClickEvent, Context, FontWeight, SharedString, Window, div, prelude::*, px};

use super::github::Load;
use super::input::TextInput;
use super::kit::{self, Button, ButtonStyle, card, section_title, spinner};
use super::overview::page;
use super::root::{Root, Route};
use super::theme::{ActiveTheme, mono_font};
use crate::valley::{ItemDetail, RunTalk, Valley};

/// How much of one verse's speech the page draws.
const SAID_CAP: usize = 700;

impl Root {
    /// Reads one item's detail into the store, ONCE: render runs many times
    /// a second, and an unconditional insert here wiped Ready back to Loading
    /// on every paint, so a clicked job never settled (Father, 12 Nov 2026:
    /// "you click one of the jobs and it doesn't give you the details").
    /// The Try-again and refresh controls remove the entry first.
    pub(super) fn load_valley_item(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.valley_items.contains_key(id) {
            return;
        }
        self.valley_items.insert(id.to_string(), Load::Loading);
        let mut cfg = self.engine.settings().valley.clone();
        if let Some(base) = self.valley_view.clone() {
            cfg.base_url = base;
        }
        let valley = Valley::from_parts(cfg);
        if !valley.configured() {
            self.valley_items
                .insert(id.to_string(), Load::Failed("not-configured".into()));
            cx.notify();
            return;
        }
        let fetch = id.to_string();
        let job = self.engine.runtime().spawn(async move { valley.item(&fetch).await });
        let put = id.to_string();
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.map_err(|e| e.to_string()).and_then(|r| r);
            let _ = this.update(cx, |this, cx| {
                this.valley_items.insert(
                    put,
                    match result {
                        Ok(v) => Load::Ready(v),
                        Err(e) => Load::Failed(e),
                    },
                );
                cx.notify();
            });
        });
        self.track(task);
    }

    /// Reads one run's conversation into the store, keyed by run id — once
    /// (same render-time guard as the item above).
    pub(super) fn load_valley_talk(&mut self, run_id: &str, cx: &mut Context<Self>) {
        if self.valley_talks.contains_key(run_id) {
            return;
        }
        self.valley_talks.insert(run_id.to_string(), Load::Loading);
        let mut cfg = self.engine.settings().valley.clone();
        if let Some(base) = self.valley_view.clone() {
            cfg.base_url = base;
        }
        let valley = Valley::from_parts(cfg);
        if !valley.configured() {
            self.valley_talks
                .insert(run_id.to_string(), Load::Failed("not-configured".into()));
            cx.notify();
            return;
        }
        let fetch = run_id.to_string();
        let job = self.engine.runtime().spawn(async move { valley.talk(&fetch).await });
        let put = run_id.to_string();
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.map_err(|e| e.to_string()).and_then(|r| r);
            let _ = this.update(cx, |this, cx| {
                this.valley_talks.insert(
                    put,
                    match result {
                        Ok(v) => Load::Ready(v),
                        Err(e) => Load::Failed(e),
                    },
                );
                cx.notify();
            });
        });
        self.track(task);
    }

    /// Sends one instruction, interrupting the running camper.
    fn send_item_instruct(&mut self, session_id: String, text: String, cx: &mut Context<Self>) {
        let valley = crate::valley::Valley::from_parts(self.engine.settings().valley.clone());
        let job = self
            .engine
            .runtime()
            .spawn(async move { valley.chat(&session_id, &text, true).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let outcome = match &result {
                Ok(_) => ("The camper stopped to listen.".to_string(), false),
                Err(e) => (e.clone(), true),
            };
            let _ = this.update(cx, |this, cx| {
                this.notify_user(outcome.0, outcome.1, cx);
            });
        });
        self.track(task);
    }

    /// The chat line that interrupts a still-running camper (interrupt:
    /// true — the Campfires path; Andrew's session stops and listens).
    fn render_item_instruct(&mut self, cx: &mut Context<Self>) -> AnyElement {
        // Created before any immutable borrow of the talks map keeps the
        // borrow checker happy (the Campfires page's lesson).
        if self.valley_item_input.is_none() {
            self.valley_item_input = Some(cx.new(|cx| {
                TextInput::new(cx, "Instruct the camper — feedback, training, a change of course…").mono()
            }));
        }
        let Some(input) = self.valley_item_input.as_ref().map(|i| i.clone()) else {
            return div().into_any_element();
        };
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .mx(px(16.))
            .mb(px(8.))
            .child(
                div()
                    .w(px(220.))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .child(input),
            )
            .child(
                Button::new("item-instruct", "Instruct")
                    .icon("flame")
                    .style(ButtonStyle::Primary)
                    .tooltip("Send, interrupting the camper mid-run")
                    .on_click(cx.listener(
                        |this: &mut Root, _: &ClickEvent, _: &mut Window, cx: &mut Context<Root>| {
                            let text = this
                                .valley_item_input
                                .as_ref()
                                .map(|i| i.read(cx).text().trim().to_string())
                                .unwrap_or_default();
                            if text.is_empty() {
                                this.notify_user("Write the instruction first.", true, cx);
                                return;
                            }
                            if let Some(input) = this.valley_item_input.as_ref() {
                                input.update(cx, |i, cx| i.set_text("", cx));
                            }
                            // The session of the still-running run on this page.
                            let session = this
                                .valley_talks
                                .values()
                                .find_map(|t| match t {
                                    Load::Ready(talk) if talk.status == "running" => {
                                        Some(talk.session_id.clone())
                                    }
                                    _ => None,
                                })
                                .unwrap_or_default()
                                .trim()
                                .to_string();
                            if session.is_empty() {
                                this.notify_user("No running camper here to instruct.", true, cx);
                                return;
                            }
                            this.send_item_instruct(session, text, cx);
                        },
                    )),
            )
            .into_any_element()
    }

    pub(super) fn render_valley_item(&mut self, id: &str, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        self.load_valley_item(id, cx);
        let back = div()
            .id("vi-back")
            .text_size(px(12.5))
            .text_color(theme.text_muted)
            .cursor_pointer()
            .hover(|st| st.text_color(theme.text))
            .child("← Back to the valley")
            .on_click(cx.listener(
                |this: &mut Root, _: &ClickEvent, window: &mut Window, cx: &mut Context<Root>| {
                    this.navigate(Route::Valley, window, cx)
                },
            ));
        let Some(Load::Ready(item)) = self.valley_items.get(id) else {
            let body = page("valley-item-page", &self.page_scroll).child(back);
            return match self.valley_items.get(id) {
                Some(Load::Failed(e)) if e == "not-configured" => body
                    .child(
                        card(&theme).p(px(18.)).text_size(px(13.)).text_color(theme.text_muted)
                            .child("The valley isn't configured in this app yet — Settings › Valley."),
                    )
                    .into_any_element(),
                Some(Load::Failed(e)) => body
                    .child(
                        card(&theme).p(px(18.)).text_size(px(13.)).text_color(theme.text_muted).child(e.clone()),
                    )
                    .child(
                        Button::new("vi-retry", "Try again")
                            .tooltip("Read this item from the valley again")
                            .on_click({
                                let key = id.to_string();
                                cx.listener(move |this: &mut Root, _: &ClickEvent, _: &mut Window, cx: &mut Context<Root>| {
                                    this.valley_items.remove(&key);
                                    this.load_valley_item(&key, cx);
                                })
                            }),
                    )
                    .into_any_element(),
                _ => body
                    .child(
                        card(&theme).p(px(18.)).flex_row().items_center().gap(px(10.))
                            .text_size(px(13.)).text_color(theme.text_muted)
                            .child(spinner("vi-loading", 14., theme.text_muted))
                            .child("Reading the item…"),
                    )
                    .into_any_element(),
            };
        };
        let item = ItemDetail::clone(item);

        // Header: the title and the fact row.
        let (pill, pill_color) = match item.status.as_str() {
            "running" => ("running", theme.accent),
            "waiting_checkpoint" => ("needs you", theme.accent),
            "completed" => ("completed", theme.success),
            "failed" => ("failed", theme.error),
            other => (other, theme.text_muted),
        };
        let header = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .mx(px(16.))
            .mt(px(10.))
            .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child(item.title.clone()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(11.5))
                    .text_color(theme.text_muted)
                    .font_family(mono_font())
                    .child(item.id.clone())
                    .when(!item.project.is_empty(), |d| d.child(item.project.clone()))
                    .child(format!("p{}", item.priority))
                    .child(div().text_color(pill_color).child(pill.to_string()))
                    .when_some(crate::valley::age_text(&item.created_at), |d, age| d.child(age)),
            );

        let mut stack: Vec<AnyElement> = Vec::new();

        // Level 2: what it's about.
        stack.push(
            div()
                .mt(px(10.))
                .child(section_title(&theme, "What it's about", None))
                .into_any_element(),
        );
        if item.description.trim().is_empty() {
            stack.push(
                card(&theme)
                    .p(px(14.))
                    .text_size(px(13.))
                    .text_color(theme.text_muted)
                    .child("No description was written for this item.")
                    .into_any_element(),
            );
        } else {
            let paragraphs: Vec<&str> = item.description.split("\n\n").collect();
            stack.push(
                card(&theme)
                    .p(px(14.))
                    .flex_col()
                    .gap(px(8.))
                    .children(paragraphs.into_iter().map(|p| {
                        div()
                            .text_size(px(13.))
                            .whitespace_normal()
                            .child(p.trim().to_string())
                    }))
                    .into_any_element(),
            );
        }

        // The error, when the claim failed: null and "" both read as none.
        if let Some(err) = item.failed_text() {
            stack.push(
                card(&theme)
                    .p(px(14.))
                    .text_size(px(12.5))
                    .text_color(theme.error)
                    .child(err.to_string())
                    .into_any_element(),
            );
        }

        // The log, newest last like a tail.
        let log = item.recent_log(12);
        if !log.is_empty() {
            stack.push(
                div()
                    .mt(px(8.))
                    .child(section_title(&theme, "The log", Some(log.len())))
                    .into_any_element(),
            );
            stack.push(
                card(&theme)
                    .p(px(14.))
                    .flex_col()
                    .gap(px(3.))
                    .children(log.iter().rev().map(|entry| {
                        div()
                            .text_size(px(11.))
                            .font_family(mono_font())
                            .text_color(theme.text_muted)
                            .child((*entry).to_string())
                    }))
                    .into_any_element(),
            );
        }

        // Level 3: the conversation of the job working this item.
        if let Some(run_id) = item.claim_run().map(str::to_string) {
            self.load_valley_talk(&run_id, cx);
            stack.push(
                div()
                    .mt(px(8.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(section_title(&theme, "On the fire", None))
                    .child(
                        Button::icon_only("vi-talk-refresh", "refresh-cw")
                            .tooltip("Read the conversation again")
                            .on_click({
                                let run_id = run_id.clone();
                                cx.listener(move |this: &mut Root, _: &ClickEvent, _: &mut Window, cx: &mut Context<Root>| {
                                    this.valley_talks.remove(&run_id);
                                    this.load_valley_talk(&run_id, cx);
                                })
                            }),
                    )
                    .into_any_element(),
            );
            // Pre-create the instruct input before borrowing the map.
            self.valley_item_input
                .get_or_insert_with(|| cx.new(|cx| TextInput::new(cx, "Instruct the camper…").mono()));
            if let Some(Load::Ready(talk)) = self.valley_talks.get(&run_id) {
                let talk = RunTalk::clone(talk);
                stack.push(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .mx(px(16.))
                        .text_size(px(11.5))
                        .font_family(mono_font())
                        .text_color(theme.text_muted)
                        .child(
                            div()
                                .text_color(match talk.status.as_str() {
                                    "running" | "waiting_checkpoint" => theme.accent,
                                    "completed" => theme.success,
                                    "failed" => theme.error,
                                    _ => theme.text_muted,
                                })
                                .child(talk.status.clone()),
                        )
                        .child(format!("· {} verses", talk.verses.len()))
                        .when(!talk.kumbaya.is_empty(), |d| d.child(format!("· {}", talk.kumbaya)))
                        .into_any_element(),
                );
                for (i, verse) in talk.verses.iter().enumerate() {
                    let mut row = div()
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        .mx(px(16.))
                        .my(px(4.));
                    match verse.status.as_str() {
                        "running" => {
                            row = row.child(spinner(
                                SharedString::from(format!("vi-vspin-{i}")),
                                12.,
                                theme.text_muted,
                            ));
                        }
                        "completed" => row = row.child(kit::icon("check", 12., theme.success)),
                        "skipped" => row = row.child(kit::icon("minus", 12., theme.text_placeholder)),
                        "failed" => row = row.child(kit::icon("x", 12., theme.error)),
                        _ => row = row.child(div().w(px(12.))),
                    }
                    row = row.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .min_w(px(0.))
                            .flex_1()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .text_size(px(12.5))
                                    .font_family(mono_font())
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(verse.verse.clone())
                                    .child(
                                        div()
                                            .text_size(px(10.5))
                                            .font_weight(FontWeight::NORMAL)
                                            .text_color(theme.text_placeholder)
                                            .child(verse.kind.clone()),
                                    ),
                            ),
                    );
                    if let Some(said) = verse.said() {
                        row = row.child(
                            div()
                                .text_size(px(12.5))
                                .text_color(theme.text_muted)
                                .whitespace_normal()
                                .child(clipped(said, SAID_CAP)),
                        );
                    }
                    stack.push(row.into_any_element());
                }
                if talk.status == "running" && !talk.session_id.trim().is_empty() {
                    stack.push(self.render_item_instruct(cx));
                }
            } else if matches!(self.valley_talks.get(&run_id), Some(Load::Failed(_))) {
                let msg = match self.valley_talks.get(&run_id) {
                    Some(Load::Failed(e)) if e == "not-configured" => {
                        "The valley isn't configured yet.".to_string()
                    }
                    Some(Load::Failed(e)) => e.clone(),
                    _ => String::new(),
                };
                stack.push(
                    card(&theme)
                        .p(px(14.))
                        .text_size(px(12.5))
                        .text_color(theme.text_muted)
                        .child(msg)
                        .into_any_element(),
                );
            }
        }

        page("valley-item-page", &self.page_scroll)
            .child(back)
            .child(header)
            .children(stack)
            .into_any_element()
    }
}

/// One paragraph of what a camper said, capped for the page.
pub(crate) fn clipped(said: &str, cap: usize) -> String {
    let mut cut = String::new();
    for (_, ch) in said.char_indices().take_while(|(i, _)| *i < cap) {
        cut.push(ch);
    }
    if cut.chars().count() < said.chars().count() {
        cut.push('…');
    }
    cut
}
