//! Builds a hash -> name dictionary from the game's own data files, so the
//! editor shows "Decaying Hand" instead of 0x7f3a19c2. Nothing is bundled.

use crate::gamedata::{self, IconRef};
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
    /// RGBA sprite sheets read from the game (never bundled): name -> (w, h, pixels)
    pub images: HashMap<String, (usize, usize, Vec<u8>)>,
    /// icon for an id hash (base items, trinkets, buildings)
    pub icons: HashMap<u32, IconRef>,
    pub mods: HashMap<u32, ModDef>,
    pub bases: HashMap<u32, BaseDef>,
    pub buildings: HashMap<u32, BuildingDef>,
    /// everything the game's "add all blueprints" cheat unlocks: (string to add, hash it is checked by)
    pub blueprint_pool: Vec<(String, u32)>,
    loader_files: Vec<(String, String)>, // (loader, text)
}

/// An item modifier (enchantment) definition.
#[derive(Clone)]
pub struct ModDef {
    pub id: String,
    pub name: String,
    pub quality: i32,
    pub tags: Vec<u32>,
    pub exclusive: u32,
    pub skill: bool,
    pub ilvl: i32,
    pub ilvl_max: i32,
}

/// A base equipment item.
#[derive(Clone)]
pub struct BaseDef {
    pub tags: Vec<u32>,
    pub skill: bool,
}

#[derive(Clone)]
pub struct BuildingDef {
    pub name: String,
    pub max_variation: i32,
}

const STAT_NAMES: [(&str, &str); 10] = [
    ("str", "Strength"), ("dex", "Dexterity"), ("int", "Intelligence"), ("vit", "Vitality"), ("foc", "Focus"),
    ("armor", "Armor"), ("resistance", "Resistance"), ("health", "Health"), ("mana", "Mana"), ("spell-power", "Spell Power"),
];

