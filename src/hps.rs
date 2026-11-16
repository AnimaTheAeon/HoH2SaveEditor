//! .hps save container: header, LZ4 blocks, MD5 footer.
//!
//! "H2SV" u32 version u64 time_ms | cstr town_name | u16 (mods<<1|flag) mods*u32 | block(null)
//! block(town) | u32 n | n * { u32 key, block(hero), u32 has_level [cstr level, block(level)] }
//! tail (current-level data, kept verbatim) | first 4 bytes of MD5(everything before)

use crate::sval::{self, V};

pub struct Block {
    raw: Vec<u8>,       // exact on-disk bytes, used when untouched
    pub val: Option<V>, // decoded value (None = null block or not decoded)
    pub err: Option<String>,
    pub dirty: bool,
}

impl Block {
    fn read(b: &[u8], p: &mut usize, decode: bool) -> Result<Block, String> {
        let start = *p;
        let u = rd32(b, p)? as usize;
        let mut val = None;
        let mut err = None;
        if u != 0 {
            let c = rd32(b, p)? as usize;
            let data = slice(b, p, if c == 0 { u } else { c })?;
            if decode {
                let bytes = if c == 0 {
                    Ok(data.to_vec())
                } else {
                    lz4_flex::block::decompress(data, u).map_err(|e| e.to_string())
                };
                match bytes.and_then(|d| sval::decode(&d)) {
                    Ok(v) => val = Some(v),
                    Err(e) => err = Some(e),
                }
            }
        }
        Ok(Block { raw: b[start..*p].to_vec(), val, err, dirty: false })
    }
    fn write(&self, o: &mut Vec<u8>) {
        match (&self.val, self.dirty) {
            (Some(v), true) => {
                let mut d = Vec::new();
                sval::encode(v, &mut d);
                let c = lz4_flex::block::compress(&d);
                o.extend_from_slice(&(d.len() as u32).to_le_bytes());
                o.extend_from_slice(&(c.len() as u32).to_le_bytes());
                o.extend_from_slice(&c);
            }
            _ => o.extend_from_slice(&self.raw),
        }
    }
    pub fn editable(&self) -> bool {
        self.val.is_some() && self.err.is_none()
    }
}

pub struct Hero {
    pub key: u32,
    pub data: Block,
    pub level: Option<(String, Block)>,
}

pub struct Save {
    pub version: u32,
    pub time_ms: u64,
    pub town_name: String,
    mods_raw: Vec<u8>, // u16 + mod hashes + null block, verbatim
    pub town: Block,
    pub heroes: Vec<Hero>,
    tail: Vec<u8>,
}

fn rd32(b: &[u8], p: &mut usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(slice(b, p, 4)?.try_into().unwrap()))
}
fn slice<'a>(b: &'a [u8], p: &mut usize, n: usize) -> Result<&'a [u8], String> {
    if *p + n > b.len() {
        return Err(format!("file truncated at {:#x}", *p));
    }
    let s = &b[*p..*p + n];
    *p += n;
    Ok(s)
}
fn cstr(b: &[u8], p: &mut usize) -> Result<String, String> {
    let e = b[*p..].iter().position(|&c| c == 0).ok_or("bad string")?;
    let s = String::from_utf8_lossy(&b[*p..*p + e]).into_owned();
    *p += e + 1;
    Ok(s)
}

impl Save {
    pub fn parse(b: &[u8]) -> Result<Save, String> {
        if b.len() < 40 || &b[0..4] != b"H2SV" {
            return Err("not a Heroes of Hammerwatch II save (missing H2SV)".into());
        }
        let body = &b[..b.len() - 4];
        if md5(body)[..4] != b[b.len() - 4..] {
            return Err("checksum mismatch - file is damaged".into());
        }
        let mut p = 4;
        let version = rd32(b, &mut p)?;
        if version != 4 {
            return Err(format!("unsupported save version {version} (editor knows 4)"));
        }
        let time_ms = u64::from_le_bytes(slice(b, &mut p, 8)?.try_into().unwrap());
        let town_name = cstr(b, &mut p)?;
        let ms = p;
        let m = u16::from_le_bytes(slice(b, &mut p, 2)?.try_into().unwrap());
        slice(b, &mut p, (m >> 1) as usize * 4)?;
        Block::read(body, &mut p, false)?; // always-null block
        let mods_raw = b[ms..p].to_vec();
        let town = Block::read(body, &mut p, true)?;
        let n = rd32(body, &mut p)?;
        let mut heroes = Vec::new();
        for _ in 0..n {
            let key = rd32(body, &mut p)?;
            let data = Block::read(body, &mut p, true)?;
            let level = if rd32(body, &mut p)? != 0 {
                let name = cstr(body, &mut p)?;
                Some((name, Block::read(body, &mut p, false)?))
            } else {
                None
            };
            heroes.push(Hero { key, data, level });
        }
        let tail = body[p..].to_vec();
        Ok(Save { version, time_ms, town_name, mods_raw, town, heroes, tail })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut o = Vec::with_capacity(1 << 18);
        o.extend_from_slice(b"H2SV");
        o.extend_from_slice(&self.version.to_le_bytes());
        o.extend_from_slice(&self.time_ms.to_le_bytes());
        o.extend_from_slice(self.town_name.as_bytes());
        o.push(0);
        o.extend_from_slice(&self.mods_raw);
        self.town.write(&mut o);
        o.extend_from_slice(&(self.heroes.len() as u32).to_le_bytes());
        for h in &self.heroes {
            o.extend_from_slice(&h.key.to_le_bytes());
            h.data.write(&mut o);
            match &h.level {
                Some((name, blk)) => {
                    o.extend_from_slice(&1u32.to_le_bytes());
                    o.extend_from_slice(name.as_bytes());
                    o.push(0);
                    blk.write(&mut o);
                }
                None => o.extend_from_slice(&0u32.to_le_bytes()),
            }
        }
        o.extend_from_slice(&self.tail);
        let h = md5(&o);
        o.extend_from_slice(&h[..4]);
        o
    }
    pub fn has_run(&self) -> bool {
        !self.tail.is_empty() || self.heroes.iter().any(|h| h.level.is_some())
    }
}

