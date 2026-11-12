//! The Valley - Andrew's campfire valley as a live 3d scene, on gpui canvas.
//!
//! Father, 13 Nov 2026: a three-dimensional window on the valley, the dock and
//! everything, like the old webpage - and "Approved. Go ahead." on increment
//! one. The projection, shading and particle math is ours, written from
//! standard technique (perspective projection, painter's-algorithm faces,
//! sin-flickered flame stacks). No third-party UI code.
//!
//! Father, 13 Nov 2026, on this same page: a camera too - drag to pan, the
//! wheel to zoom, and the Wide / At-the-fire altitudes with reset.
//! Father, 12 Nov 2026: the ambient light follows this machine's clock - at
//! night the fires and torches carry the valley and the light dances from
//! one campfire to the next, through to the river boat.

use chrono::Timelike;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{
    canvas, Animation, AnimationExt, AnyElement, ClickEvent, Context, Hsla, MouseButton,
    PathBuilder, Point, ScrollDelta, Window, div, prelude::*, px,
};

use super::kit::{self, Button, card};
use super::root::{Root, Route};
use super::theme::ActiveTheme;
use crate::valley::group_by_camper;

pub const TILE: (u32, u32) = (0xffd08a, 0xd97b16);

/// What the paint pass leaves behind for interaction and label layout: the
/// canvas bounds in window coordinates, the last projected label positions and
/// the click targets. The canvas closures cannot borrow Root, so this travels
/// through an Arc; one frame of lag is invisible.
#[derive(Default)]
pub(super) struct SceneShared {
    pub(super) bounds: Option<(f32, f32, f32, f32)>,
    pub(super) labels: Vec<SceneLabel>,
    pub(super) hits: Vec<SceneHit>,
}

pub(super) struct SceneLabel {
    x: f32,
    y: f32,
    text: String,
    sub: String,
}

pub(super) struct SceneHit {
    x: f32,
    y: f32,
    r: f32,
    camper: String,
}

type Shared = Arc<Mutex<SceneShared>>;

/// The scene's camera state (Father, 13 Nov 2026: zoom controls, panning,
/// one or two default altitudes with reset). Held on the Root as an
/// Arc<Mutex<...>> so the canvas paint pass, the event handlers and the
/// preset buttons all read and write the same camera without borrowing
/// Root in the closures.
pub(super) struct SceneCam {
    pitch: f32,
    zoom: f32,
    pan_x: f32,
    pan_y: f32,
    /// The live drag: last x, last y, and how far the gesture has moved
    /// (the click gate skips a hit when a drag moved more than 6 px).
    drag: Option<(f32, f32, f32)>,
}

impl Default for SceneCam {
    fn default() -> Self {
        Self::wide()
    }
}

impl SceneCam {
    /// The wide field the scene opened with.
    fn wide() -> Self {
        Self { pitch: 0.56, zoom: 1.0, pan_x: 0.0, pan_y: 0.0, drag: None }
    }

    /// Down at fire level.
    fn close() -> Self {
        Self { pitch: 0.92, zoom: 1.9, pan_x: 0.0, pan_y: 0.0, drag: None }
    }

    fn zoom_by(&mut self, factor: f32) {
        self.zoom = (self.zoom * factor).clamp(0.55, 4.5);
    }

    fn pan_by(&mut self, dx: f32, dy: f32) {
        self.pan_x = (self.pan_x + dx).clamp(-1800.0, 1800.0);
        self.pan_y = (self.pan_y + dy).clamp(-1200.0, 1200.0);
    }
}

struct Palette {
    ground: Hsla,
    water: Hsla,
    foam: Hsla,
    wood_a: Hsla,
    wood_b: Hsla,
    stone: Hsla,
    smoke: Hsla,
    glow: Hsla,
}

fn hsl(h: f32, s: f32, l: f32, a: f32) -> Hsla {
    Hsla { h: h.fract(), s: s.clamp(0.0, 1.0), l: l.clamp(0.0, 1.0), a: a.clamp(0.0, 1.0) }
}

fn shade(c: Hsla, dl: f32, da: f32) -> Hsla {
    Hsla { h: c.h, s: c.s, l: (c.l + dl).clamp(0.02, 0.98), a: (c.a + da).clamp(0.0, 1.0) }
}

