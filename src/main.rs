#![cfg_attr(windows, windows_subsystem = "windows")]
mod hps;
mod names;
mod sval;
mod theme;
mod zip;

use eframe::egui::{self, Color32, RichText, Ui};
use hps::{Block, Save};
use names::{Kind, Names};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use sval::V;
use theme::{section, Section, gray, semibold, BLUE, GRAY, LABEL, RED};

const MATS: [&str; 7] = ["Gold", "Wood", "Stone", "Iron", "Crystals", "Dust", "Ore"];
const QUAL: [(i32, &str); 9] = [
    (0, "None"), (1, "Common"), (2, "Uncommon"), (4, "Rare"), (8, "Epic"),
    (16, "Legendary"), (32, "Quest"), (64, "Unique"), (128, "Cursed"),
];

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() > 2 && a[1] == "--selftest" {
        let mut bad = 0;
        for f in &a[2..] {
            let r = std::fs::read(f).map_err(|e| e.to_string()).and_then(|b| hps::selftest(&b));
            if r.is_err() {
                bad += 1;
            }
            println!("{f}: {}", r.unwrap_or_else(|e| format!("FAIL {e}")));
        }
        std::process::exit(bad);
    }
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 780.0]).with_min_inner_size([900.0, 560.0])
            .with_title("Hammersmith")
            .with_icon(std::sync::Arc::new(egui::IconData { rgba: include_bytes!("../assets/icon64.rgba").to_vec(), width: 64, height: 64 })),
        ..Default::default()
    };
    let _ = eframe::run_native("hammersmith", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))));
}

#[derive(PartialEq, Clone, Copy)]
enum View {
    Town,
    Hero(usize),
    Raw,
    Settings,
}

struct App {
    dir: String,
    files: Vec<(PathBuf, u64)>, // path, header time
    file: Option<PathBuf>,
    save: Option<Save>,
    view: View,
    raw_sel: usize,
    status: (String, bool),
    status_t: f64,
    names: Option<Names>,
    names_rx: Option<Receiver<Result<Names, String>>>,
    names_dir: Option<PathBuf>,
    filter: String,
    filter2: String,
    show_backups: bool,
}

fn steam_roots() -> Vec<PathBuf> {
    let mut v = vec![];
    for var in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Ok(p) = std::env::var(var) {
            v.push(PathBuf::from(p).join("Steam"));
        }
    }
    v.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
    v.push(PathBuf::from(r"C:\Steam"));
    v
}

