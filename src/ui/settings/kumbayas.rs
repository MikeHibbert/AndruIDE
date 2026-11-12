//! Settings › Prompts, second card: Andrew's kumbayas — the ceremonies his
//! songbook carries. Observe them (list, detail, each verse's prompt) and
//! modify the summary or a verse's prompt; saving round-trips the whole
//! definition through POST /kumbayas, which is his songbook's own upsert (it
//! bumps the version and marks dependents outdated), so nothing is lost.
//! (Father, 13 Nov 2026: "On the settings prompts page where it says session
//! actions, we can put our kumbays there. And then they can be observed and
//! modified if needs be from there.")

use std::time::Instant;

use gpui::{AnyElement, Context, Entity, SharedString, div, prelude::*, px};

use super::SettingsPage;
use crate::ui::github::Load;
use crate::ui::input::TextInput;
use crate::ui::kit::{self, Button, ButtonStyle, card, section_title};
use crate::ui::theme::{mono_font, Theme};
use crate::valley::{KumbayaSummary, Valley};

/// One ceremony, opened: its name and the whole definition as the valley
/// holds it, so a save carries every field we didn't touch.
pub(super) struct KumbayaView {
    name: String,
    value: serde_json::Value,
}

/// A summary or one verse's prompt being changed.
pub(super) struct KumbayaEdit {
    target: KumbayaEditTarget,
    input: Entity<TextInput>,
}

enum KumbayaEditTarget {
    Summary,
    /// A verse, by its index in `steps`.
    Step(usize),
}