struct Cam {
    cos_y: f32,
    sin_y: f32,
    cos_p: f32,
    sin_p: f32,
    dist: f32,
    cx: f32,
    cy: f32,
    scale: f32,
}

impl Cam {
    fn new(b: (f32, f32, f32, f32), yaw: f32, pitch: f32) -> Self {
        Cam {
            cos_y: yaw.cos(),
            sin_y: yaw.sin(),
            cos_p: pitch.cos(),
            sin_p: pitch.sin(),
            dist: 17.0,
            cx: b.0 + b.2 * 0.5,
            cy: b.1 + b.3 * 0.60,
            scale: (b.2.min(b.3 * 1.5)) * 0.21,
        }
    }

    fn project(&self, p: [f32; 3]) -> Option<(f32, Point<f32>)> {
        let x = p[0] * self.cos_y - p[2] * self.sin_y;
        let z = p[0] * self.sin_y + p[2] * self.cos_y;
        let y2 = p[1] * self.cos_p - z * self.sin_p;
        let z2 = p[1] * self.sin_p + z * self.cos_p + self.dist;
        if z2 < 0.6 {
            return None;
        }
        Some((
            z2,
            Point::new(self.cx + self.scale * x / z2, self.cy - self.scale * y2 / z2),
        ))
    }
}

struct Face {
    pts: Vec<Point<f32>>,
    color: Hsla,
    depth: f32,
}

fn face(faces: &mut Vec<Face>, cam: &Cam, world: &[[f32; 3]], color: Hsla) {
    let mut pts = Vec::with_capacity(world.len());
    let mut depth = 0.0;
    for p in world {
        match cam.project(*p) {
            Some((d, pt)) => {
                depth += d;
                pts.push(pt);
            }
            None => return,
        }
    }
    faces.push(Face { pts, color, depth: depth / world.len() as f32 });
}

fn ring(center: [f32; 3], radius: f32, n: usize, wob: f32, t: f32, phase: f32) -> Vec<[f32; 3]> {
    (0..n)
        .map(|i| {
            let a = std::f32::consts::TAU * i as f32 / n as f32;
            let r = radius + wob * (t * 1.5 + a * 3.0 + phase).sin();
            [center[0] + r * a.cos(), center[1], center[2] + r * a.sin()]
        })
        .collect()
}

fn disc(_cam: &Cam, center: [f32; 3], radius: f32, n: usize) -> Vec<[f32; 3]> {
    ring(center, radius, n, 0.0, 0.0, 0.0)
}

/// One camper's fire: stones, logs, flame stack, glow, embers, smoke.
#[derive(Clone)]
struct FireSite {
    camper: String,
    x: f32,
    z: f32,
    phase: f32,
    running: usize,
    waiting: usize,
}

