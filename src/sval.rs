//! SValue binary codec used by Heroes of Hammerwatch II saves.
//! Tags and compact encodings mirror the game's own serializer exactly,
//! so unmodified data re-encodes byte-for-byte.

#[derive(Clone, Debug, PartialEq)]
pub enum V {
    Null,
    Arr(Vec<V>),
    Dict(Vec<(String, V)>),
    Bool(bool),
    Float(f32),
    Int(i32),
    Str(String),
    Bytes(Vec<u8>),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Long(i64),
    Double(f64),
}

pub struct Reader<'a> {
    b: &'a [u8],
    pub p: usize,
}

type R<T> = Result<T, String>;

impl<'a> Reader<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Reader { b, p: 0 }
    }
    fn take(&mut self, n: usize) -> R<&'a [u8]> {
        if self.p + n > self.b.len() {
            return Err(format!("unexpected end of data at {:#x}", self.p));
        }
        let s = &self.b[self.p..self.p + n];
        self.p += n;
        Ok(s)
    }
    fn u8(&mut self) -> R<u8> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> R<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> R<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn fs<const N: usize>(&mut self) -> R<[f32; N]> {
        let mut a = [0f32; N];
        for x in a.iter_mut() {
            *x = self.f32()?;
        }
        Ok(a)
    }
    fn cstr(&mut self) -> R<String> {
        let rest = &self.b[self.p..];
        let e = rest.iter().position(|&c| c == 0).ok_or("unterminated string")?;
        let s = String::from_utf8_lossy(&rest[..e]).into_owned();
        self.p += e + 1;
        Ok(s)
    }
    pub fn value(&mut self) -> R<V> {
        let t = self.u8()?;
        Ok(match t {
            0x00 => V::Null,
            0x01 => {
                let n = self.u32()? as usize;
                let mut v = Vec::with_capacity(n.min(1 << 16));
                for _ in 0..n {
                    v.push(self.value()?);
                }
                V::Arr(v)
            }
            0x02 => {
                let n = self.u32()? as usize;
                let mut v = Vec::with_capacity(n.min(1 << 16));
                for _ in 0..n {
                    let k = self.cstr()?;
                    v.push((k, self.value()?));
                }
                V::Dict(v)
            }
            0x04 => V::Float(self.f32()?),
            0x05 => V::Int(self.u32()? as i32),
            0x06 => V::Str(self.cstr()?),
            0x07 => {
                let n = self.u32()? as usize;
                V::Bytes(self.take(n)?.to_vec())
            }
            0x08 => V::Vec2(self.fs()?),
            0x09 => V::Vec3(self.fs()?),
            0x0a => V::Vec4(self.fs()?),
            0x0b => V::Long(i64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            0x10 => V::Double(f64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            0x65 => V::Arr(vec![]),
            0x66 => V::Dict(vec![]),
            0x67 => V::Bool(true),
            0x68 => V::Bool(false),
            0x69 => V::Float(0.0),
            0x6a => V::Double(0.0),
            0x6e => V::Int(0),
            0x73 => V::Long(0),
            0x78 => V::Str(String::new()),
            0x79 => V::Bytes(vec![]),
            0x7a => V::Vec2([0.0; 2]),
            0x7b => V::Vec2([self.f32()?, 0.0]),
            0x8c => {
                let x = self.f32()?;
                V::Vec2([x, x])
            }
            0x7c => V::Vec3([0.0; 3]),
            0x8d => {
                let x = self.f32()?;
                V::Vec3([x; 3])
            }
            0x7f => V::Vec4([0.0; 4]),
            0x8e => {
                let x = self.f32()?;
                V::Vec4([x; 4])
            }
            _ => return Err(format!("unsupported value tag {t:#04x} at {:#x}", self.p - 1)),
        })
    }
}

pub fn decode(b: &[u8]) -> R<V> {
    let mut r = Reader::new(b);
    let v = r.value()?;
    if r.p != b.len() {
        return Err(format!("{} trailing bytes", b.len() - r.p));
    }
    Ok(v)
}

fn wf(o: &mut Vec<u8>, a: &[f32]) {
    for x in a {
        o.extend_from_slice(&x.to_le_bytes());
    }
}

pub fn encode(v: &V, o: &mut Vec<u8>) {
    match v {
        V::Null => o.push(0),
        V::Arr(a) if a.is_empty() => o.push(0x65),
        V::Arr(a) => {
            o.push(1);
            o.extend_from_slice(&(a.len() as u32).to_le_bytes());
            for x in a {
                encode(x, o);
            }
        }
        V::Dict(d) if d.is_empty() => o.push(0x66),
        V::Dict(d) => {
            o.push(2);
            o.extend_from_slice(&(d.len() as u32).to_le_bytes());
            for (k, x) in d {
                o.extend_from_slice(k.as_bytes());
                o.push(0);
                encode(x, o);
            }
        }
        V::Bool(b) => o.push(if *b { 0x67 } else { 0x68 }),
        V::Float(f) if *f == 0.0 => o.push(0x69),
        V::Float(f) => {
            o.push(4);
            wf(o, &[*f]);
        }
        V::Int(0) => o.push(0x6e),
        V::Int(i) => {
            o.push(5);
            o.extend_from_slice(&i.to_le_bytes());
        }
        V::Str(s) if s.is_empty() => o.push(0x78),
        V::Str(s) => {
            o.push(6);
            o.extend_from_slice(s.as_bytes());
            o.push(0);
        }
        V::Bytes(b) if b.is_empty() => o.push(0x79),
        V::Bytes(b) => {
            o.push(7);
            o.extend_from_slice(&(b.len() as u32).to_le_bytes());
            o.extend_from_slice(b);
        }
        V::Vec2([x, y]) => {
            if x == y {
                if *x == 0.0 {
                    o.push(0x7a)
                } else {
                    o.push(0x8c);
                    wf(o, &[*x]);
                }
            } else if *y == 0.0 {
                o.push(0x7b);
                wf(o, &[*x]);
            } else {
                o.push(8);
                wf(o, &[*x, *y]);
            }
        }
        V::Vec3(a) => {
            if a.iter().all(|x| *x == a[0]) {
                if a[0] == 0.0 {
                    o.push(0x7c)
                } else {
                    o.push(0x8d);
                    wf(o, &a[..1]);
                }
            } else {
                o.push(9);
                wf(o, a);
            }
        }
        V::Vec4(a) => {
            if a.iter().all(|x| *x == a[0]) {
                if a[0] == 0.0 {
                    o.push(0x7f)
                } else {
                    o.push(0x8e);
                    wf(o, &a[..1]);
                }
            } else {
                o.push(0x0a);
                wf(o, a);
            }
        }
        V::Long(0) => o.push(0x73),
        V::Long(l) => {
            o.push(0x0b);
            o.extend_from_slice(&l.to_le_bytes());
        }
        V::Double(d) if *d == 0.0 => o.push(0x6a),
        V::Double(d) => {
            o.push(0x10);
            o.extend_from_slice(&d.to_le_bytes());
        }
    }
}

// ---- convenience accessors ----
impl V {
    pub fn get(&self, k: &str) -> Option<&V> {
        match self {
            V::Dict(d) => d.iter().find(|(n, _)| n == k).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn get_mut(&mut self, k: &str) -> Option<&mut V> {
        match self {
            V::Dict(d) => d.iter_mut().find(|(n, _)| n == k).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn int(&self) -> Option<i64> {
        match self {
            V::Int(i) => Some(*i as i64),
            V::Long(l) => Some(*l),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            V::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn arr(&self) -> Option<&Vec<V>> {
        match self {
            V::Arr(a) => Some(a),
            _ => None,
        }
    }
    pub fn arr_mut(&mut self) -> Option<&mut Vec<V>> {
        match self {
            V::Arr(a) => Some(a),
            _ => None,
        }
    }
    pub fn kind(&self) -> &'static str {
        match self {
            V::Null => "null",
            V::Arr(_) => "array",
            V::Dict(_) => "dict",
            V::Bool(_) => "bool",
            V::Float(_) => "float",
            V::Int(_) => "int",
            V::Str(_) => "string",
            V::Bytes(_) => "bytes",
            V::Vec2(_) => "vec2",
            V::Vec3(_) => "vec3",
            V::Vec4(_) => "vec4",
            V::Long(_) => "long",
            V::Double(_) => "double",
        }
    }
}