impl SettingsPage {
    /// Reads the songbook list (when the Prompts tab opens, and after every
    /// save or retire).
    pub(super) fn load_kumbayas(&mut self, cx: &mut Context<Self>) {
        if matches!(self.kumbayas, Some(Load::Loading)) {
            return;
        }
        self.kumbayas = Some(Load::Loading);
        self.kumbaya_at = Some(Instant::now());
        let valley = Valley::from_parts(self.engine.settings().valley.clone());
        let job = self.engine.runtime().spawn(async move { valley.kumbayas().await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                this.kumbayas = Some(match result {
                    Ok(list) => Load::Ready(list),
                    Err(e) => Load::Failed(e),
                });
                cx.notify()
            });
        });
        self._tasks.push(task);
    }

    /// Reads one ceremony's full definition.
    fn open_kumbaya(&mut self, name: String, cx: &mut Context<Self>) {
        self.kumbaya_arm = None;
        self.kumbaya_edit = None;
        self.kumbaya_open_name = Some(name.clone());
        self.kumbaya_open = None;
        cx.notify();
        let valley = Valley::from_parts(self.engine.settings().valley.clone());
        // The name goes along on the fetch and into the view: clone one for
        // each move.
        let fetch = name.clone();
        let job = self
            .engine
            .runtime()
            .spawn(async move { valley.kumbaya_raw(&fetch).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                this.kumbaya_open = Some(match result {
                    Ok(value) => Ok(KumbayaView { name, value }),
                    Err(e) => Err(e),
                });
                cx.notify()
            });
        });
        self._tasks.push(task);
    }

    fn close_kumbaya(&mut self, cx: &mut Context<Self>) {
        self.kumbaya_open_name = None;
        self.kumbaya_open = None;
        self.kumbaya_edit = None;
        self.kumbaya_arm = None;
        cx.notify();
    }

    /// Starts changing the summary (its current text prefills the field).
    fn edit_summary(&mut self, cx: &mut Context<Self>) {
        let text = self.kumbaya_summary_text().unwrap_or_default();
        let input = cx.new(|cx| TextInput::new(cx, "What the ceremony is for").with_text(text));
        self.kumbaya_edit = Some(KumbayaEdit { target: KumbayaEditTarget::Summary, input });
        cx.notify();
    }

    /// Starts changing one verse's prompt (its current text prefills).
    fn edit_step(&mut self, index: usize, cx: &mut Context<Self>) {
        let text = self.kumbaya_step_text(index);
        let input = cx.new(|cx| TextInput::new(cx, "The verse's prompt").mono().with_text(text));
        self.kumbaya_edit = Some(KumbayaEdit { target: KumbayaEditTarget::Step(index), input });
        cx.notify();
    }

    fn kumbaya_summary_text(&self) -> Option<String> {
        match &self.kumbaya_open {
            Some(Ok(v)) => v.summary_text(),
            _ => None,
        }
    }

    fn kumbaya_step_text(&self, index: usize) -> String {
        match &self.kumbaya_open {
            Some(Ok(v)) => v
                .value
                .get("steps")
                .and_then(|s| s.get(index))
                .and_then(|s| s.get("prompt"))
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string(),
            _ => String::new(),
        }
    }

    /// Saves the summary or verse prompt: the whole definition round-trips
    /// through his POST /kumbayas (the songbook's own upsert), unchanged
    /// fields included.
    fn save_kumbaya_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.kumbaya_edit.take() else { return; };
        let text = edit.input.read(cx).text().trim().to_string();
        let Some(Ok(view)) = &self.kumbaya_open else { return; };
        let name = view.name.clone();
        let mut value = view.value.clone();
        match edit.target {
            KumbayaEditTarget::Summary => value["summary"] = serde_json::Value::String(text),
            KumbayaEditTarget::Step(i) => {
                let Some(step) = value.get_mut("steps").and_then(|s| s.get_mut(i)) else {
                    return;
                };
                step["prompt"] = serde_json::Value::String(text);
            }
        }
        let valley = Valley::from_parts(self.engine.settings().valley.clone());
        let job = self
            .engine
            .runtime()
            .spawn(async move { valley.save_kumbaya(value).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| match result {
                Ok(()) => {
                    this.saved_at = Some(Instant::now());
                    this.save_error = None;
                    this.open_kumbaya(name, cx);
                    this.load_kumbayas(cx);
                    cx.notify();
                }
                Err(e) => {
                    this.save_error = Some(e);
                    cx.notify();
                }
            });
        });
        self._tasks.push(task);
    }

    /// Arms the retire (a second click confirms — a ceremony isn't deleted
    /// by a slip of the finger).
    fn arm_retire(&mut self, name: String, cx: &mut Context<Self>) {
        self.kumbaya_arm = Some(name);
        cx.notify();
    }

    /// Retires the ceremony through his DELETE /kumbayas/{name}.
    fn retire_kumbaya(&mut self, name: String, cx: &mut Context<Self>) {
        let name_for_check = name.clone();
        let valley = Valley::from_parts(self.engine.settings().valley.clone());
        let job = self
            .engine
            .runtime()
            .spawn(async move { valley.delete_kumbaya(&name).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| {
                this.kumbaya_arm = None;
                match result {
                    Ok(()) => {
                        this.saved_at = Some(Instant::now());
                        this.save_error = None;
                        if this.kumbaya_open_name.as_deref() == Some(&name_for_check) {
                            this.kumbaya_open_name = None;
                            this.kumbaya_open = None;
                            this.kumbaya_edit = None;
                        }
                        this.load_kumbayas(cx);
                        cx.notify();
                    }
                    Err(e) => {
                        this.save_error = Some(e);
                        cx.notify();
                    }
                }
            });
        });
        self._tasks.push(task);
    }

    /// The kumbayas card, under the session actions.
    pub(super) fn kumbayas_section(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let configured = self.engines_valley_configured();
        let mut body = div().flex().flex_col().gap(px(10.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().flex_1().child(section_title(theme, "Kumbayas", None)))
                .when(configured, |d| {
                    d.child(
                        Button::icon_only("kb-refresh", "refresh-cw")
                            .tooltip("Read the ceremonies again")
                            .on_click(cx.listener(|this, _, _, cx| this.load_kumbayas(cx))),
                    )
                }),
        );
        if !configured {
            body = body.child(
                card(theme).px(px(14.)).py(px(12.)).child(note(theme, "Andrew's valley isn't configured in this app yet — set it in Settings › Valley.")),
            );
            return body;
        }
        let list: AnyElement = match &self.kumbayas {
            None | Some(Load::Loading) => div()
                .px(px(14.))
                .py(px(12.))
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(12.5))
                .text_color(theme.text_muted)
                .child(kit::spinner("kb-load", 12., theme.text_muted))
                .child("Reading Andrew's ceremonies…")
                .into_any_element(),
            Some(Load::Failed(e)) if e == "not-configured" => card(theme)
                .px(px(14.))
                .py(px(12.))
                .child(note(theme, "Andrew's valley isn't answering — set it in Settings › Valley."))
                .into_any_element(),
            Some(Load::Failed(e)) => card(theme)
                .px(px(14.))
                .py(px(12.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(note(theme, "The ceremonies wouldn't read:"))
                .child(div().text_size(px(12.)).text_color(theme.error).child(e.clone()))
                .child(Button::new("kb-retry", "Try again").icon("refresh-cw").on_click(cx.listener(|this, _, _, cx| this.load_kumbayas(cx))))
                .into_any_element(),
            Some(Load::Ready(list)) => {
                if list.is_empty() {
                    card(theme).px(px(14.)).py(px(12.)).child(note(theme, "Andrew's songbook carries no ceremonies yet.")).into_any_element()
                } else {
                    let rows: Vec<AnyElement> = list
                        .iter()
                        .map(|c| kumbaya_row(theme, c, self.kumbaya_arm.as_deref() == Some(c.name.as_str()), cx))
                        .collect();
                    card(theme).children(rows).into_any_element()
                }
            }
        };
        body = body.child(list);

        // The ceremony opened: its whole definition, readable and editable.
        if let Some(open_name) = &self.kumbaya_open_name {
            let view: AnyElement = match &self.kumbaya_open {
                None => div()
                    .px(px(14.))
                    .py(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(12.5))
                    .text_color(theme.text_muted)
                    .child(kit::spinner("kb-view", 12., theme.text_muted))
                    .child(format!("Reading {open_name}…"))
                    .into_any_element(),
                Some(Err(e)) => div().px(px(14.)).py(px(10.)).text_size(px(12.)).text_color(theme.error).child(e.clone()).into_any_element(),
                Some(Ok(view)) => self.render_kumbaya_view(theme, view, cx),
            };
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .font_family(mono_font())
                                    .text_size(px(13.))
                                    .text_color(theme.text_muted)
                                    .child(open_name.clone()),
                            )
                            .child(Button::new("kb-close", "Close").style(ButtonStyle::Ghost).on_click(cx.listener(|this, _, _, cx| this.close_kumbaya(cx)))),
                    )
                    .child(card(theme).child(view)),
            );
        }
        if self.saved_at.is_some() {
            body = body.child(div().text_size(px(12.)).text_color(theme.success).child("Saved."));
        }
        if let Some(e) = &self.save_error {
            body = body.child(div().text_size(px(12.)).text_color(theme.error).child(e.clone()));
        }
        body
    }

    fn engines_valley_configured(&self) -> bool {
        !self.engine.settings().valley.base_url.trim().is_empty()
    }

    /// The opened ceremony: summary and every verse, each editable when it's
    /// a language-model verse (its prompt is the thing Father changes).
    fn render_kumbaya_view(&self, theme: &Theme, view: &KumbayaView, cx: &mut Context<Self>) -> AnyElement {
        let steps: &[serde_json::Value] = view
            .value
            .get("steps")
            .and_then(|s| s.as_array())
            .map(|s| s.as_slice())
            .unwrap_or(&[]);
        let summary_editing = matches!(
            self.kumbaya_edit.as_ref().map(|e| &e.target),
            Some(KumbayaEditTarget::Summary)
        );
        let mut body = div().flex().flex_col().gap(px(10.)).px(px(14.)).py(px(12.)).child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().flex_1().text_size(px(14.)).child(view.name.clone()))
                .when_some(view.value.get("version").and_then(|v| v.as_i64()), |d, v| {
                    d.child(
                        div()
                            .font_family(mono_font())
                            .text_size(px(11.))
                            .text_color(theme.text_placeholder)
                            .child(format!("v{v}")),
                    )
                }),
        );

        // The summary, with its edit pair.
        body = body.child(
            div().flex().flex_col().gap(px(6.)).child(div().text_size(px(11.)).text_color(theme.text_placeholder).child("Summary")).child(
                match (summary_editing, self.kumbaya_edit.as_ref().map(|e| e.input.clone())) {
                    (true, Some(input)) => div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(input)
                        .child(
                            div().flex().gap(px(6.)).child(
                                Button::new("kb-save-summary", "Save")
                                    .style(ButtonStyle::Primary)
                                    .on_click(cx.listener(|this, _, _, cx| this.save_kumbaya_edit(cx))),
                            ),
                        )
                        .into_any_element(),
                    (false, _) => div()
                        .flex()
                        .items_start()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .text_size(px(12.5))
                                .text_color(theme.text_muted)
                                .child(view.summary_text().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "(no summary written)".into())),
                        )
                        .child(
                            Button::new("kb-edit-summary", "Edit")
                                .style(ButtonStyle::Ghost)
                                .on_click(cx.listener(|this, _, _, cx| this.edit_summary(cx))),
                        )
                        .into_any_element(),
                    _ => note(theme, "(no summary written)").into_any_element(),
                },
            ),
        );

        // The verses.
        body = body.child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(div().text_size(px(11.)).text_color(theme.text_placeholder).child(format!("Verses ({})", steps.len()))),
        );
        for (i, step) in steps.iter().enumerate() {
            let label = step
                .get("verse")
                .and_then(|v| v.as_str())
                .or_else(|| step.get("name").and_then(|v| v.as_str()))
                .unwrap_or("verse");
            let kind = step.get("kind").and_then(|k| k.as_str()).unwrap_or("");
            let human = step.get("requires_human").and_then(|r| r.as_bool()).unwrap_or(false);
            let mut row = div().flex().flex_col().gap(px(4.)).px(px(10.)).py(px(8.)).rounded(crate::ui::kit::corner(8.)).border_1().border_color(theme.border_variant).child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().min_w(px(0.)).truncate().font_family(mono_font()).text_size(px(12.5)).child(label.to_string()))
                    .when(!kind.is_empty(), |d| {
                        d.child(div().text_size(px(10.5)).text_color(theme.text_placeholder).child(kind.to_string()))
                    })
                    .when(human, |d| d.child(div().text_size(px(10.5)).text_color(theme.warning).child("needs you"))),
            );
            if kind == "llm" {
                let editing_here = self
                    .kumbaya_edit
                    .as_ref()
                    .and_then(|e| match &e.target {
                        KumbayaEditTarget::Step(n) if *n == i => Some(e.input.clone()),
                        _ => None,
                    });
                let control: AnyElement = match editing_here {
                    Some(input) => div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(input)
                        .child(
                            div()
                                .flex()
                                .gap(px(6.))
                                .child(
                                    Button::new(SharedString::from(format!("kb-save-step-{i}")), "Save")
                                        .style(ButtonStyle::Primary)
                                        .on_click(cx.listener(|this, _, _, cx| this.save_kumbaya_edit(cx))),
                                )
                                .child(
                                    Button::new(SharedString::from(format!("kb-cancel-step-{i}")), "Cancel")
                                        .style(ButtonStyle::Ghost)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.kumbaya_edit = None;
                                            cx.notify();
                                        })),
                                ),
                        )
                        .into_any_element(),
                    None => {
                        let text = step.get("prompt").and_then(|p| p.as_str()).unwrap_or("");
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(div().w_full().text_size(px(12.)).text_color(theme.text_muted).child(crate::ui::valley_item::clipped(text, 400)))
                            .child(
                                Button::new(SharedString::from(format!("kb-edit-step-{i}")), "Edit verse")
                                    .style(ButtonStyle::Ghost)
                                    .on_click(cx.listener(move |this, _, _, cx| this.edit_step(i, cx))),
                            )
                            .into_any_element()
                    }
                };
                row = row.child(control);
            }
            body = body.child(row);
        }
        body.into_any_element()
    }
}