impl Root {
    pub fn render_valley_scene(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.maybe_load_runs(cx);

        let shared: Shared = self.scene.clone();
        let failed = match self.valley_runs.as_ref() {
            Some(super::github::Load::Failed(e)) => Some(e.clone()),
            _ => None,
        };
        if failed.as_deref() == Some("not-configured") {
            let theme = cx.theme();
            return div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(
                    card(&theme)
                        .p(px(14.0))
                        .child(div().text_size(px(13.0)).text_color(theme.text_muted).child(
                            "The valley is not configured in this app yet - set it in Settings > Valley.",
                        )),
                )
                .into_any_element();
        }

        // owned snapshot
        let mut sites: Vec<FireSite> = Vec::new();
        let mut torches = 0usize;
        if let Some(super::github::Load::Ready(runs)) = self.valley_runs.as_ref() {
            for (camper, group) in group_by_camper(runs) {
                let running = group.iter().filter(|r| r.status == "running").count();
                let waiting = group.iter().filter(|r| r.status == "waiting_checkpoint").count();
                torches += waiting.min(1);
                sites.push(FireSite {
                    camper,
                    x: 0.0,
                    z: 0.0,
                    phase: (sites.len() as f32) * 1.71,
                    running,
                    waiting,
                });
            }
        }
        let pending = match self.valley_data.as_ref() {
            Some(super::github::Load::Ready(v)) => {
                v.items.iter().filter(|i| i.status == "pending").count()
            }
            _ => 0,
        };
        if sites.is_empty() {
            sites.push(FireSite {
                camper: String::new(),
                x: 0.0,
                z: 0.0,
                phase: 0.0,
                running: 0,
                waiting: 0,
            });
        }
        // ring the fires round the green, cap at nine sites
        let n_shown = sites.len().min(9);
        for (i, site) in sites.iter_mut().take(n_shown).enumerate() {
            let a = std::f32::consts::TAU * i as f32 / n_shown as f32 - 1.9;
            let r = 3.3 + 0.9 * (a + std::f32::consts::FRAC_PI_2).sin().abs();
            site.x = r * (a + 0.4).sin();
            site.z = r * (a + 0.4).cos() * 0.9;
        }

        let t = self
            .scene_start
            .get_or_insert_with(Instant::now)
            .elapsed()
            .as_secs_f32();
        let dark = cx.theme().canvas.l < 0.5;

        // The ambient light follows the clock on this machine (Father,
        // 12 Nov 2026): as night falls the fires and the torches carry the
        // valley and the light dances from one campfire to the next, on to
        // the river boat. Bright around 13:00, fully dark outside 06:00..20:00.
        let now_local = chrono::Local::now();
        let hour = now_local.hour() as f32 + now_local.minute() as f32 / 60.0;
        let daylight = ((hour - 13.0) * std::f32::consts::PI / 14.0).cos().clamp(0.0, 1.0);
        let night = 1.0 - daylight;

        let shared_prepaint = shared.clone();
        let shared_paint = shared.clone();
        let sites_paint = sites.clone();
        let torches_paint = torches;
        let pending_paint = pending;
        let t_paint = t;
        let dark_paint = dark;
        let night_paint = night;
        // The camera travels into the paint pass, the event handlers and
        // the preset buttons by Arc, like the shared paint state.
        let cam_shared = self.scene_cam.clone();
        let cam_paint = cam_shared.clone();

        let scene_canvas = canvas(
            move |bounds, _, _| {
                if let Ok(mut s) = shared_prepaint.lock() {
                    s.bounds = Some((
                        f32::from(bounds.left()),
                        f32::from(bounds.top()),
                        f32::from(bounds.size.width),
                        f32::from(bounds.size.height),
                    ));
                }
            },
            move |b, _, window: &mut Window, _| {
                let (bx, by, w, h) = (
                    f32::from(b.left()),
                    f32::from(b.top()),
                    f32::from(b.size.width),
                    f32::from(b.size.height),
                );
                if w < 60.0 || h < 60.0 {
                    return;
                }
                let cam_state = cam_paint
                    .lock()
                    .map(|c| (c.pitch, c.zoom, c.pan_x, c.pan_y))
                    .unwrap_or((0.56, 1.0, 0.0, 0.0));
                let sway = (t_paint * std::f32::consts::TAU / 26.0).sin() * 0.06 - 0.32;
                let mut cam = Cam::new((bx, by, w, h), sway, cam_state.0);
                cam.scale *= cam_state.1;
                cam.cx += cam_state.2;
                cam.cy += cam_state.3;
                let cam = cam;
                let pal = palette(dark_paint, 1.0 - night_paint);
                let mut faces: Vec<Face> = Vec::new();
                let mut labels: Vec<SceneLabel> = Vec::new();
                let mut hits: Vec<SceneHit> = Vec::new();

                // the green
                face(&mut faces, &cam, &ring([0., 0., 0.], 8.2, 28, 0., 0., 0.), pal.ground);

                // the lake, breathing quietly
                face(&mut faces, &cam, &ring([4.7, 0.01, 0.9], 2.9, 30, 0.05, t_paint, 1.3), pal.water);
                face(
                    &mut faces,
                    &cam,
                    &ring([4.7, 0.02, 0.9], 2.55, 30, 0.05, t_paint, 1.3),
                    pal.foam,
                );

                // the dock: five planks marching in, with posts
                let (dx, dz) = (0.86_f32, 0.28_f32);
                let norm = (dx * dx + dz * dz).sqrt();
                let (ux, uz) = (dx / norm, dz / norm);
                for i in 0..5 {
                    let u = 2.35 + i as f32 * 0.6;
                    let (px_, pz_) = (-uz * 0.2, ux * 0.2);
                    face(
                        &mut faces,
                        &cam,
                        &[
                            [u * ux + px_, 0.03, u * uz + pz_],
                            [u * ux - px_, 0.03, u * uz - pz_],
                            [(u + 0.52) * ux - px_, 0.03, (u + 0.52) * uz - pz_],
                            [(u + 0.52) * ux + px_, 0.03, (u + 0.52) * uz + pz_],
                        ],
                        if i % 2 == 0 { pal.wood_a } else { shade(pal.wood_b, -0.02, 0.) },
                    );
                    if i == 2 || i == 4 {
                        for s in [-1.0, 1.0] {
                            let (bxx, bzz) = (u * ux + s * -uz * 0.24, u * uz + s * ux * 0.24);
                            face(
                                &mut faces,
                                &cam,
                                &[
                                    [bxx + uz * 0.05, 0.0, bzz - ux * 0.05],
                                    [bxx - uz * 0.05, 0.0, bzz + ux * 0.05],
                                    [bxx - uz * 0.05, 0.34, bzz + ux * 0.05],
                                    [bxx + uz * 0.05, 0.34, bzz - ux * 0.05],
                                ],
                                pal.wood_b,
                            );
                        }
                    }
                }

                // torches along the dock - one lit per waiting run
                for (k, u) in [2.6, 3.8, 5.0].iter().enumerate() {
                    let (bxx, bzz) = (*u * ux + uz * 0.3, *u * uz - ux * 0.3);
                    face(
                        &mut faces,
                        &cam,
                        &[
                            [bxx + uz * 0.05, 0.0, bzz - ux * 0.05],
                            [bxx - uz * 0.05, 0.0, bzz + ux * 0.05],
                            [bxx - uz * 0.05, 0.55, bzz + ux * 0.05],
                            [bxx + uz * 0.05, 0.55, bzz - ux * 0.05],
                        ],
                        pal.wood_b,
                    );
                    if k < torches_paint {
                        flame(
                            &mut faces,
                            &cam,
                            [bxx, 0.58, bzz],
                            // night raises every flame (Father, 12 Nov 2026)
                            0.55 * (0.8 + 0.6 * night_paint),
                            t_paint,
                            k as f32 * 2.1,
                            &pal,
                        );
                    } else if night_paint > 0.2 {
                        // at night the dock torches stay lit even when
                        // nothing waits on them
                        flame(
                            &mut faces,
                            &cam,
                            [bxx, 0.58, bzz],
                            0.22 * night_paint,
                            t_paint,
                            k as f32 * 2.1,
                            &pal,
                        );
                    }
                }

                // the river boat, in when work is pending
                if pending_paint > 0 {
                    let bob = (t_paint * 1.9).sin() * 0.05;
                    let (bxx, bzz) = (4.9 + 0.25 * (t_paint * 0.35).sin(), 1.35);
                    let (px_, pz_) = (-uz * 0.18, ux * 0.18);
                    face(
                        &mut faces,
                        &cam,
                        &[
                            [bxx + ux * 0.45, 0.06 + bob, bzz + uz * 0.45],
                            [bxx - ux * 0.45 + px_, 0.06 + bob, bzz - uz * 0.45 + pz_],
                            [bxx - ux * 0.45 - px_, 0.06 + bob, bzz - uz * 0.45 - pz_],
                        ],
                        shade(pal.wood_a, 0.02, 0.0),
                    );
                    face(
                        &mut faces,
                        &cam,
                        &[
                            [bxx - uz * 0.03, 0.06 + bob, bzz + ux * 0.03],
                            [bxx + uz * 0.03, 0.06 + bob, bzz - ux * 0.03],
                            [bxx + uz * 0.03, 0.52 + bob, bzz - ux * 0.03],
                            [bxx - uz * 0.03, 0.52 + bob, bzz + ux * 0.03],
                        ],
                        pal.wood_b,
                    );
                    face(
                        &mut faces,
                        &cam,
                        &[
                            [bxx, 0.52 + bob, bzz],
                            [bxx + uz * 0.22, 0.44 + bob, bzz - ux * 0.22],
                            [bxx + uz * 0.14, 0.28 + bob, bzz - ux * 0.14],
                        ],
                        hsl(0.07, 0.8, 0.55, 0.8),
                    );
                    // the water round the boat catches the light as the
                    // dance passes through it (Father, 12 Nov 2026)
                    if night_paint > 0.05 {
                        face(
                            &mut faces,
                            &cam,
                            &ring([bxx, 0.045, bzz], 1.4, 14, 0.0, 0.0, 0.0),
                            hsl(
                                pal.glow.h,
                                pal.glow.s,
                                pal.glow.l,
                                0.10 * night_paint * (1.0 + 0.5 * (t_paint * 1.1).sin()),
                            ),
                        );
                    }
                }

                // the campfires
                for site in &sites_paint {
                    let base = [site.x, 0.0, site.z];
                    face(&mut faces, &cam, &disc(&cam, [site.x, 0.02, site.z], 0.62, 10), shade(pal.stone, -0.04, 0.));
                    for s in 0..8 {
                        let a = std::f32::consts::TAU * s as f32 / 8.0 + site.phase * 0.3;
                        face(
                            &mut faces,
                            &cam,
                            &disc(
                                &cam,
                                [site.x + 0.4 * a.cos(), 0.04, site.z + 0.4 * a.sin()],
                                0.055,
                                6,
                            ),
                            pal.stone,
                        );
                    }
                    face(
                        &mut faces,
                        &cam,
                        &[
                            [site.x - 0.2, 0.06, site.z - 0.06],
                            [site.x + 0.2, 0.06, site.z - 0.06],
                            [site.x + 0.16, 0.12, site.z + 0.08],
                            [site.x - 0.16, 0.12, site.z + 0.08],
                        ],
                        pal.wood_b,
                    );
                    let lit = site.running > 0 || site.waiting > 0;
                    if lit {
                        let breathe = if site.waiting > 0 {
                            1.0 + 0.12 * (t_paint * 1.8 + site.phase).sin()
                        } else {
                            1.0
                        };
                        // glow pools
                        for (gr, ga) in [(0.95, 0.11), (1.45, 0.055)] {
                            face(
                                &mut faces,
                                &cam,
                                &ring([site.x, 0.03, site.z], gr * breathe, 14, 0.0, 0.0, 0.0),
                                hsl(pal.glow.h, pal.glow.s, pal.glow.l, ga * breathe.max(1.0)),
                            );
                        }
                        // embers
                        for e in 0..5 {
                            let prog = (t_paint * 0.4 + e as f32 * 0.29) % 1.0;
                            let a = t_paint * 0.9 + e as f32 * 1.3 + site.phase;
                            face(
                                &mut faces,
                                &cam,
                                &disc(
                                    &cam,
                                    [
                                        site.x + 0.12 * (a + prog * 1.4).cos(),
                                        0.15 + prog * 0.55,
                                        site.z + 0.12 * (a + prog * 1.4).sin(),
                                    ],
                                    0.02 + 0.008 * prog,
                                    5,
                                ),
                                hsl(0.07, 0.9, 0.6, 0.85 * (1.0 - prog)),
                            );
                        }
                        // smoke
                        for s in 0..3 {
                            let prog = (t_paint * 0.16 + s as f32 * 0.33) % 1.0;
                            face(
                                &mut faces,
                                &cam,
                                &disc(
                                    &cam,
                                    [site.x + 0.05 * prog + s as f32 * 0.02, 0.5 + prog * 0.9, site.z],
                                    0.07 + prog * 0.17,
                                    7,
                                ),
                                hsl(pal.smoke.h, pal.smoke.s, pal.smoke.l, 0.09 * (1.0 - prog)),
                            );
                        }
                        flame(
                            &mut faces,
                            &cam,
                            base,
                            // night raises the fires that are burning
                            // (Father, 12 Nov 2026)
                            scale_of(site) * (0.75 + 0.65 * night_paint),
                            t_paint,
                            site.phase,
                            &pal,
                        );
                        // the light dances round the ring: each pool breathes
                        // on its own phase, so the glow passes from one
                        // campfire to the next (Father, 12 Nov 2026)
                        if night_paint > 0.05 {
                            let dance =
                                0.75 + 0.25 * (t_paint * 0.9 + site.phase * 1.7).sin();
                            face(
                                &mut faces,
                                &cam,
                                &ring(
                                    [site.x, 0.035, site.z],
                                    1.9 * breathe,
                                    16,
                                    0.0,
                                    0.0,
                                    0.0,
                                ),
                                hsl(
                                    pal.glow.h,
                                    pal.glow.s,
                                    pal.glow.l,
                                    0.14 * night_paint * dance * breathe.max(1.0),
                                ),
                            );
                        }
                    } else {
                        // cold fire: low coals
                        for e in 0..3 {                            let prog = (t_paint * 0.12 + e as f32 * 0.41) % 1.0;
                            face(
                                &mut faces,
                                &cam,
                                &disc(
                                    &cam,
                                    [site.x + 0.06 * prog, 0.05 + prog * 0.14, site.z],
                                    0.016,
                                    5,
                                ),
                                hsl(0.06, 0.7, 0.45, 0.5 * (1.0 - prog)),
                            );
                        }
                        // at night a cold fire keeps a coal glow on the
                        // ground, so the valley never goes fully dark
                        if night_paint > 0.2 {
                            face(
                                &mut faces,
                                &cam,
                                &ring([site.x, 0.035, site.z], 0.62, 12, 0.0, 0.0, 0.0),
                                hsl(pal.glow.h, pal.glow.s, pal.glow.l, 0.07 * night_paint),
                            );
                        }
                    }
                    // label anchor + hit target
                    if let Some((_, pt)) = cam.project([site.x, 1.05, site.z]) {
                        let sub = if site.waiting > 0 {
                            "NEEDS YOU".to_string()
                        } else if site.running > 0 {
                            format!("running x{}", site.running)
                        } else if site.camper.is_empty() {
                            "the valley is quiet".to_string()
                        } else {
                            "embers".to_string()
                        };
                        labels.push(SceneLabel {
                            x: pt.x,
                            y: pt.y,
                            text: if site.camper.is_empty() { "the watch fire".into() } else { site.camper.clone() },
                            sub,
                        });
                        if !site.camper.is_empty() {
                            hits.push(SceneHit { x: pt.x, y: pt.y + 40.0, r: 34.0, camper: site.camper.clone() });
                        }
                    }
                }

                faces.sort_by(|a, b| b.depth.partial_cmp(&a.depth).unwrap_or(std::cmp::Ordering::Equal));
                for f in &faces {
                    let mut pb = PathBuilder::fill();
                    pb.add_polygon(
                        &f.pts.iter().map(|p| Point::new(px(p.x), px(p.y))).collect::<Vec<_>>(),
                        true,
                    );
                    if let Ok(path) = pb.build() {
                        window.paint_path(path, f.color);
                    }
                }
                if let Ok(mut s) = shared_paint.lock() {
                    s.labels = labels;
                    s.hits = hits;
                }
            },
        );

        // per-frame tick: the animation wrapper re-renders the subtree each
        // 40ms slice, so flames, embers and the lake march on
        let scene = scene_canvas
            .size_full()
            .with_animation(
                "valley-scene-tick",
                Animation::new(Duration::from_millis(40)).repeat(),
                |el, _| el,
            )
            .into_any_element();

        // labels overlay (previous frame's projections)
        let mut overlay = Vec::new();
        let (lb_x, lb_y) = shared.lock().ok().and_then(|s| s.bounds).map(|b| (b.0, b.1)).unwrap_or((0.0, 0.0));
        if let Ok(s) = shared.lock() {
            for l in &s.labels {
                overlay.push(
                    div()
                        .absolute()
                        .left(px((l.x - lb_x).round()))
                        .top(px((l.y - lb_y).round()))
                        .flex()
                        .flex_col()
                        .items_center()
                        .flex_none()
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(cx.theme().text)
                                .child(l.text.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(10.5))
                                .text_color(if l.sub == "NEEDS YOU" {
                                    cx.theme().accent
                                } else {
                                    cx.theme().text_placeholder
                                })
                                .child(l.sub.clone()),
                        ),
                );
            }
        }

