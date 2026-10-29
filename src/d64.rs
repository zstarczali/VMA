//! A 1541 disk image (D64, 35 tracks) with PRG files, without c1541 — the UltimateBasic compiler's
//! `build_d64` (`ub build … --d64`).
//!
//! Layout as the 1541 DOS writes it: BAM at 18/0, directory from 18/1 (8 entries of 32 bytes per
//! sector), file data on tracks 17 → 1, then 19 → 35, with the DOS interleave of 10 sectors.

use std::fmt;

pub const TRACKS: usize = 35;
pub const SECTORS: usize = 683;
pub const IMAGE_SIZE: usize = SECTORS * 256;
/// the directory of a 1541 disk: 18 sectors × 8 entries
pub const MAX_FILES: usize = 144;
const INTERLEAVE: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum D64Error {
    TooManyFiles(usize),
    /// the file that did not fit
    DiskFull(String),
}

impl fmt::Display for D64Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            D64Error::TooManyFiles(n) => write!(f, "too many files ({n}), a D64 directory holds {MAX_FILES}"),
            D64Error::DiskFull(name) => write!(f, "disk full while writing {name} (664 blocks max)"),
        }
    }
}

impl std::error::Error for D64Error {}

pub fn sectors_for_track(t: usize) -> usize {
    match t {
        1..=17 => 21,
        18..=24 => 19,
        25..=30 => 18,
        _ => 17,
    }
}

/// Byte offset of sector `s` on track `t` (1-based) in the image.
pub fn sector_offset(t: usize, s: usize) -> usize {
    ((1..t).map(sectors_for_track).sum::<usize>() + s) * 256
}

/// A name as the VA's c1541 export writes it (`sanitize_disk_name`): ASCII without `" , / \ :`,
/// at most 16 characters, in upper case (PETSCII capitals); "DISK" when nothing is left.
pub fn petscii_name(name: &str) -> Vec<u8> {
    let bytes: Vec<u8> = name
        .chars()
        .filter(|c| c.is_ascii() && !c.is_ascii_control() && !matches!(c, '"' | ',' | '/' | '\\' | ':'))
        .take(16)
        .map(|c| c.to_ascii_uppercase() as u8)
        .collect();
    if bytes.is_empty() { b"DISK".to_vec() } else { bytes }
}

fn padded(name: &[u8]) -> [u8; 16] {
    let mut out = [0xA0; 16];
    out[..name.len().min(16)].copy_from_slice(&name[..name.len().min(16)]);
    out
}