fn find_save_dir() -> Option<PathBuf> {
    for root in steam_roots() {
        if let Ok(rd) = std::fs::read_dir(root.join("userdata")) {
            for u in rd.flatten() {
                let p = u.path().join("619820/remote/saves");
                if p.is_dir() {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Locate the game install from the save path (…/Steam/userdata/…) via Steam's library list.
fn find_game_dir(save_dir: &Path) -> Option<PathBuf> {
    if let Some(g) = std::env::var_os("HOH2_GAME").map(PathBuf::from).filter(|g| g.join("res/assets.bin").is_file()) {
        return Some(g);
    }
    let mut roots: Vec<PathBuf> = save_dir
        .ancestors()
        .find(|p| p.file_name().map(|n| n == "userdata").unwrap_or(false))
        .and_then(|p| p.parent())
        .map(|p| vec![p.to_path_buf()])
        .unwrap_or_default();
    roots.extend(steam_roots());
    let mut libs = vec![];
    for r in &roots {
        libs.push(r.clone());
        if let Ok(t) = std::fs::read_to_string(r.join("steamapps/libraryfolders.vdf")) {
            for line in t.lines() {
                let l = line.trim();
                if let Some(rest) = l.strip_prefix("\"path\"") {
                    libs.push(PathBuf::from(rest.trim().trim_matches('"').replace("\\\\", "\\")));
                }
            }
        }
    }
    libs.into_iter()
        .map(|l| l.join("steamapps/common/Heroes of Hammerwatch 2"))
        .find(|g| g.join("res/assets.bin").is_file())
}

fn backup_dir(save_dir: &Path) -> PathBuf {
    match std::env::var("LOCALAPPDATA") {
        Ok(l) => PathBuf::from(l).join("HeroesofHammerwatch2/editor_backups"),
        Err(_) => save_dir.join("../editor_backups"),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn stamp() -> String {
    // yyyymmdd-hhmmss in UTC without pulling in a date crate
    let s = now_ms() / 1000;
    let (d, t) = (s / 86400, s % 86400);
    let (mut y, mut doy) = (1970, d as i64);
    loop {
        let len = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 366 } else { 365 };
        if doy < len {
            break;
        }
        doy -= len;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let ml = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0;
    while doy >= ml[m] {
        doy -= ml[m];
        m += 1;
    }
    format!("{y}{:02}{:02}-{:02}{:02}{:02}", m + 1, doy + 1, t / 3600, t / 60 % 60, t % 60)
}

impl App {
    fn new(cc: &eframe::CreationContext) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut a = App {
            dir: std::env::args().nth(1).filter(|a| Path::new(a).is_dir()).map(PathBuf::from).or_else(find_save_dir).map(|p| p.display().to_string()).unwrap_or_default(),
            files: vec![],
            file: None,
            save: None,
            view: View::Town,
            raw_sel: 0,
            status: (String::new(), false),
            status_t: f64::NEG_INFINITY,
            names: None,
            names_rx: None,
            names_dir: None,
            filter: String::new(),
            filter2: String::new(),
            show_backups: false,
        };
        a.scan();
        a
    }

    fn msg(&mut self, s: impl Into<String>, err: bool) {
        self.status = (s.into(), err);
        self.status_t = f64::NAN; // stamped with the frame time on next paint
    }

    fn scan(&mut self) {
        self.files.clear();
        let dir = PathBuf::from(&self.dir);
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().map(|x| x == "hps").unwrap_or(false) {
                    let t = std::fs::read(&p).ok().filter(|b| b.len() > 16).map(|b| u64::from_le_bytes(b[8..16].try_into().unwrap())).unwrap_or(0);
                    self.files.push((p, t));
                }
            }
        }
        self.files.sort_by(|a, b| b.1.cmp(&a.1));
        if self.files.is_empty() {
            self.save = None;
            self.msg("No .hps saves found. Enter your saves folder (…\\Steam\\userdata\\<id>\\619820\\remote\\saves).", true);
            return;
        }
        let newest = self.files[0].0.clone();
        self.open(newest);
        if self.names.is_none() && self.names_rx.is_none() {
            if let Some(g) = find_game_dir(&dir) {
                let (tx, rx) = std::sync::mpsc::channel();
                let g2 = g.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(Names::load(&g2));
                });
                self.names_rx = Some(rx);
                self.names_dir = Some(g);
            }
        }
    }

    fn open(&mut self, p: PathBuf) {
        match std::fs::read(&p).map_err(|e| e.to_string()).and_then(|b| Save::parse(&b)) {
            Ok(s) => {
                let run = s.has_run();
                self.save = Some(s);
                self.msg(
                    format!("Loaded {}{}", p.file_name().unwrap().to_string_lossy(), if run { " · a dungeon run is in progress (its level data is left untouched)" } else { "" }),
                    false,
                );
                self.file = Some(p);
                if let View::Hero(i) = self.view {
                    if i >= self.save.as_ref().unwrap().heroes.len() {
                        self.view = View::Town;
                    }
                }
            }
            Err(e) => {
                self.save = None;
                self.msg(format!("Could not open {}: {e}", p.display()), true);
            }
        }
    }


    /// Zip every .hps in the saves folder into one timestamped archive.
    fn backup_all(&self, label: &str) -> Result<PathBuf, String> {
        let dir = PathBuf::from(&self.dir);
        let mut files = vec![];
        for e in std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten() {
            let p = e.path();
            if p.extension().map(|x| x == "hps").unwrap_or(false) {
                files.push((p.file_name().unwrap().to_string_lossy().to_string(), std::fs::read(&p).map_err(|e| e.to_string())?));
            }
        }
        if files.is_empty() {
            return Err("no .hps files to back up".into());
        }
        files.sort();
        let bdir = backup_dir(&dir);
        std::fs::create_dir_all(&bdir).map_err(|e| format!("backup folder: {e}"))?;
        let out = bdir.join(format!("{label}_{}.zip", stamp()));
        std::fs::write(&out, zip::write(&files)).map_err(|e| e.to_string())?;
        Ok(out)
    }

    fn restore_zip(&mut self, z: &Path) -> Result<usize, String> {
        let entries = zip::read(&std::fs::read(z).map_err(|e| e.to_string())?)?;
        let entries: Vec<_> = entries.into_iter().filter(|(n, _)| n.ends_with(".hps") && !n.contains(['/', '\\'])).collect();
        for (n, d) in &entries {
            Save::parse(d).map_err(|e| format!("{n} in backup is not valid: {e}"))?;
        }
        self.backup_all("before_restore")?; // safety net
        let dir = PathBuf::from(&self.dir);
        for (n, d) in &entries {
            std::fs::write(dir.join(n), d).map_err(|e| format!("{n}: {e}"))?;
        }
        Ok(entries.len())
    }

    fn write(&mut self) {
        let (Some(path), Some(save)) = (self.file.clone(), self.save.as_mut()) else { return };
        let res = (|| -> Result<PathBuf, String> {
            let old = std::fs::read(&path).map_err(|e| format!("read: {e}"))?;
            let bdir = backup_dir(path.parent().unwrap());
            std::fs::create_dir_all(&bdir).map_err(|e| format!("backup folder: {e}"))?;
            let stem = path.file_stem().unwrap().to_string_lossy().to_string();
            let bpath = bdir.join(format!("{stem}_{}.hps", stamp()));
            std::fs::write(&bpath, &old).map_err(|e| format!("backup: {e}"))?;
            save.time_ms = save.time_ms.max(now_ms());
            // to_bytes() re-encodes every edited block, recompresses it and
            // recomputes the MD5 checksum over the whole file, every time.
            let out = save.to_bytes();
            let same = |chk: &Save| chk.town.val == save.town.val && chk.heroes.len() == save.heroes.len()
                && chk.heroes.iter().zip(&save.heroes).all(|(a, b)| a.data.val == b.data.val) && chk.town_name == save.town_name;
            if !same(&Save::parse(&out)?) {
                return Err("verification failed; nothing was written".into());
            }
            let tmp = path.with_extension("hps.tmp");
            std::fs::write(&tmp, &out).map_err(|e| format!("write: {e}"))?;
            std::fs::rename(&tmp, &path).map_err(|e| format!("replace: {e}"))?;
            // read back what actually landed on disk and check it again
            let disk = std::fs::read(&path).map_err(|e| format!("re-read: {e}"))?;
            if disk != out || !same(&Save::parse(&disk)?) {
                std::fs::write(&path, &old).map_err(|e| format!("rollback failed: {e}"))?;
                return Err("file on disk didn't verify; original restored".into());
            }
            Ok(bpath)
        })();
        match res {
            Ok(b) => {
                self.open(path.clone());
                self.msg(format!("Saved & verified (checksum recomputed) · backup: {}", b.file_name().unwrap().to_string_lossy()), false);
            }
            Err(e) => self.msg(format!("Save failed: {e}"), true),
        }
    }
}

// ---------------- helpers ----------------

fn names_label(n: Option<&Names>, h: i64) -> String {
    match n {
        Some(n) => n.label(h as u32),
        None => format!("#{:08x}", h as u32),
    }
}

/// "Pretty Name (id)" -> ("Pretty Name", "id")
fn split_label(l: &str) -> (String, String) {
    match l.rsplit_once(" (") {
        Some((n, id)) => (n.to_string(), id.trim_end_matches(')').to_string()),
        None => (l.to_string(), String::new()),
    }
}

/// "mod_spell_crit_chance" -> "Spell Crit Chance"
fn pretty_id(s: &str) -> String {
    if s.contains(' ') || s.starts_with('#') {
        return s.to_string();
    }
    s.trim_start_matches("mod_").split('_').filter(|w| !w.is_empty()).map(|w| {
        let mut c = w.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    }).collect::<Vec<_>>().join(" ")
}

fn class_style(class: &str) -> (&'static str, Color32) {
    let c = class.split('_').next().unwrap_or("");
    match c {
        "paladin" => ("P", theme::YELLOW),
        "warlock" => ("W", theme::PURPLE),
        "wizard" => ("Z", theme::BLUE),
        "warrior" => ("B", theme::RED),
        "rogue" => ("R", theme::GREEN),
        "sorcerer" => ("S", theme::ORANGE),
        "ranger" => ("H", theme::TEAL),
        "priest" => ("C", theme::PINK),
        _ => ("?", theme::GRAY3),
    }
}

fn qual_color(q: i32) -> Color32 {
    match q {
        1 => LABEL,
        2 => theme::GREEN,
        4 => Color32::from_rgb(64, 156, 255),
        8 => theme::PURPLE,
        16 => theme::ORANGE,
        64 => theme::YELLOW,
        128 => RED,
        _ => GRAY,
    }
}

fn qual_name(q: i32) -> &'static str {
    QUAL.iter().find(|x| x.0 == q).map(|x| x.1).unwrap_or("?")
}

/// Number row: label … [Max] [value] [stepper]
fn num_row(s: &mut Section, label: &str, v: &mut V, step: bool, max: Option<i32>) -> bool {
    let mut c = false;
    s.kv(label, |ui| {
        if step {
            let d = theme::stepper(ui);
            if d != 0 {
                if let V::Int(i) = v {
                    *i = i.saturating_add(d).max(0);
                    c = true;
                }
            }
        }
        c |= match v {
            V::Int(i) => ui.add(egui::DragValue::new(i).speed(1.0)).changed(),
            V::Long(i) => ui.add(egui::DragValue::new(i).speed(1.0)).changed(),
            V::Float(f) => ui.add(egui::DragValue::new(f).speed(0.01)).changed(),
            _ => {
                ui.label(gray("—"));
                false
            }
        };
        if let Some(m) = max {
            if theme::text_btn(ui, "Max", BLUE, false, true) {
                *v = V::Int(m);
                c = true;
            }
        }
    });
    c
}

fn picker(s: &mut Section, names: &Names, kind: Kind, filter: &mut String, hint: &str) -> Option<String> {
    let mut picked = None;
    s.row(|ui| {
        theme::search(ui, filter, hint);
    });
    let f = filter.to_lowercase();
    if f.len() >= 2 {
        let list: Vec<_> = names.list(kind).into_iter().filter(|d| d.name.to_lowercase().contains(&f) || d.id.contains(&f)).take(12).collect();
        if list.is_empty() {
            s.row(|ui| ui.label(gray("No matches")));
        }
        for d in list {
            s.row(|ui| {
                let r = ui.add(egui::Button::new(RichText::new(format!("Add {}", d.name)).color(BLUE)).frame(false));
                ui.label(gray(&d.id).small());
                if r.clicked() {
                    picked = Some(d.id.clone());
                }
            });
        }
    }
    if picked.is_some() {
        filter.clear();
    }
    picked
}

/// Equipment list with iOS-style expandable rows.
/// Item: [quality, baseHash, intensity, ilvl, upgrade, colorA, colorB, mods…]
fn gear_rows(s: &mut Section, items: &mut Vec<V>, names: Option<&Names>, salt: &str, equipped: bool) -> bool {
    let mut c = false;
    let mut kill = None;
    for (i, it) in items.iter_mut().enumerate() {
        let Some(a) = it.arr_mut().filter(|a| a.len() >= 5) else {
            s.row(|ui| ui.label(gray(if matches!(it, V::Null) { "Empty slot" } else { "Unrecognised item" })));
            continue;
        };
        let id = egui::Id::new((salt, i));
        let open = s.ui.data(|d| d.get_temp::<bool>(id).unwrap_or(false));
        let q = a[0].int().unwrap_or(0) as i32;
        let (name, base) = split_label(&names_label(names, a[1].int().unwrap_or(0)));
        let mods: Vec<String> = a.iter().skip(7).map(|m| pretty_id(&split_label(&names_label(names, m.int().unwrap_or(0))).0)).collect();
        let clicked = s.row(|ui| {
            ui.vertical(|ui| {
                ui.label(semibold(&name, 15.0).color(qual_color(q)));
                ui.label(gray(format!("{} · ilvl {} · +{}{}", qual_name(q), a[3].int().unwrap_or(0), a[4].int().unwrap_or(0),
                    if mods.is_empty() { String::new() } else { format!(" · {} mods", mods.len()) })).size(12.5));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| theme::chevron(ui, open));
            let full = egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), ui.min_rect().y_range());
            theme::click_area(ui, full, id.with("row")).clicked()
        });
        if clicked {
            s.ui.data_mut(|d| d.insert_temp(id, !open));
        }
        if open {
            s.kv(gray("    Quality"), |ui| {
                egui::ComboBox::from_id_salt(id.with("q")).selected_text(RichText::new(qual_name(q)).color(qual_color(q))).show_ui(ui, |ui| {
                    for (v, n) in QUAL {
                        if ui.selectable_label(q == v, RichText::new(n).color(qual_color(v))).clicked() {
                            a[0] = V::Int(v);
                            c = true;
                        }
                    }
                });
            });
            c |= num_row(s, "    Item level", &mut a[3], true, None);
            c |= num_row(s, "    Upgrade level", &mut a[4], true, None);
            if !mods.is_empty() {
                s.kv(gray("    Modifiers"), |ui| ui.label(gray(mods.join(", "))));
            }
            if !base.is_empty() {
                s.kv(gray("    Base item"), |ui| ui.label(gray(&base)));
            }
            s.row(|ui| {
                ui.add_space(16.0);
                if theme::text_btn(ui, if equipped { "Unequip & Delete" } else { "Delete Item" }, RED, false, true) {
                    kill = Some(i);
                }
            });
        }
    }
    if let Some(i) = kill {
        if equipped {
            items[i] = V::Null;
        } else {
            items.remove(i);
        }
        c = true;
    }
    c
}