        let theme = cx.theme();
        let running_total = sites.iter().filter(|s| s.running > 0).count();
        let waiting_total = torches;
        let statusline = format!(
            "{} on the fire - {} warm - {} needing you - {} on the board",
            sites.len(),
            running_total,
            waiting_total,
            pending
        );

        let shared_click = shared.clone();
        // The camera handlers each take their own Arc clone; the paint pass
        // holds cam_paint, so nothing here borrows Root.
        let cam_drag = cam_shared.clone();
        let cam_move = cam_shared.clone();
        let cam_wheel = cam_shared.clone();
        let cam_click = cam_shared.clone();
        let cam_zoom_in = cam_shared.clone();
        let cam_zoom_out = cam_shared.clone();
        let cam_preset_wide = cam_shared.clone();
        let cam_preset_close = cam_shared.clone();
        div()
            .id("valley-scene-root")
            .size_full()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(card(&theme).px(px(14.0)).py(px(10.0)).flex().items_center().gap(px(10.0))
                .child(div().flex_1().flex_col().gap(px(2.0))
                    .child(div().text_size(px(22.0)).font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme.text).child("The Valley"))
                    .child(div().text_size(px(12.0)).text_color(theme.text_muted).child(statusline)))
                .child(Button::icon_only("valley-scene-refresh", "refresh-cw").tooltip("Breathe the valley in again").on_click(cx.listener(|this, _, _, cx| {
                    this.valley_runs = None;
                    this.maybe_load_runs(cx);
                }))))
            .child(
                div()
                    .id("valley-scene-stage")
                    .relative()
                    .flex_1()
                    .min_h(px(340.0))
                    .rounded(kit::corner(12.0))
                    .overflow_hidden()
                    .bg(theme.canvas)
                    .child(scene)
                    .children(overlay)
                    .on_mouse_down(
                        MouseButton::Left,
                        move |ev: &gpui::MouseDownEvent, _, _| {
                            if let Ok(mut c) = cam_drag.lock() {
                                c.drag = Some((f32::from(ev.position.x), f32::from(ev.position.y), 0.0));
                            }
                        },
                    )
                    .on_mouse_move(move |ev: &gpui::MouseMoveEvent, _, _| {
                        if !ev.dragging() {
                            return;
                        }
                        if let Ok(mut c) = cam_move.lock() {
                            if let Some((last_x, last_y, moved)) = c.drag {
                                let x = f32::from(ev.position.x);
                                let y = f32::from(ev.position.y);
                                let (dx, dy) = (x - last_x, y - last_y);
                                c.drag = Some((x, y, moved + dx.abs() + dy.abs()));
                                // The scene follows the hand: the paint pass
                                // adds the pan onto the projection centre.
                                c.pan_by(dx, dy);
                            }
                        }
                    })
                    .on_scroll_wheel(move |ev: &gpui::ScrollWheelEvent, _, _| {
                        if let Ok(mut c) = cam_wheel.lock() {
                            let dy = match ev.delta {
                                ScrollDelta::Lines(l) => -l.y * 24.0,
                                ScrollDelta::Pixels(p) => f32::from(p.y),
                            };
                            // One notch at a time; clamp each wheel step so a
                            // fast spin swings gently (Father, 13 Nov 2026).
                            let factor = (1.0 - dy * 0.003).clamp(0.85, 1.18);
                            c.zoom_by(factor);
                        }
                    })
                    .on_click(cx.listener(move |this: &mut Root, ev: &ClickEvent, window: &mut gpui::Window, cx: &mut Context<Root>| {
                        // A drag that moved further than a tap is a pan, not a
                        // pick - the click gate skips it (Father, 13 Nov 2026).
                        let moved = cam_click
                            .lock()
                            .ok()
                            .map(|mut c| {
                                let m = c.drag.map(|d| d.2).unwrap_or(0.0);
                                c.drag = None;
                                m
                            })
                            .unwrap_or(0.0);
                        if moved > 6.0 {
                            return;
                        }
                        let ClickEvent::Mouse(click) = ev else {
                            return;
                        };
                        let posx = f32::from(click.up.position.x);
                        let posy = f32::from(click.up.position.y);
                        let hit = shared_click.lock().ok().and_then(|s| {
                            s.hits
                                .iter()
                                .map(|h| {
                                    let d = ((h.x - posx).powi(2) + (h.y - posy).powi(2)).sqrt();
                                    (d, h.camper.clone())
                                })
                                .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
                                .filter(|(d, _)| *d <= 46.0)
                                .map(|(_, c)| c)
                        });
                        if let Some(camper) = hit {
                            this.campfire_open = Some(camper);
                            this.campfire_torch = None;
                            this.navigate(Route::Campfires, window, cx);
                        }
                    }))
                    // The camera strip: zoom, and the two altitudes Father
                    // asked for (Father, 13 Nov 2026: reset presets).
                    .child(
                        div()
                            .absolute()
                            .top(px(10.0))
                            .right(px(12.0))
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .child(Button::new("sc-zoom-in", "+").tooltip("Zoom in").on_click(move |_, _, _| {
                                if let Ok(mut c) = cam_zoom_in.lock() {
                                    c.zoom_by(1.25);
                                }
                            }))
                            .child(Button::new("sc-zoom-out", "-").tooltip("Zoom out").on_click(move |_, _, _| {
                                if let Ok(mut c) = cam_zoom_out.lock() {
                                    c.zoom_by(1.0 / 1.25);
                                }
                            }))
                            .child(Button::new("sc-preset-wide", "Wide").tooltip("Back to the wide field").on_click(move |_, _, _| {
                                if let Ok(mut c) = cam_preset_wide.lock() {
                                    *c = SceneCam::wide();
                                }
                            }))
                            .child(Button::new("sc-preset-close", "At the fire").tooltip("Down at fire level").on_click(move |_, _, _| {
                                if let Ok(mut c) = cam_preset_close.lock() {
                                    *c = SceneCam::close();
                                }
                            })),
                    ),
            )
            .into_any_element()
    }
}

