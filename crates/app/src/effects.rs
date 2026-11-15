use std::time::{Duration, Instant};

use gpui_kit::*;

use crate::prefs::Effects;
use crate::theme::{self, c, ca};

pub struct Burst {
    origin: Point<Pixels>,
    start: Instant,
    seed: u64,
    text: SharedString,
    mode: Effects,
}

pub struct Flight {
    from: Point<Pixels>,
    to: Point<Pixels>,
    start: Instant,
    text: SharedString,
    mode: Effects,
}

#[derive(Default)]
pub struct Overlay {
    bursts: Vec<Burst>,
    flights: Vec<Flight>,
}

const BURST: Duration = Duration::from_millis(650);
const FADE: Duration = Duration::from_millis(150);
const FLIGHT: Duration = Duration::from_millis(520);

fn rand(seed: u64, i: u64) -> f32 {
    let mut x = seed ^ i.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 33;
    x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    x ^= x >> 33;
    (x % 10_000) as f32 / 10_000.
}

impl Overlay {
    pub fn explode(&mut self, origin: Point<Pixels>, text: impl Into<SharedString>, mode: Effects) {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(7, |d| d.as_nanos() as u64);
        self.bursts.push(Burst { origin, start: Instant::now(), seed, text: text.into(), mode });
    }

    pub fn fly(&mut self, from: Point<Pixels>, to: Point<Pixels>, text: impl Into<SharedString>, mode: Effects) {
        if mode == Effects::Off {
            return;
        }
        self.flights.push(Flight { from, to, start: Instant::now(), text: text.into(), mode });
    }

    #[cfg(test)]
    pub fn flights(&self) -> Vec<Effects> {
        self.flights.iter().map(|f| f.mode).collect()
    }

    #[cfg(test)]
    pub fn bursts(&self) -> Vec<Effects> {
        self.bursts.iter().map(|b| b.mode).collect()
    }

    pub fn is_active(&mut self) -> bool {
        self.bursts.retain(|b| b.start.elapsed() < if b.mode == Effects::Off { FADE } else { BURST });
        self.flights.retain(|f| f.start.elapsed() < FLIGHT);
        !self.bursts.is_empty() || !self.flights.is_empty()
    }

    /// The overlay element; call only while [`Self::is_active`].
    pub fn element(&self) -> impl IntoElement {
        let bursts: Vec<_> = self.bursts.iter().map(|b| (b.origin, b.start, b.seed, b.text.clone(), b.mode)).collect();
        let flights: Vec<_> = self.flights.iter().map(|f| (f.from, f.to, f.start, f.text.clone(), f.mode)).collect();
        canvas(
            |_, _, _| (),
            move |_, _, window, cx| {
                for (origin, start, seed, text, mode) in &bursts {
                    paint_burst(*origin, start.elapsed(), *seed, text, *mode, window, cx);
                }
                for (from, to, start, text, mode) in &flights {
                    paint_flight(*from, *to, start.elapsed(), text, *mode, window, cx);
                }
                window.request_animation_frame();
            },
        )
        .absolute()
        .inset_0()
    }
}

fn paint_label(p: Point<Pixels>, text: &SharedString, size: Pixels, color: Hsla, window: &mut Window, cx: &mut App) {
    if text.is_empty() {
        return;
    }
    let run = TextRun { len: text.len(), font: font(theme::UI), color, background_color: None, underline: None, strikethrough: None };
    let line = window.text_system().shape_line(text.clone(), size, &[run], None);
    let _ = line.paint(p, size * 1.4, TextAlign::Left, None, window, cx);
}

fn paint_burst(o: Point<Pixels>, t: Duration, seed: u64, text: &SharedString, mode: Effects, window: &mut Window, cx: &mut App) {
    let t = t.as_secs_f32();
    let k = (t / FADE.as_secs_f32()).min(1.);
    paint_label(point(o.x, o.y - px(10.)), text, px(13.), ca(theme::text_2(), 1. - k), window, cx);
    if mode == Effects::Off {
        return;
    }
    let life = BURST.as_secs_f32();
    let n = if mode == Effects::On { 28 } else { 10 };
    let colors = [theme::accent(), theme::para_bar(), theme::danger(), theme::text(), 0xf2b45a];
    for i in 0..n {
        let a = rand(seed, i) * std::f32::consts::TAU;
        let speed = 90. + 190. * rand(seed, i + 100);
        let x = o.x + px(a.cos() * speed * t);
        let y = o.y + px(a.sin() * speed * t + 260. * t * t);
        let r = px(1.2 + 2.2 * rand(seed, i + 200)) * (1. - t / life).max(0.);
        let color = ca(colors[(i as usize) % colors.len()], (1. - t / life).max(0.));
        window.paint_quad(fill(Bounds::new(point(x - r, y - r), size(r * 2., r * 2.)), color).corner_radii(r));
    }
    if mode == Effects::On {
        let r = px(8. + 46. * (t / life));
        let alpha = 0.22 * (1. - t / life).max(0.);
        window.paint_quad(fill(Bounds::new(point(o.x - r, o.y - r), size(r * 2., r * 2.)), ca(theme::muted(), alpha)).corner_radii(r));
    }
    if t < 0.08 {
        let r = px(10. * (1. - t / 0.08));
        window.paint_quad(fill(Bounds::new(point(o.x - r, o.y - r), size(r * 2., r * 2.)), ca(0xfff3d6, 0.9)).corner_radii(r));
    }
}

fn paint_flight(from: Point<Pixels>, to: Point<Pixels>, t: Duration, text: &SharedString, mode: Effects, window: &mut Window, cx: &mut App) {
    let k = (t.as_secs_f32() / FLIGHT.as_secs_f32()).min(1.);
    let e = 1. - (1. - k).powi(3);
    let lift = if mode == Effects::On { 60. * (std::f32::consts::PI * e).sin() } else { 0. };
    let p = point(from.x + (to.x - from.x) * e, from.y + (to.y - from.y) * e - px(lift));
    let short: String = text.chars().take(36).collect();
    let label: SharedString = if text.chars().count() > 36 { format!("{short}…").into() } else { short.into() };
    let w = px(8.) * label.chars().count() as f32 + px(16.);
    window.paint_quad(
        quad(Bounds::new(point(p.x - px(8.), p.y - px(4.)), size(w, px(26.))), px(5.), ca(theme::bg_raised(), 1. - k * 0.5), px(1.), ca(theme::accent(), 0.6 * (1. - k)), BorderStyle::Solid),
    );
    paint_label(p, &label, px(12.), c(theme::text_2()), window, cx);
}
