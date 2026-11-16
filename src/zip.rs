//! Minimal "stored" ZIP writer/reader (saves are already LZ4-compressed,
//! so deflate would gain little). Output opens in Windows Explorer.

fn crc32(d: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in d {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
        }
    }
    !c
}

pub fn write(files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let (mut out, mut cd) = (Vec::new(), Vec::new());
    let le16 = |v: &mut Vec<u8>, x: u16| v.extend_from_slice(&x.to_le_bytes());
    let le32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
    for (name, data) in files {
        let (crc, off, n) = (crc32(data), out.len() as u32, data.len() as u32);
        le32(&mut out, 0x0403_4b50);
        for x in [20, 0, 0, 0, 0x21] {
            le16(&mut out, x); // version, flags, stored, time, date(1980-01-01)
        }
        for x in [crc, n, n] {
            le32(&mut out, x);
        }
        le16(&mut out, name.len() as u16);
        le16(&mut out, 0);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);

        le32(&mut cd, 0x0201_4b50);
        for x in [20, 20, 0, 0, 0, 0x21] {
            le16(&mut cd, x);
        }
        for x in [crc, n, n] {
            le32(&mut cd, x);
        }
        for x in [name.len() as u16, 0, 0, 0, 0] {
            le16(&mut cd, x);
        }
        le32(&mut cd, 0);
        le32(&mut cd, off);
        cd.extend_from_slice(name.as_bytes());
    }
    let (cd_off, cd_len, n) = (out.len() as u32, cd.len() as u32, files.len() as u16);
    out.extend_from_slice(&cd);
    le32(&mut out, 0x0605_4b50);
    for x in [0, 0, n, n] {
        le16(&mut out, x);
    }
    le32(&mut out, cd_len);
    le32(&mut out, cd_off);
    le16(&mut out, 0);
    out
}

/// Reads stored entries (the kind `write` produces). CRCs are checked.
pub fn read(z: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut v = vec![];
    let mut p = 0;
    let u16_at = |p: usize| u16::from_le_bytes([z[p], z[p + 1]]) as usize;
    let u32_at = |p: usize| u32::from_le_bytes(z[p..p + 4].try_into().unwrap());
    while p + 30 <= z.len() && u32_at(p) == 0x0403_4b50 {
        if u16_at(p + 8) != 0 {
            return Err("zip entry is compressed; only editor backups can be restored".into());
        }
        let (crc, n, nl, xl) = (u32_at(p + 14), u32_at(p + 18) as usize, u16_at(p + 26), u16_at(p + 28));
        let name = String::from_utf8_lossy(&z[p + 30..p + 30 + nl]).into_owned();
        let s = p + 30 + nl + xl;
        let data = z.get(s..s + n).ok_or("zip truncated")?.to_vec();
        if crc32(&data) != crc {
            return Err(format!("{name}: CRC mismatch"));
        }
        v.push((name, data));
        p = s + n;
    }
    Ok(v)
}
