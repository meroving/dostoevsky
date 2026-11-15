use std::sync::{Arc, OnceLock};

use gpui_kit::{App, Global};
use rodio::Source;

pub struct Sounds {
    output: Arc<OnceLock<rodio::mixer::Mixer>>,
    high: Arc<Vec<f32>>,
    low: Arc<Vec<f32>>,
}

impl Global for Sounds {}

#[cfg(test)]
thread_local! {
    pub static PLAYED: std::cell::RefCell<Vec<bool>> = const { std::cell::RefCell::new(Vec::new()) };
}

const RATE: u32 = 44_100;

fn tone(freq: f32, ms: f32) -> Vec<f32> {
    let n = (RATE as f32 * ms / 1000.) as usize;
    let attack = RATE as f32 * 0.006;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let env = (i as f32 / attack).min(1.) * (-t * 5000. / ms / 1.4).exp();
            let w = std::f32::consts::TAU * freq * t;
            env * (w.sin() + 0.22 * (2. * w).sin()) * 0.5
        })
        .collect()
}

impl Sounds {
    pub fn init(cx: &mut App) {
        let output = Arc::new(OnceLock::new());
        let opened = output.clone();
        // CoreAudio takes ~85 ms to open and the stream can't change threads: open it here, keep it.
        let spawned = std::thread::Builder::new().name("sound output".into()).spawn(move || {
            let Ok(stream) = rodio::OutputStreamBuilder::open_default_stream() else { return };
            let _ = opened.set(stream.mixer().clone());
            loop {
                std::thread::park();
            }
        });
        if let Err(e) = spawned {
            eprintln!("dostoevsky: no sound: {e}");
        }
        cx.set_global(Sounds {
            output,
            high: Arc::new(tone(880., 160.)),
            low: Arc::new(tone(523.25, 320.)),
        });
    }

    /// Plays the tone for landing on the original (`true`) or an alternative.
    pub fn play(cx: &App, original: bool) {
        let prefs = crate::prefs::Prefs::get(cx);
        if !prefs.sounds || prefs.volume <= 0. {
            return;
        }
        #[cfg(test)]
        PLAYED.with(|p| p.borrow_mut().push(original));
        let Some(s) = cx.try_global::<Sounds>() else { return };
        let Some(output) = s.output.get() else { return };
        let data = if original { &s.low } else { &s.high };
        let buf = rodio::buffer::SamplesBuffer::new(1, RATE, data.as_ref().clone());
        output.add(buf.amplify(prefs.volume));
    }
}