// ---------------- views ----------------

fn town_view(ui: &mut Ui, save: &mut Save, names: Option<&Names>) -> bool {
    let mut ch = false;
    theme::large_title(ui, "Town", &save.town_name.clone());
    section(ui, Some("Name"), None, |s| {
        s.kv("Town name", |ui| ch |= theme::text_field(ui, &mut save.town_name, 260.0).changed());
    });
    if ch {
        save.town.dirty = true;
    }
    let blk = &mut save.town;
    if !blk.editable() {
        ui.colored_label(RED, format!("Town data can't be decoded safely: {}", blk.err.clone().unwrap_or_default()));
        return ch;
    }
    let t = blk.val.as_mut().unwrap();
    let mut c = false;
    section(ui, Some("Resources"), Some("Max sets a resource to 999,999."), |s| {
        if let Some(m) = t.get_mut("materials").and_then(|m| m.arr_mut()) {
            for (i, v) in m.iter_mut().enumerate() {
                c |= num_row(s, MATS.get(i).copied().unwrap_or("?"), v, false, Some(999_999));
            }
        }
    });
    section(ui, Some("Progress"), None, |s| {
        if let Some(v) = t.get_mut("ngp") {
            c |= num_row(s, "New Game+", v, true, None);
        }
        if let Some(bp) = t.get("blueprints").and_then(|b| b.arr()) {
            let n = bp.len();
            s.kv("Blueprints unlocked", |ui| ui.label(gray(n.to_string())));
        }
        if let Some(st) = t.get("town-stash").and_then(|s| s.arr()) {
            let n: usize = st.iter().filter_map(|tab| tab.get("items").and_then(|i| i.arr())).map(|i| i.len()).sum();
            let tabs = st.len();
            s.kv("Town stash", |ui| ui.label(gray(format!("{n} items in {tabs} tab{}", if tabs == 1 { "" } else { "s" }))));
        }
    });
    section(ui, Some("Buildings"), None, |s| {
        if let Some(b) = t.get("buildings").and_then(|b| b.arr()) {
            for x in b {
                let h = x.get("building-def").and_then(|v| v.int()).unwrap_or(0);
                let (n, _) = split_label(&names_label(names, h));
                s.kv(n, |ui| ui.label(RichText::new("Built").color(theme::GREEN)));
            }
        }
    });
    if c {
        blk.dirty = true;
    }
    ch | c
}

