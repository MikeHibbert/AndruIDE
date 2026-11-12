//! Andrew on the machine: his conversations beside wherever Dad opens him.
//! A list of his sessions on the right, the transcript of one on the left,
//! and a composer that talks to him through the valley's chat. (Father,
//! 13 Nov 2026: run Andrew from the machine page "like he was some kind of
//! like open code kind of user interface that you can then manage sessions
//! with and do communication with him.")

use gpui::{AnyElement, ClickEvent, Context, FontWeight, SharedString, Window, div, prelude::*, px};

use super::github::Load;
use super::input::TextInput;
use super::kit::{self, Button, ButtonStyle, card, section_title, spinner};
use super::overview::page;
use super::root::Root;
use super::theme::{ActiveTheme, Theme, mono_font};
use crate::valley;

impl Root {
    /// Asks the valley for Andrew's conversations (again).
    pub(super) fn load_andrew_sessions(&mut self, cx: &mut Context<Root>) {
        self.andrew_sessions = Some(Load::Loading);
        self.andrew_sessions_at = Some(std::time::Instant::now());
        let valley = valley::Valley::from_parts(self.engine.settings().valley.clone());
        if !valley.configured() {
            self.andrew_sessions = Some(Load::Failed("not-configured".to_string()));
            cx.notify();
            return;
        }
        let job = self.engine.runtime().spawn(async move { valley.sessions().await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                this.andrew_sessions = Some(match result {
                    Ok(Ok(list)) => Load::Ready(list),
                    Ok(Err(e)) => Load::Failed(e),
                    Err(e) => Load::Failed(e.to_string()),
                });
                cx.notify();
            });
        });
        self.track(task);
        cx.notify();
    }

    /// Loads the conversations when the list is empty or stale (a failed
    /// ask retries sooner: the valley goes quiet under pool load).
    pub(super) fn maybe_load_andrew_sessions(&mut self, cx: &mut Context<Root>) {
        let failed = matches!(self.andrew_sessions, Some(Load::Failed(_)));
        let window = if failed { 15 } else { 60 };
        let stale = self.andrew_sessions.is_none()
            || self
                .andrew_sessions_at
                .is_some_and(|t| t.elapsed().as_secs() > window);
        if stale && !matches!(self.andrew_sessions, Some(Load::Loading)) {
            self.load_andrew_sessions(cx);
        }
    }

    /// Reads one conversation, once: renders run often; load runs once
    /// per visit (try-again and the refresh control remove the entry
    /// first, so a re-ask is always possible).
    pub(super) fn load_andrew_transcript(&mut self, key: &str, cx: &mut Context<Root>) {
        if self.andrew_talks.contains_key(key) {
            return;
        }
        self.andrew_talks
            .insert(key.to_string(), Load::Loading);
        let valley = valley::Valley::from_parts(self.engine.settings().valley.clone());
        if !valley.configured() {
            self.andrew_talks
                .insert(key.to_string(), Load::Failed("not-configured".to_string()));
            cx.notify();
            return;
        }
        let fetch = key.to_string();
        let put = key.to_string();
        let job = self
            .engine
            .runtime()
            .spawn(async move { valley.read_session(&fetch).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                this.andrew_talks.insert(
                    put,
                    match result {
                        Ok(Ok(read)) => Load::Ready(read),
                        Ok(Err(e)) => Load::Failed(e),
                        Err(e) => Load::Failed(e.to_string()),
                    },
                );
                cx.notify();
            });
        });
        self.track(task);
        cx.notify();
    }

    /// Opens another of Andrew's conversations from the list.
    fn andrew_select(&mut self, key: String, cx: &mut Context<Root>) {
        let fresh = self
            .andrew_read
            .as_ref()
            .is_none_or(|current| current != &key);
        self.andrew_read = Some(key.clone());
        if fresh {
            self.load_andrew_transcript(&key, cx);
        }
        cx.notify();
    }

    /// Sends what Dad wrote: a plain chat turn (not an interrupt) on the
    /// thread this page was opened with.
    fn andrew_chat_send(&mut self, key: String, cx: &mut Context<Root>) {
        let text = self
            .andrew_text
            .as_ref()
            .map(|i| i.read(cx).text().trim().to_string())
            .unwrap_or_default();
        if text.is_empty() {
            self.notify_user("Write the message first.", true, cx);
            return;
        }
        if let Some(input) = self.andrew_text.as_ref() {
            input.update(cx, |i, cx| i.set_text("", cx));
        }
        let valley = valley::Valley::from_parts(self.engine.settings().valley.clone());
        let job = self
            .engine
            .runtime()
            .spawn(async move { valley.chat(&key, &text, false).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(_)) => {
                        this.notify_user("Sent — Andrew is thinking.", false, cx);
                    }
                    Ok(Err(e)) => this.notify_user(e, true, cx),
                    Err(e) => this.notify_user(e.to_string(), true, cx),
                }
                cx.notify();
            });
        });
        self.track(task);
    }

    /// Opens the thread with a message handed in (the New-session dialog's
    /// opening line): sends it and drops the cached transcript so the page
    /// reloads with the turn in it.
    pub(super) fn andrew_send_open(&mut self, key: &str, text: String, cx: &mut Context<Root>) {
        let valley = valley::Valley::from_parts(self.engine.settings().valley.clone());
        let key_for_job = key.to_string();
        let key = key.to_string();
        let job = self
            .engine
            .runtime()
            .spawn(async move { valley.chat(&key_for_job, &text, false).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(_)) => {
                        // The cached transcript predates the new turn: drop
                        // it so the page reads the fresh one.
                        this.andrew_talks.remove(&key);
                        this.notify_user("Sent - Andrew is thinking.", false, cx);
                    }
                    Ok(Err(e)) => this.notify_user(e, true, cx),
                    Err(e) => this.notify_user(e.to_string(), true, cx),
                }
                cx.notify();
            });
        });
        self.track(task);
    }

    /// The page: Andrew's conversations, the one Dad is reading, and the
    /// composer.
    pub(super) fn render_andrew_chat(&mut self, key: &str, cx: &mut Context<Self>) -> AnyElement {
        self.maybe_load_andrew_sessions(cx);
        let theme = cx.theme();
        self.load_andrew_transcript(key, cx);

        // The composer is made before the page is laid out: rendering
        // borrows the page's state, and making it later would fight that
        // borrow (the campfires page hit this first).
        if self.andrew_text.is_none() {
            self.andrew_text = Some(cx.new(|cx| TextInput::new(cx, "Say something to Andrew…").mono()));
        }
        let input = self
            .andrew_text
            .as_ref()
            .map(|i| i.clone())
            .unwrap_or_else(|| cx.new(|cx| TextInput::new(cx, "Say something to Andrew…").mono()));

        let header = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .mx(px(29.))
            .mt(px(9.))
            .mb(px(6.))
            .flex_none()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .min_w(px(0.))
                    .flex_1()
                    .child(
                        div()
                            .text_size(px(22.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child("Andrew"),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_family(mono_font())
                            .text_color(theme.text_placeholder)
                            .text_ellipsis()
                            .child(key.to_string()),
                    ),
            )
            .child(
                Button::icon_only("mc-refresh", "refresh-cw")
                    .tooltip("Ask the valley for his conversations again")
                    .on_click(cx.listener(|this, _ev: &ClickEvent, _win: &mut Window, cx: &mut Context<Root>| {
                        this.andrew_sessions = None;
                        this.load_andrew_sessions(cx);
                    })),
            );

        // The right column: his conversations.
        let list = self.render_andrew_session_list(&theme, key, cx);
        // The left column: the transcript, then the composer.
        let picked = self.andrew_read.clone().unwrap_or_else(|| key.to_string());
        let transcript = match self.andrew_talks.get(&picked) {
            None | Some(Load::Loading) => card(&theme)
                .mx(px(4.))
                .p(px(12.))
                .gap(px(8.))
                .child(spinner(
                    (SharedString::from(format!("mc-spin-{picked}")), picked.len()),
                    14.,
                    theme.text_placeholder,
                ))
                .child("Reading the conversation…"),
            Some(Load::Failed(e)) if e == "not-configured" => card(&theme)
                .mx(px(4.))
                .p(px(12.))
                .text_size(px(13.))
                .text_color(theme.text_placeholder)
                .child("Andrew has no valley to talk through yet — set it in Settings › Valley."),
            Some(Load::Failed(e)) => {
                let key_for_retry = picked.clone();
                let error_text = e.clone();
                card(&theme)
                    .mx(px(4.))
                    .p(px(12.))
                    .gap(px(10.))
                    .text_size(px(13.))
                    .text_color(theme.text_muted)
                    .child(error_text)
                    .child(
                        Button::new(SharedString::from(format!("mc-retry-{key_for_retry}")), "Try again")
                            .on_click(cx.listener(move |this, _ev: &ClickEvent, _win: &mut Window, cx: &mut Context<Root>| {
                                this.andrew_talks.remove(&key_for_retry);
                                this.load_andrew_transcript(&key_for_retry, cx);
                            })),
                    )
            }
            Some(Load::Ready(read)) => {
                let mut rows: Vec<AnyElement> = Vec::new();
                for turn in &read.messages {
                    let who: String = if turn.role == "assistant" {
                        "Andrew".to_string()
                    } else if turn.role == "user" {
                        "Me".to_string()
                    } else {
                        turn.role.clone()
                    };
                    let (color, body_color) = if turn.role == "user" {
                        (theme.text_muted, theme.text_muted)
                    } else if turn.role == "assistant" {
                        (theme.text, theme.text)
                    } else {
                        (theme.text_placeholder, theme.text)
                    };
                    rows.push(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(color)
                                    .child(who),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(body_color)
                                    .whitespace_normal()
                                    .child(
                                        super::valley_item::clipped(&turn.content, 2000)
                                            .to_string(),
                                    ),
                            )
                            .into_any_element(),
                    );
                }
                if read.messages.is_empty() {
                    rows.push(
                        div()
                            .text_size(px(12.5))
                            .text_color(theme.text_placeholder)
                            .child("No words yet — say hello.")
                            .into_any_element(),
                    );
                }
                card(&theme)
                    .mx(px(4.))
                    .p(px(12.))
                    .gap(px(14.))
                    .children(rows)
            }
        };
        let transcript_key = picked.clone();
        let composer = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .mx(px(4.))
            .flex_none()
            .child(
                div()
                    .w(px(360.))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .min_w(px(0.))
                    .child(input),
            )
            .child(
                Button::new("mc-send", "Send")
                    .style(ButtonStyle::Primary)
                    .on_click(cx.listener(move |this, _ev: &ClickEvent, _win: &mut Window, cx: &mut Context<Root>| {
                        this.andrew_chat_send(transcript_key.clone(), cx);
                    })),
            );

        let body = div()
            .flex()
            .gap(px(12.))
            .px(px(6.))
            .flex_1()
            .min_h(px(0.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(transcript)
                    .child(div().flex().flex_col().gap(px(10.)).child(composer)),
            )
            .child(list);

        page("andrew-chat-page", &self.page_scroll)
            .child(header)
            .child(body)
            .into_any_element()
    }

    /// His conversations: the session list, one row each, the one open
    /// highlighted.
    fn render_andrew_session_list(
        &self,
        theme: &Theme,
        key: &str,
        cx: &mut Context<Root>,
    ) -> AnyElement {
        let picked = self.andrew_read.clone().unwrap_or_else(|| key.to_string());
        let mut rows: Vec<AnyElement> = Vec::new();
        match &self.andrew_sessions {
            None | Some(Load::Loading) => rows.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(12.5))
                    .text_color(theme.text_muted)
                    .child(spinner(
                        (SharedString::from(format!("mc-load-{key}")), key.len()),
                        11.,
                        theme.text_placeholder,
                    ))
                    .child("Reading his conversations…")
                    .into_any_element(),
            ),
            Some(Load::Failed(e)) if e == "not-configured" => {
                rows.push(
                    div()
                        .text_size(px(12.5))
                        .text_color(theme.text_placeholder)
                        .child(
                            "Andrew has no valley to talk through yet — set it in Settings › Valley.",
                        )
                        .into_any_element(),
                );
            }
            Some(Load::Failed(e)) => rows.push(
                div()
                    .text_size(px(12.5))
                    .text_color(theme.text_placeholder)
                    .child(format!("His conversations: {e}"))
                    .into_any_element(),
            ),
            Some(Load::Ready(list)) => {
                for row in list {
                    let id = row.session_id.clone();
                    let id_for_click = id.clone();
                    let meta = format!(
                        "{} · {} · {}",
                        row.phase.clone().unwrap_or_else(|| "chat".into()),
                        match row.message_count {
                            Some(n) => format!("{n} messages"),
                            None => "messages —".to_string(),
                        },
                        row.last_active.clone().unwrap_or_default(),
                    );
                    rows.push(
                        div()
                            .id(SharedString::from(format!("mc-sess-{id}")))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .px(px(8.))
                            .py(px(6.))
                            .rounded(kit::corner(7.))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.ghost_hover))
                            .when(picked == id, |d| d.bg(theme.element_selected))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .truncate()
                                    .child(
                                        row.preview
                                            .clone()
                                            .filter(|p| !p.trim().is_empty())
                                            .unwrap_or_else(|| row.session_id.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(theme.text_placeholder)
                                    .truncate()
                                    .child(meta),
                            )
                            .on_click(cx.listener(move |this, _e: &ClickEvent, _w: &mut Window, cx: &mut Context<Root>| {
                                this.andrew_select(id_for_click.clone(), cx);
                            }))
                            .into_any_element(),
                    );
                }
                if rows.is_empty() {
                    rows.push(
                        div()
                            .text_size(px(12.5))
                            .text_color(theme.text_placeholder)
                            .child("No conversations yet.")
                            .into_any_element(),
                    );
                }
            }
        }
        card(&theme)
            .id("mc-sess-list")
            .flex_none()
            .w(px(320.))
            .p(px(10.))
            .gap(px(6.))
            .max_h(px(520.))
            .overflow_y_scroll()
            .child(section_title(theme, "His conversations", None))
            .children(rows)
            .into_any_element()
    }
}
