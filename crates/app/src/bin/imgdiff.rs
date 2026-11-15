//! Compares a screenshot with its baseline (scripts/run_flows.sh).
//! Exit status: 0 match, 1 mismatch, 2 error.

use std::process::ExitCode;

use image::{Rgba, RgbaImage};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: imgdiff baseline.png actual.png diff.png [tolerance] [max-share]");
        return ExitCode::from(2);
    }
    let tolerance: u8 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(16);
    let max_share: f64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0.00002);
    let load = |p: &str| image::open(p).map(|i| i.to_rgba8()).map_err(|e| format!("{p}: {e}"));
    let (base, actual) = match (load(&args[0]), load(&args[1])) {
        (Ok(b), Ok(a)) => (b, a),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("imgdiff: {e}");
            return ExitCode::from(2);
        }
    };
    if base.dimensions() != actual.dimensions() {
        println!("size differs: baseline {:?}, actual {:?}", base.dimensions(), actual.dimensions());
        return ExitCode::from(1);
    }
    let (w, h) = actual.dimensions();
    let mut diff = RgbaImage::new(w, h);
    let mut differing = 0u64;
    for (x, y, a) in actual.enumerate_pixels() {
        let b = base.get_pixel(x, y);
        let off = a.0.iter().zip(b.0.iter()).any(|(p, q)| p.abs_diff(*q) > tolerance);
        if off {
            differing += 1;
            diff.put_pixel(x, y, Rgba([255, 40, 40, 255]));
        } else {
            let grey = ((a[0] as u16 + a[1] as u16 + a[2] as u16) / 3 / 4) as u8;
            diff.put_pixel(x, y, Rgba([grey, grey, grey, 255]));
        }
    }
    let total = u64::from(w) * u64::from(h);
    let share = differing as f64 / total as f64;
    println!("{differing} of {total} pixels differ ({:.3}%)", share * 100.);
    if share <= max_share {
        return ExitCode::SUCCESS;
    }
    if let Err(e) = diff.save(&args[2]) {
        eprintln!("imgdiff: {}: {e}", args[2]);
    }
    ExitCode::from(1)
}