fn hero_view(ui: &mut Ui, blk: &mut Block, names: Option<&Names>, filter: &mut String, filter2: &mut String, in_run: bool) -> bool {
    if !blk.editable() {
        ui.colored_label(RED, format!("This hero can't be decoded safely: {}", blk.err.clone().unwrap_or_default()));
        return false;
    }
    let h = blk.val.as_mut().unwrap();
    let mut c = false;
    let class = h.get("class").and_then(|v| v.str()).unwrap_or("?").to_string();
    let (class_n, _) = split_label(&names.map(|n| n.id_label(&class)).unwrap_or(class.clone()));
    let lvl = h.get("level").and_then(|v| v.int()).unwrap_or(1) as i32;
    let title = h.get("name").and_then(|v| v.str()).filter(|s| !s.is_empty()).unwrap_or("Empty Hero Slot").to_string();
    theme::large_title(ui, &title, &format!("{class_n} · Level {lvl}"));

    let footer = if in_run { Some("This hero is mid-run. The game may overwrite changes when the run ends.") } else { None };
    section(ui, Some("Profile"), footer, |s| {
        if let Some(V::Str(n)) = h.get_mut("name") {
            s.kv("Name", |ui| c |= theme::text_field(ui, n, 240.0).changed());
        }
        s.kv("Class", |ui| ui.label(gray(&class_n)));
        if let Some(V::Bool(d)) = h.get_mut("dead") {
            s.kv("Status", |ui| {
                if *d {
                    if theme::filled_btn(ui, "Revive", BLUE) {
                        *d = false;
                        c = true;
                    }
                    ui.label(RichText::new("Fallen").color(RED));
                } else {
                    ui.label(RichText::new("Alive").color(theme::GREEN));
                }
            });
        }
    });
    section(ui, Some("Progression"), Some("Level and Experience should agree. Raising Experience and letting the game level you up is safest."), |s| {
        for (k, label) in [("level", "Level"), ("experience", "Experience"), ("ngp", "New Game+"), ("bonus-stats", "Unspent stat points"),
            ("bonus-skills", "Unspent skill points"), ("rerolls-used", "Rerolls used"), ("dash-charges", "Dash charges"), ("valor", "Valor")] {
            if let Some(v) = h.get_mut(k) {
                c |= num_row(s, label, v, k != "experience", None);
            }
        }
    });
    section(ui, Some("Attributes"), None, |s| {
        for (k, label) in [("picked-str", "Strength"), ("picked-dex", "Dexterity"), ("picked-int", "Intelligence"), ("picked-vit", "Vitality"), ("picked-foc", "Focus")] {
            if let Some(v) = h.get_mut(k) {
                c |= num_row(s, label, v, true, None);
            }
        }
    });
    section(ui, Some("Elixirs"), None, |s| {
        for (k, label) in [("elixir-str", "Strength"), ("elixir-dex", "Dexterity"), ("elixir-int", "Intelligence"), ("elixir-vit", "Vitality"), ("elixir-foc", "Focus")] {
            if let Some(v) = h.get_mut(k) {
                c |= num_row(s, label, v, true, None);
            }
        }
    });
    section(ui, Some("Equipped"), Some("Tap an item to edit its quality and levels."), |s| {
        if let Some(a) = h.get_mut("equipped").and_then(|m| m.arr_mut()) {
            c |= gear_rows(s, a, names, "eq", true);
        }
    });
    if !matches!(h.get("equipment"), Some(V::Arr(_))) {
        if let Some(v) = h.get_mut("equipment") {
            *v = V::Arr(vec![]);
        }
    }
    section(ui, Some("Inventory"), Some("New items are added as Legendary at the hero's level, with no modifiers."), |s| {
        if let Some(a) = h.get_mut("equipment").and_then(|m| m.arr_mut()) {
            if a.is_empty() {
                s.row(|ui| ui.label(gray("No items")));
            }
            c |= gear_rows(s, a, names, "inv", false);
            if let Some(n) = names {
                if let Some(id) = picker(s, n, Kind::Equipment, filter, "Add equipment") {
                    a.push(V::Arr(vec![V::Int(16), V::Int(hps::hash_string(&id) as i32), V::Float(1.0), V::Int(lvl.max(1)), V::Int(0), V::Int(0), V::Int(0)]));
                    c = true;
                }
            }
        }
    });
    if !matches!(h.get("trinkets"), Some(V::Arr(_))) {
        if let Some(v) = h.get_mut("trinkets") {
            *v = V::Arr(vec![]);
        }
    }
    section(ui, Some("Trinkets"), None, |s| {
        if let Some(a) = h.get_mut("trinkets").and_then(|m| m.arr_mut()) {
            let mut kill = None;
            for (i, it) in a.iter().enumerate() {
                let l = it.str().map(|x| names.map(|n| n.id_label(x)).unwrap_or(x.to_string())).unwrap_or_else(|| format!("{it:?}"));
                let (n, _) = split_label(&l);
                s.kv(n, |ui| {
                    if theme::text_btn(ui, "Remove", RED, false, true) {
                        kill = Some(i);
                    }
                });
            }
            if a.is_empty() {
                s.row(|ui| ui.label(gray("No trinkets")));
            }
            if let Some(i) = kill {
                a.remove(i);
                c = true;
            }
            if let Some(n) = names {
                if let Some(id) = picker(s, n, Kind::Trinket, filter2, "Add trinket") {
                    a.push(V::Str(id));
                    c = true;
                }
            }
        }
    });
    section(ui, Some("Carried materials"), None, |s| {
        if let Some(m) = h.get_mut("materials").and_then(|m| m.arr_mut()) {
            for (i, v) in m.iter_mut().enumerate() {
                c |= num_row(s, MATS.get(i).copied().unwrap_or("?"), v, false, None);
            }
        } else {
            s.row(|ui| ui.label(gray("None")));
        }
    });
    section(ui, Some("Skills"), None, |s| {
        if let Some(V::Dict(d)) = h.get_mut("picked-skills") {
            for (k, v) in d.iter_mut() {
                let (n, _) = split_label(&names.map(|n| n.id_label(k)).unwrap_or(k.clone()));
                c |= num_row(s, &n, v, true, None);
            }
        }
    });
    if c {
        blk.dirty = true;
    }
    c
}