/// The image with `files` (name, PRG bytes with their load address) as PRG entries in this order;
/// `disk_name` / `disk_id` (two characters) in the header.
pub fn build_d64(disk_name: &str, disk_id: &str, files: &[(&str, &[u8])]) -> Result<Vec<u8>, D64Error> {
    if files.len() > MAX_FILES {
        return Err(D64Error::TooManyFiles(files.len()));
    }
    let mut disk = vec![0u8; IMAGE_SIZE];
    let mut used = vec![[false; 21]; TRACKS + 1];

    // track 18: the BAM + the directory sectors
    let dir_sectors = files.len().div_ceil(8).max(1);
    for s in 0..=dir_sectors {
        used[18][s] = true;
    }

    // data sectors: tracks 17 down to 1, then 19 up to 35, interleave 10
    let track_order: Vec<usize> = (1..=17).rev().chain(19..=TRACKS).collect();
    let (mut ti, mut next) = (0usize, 0usize);
    let mut chains: Vec<Vec<(usize, usize)>> = Vec::with_capacity(files.len());
    for (name, data) in files {
        let count = data.len().div_ceil(254).max(1);
        let mut chain = Vec::with_capacity(count);
        for _ in 0..count {
            // a free sector on the current track from `next` on, else the next track
            let found = loop {
                let Some(&t) = track_order.get(ti) else { break None };
                let spt = sectors_for_track(t);
                if let Some(k) = (0..spt).find(|k| !used[t][(next + k) % spt]) {
                    break Some((t, (next + k) % spt));
                }
                ti += 1;
                next = 0;
            };
            let Some((t, s)) = found else {
                return Err(D64Error::DiskFull(String::from_utf8_lossy(&petscii_name(name)).into_owned()));
            };
            used[t][s] = true;
            chain.push((t, s));
            next = (s + INTERLEAVE) % sectors_for_track(t);
        }
        chains.push(chain);
    }

    // the data: each sector links the next one; the last holds the offset of its last byte
    for ((_, data), chain) in files.iter().zip(&chains) {
        for (i, &(t, s)) in chain.iter().enumerate() {
            let o = sector_offset(t, s);
            let part = &data[(i * 254).min(data.len())..((i + 1) * 254).min(data.len())];
            match chain.get(i + 1) {
                Some(&(nt, ns)) => {
                    disk[o] = nt as u8;
                    disk[o + 1] = ns as u8;
                }
                None => {
                    disk[o] = 0;
                    disk[o + 1] = (part.len() + 1) as u8;
                }
            }
            disk[o + 2..o + 2 + part.len()].copy_from_slice(part);
        }
    }

    // BAM: directory link, DOS version 'A', a free count + bitmap per track, name, ID, "2A"
    let bam = sector_offset(18, 0);
    disk[bam..bam + 3].copy_from_slice(&[18, 1, 0x41]);
    for t in 1..=TRACKS {
        let p = bam + 4 + (t - 1) * 4;
        let mut free = 0u8;
        for s in 0..sectors_for_track(t) {
            if !used[t][s] {
                free += 1;
                disk[p + 1 + s / 8] |= 1 << (s % 8);
            }
        }
        disk[p] = free;
    }
    disk[bam + 0x90..bam + 0xA0].copy_from_slice(&padded(&petscii_name(disk_name)));
    let id: Vec<u8> = disk_id.bytes().map(|b| b.to_ascii_uppercase()).chain(std::iter::repeat(b'0')).take(2).collect();
    disk[bam + 0xA0..bam + 0xAB].copy_from_slice(&[0xA0, 0xA0, id[0], id[1], 0xA0, b'2', b'A', 0xA0, 0xA0, 0xA0, 0xA0]);

    // the directory: 8 entries per sector, the first two bytes of a sector link the next one
    for ds in 0..dir_sectors {
        let dir = sector_offset(18, ds + 1);
        if ds + 1 < dir_sectors {
            disk[dir] = 18;
            disk[dir + 1] = (ds + 2) as u8;
        } else {
            disk[dir] = 0;
            disk[dir + 1] = 0xFF;
        }
        for (ei, fi) in (ds * 8..files.len().min(ds * 8 + 8)).enumerate() {
            let de = dir + ei * 32;
            let chain = &chains[fi];
            disk[de + 2] = 0x82; // PRG, closed
            disk[de + 3] = chain[0].0 as u8;
            disk[de + 4] = chain[0].1 as u8;
            disk[de + 5..de + 21].copy_from_slice(&padded(&petscii_name(files[fi].0)));
            disk[de + 30] = chain.len() as u8;
            disk[de + 31] = (chain.len() >> 8) as u8;
        }
    }
    Ok(disk)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The files of an image through its directory and sector chains: (name, bytes, blocks).
    fn read_back(disk: &[u8]) -> Vec<(Vec<u8>, Vec<u8>, usize)> {
        let mut out = Vec::new();
        let (mut t, mut s) = (18usize, 1usize);
        while t != 0 {
            let dir = sector_offset(t, s);
            for e in 0..8 {
                let de = dir + e * 32;
                if disk[de + 2] != 0x82 {
                    continue;
                }
                let name: Vec<u8> = disk[de + 5..de + 21].iter().copied().take_while(|&b| b != 0xA0).collect();
                let (mut ft, mut fs) = (disk[de + 3] as usize, disk[de + 4] as usize);
                let mut bytes = Vec::new();
                while ft != 0 {
                    let o = sector_offset(ft, fs);
                    let (nt, ns) = (disk[o] as usize, disk[o + 1] as usize);
                    bytes.extend_from_slice(&disk[o + 2..if nt == 0 { o + ns + 1 } else { o + 256 }]);
                    (ft, fs) = (nt, ns);
                }
                out.push((name, bytes, disk[de + 30] as usize | (disk[de + 31] as usize) << 8));
            }
            (t, s) = (disk[dir] as usize, disk[dir + 1] as usize);
        }
        out
    }

    #[test]
    fn files_read_back_through_the_directory() {
        let big: Vec<u8> = (0..20000u32).map(|i| (i * 7) as u8).collect();
        let files: Vec<(String, Vec<u8>)> = (0..10).map(|i| (format!("file{i}"), if i == 3 { big.clone() } else { vec![0x01, 0x08, i as u8] })).collect();
        let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
        let disk = build_d64("my disk", "01", &refs).unwrap();
        assert_eq!(disk.len(), IMAGE_SIZE);
        let back = read_back(&disk);
        assert_eq!(back.len(), 10);
        for ((name, data), (bname, bdata, blocks)) in files.iter().zip(&back) {
            assert_eq!(bname, &name.to_uppercase().into_bytes());
            assert_eq!(bdata, data);
            assert_eq!(*blocks, data.len().div_ceil(254));
        }
        let bam = sector_offset(18, 0);
        assert_eq!(&disk[bam + 0x90..bam + 0x98], b"MY DISK\xA0");
        assert_eq!(&disk[bam + 0xA2..bam + 0xA7], b"01\xA02A");
        // 664 free blocks outside track 18, minus the files' blocks
        let free: usize = (1..=TRACKS).filter(|&t| t != 18).map(|t| disk[bam + 4 + (t - 1) * 4] as usize).sum();
        assert_eq!(free, 664 - back.iter().map(|f| f.2).sum::<usize>());
        // the first file starts on track 17, sector 0; its next sector is 10 further on
        assert_eq!((disk[sector_offset(18, 1) + 3], disk[sector_offset(18, 1) + 4]), (17, 0));
    }

    #[test]
    fn names_limits_and_full_disk() {
        assert_eq!(petscii_name("a\"b,c:d/e\\f"), b"ABCDEF".to_vec());
        assert_eq!(petscii_name(""), b"DISK".to_vec());
        assert_eq!(petscii_name("a-very-long-file-name"), b"A-VERY-LONG-FILE".to_vec());
        let one = [0u8; 3];
        let many: Vec<(&str, &[u8])> = (0..145).map(|_| ("x", &one[..])).collect();
        assert_eq!(build_d64("d", "01", &many), Err(D64Error::TooManyFiles(145)));
        let huge = vec![0u8; 665 * 254];
        assert_eq!(build_d64("d", "01", &[("big", &huge)]), Err(D64Error::DiskFull("BIG".into())));
        assert!(build_d64("d", "01", &[("max", &huge[..664 * 254])]).is_ok());
    }
}
