/// No directories, no zip64 (a book is a few megabytes).
#[derive(Default)]
pub struct Zip {
    out: Vec<u8>,
    central: Vec<u8>,
    count: u16,
}

/// 1980-01-01 00:00, zip's earliest time: packages stay byte-for-byte the same for the same text.
const DOS_TIME: u16 = 0;
const DOS_DATE: u16 = (1 << 5) | 1;

impl Zip {
    pub fn new() -> Zip {
        Zip::default()
    }

    /// An entry kept as it is (ePub's `mimetype` must be the first, stored).
    pub fn stored(&mut self, name: &str, data: &[u8]) {
        self.entry(name, data, data, 0);
    }

    pub fn deflated(&mut self, name: &str, data: &[u8]) {
        let packed = miniz_oxide::deflate::compress_to_vec(data, 6);
        if packed.len() < data.len() {
            self.entry(name, data, &packed, 8);
        } else {
            self.entry(name, data, data, 0);
        }
    }

    fn entry(&mut self, name: &str, data: &[u8], body: &[u8], method: u16) {
        let crc = crc32fast::hash(data);
        let offset = self.out.len() as u32;
        let name_b = name.as_bytes();
        // Bit 11: the name is UTF-8.
        let flags: u16 = 1 << 11;
        let local = &mut self.out;
        local.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        local.extend_from_slice(&20u16.to_le_bytes());
        local.extend_from_slice(&flags.to_le_bytes());
        local.extend_from_slice(&method.to_le_bytes());
        local.extend_from_slice(&DOS_TIME.to_le_bytes());
        local.extend_from_slice(&DOS_DATE.to_le_bytes());
        local.extend_from_slice(&crc.to_le_bytes());
        local.extend_from_slice(&(body.len() as u32).to_le_bytes());
        local.extend_from_slice(&(data.len() as u32).to_le_bytes());
        local.extend_from_slice(&(name_b.len() as u16).to_le_bytes());
        local.extend_from_slice(&0u16.to_le_bytes());
        local.extend_from_slice(name_b);
        local.extend_from_slice(body);

        let c = &mut self.central;
        c.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        c.extend_from_slice(&20u16.to_le_bytes());
        c.extend_from_slice(&20u16.to_le_bytes());
        c.extend_from_slice(&flags.to_le_bytes());
        c.extend_from_slice(&method.to_le_bytes());
        c.extend_from_slice(&DOS_TIME.to_le_bytes());
        c.extend_from_slice(&DOS_DATE.to_le_bytes());
        c.extend_from_slice(&crc.to_le_bytes());
        c.extend_from_slice(&(body.len() as u32).to_le_bytes());
        c.extend_from_slice(&(data.len() as u32).to_le_bytes());
        c.extend_from_slice(&(name_b.len() as u16).to_le_bytes());
        c.extend_from_slice(&0u16.to_le_bytes()); // extra
        c.extend_from_slice(&0u16.to_le_bytes()); // comment
        c.extend_from_slice(&0u16.to_le_bytes()); // disk
        c.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        c.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        c.extend_from_slice(&offset.to_le_bytes());
        c.extend_from_slice(name_b);
        self.count += 1;
    }

    pub fn finish(mut self) -> Vec<u8> {
        let start = self.out.len() as u32;
        let size = self.central.len() as u32;
        self.out.append(&mut self.central);
        let e = &mut self.out;
        e.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        e.extend_from_slice(&0u16.to_le_bytes());
        e.extend_from_slice(&0u16.to_le_bytes());
        e.extend_from_slice(&self.count.to_le_bytes());
        e.extend_from_slice(&self.count.to_le_bytes());
        e.extend_from_slice(&size.to_le_bytes());
        e.extend_from_slice(&start.to_le_bytes());
        e.extend_from_slice(&0u16.to_le_bytes());
        self.out
    }
}

/// Reads a zip this module wrote (and simple others): for tests and checking a package.
pub fn read(zip: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    let u16_at = |i: usize| -> Option<u16> { Some(u16::from_le_bytes(zip.get(i..i + 2)?.try_into().ok()?)) };
    let u32_at = |i: usize| -> Option<u32> { Some(u32::from_le_bytes(zip.get(i..i + 4)?.try_into().ok()?)) };
    let end = (0..zip.len().saturating_sub(21)).rev().find(|&i| u32_at(i) == Some(0x0605_4b50))?;
    let count = u16_at(end + 10)? as usize;
    let mut at = u32_at(end + 16)? as usize;
    let mut out = Vec::new();
    for _ in 0..count {
        if u32_at(at)? != 0x0201_4b50 {
            return None;
        }
        let method = u16_at(at + 10)?;
        let packed = u32_at(at + 20)? as usize;
        let n = u16_at(at + 28)? as usize;
        let extra = u16_at(at + 30)? as usize;
        let comment = u16_at(at + 32)? as usize;
        let local = u32_at(at + 42)? as usize;
        let name = String::from_utf8(zip.get(at + 46..at + 46 + n)?.to_vec()).ok()?;
        let ln = u16_at(local + 26)? as usize;
        let le = u16_at(local + 28)? as usize;
        let body = zip.get(local + 30 + ln + le..local + 30 + ln + le + packed)?;
        let data = match method {
            0 => body.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec(body).ok()?,
            _ => return None,
        };
        out.push((name, data));
        at += 46 + n + extra + comment;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_what_it_reads() {
        let mut z = Zip::new();
        z.stored("mimetype", b"application/epub+zip");
        let long = "Он шёл медленно. ".repeat(200);
        z.deflated("OEBPS/текст.xhtml", long.as_bytes());
        let bytes = z.finish();
        // ePub readers check these bytes: mimetype first, stored, at offset 38.
        assert_eq!(&bytes[30..38], b"mimetype");
        assert_eq!(&bytes[38..58], b"application/epub+zip");
        let entries = read(&bytes).unwrap();
        assert_eq!(entries[0], ("mimetype".to_string(), b"application/epub+zip".to_vec()));
        assert_eq!(entries[1].0, "OEBPS/текст.xhtml");
        assert_eq!(entries[1].1, long.as_bytes());
        assert!(bytes.len() < long.len());
    }

    #[test]
    fn the_same_text_makes_the_same_bytes() {
        let make = || {
            let mut z = Zip::new();
            z.deflated("a.xml", b"<a/>");
            z.finish()
        };
        assert_eq!(make(), make());
    }
}