// ---------- MD5 (RFC 1321), tiny implementation ----------
pub fn md5(msg: &[u8]) -> [u8; 16] {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14,
        20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6,
        10, 15, 21,
    ];
    let k: Vec<u32> = (0..64).map(|i| ((i as f64 + 1.0).sin().abs() * 4294967296.0) as u32).collect();
    let mut st = [0x67452301u32, 0xefcdab89, 0x98badcfe, 0x10325476];
    let mut m = msg.to_vec();
    let bits = (msg.len() as u64).wrapping_mul(8);
    m.push(0x80);
    while m.len() % 64 != 56 {
        m.push(0);
    }
    m.extend_from_slice(&bits.to_le_bytes());
    for ch in m.chunks(64) {
        let w: Vec<u32> = ch.chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        let [mut a, mut b, mut c, mut d] = st;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let t = d;
            d = c;
            c = b;
            b = b.wrapping_add(a.wrapping_add(f).wrapping_add(k[i]).wrapping_add(w[g]).rotate_left(S[i]));
            a = t;
        }
        st[0] = st[0].wrapping_add(a);
        st[1] = st[1].wrapping_add(b);
        st[2] = st[2].wrapping_add(c);
        st[3] = st[3].wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for (i, x) in st.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_le_bytes());
    }
    out
}

/// The game's HashString(): MurmurHash3 x86_32 with seed 0xF808409C.
pub fn hash_string(s: &str) -> u32 {
    let d = s.as_bytes();
    let (c1, c2) = (0xcc9e2d51u32, 0x1b873593u32);
    let mut h = 0xf808409cu32;
    let n = d.len() / 4;
    for i in 0..n {
        let mut k = u32::from_le_bytes(d[i * 4..i * 4 + 4].try_into().unwrap());
        k = k.wrapping_mul(c1).rotate_left(15).wrapping_mul(c2);
        h = (h ^ k).rotate_left(13).wrapping_mul(5).wrapping_add(0xe6546b64);
    }
    let t = &d[n * 4..];
    let mut k = 0u32;
    if t.len() >= 3 {
        k ^= (t[2] as u32) << 16;
    }
    if t.len() >= 2 {
        k ^= (t[1] as u32) << 8;
    }
    if !t.is_empty() {
        k ^= t[0] as u32;
        h ^= k.wrapping_mul(c1).rotate_left(15).wrapping_mul(c2);
    }
    h ^= d.len() as u32;
    h ^= h >> 16;
    h = h.wrapping_mul(0x85ebca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2ae35);
    h ^ (h >> 16)
}

/// Self-test: every decoded block must re-encode to identical bytes,
/// and an all-dirty rewrite must parse back to identical values.
pub fn selftest(b: &[u8]) -> Result<String, String> {
    let s = Save::parse(b)?;
    if s.to_bytes() != b {
        return Err("untouched rewrite differs".into());
    }
    let mut blocks: Vec<&Block> = vec![&s.town];
    blocks.extend(s.heroes.iter().map(|h| &h.data));
    for blk in &blocks {
        if let Some(e) = &blk.err {
            return Err(format!("block decode error: {e}"));
        }
        let mut p = 0;
        let u = rd32(&blk.raw, &mut p)? as usize;
        let c = rd32(&blk.raw, &mut p)? as usize;
        let orig = lz4_flex::block::decompress(&blk.raw[8..8 + c], u).map_err(|e| e.to_string())?;
        let mut re = Vec::new();
        sval::encode(blk.val.as_ref().unwrap(), &mut re);
        if re != orig {
            let i = re.iter().zip(&orig).position(|(a, b)| a != b).unwrap_or(re.len().min(orig.len()));
            return Err(format!("re-encode mismatch at {i:#x}"));
        }
    }
    let mut s2 = Save::parse(b)?;
    s2.town.dirty = true;
    for h in &mut s2.heroes {
        h.data.dirty = true;
    }
    let out = s2.to_bytes();
    let s3 = Save::parse(&out)?;
    if s3.town.val != s.town.val || s3.heroes.iter().zip(&s.heroes).any(|(a, b)| a.data.val != b.data.val) {
        return Err("dirty rewrite changed values".into());
    }
    Ok(format!("ok: {} heroes, run={}, {} bytes", s.heroes.len(), s.has_run(), b.len()))
}