/// Generic tree editor for anything the friendly views don't cover.
fn raw_tree(ui: &mut Ui, label: &str, v: &mut V, names: Option<&Names>, depth: usize) -> bool {
    let mut c = false;
    match v {
        V::Dict(d) => {
            egui::CollapsingHeader::new(format!("{label}  {{{}}}", d.len())).id_salt(ui.next_auto_id()).default_open(depth == 0).show(ui, |ui| {
                for (k, x) in d.iter_mut() {
                    c |= raw_tree(ui, k, x, names, depth + 1);
                }
            });
        }
        V::Arr(a) => {
            egui::CollapsingHeader::new(format!("{label}  [{}]", a.len())).id_salt(ui.next_auto_id()).show(ui, |ui| {
                let mut kill = None;
                for (i, x) in a.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.add(egui::Button::new(RichText::new("−").color(RED)).frame(false)).on_hover_text("Delete element").clicked() {
                            kill = Some(i);
                        }
                        ui.vertical(|ui| c |= raw_tree(ui, &format!("[{i}]"), x, names, depth + 1));
                    });
                }
                if let Some(i) = kill {
                    a.remove(i);
                    c = true;
                }
            });
        }
        _ => {
            ui.horizontal(|ui| {
                ui.label(label);
                match v {
                    V::Bool(b) => c |= theme::switch(ui, b).changed(),
                    V::Int(i) => {
                        c |= ui.add(egui::DragValue::new(i)).changed();
                        if i.unsigned_abs() > 100_000 {
                            if let Some(n) = names {
                                let l = n.label(*i as u32);
                                if !l.starts_with('#') {
                                    ui.label(gray(l));
                                }
                            }
                        }
                    }
                    V::Long(i) => c |= ui.add(egui::DragValue::new(i)).changed(),
                    V::Float(f) => c |= ui.add(egui::DragValue::new(f).speed(0.01)).changed(),
                    V::Double(f) => c |= ui.add(egui::DragValue::new(f).speed(0.01)).changed(),
                    V::Str(s) => c |= ui.text_edit_singleline(s).changed(),
                    V::Vec2(a) => a.iter_mut().for_each(|x| c |= ui.add(egui::DragValue::new(x)).changed()),
                    V::Vec3(a) => a.iter_mut().for_each(|x| c |= ui.add(egui::DragValue::new(x)).changed()),
                    V::Vec4(a) => a.iter_mut().for_each(|x| c |= ui.add(egui::DragValue::new(x)).changed()),
                    other => {
                        ui.label(gray(other.kind()));
                    }
                }
            });
        }
    }
    c
}

