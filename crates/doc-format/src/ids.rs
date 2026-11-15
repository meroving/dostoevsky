use doc_model::GroupId;

const SPACE5: u64 = 36u64.pow(5);
const SPACE8: u64 = 36u64.pow(8);
const MUL: u64 = 15_485_863; // prime, coprime with 36
const ADD: u64 = 31_337_731;
const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

fn base36(mut v: u64, width: usize) -> String {
    let mut out = vec![b'0'; width];
    for slot in out.iter_mut().rev() {
        *slot = DIGITS[(v % 36) as usize];
        v /= 36;
    }
    String::from_utf8(out).expect("ascii")
}

fn parse36(s: &str) -> Option<u64> {
    s.bytes().try_fold(0u64, |acc, b| {
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'z' => b - b'a' + 10,
            _ => return None,
        };
        Some(acc * 36 + d as u64)
    })
}

fn mul_inverse() -> u64 {
    let (mut t, mut new_t, mut r, mut new_r) = (0i128, 1i128, SPACE5 as i128, MUL as i128);
    while new_r != 0 {
        let q = r / new_r;
        (t, new_t) = (new_t, t - q * new_t);
        (r, new_r) = (new_r, r - q * new_r);
    }
    t.rem_euclid(SPACE5 as i128) as u64
}

pub fn encode(id: GroupId) -> String {
    if id.0 < SPACE5 {
        base36((id.0 * MUL + ADD) % SPACE5, 5)
    } else {
        base36(id.0 % SPACE8, 8)
    }
}

pub fn decode(s: &str) -> Option<GroupId> {
    let v = parse36(s)?;
    match s.len() {
        5 => Some(GroupId(
            (v + SPACE5 - ADD) % SPACE5 * mul_inverse() % SPACE5,
        )),
        8 if v >= SPACE5 => Some(GroupId(v)),
        _ => None,
    }
}

pub fn is_valid(s: &str) -> bool {
    (5..=8).contains(&s.len()) && parse36(s).is_some()
}

/// A valid id from `seed`, for renaming a kept record whose id collides.
pub fn fallback(seed: &str, attempt: u64) -> String {
    // FNV-1a: deterministic across runs and platforms.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in seed.bytes().chain(attempt.to_le_bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    base36(SPACE5 + h % (SPACE8 - SPACE5), 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for id in (0..5000).chain([SPACE5 - 1, SPACE5, SPACE5 + 7, SPACE8 - 1]) {
            let s = encode(GroupId(id));
            assert!(is_valid(&s), "{s}");
            assert_eq!(decode(&s), Some(GroupId(id)), "{s}");
        }
        assert_ne!(encode(GroupId(1)), encode(GroupId(2)));
        assert_eq!(decode("group1"), None);
        assert_eq!(decode("00000001"), None, "8-char ids below 36⁵ are foreign");
        assert_eq!(decode("K3F9A"), None);
        assert!(is_valid(&fallback("k3f9a", 0)));
        assert!(decode(&fallback("k3f9a", 0)).is_some());
    }
}
