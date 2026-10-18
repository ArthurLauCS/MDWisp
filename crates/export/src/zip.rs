//! A minimal ZIP writer.
//!
//! Hand-rolled rather than pulled from a crate: the format is small, stable and
//! fully specified, and `miniz_oxide` already supplies the only hard part (raw
//! deflate). A zip crate here would buy nothing but dependency surface for a
//! file this process writes once and never reads back.

use chrono::{Datelike, NaiveDateTime, Timelike};

pub struct ZipEntry {
    /// Forward-slash path inside the archive. Never absolute.
    pub name: String,
    pub data: Vec<u8>,
    /// Directory entries carry no bytes; they exist so empty folders survive.
    pub directory: bool,
}

const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[n] = c;
        n += 1;
    }
    table
};

fn crc32(buf: &[u8]) -> u32 {
    !buf.iter().fold(!0u32, |c, &b| CRC_TABLE[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8))
}

/// MS-DOS date/time, which is what the archive header has always stored.
fn dos_stamp(d: NaiveDateTime) -> (u16, u16) {
    let time = (d.hour() << 11) | (d.minute() << 5) | (d.second() / 2);
    let date = ((d.year() as u32 - 1980) << 9) | (d.month() << 5) | d.day();
    (time as u16, date as u16)
}

/// `now` is local time: archive tools show the stamp as-is.
pub fn build_zip(entries: &[ZipEntry], now: NaiveDateTime) -> Vec<u8> {
    let (time, date) = dos_stamp(now);
    let mut out = Vec::new();
    let mut central = Vec::new();

    for entry in entries {
        let name = entry.name.as_bytes();
        let raw = &entry.data;
        let crc = crc32(raw);

        // Storing beats deflating when the file is already compressed or tiny:
        // deflate carries a per-stream header and can grow the payload.
        let deflated = miniz_oxide::deflate::compress_to_vec(raw, 9);
        let use_deflate = !entry.directory && deflated.len() < raw.len();
        let body = if use_deflate { &deflated } else { raw };
        let method: u16 = if use_deflate { 8 } else { 0 };
        let offset = out.len() as u32;

        // Shared tail of both headers, from "method" through "name length".
        let mut common = Vec::with_capacity(20);
        common.extend(method.to_le_bytes());
        common.extend(time.to_le_bytes());
        common.extend(date.to_le_bytes());
        common.extend(crc.to_le_bytes());
        common.extend((body.len() as u32).to_le_bytes());
        common.extend((raw.len() as u32).to_le_bytes());
        common.extend((name.len() as u16).to_le_bytes());

        out.extend(0x0403_4b50u32.to_le_bytes());
        out.extend(20u16.to_le_bytes()); // version needed
        // Bit 11: entry names are UTF-8. Without it Chinese filenames mojibake
        // in tools that read the flag instead of guessing.
        out.extend(0x0800u16.to_le_bytes());
        out.extend(&common);
        out.extend(0u16.to_le_bytes()); // extra field length
        out.extend(name);
        out.extend(body);

        central.extend(0x0201_4b50u32.to_le_bytes());
        central.extend(20u16.to_le_bytes()); // version made by
        central.extend(20u16.to_le_bytes()); // version needed
        central.extend(0x0800u16.to_le_bytes());
        central.extend(&common);
        central.extend([0u8; 8]); // extra, comment, disk number, internal attrs
        // External attrs: 040755 for a directory, 0100644 for a file. Unix hosts
        // read the high 16 bits, so an empty folder is not lost on extraction.
        central.extend((if entry.directory { 0x41ed_0010u32 } else { 0x81a4_0000 }).to_le_bytes());
        central.extend(offset.to_le_bytes());
        central.extend(name);
    }

    let central_at = out.len() as u32;
    let count = entries.len() as u16;
    out.extend(&central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]); // this disk, disk with central directory
    out.extend(count.to_le_bytes());
    out.extend(count.to_le_bytes());
    out.extend((central.len() as u32).to_le_bytes());
    out.extend(central_at.to_le_bytes());
    out.extend(0u16.to_le_bytes()); // comment length
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn now() -> NaiveDateTime {
        chrono::Local::now().naive_local()
    }

    fn u16_at(b: &[u8], at: usize) -> usize {
        u16::from_le_bytes([b[at], b[at + 1]]) as usize
    }

    fn u32_at(b: &[u8], at: usize) -> usize {
        u32::from_le_bytes(b[at..at + 4].try_into().unwrap()) as usize
    }

    pub struct ReadEntry {
        pub name: String,
        pub flags: usize,
        pub method: usize,
        pub crc: usize,
        pub data: Vec<u8>,
    }

    /// Written from the format spec, not from the writer's helpers — a writer
    /// and reader that share a bug would agree with each other and both be wrong.
    pub fn read_zip(buf: &[u8]) -> Vec<ReadEntry> {
        let end_at = buf.windows(4).rposition(|w| w == [0x50, 0x4b, 0x05, 0x06]).unwrap();
        let count = u16_at(buf, end_at + 10);
        let central_size = u32_at(buf, end_at + 12);
        let central_at = u32_at(buf, end_at + 16);
        assert_eq!(central_at + central_size, end_at);

        let mut out = Vec::new();
        let mut p = central_at;
        for _ in 0..count {
            assert_eq!(u32_at(buf, p), 0x0201_4b50);
            let flags = u16_at(buf, p + 8);
            let method = u16_at(buf, p + 10);
            let crc = u32_at(buf, p + 16);
            let compressed = u32_at(buf, p + 20);
            let uncompressed = u32_at(buf, p + 24);
            let name_len = u16_at(buf, p + 28);
            let extra_len = u16_at(buf, p + 30);
            let comment_len = u16_at(buf, p + 32);
            let local_at = u32_at(buf, p + 42);
            let name = String::from_utf8(buf[p + 46..p + 46 + name_len].to_vec()).unwrap();

            // The local header must agree with the central directory, and it
            // must actually sit where the directory says it does.
            assert_eq!(u32_at(buf, local_at), 0x0403_4b50);
            let local_name_len = u16_at(buf, local_at + 26);
            let local_extra_len = u16_at(buf, local_at + 28);
            assert_eq!(u16_at(buf, local_at + 8), method);
            assert_eq!(u32_at(buf, local_at + 14), crc);
            assert_eq!(&buf[local_at + 30..local_at + 30 + local_name_len], name.as_bytes());

            let start = local_at + 30 + local_name_len + local_extra_len;
            let body = &buf[start..start + compressed];
            let data = if method == 8 { miniz_oxide::inflate::decompress_to_vec(body).unwrap() } else { body.to_vec() };
            assert_eq!(data.len(), uncompressed);

            out.push(ReadEntry { name, flags, method, crc, data });
            p += 46 + name_len + extra_len + comment_len;
        }
        out
    }

    fn entry(name: &str, text: &str) -> ZipEntry {
        ZipEntry { name: name.into(), data: text.as_bytes().to_vec(), directory: false }
    }

    #[test]
    fn round_trips_a_single_entry() {
        let zip = build_zip(&[entry("a.md", "# 标题")], now());
        let read = read_zip(&zip);
        assert_eq!(read[0].name, "a.md");
        assert_eq!(read[0].data, "# 标题".as_bytes());
    }

    #[test]
    fn preserves_a_chinese_filename_byte_for_byte() {
        let zip = build_zip(&[entry("发布说明_img/架构草图.png", "x")], now());
        let read = read_zip(&zip);
        assert_eq!(read[0].name, "发布说明_img/架构草图.png");
        assert_eq!(read[0].flags & 0x0800, 0x0800);
    }

    #[test]
    fn keeps_several_entries_distinct_and_ordered() {
        let zip = build_zip(
            &[entry("发布说明.md", "body"), entry("发布说明_img/a.png", "png"), entry("另一篇.md", "other")],
            now(),
        );
        let names: Vec<_> = read_zip(&zip).into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["发布说明.md", "发布说明_img/a.png", "另一篇.md"]);
    }

    #[test]
    fn deflates_text_but_stores_incompressible_bytes() {
        // xorshift: incompressible enough, and reproducible.
        let mut s = 0x2545_f491u32;
        let noise: Vec<u8> = (0..4096)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                s as u8
            })
            .collect();
        let zip = build_zip(
            &[entry("notes.md", &"x".repeat(4096)), ZipEntry { name: "noise.bin".into(), data: noise.clone(), directory: false }],
            now(),
        );
        let read = read_zip(&zip);
        assert_eq!(read[0].method, 8);
        assert_eq!(read[1].method, 0);
        assert_eq!(read[1].data, noise);
    }

    #[test]
    fn writes_a_directory_entry_that_survives_as_an_empty_folder() {
        let zip = build_zip(&[ZipEntry { name: "空文件夹/".into(), data: vec![], directory: true }], now());
        let read = read_zip(&zip);
        assert_eq!(read[0].name, "空文件夹/");
        assert!(read[0].data.is_empty());
    }

    #[test]
    fn handles_an_empty_archive() {
        assert!(read_zip(&build_zip(&[], now())).is_empty());
    }

    #[test]
    fn records_a_crc_that_matches_the_uncompressed_bytes() {
        let read = read_zip(&build_zip(&[entry("a.md", "hello")], now()));
        // Independently: crc32("hello") is a known constant.
        assert_eq!(read[0].crc, 0x3610_a686);
    }
}