/// iPadOS sidebar row. Returns (tapped, row rect). The selected highlight is
/// drawn separately so it can glide between rows.
fn side_item(ui: &mut Ui, sel: bool, glyph: &str, col: Color32, title: &str, trailing: &str) -> (bool, egui::Rect) {
    let r = egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            theme::tile(ui, glyph, col);
            ui.add(egui::Label::new(RichText::new(title).size(15.0).color(LABEL)).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(trailing).size(13.5).color(if sel { LABEL.gamma_multiply(0.9) } else { GRAY }));
            });
        });
    });
    let rect = r.response.rect;
    let resp = theme::click_area(ui, rect, ui.id().with(("side", title, glyph)));
    if resp.hovered() && !sel {
        ui.painter().rect_filled(rect, 12.0, Color32::from_white_alpha(8));
    }
    (resp.clicked(), rect)
}

fn side_header(ui: &mut Ui, t: &str) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(semibold(t, 13.0).color(GRAY));
    });
    ui.add_space(2.0);
}

impl eframe::App for App {
    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        [0.024, 0.027, 0.047, 1.0]
    }

    fn update(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
        theme::background(ctx);
        ctx.request_repaint_after(std::time::Duration::from_millis(100)); // gentle light drift
        if let Some(rx) = &self.names_rx {
            if let Ok(r) = rx.try_recv() {
                match r {
                    Ok(n) => self.names = Some(n),
                    Err(e) => self.msg(format!("Item names unavailable: {e}"), true),
                }
                self.names_rx = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
        let now = ctx.input(|i| i.time);
        if self.status_t.is_nan() {
            self.status_t = now;
        }

        // ---------- sidebar ----------
        let names = self.names.as_ref();
        egui::SidePanel::left("nav")
            .resizable(false)
            .exact_width(290.0)
            .show_separator_line(false)
            .frame(egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 2, top: 10, bottom: 10 }))
            .show(ctx, |ui| {
                // the glass sheet is drawn fully inside the panel so nothing gets clipped
                let gr = theme::glass(theme::SIDEBAR, 26).inner_margin(egui::Margin { left: 10, right: 10, top: 18, bottom: 10 }).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    let tex = ui.ctx().data_mut(|d| d.get_temp::<egui::TextureHandle>(egui::Id::new("appicon"))).unwrap_or_else(|| {
                        let img = egui::ColorImage::from_rgba_unmultiplied([64, 64], include_bytes!("../assets/icon64.rgba"));
                        let t = ui.ctx().load_texture("appicon", img, egui::TextureOptions::LINEAR);
                        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("appicon"), t.clone()));
                        t
                    });
                    ui.add(egui::Image::new(&tex).fit_to_exact_size(egui::vec2(44.0, 44.0)));
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Hammersmith").font(theme::bold(24.0)).color(LABEL));
                        ui.label(gray("Hammerwatch II Save Editor").size(12.5));
                    });
                });
                ui.add_space(8.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let Some(save) = self.save.as_ref() else {
                        if side_item(ui, self.view == View::Settings, "S", theme::GRAY3, "Settings", "").0 {
                            self.view = View::Settings;
                        }
                        return;
                    };
                    let hl = ui.painter().add(egui::Shape::Noop);
                    let mut sel_rect = None;
                    let mut item = |ui: &mut Ui, v: View, g: &str, col: Color32, title: &str, trail: &str, view: &mut View| {
                        let (clicked, r) = side_item(ui, *view == v, g, col, title, trail);
                        if *view == v {
                            sel_rect = Some(r);
                        }
                        if clicked {
                            *view = v;
                        }
                    };
                    item(ui, View::Town, "T", theme::ORANGE, "Town", if save.town.dirty { "Edited" } else { "" }, &mut self.view);
                    side_header(ui, "Heroes");
                    for (i, h) in save.heroes.iter().enumerate() {
                        let v = h.data.val.as_ref();
                        let name = v.and_then(|v| v.get("name")).and_then(|n| n.str()).filter(|n| !n.is_empty()).unwrap_or("Empty Slot");
                        let lvl = v.and_then(|v| v.get("level")).and_then(|n| n.int()).unwrap_or(0);
                        let class = v.and_then(|v| v.get("class")).and_then(|n| n.str()).unwrap_or("");
                        let (g, col) = class_style(class);
                        let trail = if h.data.dirty { "Edited".to_string() } else if lvl > 0 { format!("Lv {lvl}") } else { String::new() };
                        item(ui, View::Hero(i), g, col, name, &trail, &mut self.view);
                    }
                    side_header(ui, "More");
                    item(ui, View::Raw, "{}", theme::INDIGO, "Advanced", "", &mut self.view);
                    item(ui, View::Settings, "S", theme::GRAY3, "Settings", "", &mut self.view);
                    // liquid selection: a blue glass lozenge that glides to the selected row
                    if let Some(r) = sel_rect {
                        let ctx = ui.ctx().clone();
                        let y = ctx.animate_value_with_time(egui::Id::new("hl_y"), r.top(), 0.22);
                        let h = ctx.animate_value_with_time(egui::Id::new("hl_h"), r.height(), 0.22);
                        let rr = egui::Rect::from_min_size(egui::pos2(r.left(), y), egui::vec2(r.width(), h));
                        let mut shapes = vec![egui::Shape::rect_filled(rr, 12.0, Color32::from_rgba_unmultiplied(10, 132, 255, 165))];
                        shapes.push(egui::Shape::rect_stroke(rr, 12.0, egui::Stroke::new(1.0f32, Color32::from_white_alpha(46)), egui::StrokeKind::Inside));
                        ui.painter().set(hl, egui::Shape::Vec(shapes));
                        let p = ui.painter().clone();
                        theme::sheen(&p, rr, 12.0);
                    }
                });
                });
                theme::sheen(ui.painter(), gr.response.rect, 26.0);
            });

        // ---------- detail ----------
        let mut do_save = false;
        let mut do_discard = false;
        let mut do_backup = false;
        let mut pick = None;
        let filter = &mut self.filter;
        let filter2 = &mut self.filter2;
        let raw_sel = &mut self.raw_sel;
        let dirty = self.save.as_ref().map(|s| s.town.dirty || s.heroes.iter().any(|h| h.data.dirty)).unwrap_or(false);
        let mut rescan = false;
        egui::CentralPanel::default().frame(egui::Frame::new().inner_margin(egui::Margin { left: 18, right: 20, top: 14, bottom: 0 })).show(ctx, |ui| {
            // nav bar
            ui.horizontal(|ui| {
                let cur = self.file.as_ref().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).unwrap_or_else(|| "No save".into());
                egui::ComboBox::from_id_salt("slot").selected_text(RichText::new(cur).color(BLUE)).width(150.0).show_ui(ui, |ui| {
                    for (i, (p, _)) in self.files.iter().enumerate() {
                        let n = p.file_name().unwrap().to_string_lossy().to_string();
                        let lbl = if i == 0 { format!("{n}  (newest)") } else { n };
                        if ui.selectable_label(Some(p) == self.file.as_ref(), lbl).clicked() {
                            pick = Some(p.clone());
                        }
                    }
                });
                if self.names_rx.is_some() {
                    ui.spinner();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let r = theme::glass(theme::CELL, 255).inner_margin(egui::Margin::symmetric(14, 3)).show(ui, |ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::text_btn(ui, "Save", BLUE, true, dirty) {
                        do_save = true;
                    }
                    ui.add_space(6.0);
                    if theme::text_btn(ui, "Discard", RED, false, dirty) {
                        do_discard = true;
                    }
                    ui.add_space(6.0);
                    if theme::text_btn(ui, "Backups", BLUE, false, true) {
                        self.show_backups = true;
                    }
                    ui.add_space(6.0);
                    if theme::text_btn(ui, "Back Up All", BLUE, false, true) {
                        do_backup = true;
                    }
                    });
                    });
                    theme::sheen(ui.painter(), r.response.rect, 16.0);
                });
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(860.0));
                match (self.view, self.save.as_mut()) {
                    (View::Settings, _) | (_, None) => {
                        theme::large_title(ui, "Settings", "");
                        section(ui, Some("Saves folder"), Some("Usually Steam\\userdata\\<your id>\\619820\\remote\\saves. Found automatically."), |s| {
                            s.row(|ui| {
                                let r = ui.add(egui::TextEdit::singleline(&mut self.dir).frame(false).desired_width(ui.available_width() - 70.0));
                                if theme::text_btn(ui, "Scan", BLUE, true, true) || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                                    rescan = true;
                                }
                            });
                        });
                        section(ui, Some("Game data"), Some("Item, trinket and skill names are read from the game's own files."), |s| {
                            let gd = self.names_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "Not found".into());
                            s.kv("Install", |ui| ui.label(gray(gd)));
                            let st = if self.names.is_some() { "Loaded" } else if self.names_rx.is_some() { "Loading…" } else { "Unavailable" };
                            s.kv("Names", |ui| ui.label(gray(st)));
                        });
                        section(ui, Some("Backups"), Some("A backup is made automatically before every save."), |s| {
                            let bd = backup_dir(Path::new(&self.dir)).display().to_string();
                            s.kv("Location", |ui| ui.add(egui::Label::new(gray(bd)).truncate()));
                            s.row(|ui| {
                                if theme::text_btn(ui, "Back Up All Saves Now", BLUE, false, true) {
                                    do_backup = true;
                                }
                            });
                            s.row(|ui| {
                                if theme::text_btn(ui, "Show Backups", BLUE, false, true) {
                                    self.show_backups = true;
                                }
                            });
                        });
                        section(ui, Some("About"), None, |s| {
                            s.kv("Version", |ui| ui.label(gray(env!("CARGO_PKG_VERSION"))));
                            s.kv("Save format", |ui| ui.label(gray("H2SV v4 · LZ4 · MD5")));
                        });
                    }
                    (View::Town, Some(save)) => {
                        town_view(ui, save, names);
                    }
                    (View::Hero(i), Some(save)) => {
                        if let Some(h) = save.heroes.get_mut(i) {
                            let run = h.level.is_some();
                            hero_view(ui, &mut h.data, names, filter, filter2, run);
                        }
                    }
                    (View::Raw, Some(save)) => {
                        theme::large_title(ui, "Advanced", "Every value in the save, exactly as stored");
                        section(ui, None, Some("Changes here are applied exactly as typed, so use with care."), |s| {
                            s.kv("Data", |ui| {
                                let name_of = |i: usize| -> String {
                                    if i == 0 {
                                        "Town".into()
                                    } else {
                                        save.heroes.get(i - 1).and_then(|h| h.data.val.as_ref()).and_then(|v| v.get("name")).and_then(|n| n.str()).filter(|n| !n.is_empty()).unwrap_or("Empty slot").to_string()
                                    }
                                };
                                egui::ComboBox::from_id_salt("rawsel").selected_text(RichText::new(name_of(*raw_sel)).color(BLUE)).show_ui(ui, |ui| {
                                    for i in 0..=save.heroes.len() {
                                        ui.selectable_value(raw_sel, i, name_of(i));
                                    }
                                });
                            });
                        });
                        let blk = if *raw_sel == 0 { Some(&mut save.town) } else { save.heroes.get_mut(*raw_sel - 1).map(|h| &mut h.data) };
                        if let Some(blk) = blk {
                            let mut ch = false;
                            section(ui, None, None, |s| {
                                s.row(|ui| {
                                    ui.vertical(|ui| {
                                        if let Some(v) = blk.val.as_mut() {
                                            ch = raw_tree(ui, "root", v, names, 0);
                                        }
                                    });
                                });
                            });
                            if ch {
                                blk.dirty = true;
                            }
                        }
                    }
                }
                ui.add_space(40.0);
            });
        });

        if let Some(p) = pick {
            self.open(p);
        }
        if rescan {
            self.names = None;
            self.scan();
        }
        if do_discard {
            if let Some(p) = self.file.clone() {
                self.open(p);
            }
        }
        if do_save {
            self.write();
        }
        if do_backup {
            match self.backup_all("all_saves") {
                Ok(p) => self.msg(format!("All saves backed up to {}", p.file_name().unwrap().to_string_lossy()), false),
                Err(e) => self.msg(format!("Backup failed: {e}"), true),
            }
        }

        // ---------- backups sheet ----------
        if self.show_backups {
            let mut action: Option<PathBuf> = None;
            let mut close = false;
            let bdir = backup_dir(Path::new(&self.dir));
            let m = egui::Modal::new(egui::Id::new("backups"))
                .backdrop_color(Color32::from_black_alpha(110))
                .frame(theme::glass(Color32::from_rgba_unmultiplied(30, 30, 40, 240), 28).inner_margin(egui::Margin::same(20)))
                .show(ctx, |ui| {
                    ui.set_width(520.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Backups").font(theme::bold(17.0)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if theme::text_btn(ui, "Done", BLUE, true, true) {
                                close = true;
                            }
                        });
                    });
                    ui.add_space(10.0);
                    let mut v: Vec<PathBuf> = std::fs::read_dir(&bdir).map(|r| r.flatten().map(|e| e.path()).collect()).unwrap_or_default();
                    v.sort_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok());
                    v.reverse();
                    egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                        let (zips, slots): (Vec<_>, Vec<_>) = v.into_iter().partition(|p| p.extension().map(|e| e == "zip").unwrap_or(false));
                        theme::section_c(ui, Some("All saves"), Some("Restore All replaces every save slot. Your current saves are zipped first."), theme::CELL2, |s| {
                            if zips.is_empty() {
                                s.row(|ui| ui.label(gray("None yet. Use Back Up All.")));
                            }
                            for p in &zips {
                                s.kv(p.file_name().unwrap().to_string_lossy().to_string(), |ui| {
                                    if theme::text_btn(ui, "Restore All", BLUE, false, true) {
                                        action = Some(p.clone());
                                    }
                                });
                            }
                        });
                        theme::section_c(ui, Some("Single slots"), Some("Made automatically each time you save. Restores over the slot that's open."), theme::CELL2, |s| {
                            if slots.is_empty() {
                                s.row(|ui| ui.label(gray("None yet")));
                            }
                            for p in &slots {
                                s.kv(p.file_name().unwrap().to_string_lossy().to_string(), |ui| {
                                    if theme::text_btn(ui, "Restore", BLUE, false, true) {
                                        action = Some(p.clone());
                                    }
                                });
                            }
                        });
                    });
                    ui.label(gray(bdir.display().to_string()).size(11.5));
                    let sr = ui.min_rect().expand(20.0);
                    theme::sheen(&ui.painter().with_clip_rect(sr.expand(2.0)), sr, 28.0);
                });
            if close || m.should_close() {
                self.show_backups = false;
            }
            if let Some(b) = action {
                self.show_backups = false;
                if b.extension().map(|e| e == "zip").unwrap_or(false) {
                    match self.restore_zip(&b) {
                        Ok(n) => {
                            self.scan();
                            self.msg(format!("Restored {n} save slots from {}", b.file_name().unwrap().to_string_lossy()), false);
                        }
                        Err(e) => self.msg(format!("Restore failed: {e}"), true),
                    }
                } else if let Some(f) = self.file.clone() {
                    let r = std::fs::read(&b).map_err(|e| e.to_string()).and_then(|d| Save::parse(&d).map(|_| d)).and_then(|d| std::fs::write(&f, d).map_err(|e| e.to_string()));
                    match r {
                        Ok(()) => {
                            self.open(f.clone());
                            self.msg(format!("Restored {}", b.file_name().unwrap().to_string_lossy()), false);
                        }
                        Err(e) => self.msg(format!("Restore failed: {e}"), true),
                    }
                }
            }
        }

        // ---------- toast ----------
        let (text, err) = self.status.clone();
        let age = now - self.status_t;
        let life = if err { 10.0 } else { 5.0 };
        if !text.is_empty() && age < life {
            let alpha = ((life - age) / 0.4).clamp(0.0, 1.0) as f32;
            #[allow(deprecated)]
            let sr = ctx.screen_rect();
            egui::Area::new(egui::Id::new("toast"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_BOTTOM, [135.0, -24.0])
                .interactable(false)
                .show(ctx, |ui| {
                    ui.set_max_width(sr.width() * 0.6);
                    let r = egui::Frame::new()
                        .fill(Color32::from_rgba_unmultiplied(38, 38, 48, 215).gamma_multiply(alpha))
                        .stroke(egui::Stroke::new(1.0f32, theme::EDGE.gamma_multiply(alpha)))
                        .corner_radius(egui::CornerRadius::same(255))
                        .inner_margin(egui::Margin::symmetric(18, 10))
                        .shadow(egui::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha((120.0 * alpha) as u8) })
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let col = if err { RED } else { theme::GREEN };
                                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                                ui.painter().circle_filled(r.center(), 5.0, col.gamma_multiply(alpha));
                                ui.add(egui::Label::new(RichText::new(&text).color(LABEL.gamma_multiply(alpha))).wrap());
                            });
                        });
                    if alpha > 0.5 {
                        theme::sheen(ui.painter(), r.response.rect, 20.0);
                    }
                });
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
}