fn scale_of(site: &FireSite) -> f32 {
    match (site.waiting, site.running) {
        (w, _) if w > 0 => 1.0,
        (_, r) if r > 0 => 0.78,
        _ => 0.3,
    }
}

/// The flame: three rising sin-flickered wedges.
fn flame(faces: &mut Vec<Face>, cam: &Cam, base: [f32; 3], intensity: f32, t: f32, phase: f32, _pal: &Palette) {
    let fy = base[1] + 0.05;
    let f1 = (t * 5.1 + phase).sin();
    let f2 = (t * 3.3 + phase * 2.1).sin();
    let breath = 1.0 + 0.1 * (t * 1.8 + phase).sin();
    let layers = [
        (0.30 * intensity, ((0.5 + 0.1 * f1 + 0.05 * f2).max(0.2)) * breath, hsl(0.045, 0.85, 0.48, 0.72)),
        (0.20 * intensity, (0.38 + 0.08 * f2 + 0.04 * f1) * breath, hsl(0.07, 0.9, 0.57, 0.85)),
        (0.11 * intensity, (0.24 + 0.05 * f1) * breath, hsl(0.11, 0.95, 0.68, 0.95)),
    ];
    for (w, hgt, color) in layers {
        face(
            faces,
            cam,
            &[
                [base[0] - w, fy, base[2]],
                [base[0] + w, fy, base[2]],
                [base[0] + 0.03 * f1, fy + hgt, base[2] + 0.02 * f2],
            ],
            color,
        );
    }
}

