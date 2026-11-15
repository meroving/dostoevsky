use l10n::tr;

use crate::service::ModelInfo;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    /// ⌥⌘A and `??`.
    Quick,
    /// The Lab and trims.
    Deep,
}

impl Slot {
    /// Typical (input, output) tokens of one use.
    fn tokens(self, ideas: usize) -> (f64, f64) {
        match self {
            // A paragraph or three of context, short ideas of about 50 tokens each.
            Slot::Quick => (1750., 50. * ideas as f64),
            // A few thousand words, a list of marks plus the model's thinking.
            Slot::Deep => (9000., 3500.),
        }
    }
}

/// `None` when the price isn't known; a Quick use brings five ideas.
pub fn per_use(m: &ModelInfo, slot: Slot) -> Option<String> {
    per_use_with(m, slot, 5)
}

pub fn per_use_with(m: &ModelInfo, slot: Slot, ideas: usize) -> Option<String> {
    if m.free() {
        return Some(tr!("free").into());
    }
    let (i, o) = (m.price_in?, m.price_out?);
    let (ti, to) = slot.tokens(ideas);
    let cents = (i * ti + o * to) * 100.;
    Some(format_cents(cents))
}

pub fn format_cents(cents: f64) -> String {
    if cents <= 0. {
        tr!("free").into()
    } else if cents < 0.1 {
        format!("≈ {:.2}¢", cents)
    } else if cents < 10. {
        format!("≈ {:.1}¢", cents)
    } else if cents < 100. {
        format!("≈ {:.0}¢", cents)
    } else {
        format!("≈ ${:.2}", cents / 100.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(i: f64, o: f64) -> ModelInfo {
        ModelInfo { id: "x".into(), name: "x".into(), price_in: Some(i), price_out: Some(o) }
    }

    #[test]
    fn costs_per_use() {
        assert_eq!(per_use(&m(2e-6, 10e-6), Slot::Quick).as_deref(), Some("≈ 0.6¢"));
        assert_eq!(per_use(&m(4e-6, 20e-6), Slot::Deep).as_deref(), Some("≈ 11¢"));
        assert_eq!(per_use(&m(0.1e-6, 0.5e-6), Slot::Quick).as_deref(), Some("≈ 0.03¢"));
        assert_eq!(per_use_with(&m(2e-6, 10e-6), Slot::Quick, 10).as_deref(), Some("≈ 0.9¢"));
        assert_eq!(per_use_with(&m(4e-6, 20e-6), Slot::Deep, 10).as_deref(), Some("≈ 11¢"));
        assert_eq!(per_use(&m(0., 0.), Slot::Deep).as_deref(), Some("free"));
        assert_eq!(per_use(&ModelInfo { id: "y".into(), name: "y".into(), price_in: None, price_out: None }, Slot::Quick), None);
        assert_eq!(format_cents(150.), "≈ $1.50");
    }
}
