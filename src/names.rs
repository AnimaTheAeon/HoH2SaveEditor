//! Builds a hash -> name dictionary from the game's own data files, so the
//! editor shows "Decaying Hand" instead of 0x7f3a19c2. Nothing is bundled.

use crate::hps::hash_string;
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Trinket,
    Equipment,
    Building,
    Other,
}

#[derive(Clone)]
pub struct Def {
    pub id: String,
    pub name: String,
    pub kind: Kind,
}

#[derive(Default)]
pub struct Names {
    pub by_hash: HashMap<u32, String>, // any string seen in game data
    pub defs: HashMap<u32, Def>,       // things with an id + display name
    lang: HashMap<String, String>,
}

fn between<'a>(s: &'a str, from: usize, open: &str, close: char) -> Option<(&'a str, usize)> {
    let i = s[from..].find(open)? + from + open.len();
    let j = s[i..].find(close)? + i;
    Some((&s[i..j], j))
}

impl Names {
    pub fn load(game_dir: &Path) -> Result<Names, String> {
        let mut n = Names::default();
        let lang_dir = game_dir.join("res/language/english");
        if let Ok(rd) = std::fs::read_dir(&lang_dir) {
            for e in rd.flatten() {
                if let Ok(t) = std::fs::read_to_string(e.path()) {
                    let mut p = 0;
                    while let Some((k, e1)) = between(&t, p, "<string name=\"", '"') {
                        let Some((v, e2)) = between(&t, e1, ">", '<') else { break };
                        n.lang.insert(k.to_string(), v.to_string());
                        p = e2;
                    }
                }
            }
        }
        let bin = std::fs::read(game_dir.join("res/assets.bin")).map_err(|e| format!("assets.bin: {e}"))?;
        let mut i = 4;
        while i + 4 < bin.len() {
            let l = bin[i] as usize;
            if l >= 5 && bin[i + 1] == 0 && bin[i + 2] == 0 && bin[i + 3] == 0 && i + 13 + l < bin.len() && bin[i + 4 + l] == 1 {
                let nm = &bin[i + 4..i + 4 + l];
                if nm.iter().all(|c| c.is_ascii_alphanumeric() || b"_-/. ".contains(c)) {
                    let name = String::from_utf8_lossy(nm).into_owned();
                    let p = i + 5 + l;
                    let sz = u32::from_le_bytes(bin[p..p + 4].try_into().unwrap()) as usize;
                    let csz = u32::from_le_bytes(bin[p + 4..p + 8].try_into().unwrap()) as usize;
                    let len = if csz > 0 { csz } else { sz };
                    if p + 8 + len <= bin.len() && sz < 64 << 20 {
                        let src = &bin[p + 8..p + 8 + len];
                        let text = if name.ends_with(".sval") || name.ends_with(".inc") || name.ends_with(".unit") || name.ends_with(".as") {
                            if csz > 0 { lz4_flex::block::decompress(src, sz).ok() } else { Some(src.to_vec()) }
                        } else {
                            None
                        };
                        if let Some(t) = text {
                            n.scan(&name, &String::from_utf8_lossy(&t));
                        }
                        i = p + 8 + len;
                        continue;
                    }
                }
            }
            i += 1;
        }
        Ok(n)
    }

    fn scan(&mut self, path: &str, s: &str) {
        let kind = if path.starts_with("tweak/trinkets/") {
            Kind::Trinket
        } else if path.starts_with("tweak/items/equipment/equipment_") {
            Kind::Equipment
        } else if path.starts_with("tweak/town/buildings/") {
            Kind::Building
        } else {
            Kind::Other
        };
        // every quoted or tagged token
        for part in s.split(|c| c == '"' || c == '<' || c == '>') {
            if (2..=80).contains(&part.len()) && !part.contains(char::is_whitespace) {
                self.by_hash.entry(hash_string(part)).or_insert_with(|| part.to_string());
            }
        }
        // <string name="id">X</string> ... <string name="name">.lang.key</string>
        let mut p = 0;
        // a file's main definition (e.g. skills/wiz_meteor.sval, id "wiz_meteor"):
        // its display name is the file's first "name", which may come before the id
        let stem = path.rsplit('/').next().unwrap_or("").split('.').next().unwrap_or("");
        let first_name = between(s, 0, "name=\"name\">", '<').map(|(v, _)| v.to_string());
        while let Some((id, e)) = between(s, p, "name=\"id\">", '<') {
            p = e;
            let win_end = s[e..].find("name=\"id\">").map(|x| x + e).unwrap_or(s.len()).min(e + 1500);
            let first_id = between(s, 0, "name=\"id\">", '<').map(|(v, _)| v) == Some(id);
            let disp = if id == stem || first_id { first_name.as_deref().map(|v| self.tr(v)) } else { None };
            let disp = disp.or_else(|| between(&s[..win_end], e, "name=\"name\">", '<').map(|(v, _)| self.tr(v)));
            let disp = disp.or_else(|| self.lang.get(&format!("trinket.{id}.name")).cloned()).unwrap_or_else(|| id.to_string());
            let h = hash_string(id);
            let replace = match self.defs.get(&h) {
                Some(d) => d.kind == Kind::Other && kind != Kind::Other,
                None => true,
            };
            if replace {
                self.defs.insert(h, Def { id: id.to_string(), name: disp, kind });
            }
        }
    }

    fn tr(&self, v: &str) -> String {
        match v.strip_prefix('.') {
            Some(k) => self.lang.get(k).cloned().unwrap_or_else(|| k.to_string()),
            None => v.to_string(),
        }
    }

    pub fn label(&self, h: u32) -> String {
        if let Some(d) = self.defs.get(&h) {
            if d.name != d.id {
                return format!("{} ({})", d.name, d.id);
            }
            return d.id.clone();
        }
        self.by_hash.get(&h).cloned().unwrap_or_else(|| format!("#{h:08x}"))
    }
    pub fn id_label(&self, id: &str) -> String {
        self.label(hash_string(id))
    }
    pub fn list(&self, k: Kind) -> Vec<&Def> {
        let mut v: Vec<&Def> = self.defs.values().filter(|d| d.kind == k).collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }
}