/// The palette for the time of day: the theme's base, then the night pulls
/// the land and water down and lifts the firelight that answers (Father,
/// 12 Nov 2026: the ambient light follows the clock).
fn palette(dark: bool, daylight: f32) -> Palette {
    let mut pal = base_palette(dark);
    let night = 1.0 - daylight;
    pal.ground.l *= 1.0 - 0.45 * night;
    pal.water.l *= 1.0 - 0.35 * night;
    pal.foam.l *= 1.0 - 0.10 * night;
    pal.wood_a.l *= 1.0 - 0.35 * night;
    pal.wood_b.l *= 1.0 - 0.35 * night;
    pal.stone.l *= 1.0 - 0.25 * night;
    pal.glow = hsl(pal.glow.h, pal.glow.s, pal.glow.l, 0.30 * night);
    pal
}

fn base_palette(dark: bool) -> Palette {
    Palette {
        ground: hsl(0.34, if dark { 0.13 } else { 0.15 }, if dark { 0.150 } else { 0.350 }, 1.0),
        water: hsl(0.55, 0.42, if dark { 0.20 } else { 0.46 }, 1.0),
        foam: hsl(0.55, 0.40, if dark { 0.31 } else { 0.62 }, 0.35),
        wood_a: hsl(0.06, 0.32, if dark { 0.30 } else { 0.46 }, 1.0),
        wood_b: hsl(0.055, 0.34, if dark { 0.25 } else { 0.40 }, 1.0),
        stone: hsl(0.0, 0.0, if dark { 0.26 } else { 0.52 }, 1.0),
        smoke: hsl(0.6, 0.06, if dark { 0.45 } else { 0.60 }, 0.10),
        glow: hsl(0.075, 0.75, if dark { 0.52 } else { 0.5 }, 0.0),
    }
}
