//! Andrew's own settings, on the Valley tab: the one place where his
//! provider and tier models live (PUT /api/settings). Andrew validates tier
//! models against the provider's live catalog on every save that touches
//! one, and folds the report into the answer -- the note under Save shows
//! it. (Father, 18 Nov 2026: "they need to be able to call these APIs and
//! get information themselves... one control place and that's through
//! Andrew's settings page. AndruIDE will be expected to do the same config
//! page too.")

use crate::ui::input::TextInput;
use crate::ui::kit::{Button, ButtonStyle, card, section_title};
use crate::ui::theme::Theme;
use crate::valley::Valley;
use gpui::{Context, Entity, div, prelude::*, px};
use serde_json::json;

use super::SettingsPage;

impl SettingsPage {
    /// Reads Andrew's current settings and fills the page (once per tab
    /// visit; the valley's own connection settings are separate).
    pub(super) fn load_andrew_cfg(&mut self, cx: &mut Context<Self>) {
        if self.andrew_loading || self.andrew_saving {
            return;
        }
        self.andrew_loading = true;
        self.andrew_note = None;
        cx.notify();
        let valley = Valley::from_parts(self.engine.settings().valley.clone());
        let job = self
            .engine
            .runtime()
            .spawn(async move { valley.settings().await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                this.andrew_loading = false;
                match result {
                    Ok(s) => {
                        this.andrew_loaded = true;
                        this.andrew_note = None;
                        let fill = |input: &Entity<TextInput>, value: &Option<String>, cx: &mut Context<Self>| {
                            let text = value.clone().unwrap_or_default();
                            input.update(cx, |i, cx| i.set_text(text, cx));
                        };
                        fill(&this.andrew_provider, &s.provider, cx);
                        fill(&this.andrew_reasoning, &s.reasoning_model, cx);
                        fill(&this.andrew_simple, &s.simple_model, cx);
                        fill(&this.andrew_intent, &s.intent_model, cx);
                        fill(&this.andrew_work, &s.work_model, cx);
                    }
                    Err(e) => {
                        // Leave the page empty rather than inventing values;
                        // Save refuses until a real load succeeded.
                        this.andrew_loaded = false;
                        this.andrew_note = Some((false, e));
                    }
                }
                cx.notify();
            });
        });
        self._tasks.push(task);
    }

    /// Writes the page into Andrew's one place. An empty provider field
    /// deliberately omits the key (never wipes the valley's provider); an
    /// empty tier model posts "" (empty = that tier off / code default,
    /// Andrew's own contract). Andrew's save answer carries the live model
    /// validation, shown as the note.
    fn save_andrew(&mut self, cx: &mut Context<Self>) {
        if self.andrew_saving || self.andrew_loading {
            return;
        }
        if !self.andrew_loaded {
            self.andrew_note = Some((
                false,
                "Andrew's settings haven't loaded -- check the Valley connection above, then reopen this tab.".into(),
            ));
            cx.notify();
            return;
        }
        self.andrew_saving = true;
        self.andrew_note = None;
        cx.notify();
        let mut patch = json!({
            "reasoning_model": self.andrew_reasoning.read(cx).text().trim().to_string(),
            "simple_model": self.andrew_simple.read(cx).text().trim().to_string(),
            "intent_model": self.andrew_intent.read(cx).text().trim().to_string(),
            "work_model": self.andrew_work.read(cx).text().trim().to_string(),
        });
        let provider = self.andrew_provider.read(cx).text().trim().to_string();
        if !provider.is_empty() {
            patch["provider"] = json!(provider);
        }
        let valley = Valley::from_parts(self.engine.settings().valley.clone());
        let job = self
            .engine
            .runtime()
            .spawn(async move { valley.save_settings(patch).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                this.andrew_saving = false;
                match result {
                    Ok(answer) => {
                        this.andrew_note =
                            Some((true, crate::valley::ValleySettings::save_note(&answer)));
                    }
                    Err(e) => {
                        this.andrew_note = Some((false, e));
                    }
                }
                // Refresh what Andrew now holds (validation may have cleared
                // a stale tier id; the page must show the valley's truth).
                this.load_andrew_cfg(cx);
                cx.notify();
            });
        });
        self._tasks.push(task);
    }

    /// Andrew's config: provider + the four tier models, one Save.
    pub(super) fn andrew_section(
        &self,
        theme: &Theme,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        // Same fixed-width wrapper save_valley uses (a flex_1 wrapper that
        // isn't a flex container collapses a TextInput to ~15px; Father's
        // 11 Nov 2026 report).
        let row = |label: &'static str, input: &Entity<TextInput>| {
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .px(px(14.))
                .py(px(10.))
                .child(div().flex_1().text_size(px(13.)).child(label))
                .child(div().w(px(300.)).flex_none().flex().flex_col().child(input.clone()))
        };
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(section_title(theme, "Andrew's config", None))
            .child(
                card(theme)
                    .child(
                        div()
                            .w_full()
                            .min_w(px(0.))
                            .text_size(px(12.))
                            .text_color(theme.text_placeholder)
                            .child("Andrew's one place: his provider drives every lane, and each tier keeps its own model. Empty tier fields save as empty (that tier off / its code default); the provider never clears. Andrew validates tier models against the provider's catalog on every save."),
                    )
                    .child(row("Provider", &self.andrew_provider))
                    .child(row("Reasoning model", &self.andrew_reasoning))
                    .child(row("Simple model", &self.andrew_simple))
                    .child(row("Intent model", &self.andrew_intent))
                    .child(row("Work model", &self.andrew_work)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        Button::new("save-andrew", "Save")
                            .style(ButtonStyle::Primary)
                            .on_click(cx.listener(|this, _, _, cx| this.save_andrew(cx))),
                    )
                    .children(self.andrew_note.clone().map(|(ok, text)| {
                        div()
                            .text_size(px(12.))
                            .text_color(if ok { theme.success } else { theme.error })
                            .child(text)
                    })),
            )
    }
}