/// Decode one of the game's texture entries (type 3) from assets.bin.
fn texture(bin: &[u8], name: &str) -> Option<(usize, usize, Vec<u8>)> {
    let mut key = (name.len() as u32).to_le_bytes().to_vec();
    key.extend_from_slice(name.as_bytes());
    let i = bin.windows(key.len()).position(|w| w == &key[..])?;
    let p = i + key.len();
    if bin.get(p) != Some(&3) {
        return None;
    }
    let rd = |o: usize| u32::from_le_bytes(bin[o..o + 4].try_into().unwrap()) as usize;
    let (sz, csz, p) = (rd(p + 1), rd(p + 5), p + 9);
    let u = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize;
    if csz > 0 {
        let b = lz4_flex::block::decompress(bin.get(p..p + csz)?, sz).ok()?;
        let (w, h) = (u(&b, 0), u(&b, 4));
        Some((w, h, b.get(12..12 + w * h * 4)?.to_vec()))
    } else {
        let (w, h) = (rd(p), rd(p + 4));
        let px = lz4_flex::block::decompress(bin.get(p + 16..p + sz)?, w * h * 4).ok()?;
        Some((w, h, px))
    }
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
        n.build_defs();
        let mut sheets: Vec<String> = vec!["players/class_showcase.png".into(), "gui/icons_town.png".into()];
        for i in n.icons.values() {
            if !sheets.contains(&i.sheet) {
                sheets.push(i.sheet.clone());
            }
        }
        for img in sheets {
            if let Some(t) = texture(&bin, &img) {
                n.images.insert(img, t);
            }
        }
        Ok(n)
    }

    fn scan(&mut self, path: &str, s: &str) {
        if let Some(l) = s.find("<loader").and_then(|i| s[i..].find('>').map(|j| i + j + 1)) {
            if let Some(e) = s[l..].find("</loader>") {
                let loader = s[l..l + e].trim();
                if ["Equipment::ItemModifier::", "Equipment::BaseItem::", "Item::Trinket::LoadMultiple", "MissionBuff::", "PlayerColors::", "BuildingManagement::BuildingDef::"]
                    .iter()
                    .any(|p| loader.starts_with(p))
                {
                    self.loader_files.push((loader.to_string(), s.to_string()));
                }
            }
        }
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

    /// Turn the collected definition files into editor data.
    fn build_defs(&mut self) {
        let files = std::mem::take(&mut self.loader_files);
        let tavern = [hash_string("tavern_drink"), hash_string("tavern_food")];
        for (loader, text) in &files {
            for d in gamedata::entries(text) {
                let id = d.s("id").map(str::to_string);
                if loader.starts_with("Equipment::ItemModifier::") {
                    let Some(id) = id else { continue };
                    let bp = match d.s("blueprint-override") {
                        Some(o) => Some(hash_string(&format!("ItemModifier {o}"))),
                        None if d.flag("blueprint", true) => Some(hash_string(&format!("ItemModifier {id}"))),
                        None => None,
                    };
                    if let Some(h) = bp {
                        self.blueprint_pool.push((format!("ItemModifier {id}"), h));
                    }
                    let exclusive = match d.s("exclusive") {
                        Some("false") => 0,
                        Some("true") | None => hash_string(&id),
                        Some(x) => hash_string(x),
                    };
                    let name = self.mod_name(&d, &id);
                    let m = ModDef {
                        name,
                        quality: gamedata::quality_code(d.s("quality").unwrap_or("common")),
                        tags: d.strs("tags").iter().map(|t| hash_string(t)).collect(),
                        exclusive,
                        skill: d.s("skill").is_some(),
                        ilvl: d.int("ilvl").unwrap_or(0),
                        ilvl_max: d.int("ilvl-max").unwrap_or(999_999_999),
                        id: id.clone(),
                    };
                    self.mods.insert(hash_string(&id), m);
                } else if loader.starts_with("Equipment::BaseItem::") {
                    let Some(id) = id else { continue };
                    let h = hash_string(&id);
                    self.bases.insert(h, BaseDef { tags: d.strs("tags").iter().map(|t| hash_string(t)).collect(), skill: d.s("skill").is_some() });
                    if let Some(i) = d.icon() {
                        self.icons.insert(h, i);
                    }
                } else if loader.starts_with("Item::Trinket::") {
                    let Some(id) = id else { continue };
                    if d.flag("blueprint", true) && d.s("attune-desc").is_some() {
                        self.blueprint_pool.push((format!("Trinket {id}"), hash_string(&format!("Trinket {id}"))));
                    }
                    if let Some(i) = d.icon() {
                        self.icons.insert(hash_string(&id), i);
                    }
                } else if loader.starts_with("MissionBuff::") {
                    let Some(id) = id else { continue };
                    let set = d.s("set").map(hash_string).unwrap_or(0);
                    if tavern.contains(&set) && d.flag("blueprint", true) {
                        self.blueprint_pool.push((format!("MissionBuff {id}"), hash_string(&format!("MissionBuff {id}"))));
                    }
                } else if loader.starts_with("PlayerColors::") {
                    if let Some(bp) = d.s("blueprint").filter(|b| *b != "-") {
                        self.blueprint_pool.push((bp.to_string(), hash_string(bp)));
                    }
                } else if loader.starts_with("BuildingManagement::BuildingDef::") {
                    let Some(id) = id else { continue };
                    let h = hash_string(&id);
                    let ups = d.child("upgrades").map(|u| u.kids.iter().filter(|k| k.tag == "dict" || k.tag == "d").count()).unwrap_or(0);
                    let name = d.s("name").map(|v| self.tr(v)).unwrap_or_else(|| id.clone());
                    self.buildings.insert(h, BuildingDef { name, max_variation: ups as i32 });
                    if let Some(i) = d.icon() {
                        self.icons.insert(h, i);
                    }
                }
            }
        }
    }

    /// A readable name for a modifier: its affix name plus what it does,
    /// e.g. "Mighty: +10–20 Attack Power" or "of Speed: +10% Move Speed".
    fn mod_name(&self, d: &gamedata::Node, id: &str) -> String {
        let affix = ["prefix", "suffix"].iter().filter_map(|k| d.s(k)).map(|v| self.tr(v)).find(|t| !t.starts_with("modifiers."));
        let mut effect = None;
        if let Some(desc) = d.find("description").map(|n| n.text.trim().to_string()).filter(|t| t.starts_with('.')) {
            let (key, params) = desc.split_once('?').unwrap_or((desc.as_str(), ""));
            if let Some(mut t) = self.lang.get(key.trim_start_matches('.')).cloned() {
                for kv in params.split('&') {
                    if let Some((k, v)) = kv.split_once('=') {
                        t = t.replace(&format!("%{k}%"), v);
                    }
                }
                if !t.contains('%') {
                    effect = Some(t);
                }
            }
        }
        if effect.is_none() {
            let mut parts = vec![];
            let mut nodes: Vec<&gamedata::Node> = vec![];
            if let Some(m) = d.child("modifier") {
                nodes.push(m);
            }
            if let Some(a) = d.child("modifiers") {
                nodes.extend(a.kids.iter());
            }
            for m in nodes {
                for k in &m.kids {
                    let (Some(key), true) = (k.name.as_deref(), matches!(k.tag.as_str(), "ivec2" | "vec2" | "int" | "float" | "i" | "f")) else { continue };
                    if matches!(key, "ilvl" | "ilvl-max" | "spawn-chance" | "chance-scale") {
                        continue;
                    }
                    let vals: Vec<f32> = k.text.split_whitespace().filter_map(|x| x.parse().ok()).collect();
                    if vals.is_empty() {
                        continue;
                    }
                    let title = |s: &str| s.split('-').map(|w| { let mut c = w.chars(); c.next().map(|x| x.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() }).collect::<Vec<_>>().join(" ");
                    let r1 = |v: f32| (v * 10.0).round() / 10.0;
                    if let Some(base) = key.strip_suffix("-mul") {
                        // multipliers: 1.2 = +20%, 0.85 = -15%
                        let label = match base { "attack" => "Attack Damage".to_string(), "spell" => "Spell Damage".to_string(), b => title(b) };
                        let p: Vec<String> = vals.iter().take(2).map(|v| { let d = r1((v - 1.0) * 100.0); format!("{}{}%", if d >= 0.0 { "+" } else { "" }, d) }).collect();
                        let range = if p.len() == 2 && p[0] != p[1] { format!("{}–{}", p[0], p[1]) } else { p[0].clone() };
                        parts.push(format!("{range} {label}"));
                        continue;
                    }
                    let absolute = key.ends_with("-regen");
                    let pct = !absolute && k.text.contains('.') && vals.iter().all(|v| v.abs() < 1.5);
                    let f = |v: f32| if pct { format!("{}%", r1(v * 100.0)) } else { format!("{}", v) };
                    let label = STAT_NAMES.iter().find(|(s, _)| *s == key).map(|(_, l)| l.to_string()).unwrap_or_else(|| title(key));
                    let label = if absolute { format!("{label}/s") } else { label };
                    let range = if vals.len() >= 2 && vals[0] != vals[1] { format!("+{}–{}", f(vals[0]), f(vals[1])) } else { format!("+{}", f(vals[0])) };
                    parts.push(format!("{range} {label}"));
                }
            }
            if !parts.is_empty() {
                effect = Some(parts.join(", "));
            }
        }
        let clean = |s: String| -> String {
            // drop in-game colour codes like \cb0dfff … \d
            let mut o = String::new();
            let mut it = s.chars().peekable();
            while let Some(c) = it.next() {
                if c == '\\' {
                    match it.peek() {
                        Some('c') => { it.next(); for _ in 0..6 { it.next(); } }
                        Some(_) => { it.next(); }
                        None => {}
                    }
                } else {
                    o.push(c);
                }
            }
            o
        };
        match (affix, effect) {
            (Some(a), Some(e)) => clean(format!("{a}: {e}")),
            (None, Some(e)) => clean(e),
            // some epic modifiers only change the item's name
            (Some(a), None) if d.child("modifier").is_none() && d.child("modifiers").is_none() && d.find("description").is_none() => clean(format!("{a}: name only, no bonus")),
            (Some(a), None) => clean(a),
            (None, None) => id.trim_start_matches("mod_").replace('_', " "),
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
    /// "wizard_evocation" -> "Wizard (Evocation)", using the game's own class names.
    pub fn class_name(&self, class: &str) -> String {
        let base = class.split('_').next().unwrap_or(class);
        let cls = self.lang.get(&format!("menu.class.{base}")).cloned().unwrap_or_else(|| crate::cap_words(base));
        let spec = self.defs.get(&hash_string(class)).map(|d| d.name.clone()).filter(|s| s != class);
        match spec {
            Some(s) if s != cls => format!("{cls} ({s})"),
            _ => cls,
        }
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