impl KumbayaView {
    fn summary_text(&self) -> Option<String> {
        self.value.get("summary").and_then(|s| s.as_str()).map(str::to_string)
    }
}

/// One row of the songbook list: name, version, status, summary, and the
/// view/retire pair.
fn kumbaya_row(
    theme: &Theme,
    c: &KumbayaSummary,
    armed: bool,
    cx: &mut Context<SettingsPage>,
) -> AnyElement {
    let open_name = c.name.clone();
    let open = cx.listener(move |this, _, _, cx| {
        this.open_kumbaya(open_name.clone(), cx);
    });
    let arm_name = c.name.clone();
    let arm = cx.listener(move |this, _, _, cx| {
        this.arm_retire(arm_name.clone(), cx);
    });
    let confirm_name = c.name.clone();
    let confirm = cx.listener(move |this, _, _, cx| {
        this.retire_kumbaya(confirm_name.clone(), cx);
    });
    div()
        .id(SharedString::from(format!("kb-row-{}", c.name)))
        .flex()
        .items_center()
        .gap(px(10.))
        .px(px(14.))
        .py(px(9.))
        .border_t_1()
        .border_color(theme.border_variant)
        .cursor_pointer()
        .hover(|st| st.bg(theme.element))
        .child(kit::icon("flame", 14., theme.accent))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(div().text_size(px(13.)).child(c.name.clone()))
                        .when_some(c.version, |d, v| {
                            d.child(
                                div()
                                    .font_family(mono_font())
                                    .text_size(px(10.5))
                                    .text_color(theme.text_placeholder)
                                    .child(format!("v{v}")),
                            )
                        })
                        .when_some(c.status.clone(), |d, st| {
                            d.child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(match st.as_str() {
                                        "ready" => theme.success,
                                        _ => theme.text_placeholder,
                                    })
                                    .child(st),
                            )
                        }),
                )
                .when_some(c.summary.clone().filter(|s| !s.trim().is_empty()), |d, s| {
                    d.child(div().truncate().text_size(px(12.)).text_color(theme.text_muted).child(s))
                }),
        )
        .child(Button::new(SharedString::from(format!("kb-view-{}", c.name)), "View").style(ButtonStyle::Ghost).on_click(open))
        .child(if armed {
            Button::new(SharedString::from(format!("kb-retire-confirm-{}", c.name)), "Confirm delete")
                .style(ButtonStyle::Primary)
                .on_click(confirm)
        } else {
            Button::new(SharedString::from(format!("kb-retire-{}", c.name)), "Retire").style(ButtonStyle::Ghost).on_click(arm)
        })
        .into_any_element()
}

fn note(theme: &Theme, text: &'static str) -> gpui::Div {
    div().text_size(px(12.)).text_color(theme.text_placeholder).child(text)
}
