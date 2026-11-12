//! Valley: Andrew's work board, read live from his REST API (see
//! src/valley.rs for the client). Lanes as chips — click one and the list
//! below shows what's in it; "all" shows the whole board.

use std::time::Instant;

use gpui::{AnyElement, Context, FontWeight, SharedString, Window, div, prelude::*, px};

use super::github::Load;
use super::input::TextInput;
use super::kit::{self, Button, ButtonStyle, card, spinner};
use super::overview::page;
use super::root::{Root, Route};
use super::theme::{ActiveTheme, mono_font};
use crate::valley::{LANES, Valley};

/// The page's sidebar tile colour: campfire orange.
pub const TILE: (u32, u32) = (0xffb35c, 0xf0703f);

impl Root {
    /// Reads Andrew's board for the page.
    pub(super) fn load_valley(&mut self, cx: &mut Context<Self>) {
        self.valley_data = Some(Load::Loading);
        self.valley_at = Some(Instant::now());
        // A valley picked from the network is viewed here for the session;
        // Settings › Valley keeps the durable URL. (Father, 11 Nov 2026.)
        let mut cfg = self.engine.settings().valley.clone();
        if let Some(base) = self.valley_view.clone() {
            cfg.base_url = base;
        }
        let valley = Valley::from_parts(cfg);
        if !valley.configured() {
            self.valley_data = Some(Load::Failed("not-configured".into()));
            cx.notify();
            return;
        }
        let job = self.engine.runtime().spawn(async move { valley.status().await });
        let task = cx.spawn(async move |this, cx| {
            // The spawn yields Result<Result<ValleyStatus, String>, JoinError>; flatten it.
            let result = job
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r);
            let _ = this.update(cx, |this, cx| {
                this.valley_data = Some(match result {
                    Ok(v) => Load::Ready(v),
                    Err(e) => Load::Failed(e),
                });
                cx.notify();
            });
        });
        self.track(task);
    }

    /// Stops one run from the board's inline button: cancel the run, then
    /// re-read the board. (Father, 12 Nov 2026: "we'll be able to control
    /// there those from the valley too".)
    fn valley_stop_item(&mut self, run_id: String, cx: &mut Context<Self>) {
        let mut cfg = self.engine.settings().valley.clone();
        if let Some(base) = self.valley_view.clone() {
            cfg.base_url = base;
        }
        let valley = Valley::from_parts(cfg);
        let job = self.engine.runtime().spawn(async move { valley.cancel_run(&run_id).await });
        let task = cx.spawn(async move |this, cx| {
            let result = job.await.unwrap_or_else(|e| Err(e.to_string()));
            let _ = this.update(cx, |this, cx| match result {
                Ok(()) => {
                    this.notify_user("The run stopped.", false, cx);
                    this.valley_data = None;
                    this.load_valley(cx);
                }
                Err(e) => this.notify_user(SharedString::from(e), true, cx),
            });
        });
        self.track(task);
    }

    /// The sidebar's "Valleys on the network": a fresh mDNS listen on the
    /// blocking thread; empty when nothing answers (no valley running).
    pub(super) fn scan_valleys(&mut self, cx: &mut Context<Self>) {
        self.valleys_scanning = true;
        self.valleys_at = Some(Instant::now());
        let job = self.engine.runtime().spawn_blocking(|| {
            crate::fleet::discover::browse_valleys(std::time::Duration::from_millis(2000))
        });
        let task = cx.spawn(async move |this, cx| {
            let found = job.await.unwrap_or_default();
            let _ = this.update(cx, |this, cx| {
                this.valleys = found;
                this.valleys_scanning = false;
                cx.notify();
            });
        });
        self.track(task);
    }

    /// Scans when the last listen is old enough (5 min) and none is running;
    /// cheap enough to call every sidebar render.
    pub(super) fn maybe_scan_valleys(&mut self, cx: &mut Context<Self>) {
        if !self.valleys_scanning && self.valleys_at.is_none_or(|t| t.elapsed().as_secs() > 300) {
            self.scan_valleys(cx);
        }
    }

    /// Opens a network valley in the Valley page for this session.
    pub(super) fn open_valley(&mut self, base: String, window: &mut Window, cx: &mut Context<Self>) {
        self.valley_view = Some(base);
        self.navigate(Route::Valley, window, cx);
    }

    /// Applies a pasted Valley invite from the not-configured card: writes
    /// it into Settings › Valley (durable) and reads the board right away.
    pub(super) fn join_valley(&mut self, cx: &mut Context<Self>) {
        let text = self
            .valley_join_input
            .as_ref()
            .map(|i| i.read(cx).text().trim().to_string())
            .unwrap_or_default();
        let Some(parts) = crate::valley::parse_join(&text) else {
            self.notify_user("That doesn't look like a valley invite.", true, cx);
            return;
        };
        let mut s = self.engine.settings();
        s.valley.base_url = parts.base;
        s.valley.user = parts.user;
        s.valley.password = parts.password;
        if let Err(e) = self.engine.update_settings(s) {
            self.notify_user(SharedString::from(e), true, cx);
            return;
        }
        // A join is durable: leave any network valley behind and read fresh.
        self.valley_view = None;
        self.valley_at = None;
        self.load_valley(cx);
        self.notify_user("Joined the valley — reading its board now.", false, cx);
    }

    /// How many items are running, for the sidebar badge.
    pub(super) fn valley_badge(&self) -> Option<usize> {
        match &self.valley_data {
            Some(Load::Ready(data)) => Some(data.items.iter().filter(|i| i.status == "running").count()),
            _ => None,
        }
    }

    pub(super) fn render_valley(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        // First visit, or stale after five minutes: look again. The work
        // board is a heavy read and slow under pool load (Father, 12 Nov
        // 2026: "reading Andrew's workboard is really slow for the valley
        // tab"); the lite read is filed on Andrew's board as its own item,
        // until then this page leans on its cache and the refresh button.
        if self.valley_data.is_none() || self.valley_at.is_some_and(|t| t.elapsed().as_secs() > 300) {
            self.load_valley(cx);
        }
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
                    .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child("Valley"))
                    .child(div().text_size(px(13.)).text_color(theme.text_muted).child("Andrew's work board, read live from his API. How many items wait in each lane, and what his workers hold now."))
                    .when_some(self.valley_view.clone(), |d, base| {
                        // Click the line to leave the network valley again.
                        d.child(
                            div()
                                .id("valley-viewing")
                                .text_size(px(13.))
                                .font_family(mono_font())
                                .text_color(theme.text_muted)
                                .cursor_pointer()
                                .hover(|st| st.text_color(theme.text))
                                .child(format!("Looking at {base} on the network — click to go back to your valley."))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.valley_view = None;
                                    this.load_valley(cx);
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .when_some(self.valley_data.as_ref().and_then(|d| match d {
                        Load::Ready(data) => data.ready.as_ref(),
                        _ => None,
                    }), |d, ready| {
                        d.child(
                            div()
                                .px(px(8.))
                                .h(px(24.))
                                .flex()
                                .items_center()
                                .gap(px(4.))
                                .rounded(kit::corner(7.))
                                .text_size(px(12.))
                                .text_color(theme.text_muted)
                                .bg(theme.element)
                                .child(format!("{} · {}{}", ready.service, ready.status, if ready.draining { " · draining" } else { "" })),
                        )
                    })
                    .child(Button::icon_only("valley-refresh", "refresh-cw").tooltip("Read the board again").on_click(cx.listener(|this, _, _, cx| this.load_valley(cx)))),
            );
        let mut body = page("valley-page", &self.page_scroll).child(header);
        // Join-a-valley (Father, 12 Nov 2026: "Join a valley" first run):
        // the paste box is created once, before the state match borrows it.
        let need_join = matches!(self.valley_data.as_ref(), Some(Load::Failed(e)) if e == "not-configured");
        let join_input = if need_join {
            Some(self
                .valley_join_input
                .get_or_insert_with(|| cx.new(|cx| TextInput::new(cx, "andruide://join?base=…&user=…&password=…").mono()))
                .clone())
        } else {
            None
        };
        let data = match &self.valley_data {
            Some(Load::Ready(data)) => data,
            Some(Load::Loading) => {
                return body
                    .child(card(&theme).p(px(18.)).flex_row().items_center().gap(px(10.)).text_size(px(13.)).text_color(theme.text_muted).child(spinner("valley-loading", 14., theme.text_muted)).child("Reading Andrew's board…"))
                    .into_any_element();
            }
            Some(Load::Failed(e)) if e == "not-configured" => {
                let input = join_input.expect("pre-created just above for the not-configured arm");
                return body
                    .child(card(&theme).p(px(18.)).flex_col().gap(px(10.))
                        .child(div().text_size(px(13.)).child("The valley isn't configured in this app yet."))
                        .child(div().text_size(px(12.)).text_color(theme.text_muted).child("Paste a valley invite below — the join link or QR the valley shows you — and press Join."))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(div().w(px(360.)).flex_none().flex().flex_col().child(input))
                                .child(
                                    Button::new("valley-join", "Join the valley")
                                        .style(ButtonStyle::Primary)
                                        .on_click(cx.listener(|this, _, _, cx| this.join_valley(cx))),
                                ),
                        )
                        .child(div().text_size(px(11.)).text_color(theme.text_placeholder).child("Or set ANDRUIDE_VALLEY_URL / _USER / _PASSWORD before starting the app; Settings › Valley holds them either way.")))
                    .into_any_element();
            }
            Some(Load::Failed(e)) => {
                return body
                    .child(card(&theme).p(px(18.)).flex_row().items_center().gap(px(10.)).text_size(px(13.)).text_color(theme.text_muted).child(e.clone()))
                    .into_any_element();
            }
            None => return body.into_any_element(),
        };
        // A bare valley (contract v1 capabilities without the work tier)
        // has no board to read: say so gracefully instead of failing the
        // page. (Father, 12 Nov 2026, on Valley A: "it doesn't have an
        // API work endpoint... it's just a campfire valley".)
        if let Some(ready) = &data.ready {
            if !ready.has_work_tier() {
                return body
                    .child(
                        card(&theme)
                            .p(px(18.))
                            .flex_col()
                            .gap(px(10.))
                            .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child("This valley doesn't run a work board."))
                            .child(div().text_size(px(12.)).text_color(theme.text_muted).child("Its campers take work by handoff — watch them at the campfire instead."))
                            .child(
                                Button::new("valley-open-campfires", "Open Campfires")
                                    .style(ButtonStyle::Primary)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.navigate(Route::Campfires, window, cx)
                                    })),
                            ),
                    )
                    .into_any_element();
            }
        }
        // The lanes, as chips, count over label. Clicking a chip opens its
        // lane below; "all" opens the whole board. (Father, 12 Nov 2026:
        // "statistics at the top for running blocked. Pending complete
        // stopped and failed. click those and the list below actually
        // shows what's in there. And there should be a one for all.") The
        // list defaults to the running lane, as it always has.
        // The projects, as a chip row: the board reads one project at a
        // time, defaulting to Andrew's. (Father, 12 Nov 2026: "we need to
        // look at the different projects which are within that valley and
        // group our jobs by project. So the default one will be Andrew's
        // project".)
        let mut projects: Vec<String> = data
            .items
            .iter()
            .map(|i| i.project.clone())
            .filter(|p| !p.is_empty())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        if let Some(idx) = projects.iter().position(|p| p == "andrew") {
            let andrew = projects.remove(idx);
            projects.insert(0, andrew);
        }
        let project_sel: Option<String> = match &self.valley_project {
            Some(p) => Some(p.clone()),
            None if projects.iter().any(|p| p == "andrew") => Some("andrew".to_string()),
            None => None,
        };
        if !projects.is_empty() {
            body = body.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .children(
                        std::iter::once("all".to_string())
                            .chain(projects)
                            .map(|project| {
                                let on = project_sel.as_deref() == Some(project.as_str());
                                let proj = project.clone();
                                div()
                                    .id(SharedString::from(format!("valley-project-{project}")))
                                    .px(px(10.))
                                    .py(px(6.))
                                    .flex()
                                    .items_center()
                                    .rounded(kit::corner(7.))
                                    .cursor_pointer()
                                    .text_size(px(12.))
                                    .bg(if on { theme.element_selected } else { theme.element })
                                    .hover(|st| st.bg(theme.element_selected))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.valley_project = Some(proj.clone());
                                        cx.notify();
                                    }))
                                    .child(project)
                            }),
                    ),
            );
        }
        let counts = crate::valley::counts(&data.items);
        let selected = self.valley_lane.clone().unwrap_or_else(|| "running".to_string());
        let show_status = selected == "all";
        let mut lanes: Vec<(String, usize)> = vec![("all".to_string(), data.items.len())];
        lanes.extend(LANES.iter().map(|l| (l.to_string(), counts.get(*l).copied().unwrap_or(0))));
        for (status, n) in &counts {
            if !LANES.contains(&status.as_str()) {
                lanes.push((status.clone(), *n));
            }
        }
        body = body.child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(8.))
                .children(lanes.into_iter().map(|(status, n)| {
                    let on = status == selected;
                    let lane = status.clone();
                    div()
                        .id(SharedString::from(format!("valley-lane-{status}")))
                        .px(px(12.))
                        .py(px(8.))
                        .min_w(px(92.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .rounded(kit::corner(9.))
                        .cursor_pointer()
                        .bg(if on { theme.element_selected } else { theme.element })
                        .hover(|st| st.bg(theme.element_selected))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.valley_lane = Some(lane.clone());
                            cx.notify();
                        }))
                        .child(div().text_size(px(18.)).font_weight(FontWeight::SEMIBOLD).child(n.to_string()))
                        .child(div().text_size(px(11.5)).text_color(theme.text_muted).child(status.clone()))
                })),
        );
        // The selected lane's items ("all" = the whole board), in the
        // claim order the server already sent them, narrowed to the
        // selected project (the lane chips keep whole-board counts; the
        // list is what filters). Honest v1 — noted in the PR.
        let items: Vec<&crate::valley::WorkItem> = data
            .items
            .iter()
            .filter(|i| selected == "all" || i.status == selected)
            .filter(|i| match &project_sel {
                Some(p) if p != "all" => i.project == *p,
                _ => true,
            })
            .collect();
        if items.is_empty() {
            let on_project = project_sel.as_deref().filter(|p| *p != "all");
            let note = if selected == "all" {
                match on_project {
                    Some(p) => format!("Nothing from the {p} project on the board right now."),
                    None => "The board is empty right now.".to_string(),
                }
            } else if selected == "running" {
                match on_project {
                    Some(p) => format!("Nothing from the {p} project is running right now."),
                    None => "Nothing is running on the board right now.".to_string(),
                }
            } else {
                match on_project {
                    Some(p) => format!("Nothing from the {p} project in the {selected} lane right now."),
                    None => format!("Nothing in the {selected} lane right now."),
                }
            };
            body = body.child(
                card(&theme).p(px(18.)).flex_row().items_center().gap(px(10.)).text_size(px(13.)).text_color(theme.text_muted).child(note),
            );
        } else {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .children(items.into_iter().map(|item| {
                        // Three levels, Father 12 Nov 2026: the card opens the
                        // item's own page (detail + the job's conversation).
                        let open_id = item.id.clone();
                        // The inline stop rides running rows only, and only
                        // when the item names its claim run. (Father, 12 Nov
                        // 2026: "we'll be able to control there those from
                        // the valley too".)
                        let stop_run = if item.status == "running" { item.claim_run() } else { None };
                        card(&theme)
                            .id(SharedString::from(format!("valley-item-{}", item.id)))
                            .p(px(14.))
                            .flex_col()
                            .gap(px(4.))
                            .cursor_pointer()
                            .hover(|st| st.bg(theme.element_selected))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.navigate(Route::ValleyItem(open_id.clone()), window, cx)
                            }))
                            .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(item.title.clone()))
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
                                    .when(show_status, |d| d.child(item.status.clone()))
                                    .child(format!("p{}", item.priority))
                                    .when(stop_run.is_some(), |d| {
                                        let run_id = stop_run.clone().expect("checked is_some just above").to_string();
                                        d.child(div().flex_1()).child(
                                            Button::new(SharedString::from(format!("valley-stop-{}", item.id)), "Stop")
                                                .icon("square")
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    // The row is clickable too: keep the
                                                    // stop click from also opening the page.
                                                    cx.stop_propagation();
                                                    this.valley_stop_item(run_id.clone(), cx);
                                                })),
                                        )
                                    }),
                            )
                    })),
            );
        }
        body.into_any_element()
    }
}
