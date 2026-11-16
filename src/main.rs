#![cfg_attr(windows, windows_subsystem = "windows")]
mod hps;
mod names;
mod sval;

use eframe::egui::{self, Color32, RichText, Ui};
use hps::{Block, Save};
use names::{Kind, Names};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use sval::V;

const MATS: [&str; 7] = ["Gold", "Wood", "Stone", "Iron", "Crystals", "Dust", "Ore"];
const QUAL: [(i32, &str); 9] = [
    (0, "None"), (1, "Common"), (2, "Uncommon"), (4, "Rare"), (8, "Epic"),
    (16, "Legendary"), (32, "Quest"), (64, "Unique"), (128, "Cursed"),
];
const STATS: [(&str, &str); 18] = [
    ("level", "Level"), ("experience", "Experience"), ("ngp", "NG+"),
    ("picked-str", "Strength"), ("picked-dex", "Dexterity"), ("picked-int", "Intelligence"),
    ("picked-vit", "Vitality"), ("picked-foc", "Focus"),
    ("elixir-str", "Elixir: Strength"), ("elixir-dex", "Elixir: Dexterity"), ("elixir-int", "Elixir: Intelligence"),
    ("elixir-vit", "Elixir: Vitality"), ("elixir-foc", "Elixir: Focus"),
    ("bonus-stats", "Unspent stat points"), ("bonus-skills", "Unspent skill points"),
    ("rerolls-used", "Rerolls used"), ("valor", "Valor"), ("dash-charges", "Dash charges"),
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
            .with_inner_size([1000.0, 680.0])
            .with_title("Heroes of Hammerwatch II - Save Editor"),
        ..Default::default()
    };
    let _ = eframe::run_native("hoh2edit", opts, Box::new(|cc| Ok(Box::new(App::new(cc)))));
}

#[derive(PartialEq, Clone, Copy)]
enum View {
    Town,
    Hero(usize),
    Raw,
}

struct App {
    dir: String,
    files: Vec<(PathBuf, u64)>, // path, header time
    file: Option<PathBuf>,
    save: Option<Save>,
    view: View,
    raw_sel: usize,
    status: (String, bool),
    names: Option<Names>,
    names_rx: Option<Receiver<Result<Names, String>>>,
    names_dir: Option<PathBuf>,
    filter: String,
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
        #[cfg(windows)]
        {
            // Use Windows' own fonts instead of bundling ~1 MB of font files.
            let mut f = egui::FontDefinitions::empty();
            let dir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
            for (n, file) in [("ui", "segoeui.ttf"), ("sym", "seguisym.ttf"), ("emoji", "seguiemj.ttf"), ("mono", "consola.ttf")] {
                if let Ok(b) = std::fs::read(Path::new(&dir).join("Fonts").join(file)) {
                    f.font_data.insert(n.into(), std::sync::Arc::new(egui::FontData::from_owned(b)));
                    for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                        let list = f.families.entry(fam.clone()).or_default();
                        if n == "mono" && fam == egui::FontFamily::Monospace {
                            list.insert(0, n.into());
                        } else if n != "mono" {
                            list.push(n.into());
                        }
                    }
                }
            }
            cc.egui_ctx.set_fonts(f);
        }
        let _ = cc;
        let mut a = App {
            dir: std::env::args().nth(1).filter(|a| Path::new(a).is_dir()).map(PathBuf::from).or_else(find_save_dir).map(|p| p.display().to_string()).unwrap_or_default(),
            files: vec![],
            file: None,
            save: None,
            view: View::Town,
            raw_sel: 0,
            status: (String::new(), false),
            names: None,
            names_rx: None,
            names_dir: None,
            filter: String::new(),
            show_backups: false,
        };
        a.scan();
        a
    }

    fn msg(&mut self, s: impl Into<String>, err: bool) {
        self.status = (s.into(), err);
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
            self.msg("No .hps saves found. Enter your saves folder above (…\\Steam\\userdata\\<id>\\619820\\remote\\saves).", true);
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
                    format!("Loaded {}{}", p.file_name().unwrap().to_string_lossy(),
                        if run { "  — note: a dungeon run is in progress; its level data is kept untouched" } else { "" }),
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

    fn dirty(&self) -> bool {
        self.save.as_ref().map(|s| s.town.dirty || s.heroes.iter().any(|h| h.data.dirty)).unwrap_or(false)
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
            let out = save.to_bytes();
            // verify before touching the real file
            let chk = Save::parse(&out)?;
            if chk.town.val != save.town.val || chk.heroes.iter().zip(&save.heroes).any(|(a, b)| a.data.val != b.data.val) {
                return Err("verification failed; nothing was written".into());
            }
            let tmp = path.with_extension("hps.tmp");
            std::fs::write(&tmp, &out).map_err(|e| format!("write: {e}"))?;
            std::fs::rename(&tmp, &path).map_err(|e| format!("replace: {e}"))?;
            Ok(bpath)
        })();
        match res {
            Ok(b) => {
                let p = path.clone();
                self.open(p);
                self.msg(format!("Saved ✔  Backup: {}", b.display()), false);
            }
            Err(e) => self.msg(format!("Save failed: {e}"), true),
        }
    }
}

// ---------------- field helpers ----------------

fn int_edit(ui: &mut Ui, v: &mut V) -> bool {
    match v {
        V::Int(i) => ui.add(egui::DragValue::new(i).speed(1.0)).changed(),
        V::Long(i) => ui.add(egui::DragValue::new(i).speed(1.0)).changed(),
        V::Float(f) => ui.add(egui::DragValue::new(f).speed(0.01)).changed(),
        _ => {
            ui.label("-");
            false
        }
    }
}

fn max_btn(ui: &mut Ui, v: &mut V, max: i32) -> bool {
    if ui.small_button("max").on_hover_text(format!("Set to {max}")).clicked() {
        *v = V::Int(max);
        return true;
    }
    false
}

fn names_label(n: Option<&Names>, h: i64) -> String {
    match n {
        Some(n) => n.label(h as u32),
        None => format!("#{:08x}", h as u32),
    }
}

fn picker(ui: &mut Ui, id: &str, names: &Names, kind: Kind, filter: &mut String) -> Option<String> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.label("Add:");
        ui.add(egui::TextEdit::singleline(filter).hint_text("search…").desired_width(160.0));
    });
    let f = filter.to_lowercase();
    if f.len() >= 2 {
        egui::ScrollArea::vertical().id_salt(id).max_height(140.0).show(ui, |ui| {
            for d in names.list(kind).into_iter().filter(|d| d.name.to_lowercase().contains(&f) || d.id.contains(&f)).take(60) {
                if ui.button(format!("＋ {}  ({})", d.name, d.id)).clicked() {
                    picked = Some(d.id.clone());
                }
            }
        });
    }
    picked
}

/// Equipment item: [quality, baseHash, intensity, ilvl, upgrade, colorA, colorB, mods…]
fn equip_row(ui: &mut Ui, it: &mut V, names: Option<&Names>) -> (bool, bool) {
    let (mut ch, mut del) = (false, false);
    let Some(a) = it.arr_mut() else {
        ui.label(RichText::new(if matches!(it, V::Null) { "(empty slot)" } else { "(non-equipment item)" }).weak());
        return (false, false);
    };
    if a.len() < 5 {
        ui.label("(unrecognised item)");
        return (false, false);
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new(names_label(names, a[1].int().unwrap_or(0))).strong());
        let q = a[0].int().unwrap_or(0) as i32;
        egui::ComboBox::from_id_salt(ui.next_auto_id())
            .selected_text(QUAL.iter().find(|x| x.0 == q).map(|x| x.1).unwrap_or("?"))
            .width(90.0)
            .show_ui(ui, |ui| {
                for (v, n) in QUAL {
                    if ui.selectable_label(q == v, n).clicked() {
                        a[0] = V::Int(v);
                        ch = true;
                    }
                }
            });
        ui.label("ilvl");
        ch |= int_edit(ui, &mut a[3]);
        ui.label("upgrade");
        ch |= int_edit(ui, &mut a[4]);
        let mods: Vec<String> = a.iter().skip(7).map(|m| names_label(names, m.int().unwrap_or(0))).collect();
        let r = ui.label(RichText::new(format!("{} mods", mods.len())).weak());
        if !mods.is_empty() {
            r.on_hover_text(mods.join("\n"));
        }
        if ui.small_button("🗑").on_hover_text("Remove").clicked() {
            del = true;
        }
    });
    (ch, del)
}

// ---------------- views ----------------

fn town_view(ui: &mut Ui, save: &mut Save, names: Option<&Names>) -> bool {
    let mut ch = false;
    ui.horizontal(|ui| {
        ui.label("Town name:");
        ch |= ui.text_edit_singleline(&mut save.town_name).changed();
    });
    if ch {
        save.town.dirty = true; // header change still needs a rewrite
    }
    let blk = &mut save.town;
    if !blk.editable() {
        ui.colored_label(Color32::LIGHT_RED, format!("Town data can't be decoded safely: {}", blk.err.clone().unwrap_or_default()));
        return ch;
    }
    let t = blk.val.as_mut().unwrap();
    let mut c = false;
    ui.separator();
    ui.heading("Town resources");
    if let Some(m) = t.get_mut("materials").and_then(|m| m.arr_mut()) {
        egui::Grid::new("mats").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
            for (i, v) in m.iter_mut().enumerate() {
                ui.label(MATS.get(i).copied().unwrap_or("?"));
                c |= int_edit(ui, v);
                c |= max_btn(ui, v, 999_999);
                ui.end_row();
            }
        });
    }
    if let Some(v) = t.get_mut("ngp") {
        ui.horizontal(|ui| {
            ui.label("Town NG+ level:");
            c |= int_edit(ui, v);
        });
    }
    ui.separator();
    ui.heading("Buildings");
    if let Some(b) = t.get("buildings").and_then(|b| b.arr()) {
        ui.horizontal_wrapped(|ui| {
            for x in b {
                let h = x.get("building-def").and_then(|v| v.int()).unwrap_or(0);
                ui.label(RichText::new(names_label(names, h)).background_color(ui.visuals().faint_bg_color));
            }
        });
    }
    if let Some(bp) = t.get("blueprints").and_then(|b| b.arr()) {
        ui.label(format!("Blueprints unlocked: {}", bp.len()));
    }
    if let Some(st) = t.get("town-stash").and_then(|s| s.arr()) {
        let n: usize = st.iter().filter_map(|tab| tab.get("items").and_then(|i| i.arr())).map(|i| i.len()).sum();
        ui.label(format!("Town stash: {} tab(s), {n} item(s)", st.len()));
    }
    if c {
        blk.dirty = true;
    }
    ch | c
}

fn hero_view(ui: &mut Ui, blk: &mut Block, names: Option<&Names>, filter: &mut String) -> bool {
    if !blk.editable() {
        ui.colored_label(Color32::LIGHT_RED, format!("This hero can't be decoded safely: {}", blk.err.clone().unwrap_or_default()));
        return false;
    }
    let h = blk.val.as_mut().unwrap();
    let mut c = false;
    let class = h.get("class").and_then(|v| v.str()).unwrap_or("?").to_string();
    ui.horizontal(|ui| {
        ui.label("Name:");
        if let Some(V::Str(s)) = h.get_mut("name") {
            c |= ui.text_edit_singleline(s).changed();
        }
        ui.label(RichText::new(names.map(|n| n.id_label(&class)).unwrap_or(class)).weak());
    });
    if let Some(V::Bool(d)) = h.get_mut("dead") {
        if *d {
            ui.horizontal(|ui| {
                ui.colored_label(Color32::LIGHT_RED, "This hero is dead.");
                if ui.button("Revive").clicked() {
                    *d = false;
                    c = true;
                }
            });
        }
    }
    egui::CollapsingHeader::new(RichText::new("Stats & progression").heading()).default_open(true).show(ui, |ui| {
        egui::Grid::new("stats").num_columns(4).spacing([12.0, 4.0]).show(ui, |ui| {
            let mut col = 0;
            for (k, label) in STATS {
                if let Some(v) = h.get_mut(k) {
                    ui.label(label);
                    c |= int_edit(ui, v);
                    col += 1;
                    if col % 2 == 0 {
                        ui.end_row();
                    }
                }
            }
        });
        ui.label(RichText::new("Tip: Level and Experience should agree; raising Experience and letting the game level you up is safest.").weak().small());
    });
    egui::CollapsingHeader::new(RichText::new("Carried materials").heading()).show(ui, |ui| {
        if let Some(m) = h.get_mut("materials").and_then(|m| m.arr_mut()) {
            egui::Grid::new("hmats").num_columns(2).show(ui, |ui| {
                for (i, v) in m.iter_mut().enumerate() {
                    ui.label(MATS.get(i).copied().unwrap_or("?"));
                    c |= int_edit(ui, v);
                    ui.end_row();
                }
            });
        } else {
            ui.label("none");
        }
    });
    egui::CollapsingHeader::new(RichText::new("Equipped").heading()).default_open(true).show(ui, |ui| {
        if let Some(a) = h.get_mut("equipped").and_then(|m| m.arr_mut()) {
            for it in a.iter_mut() {
                let (ch, del) = equip_row(ui, it, names);
                c |= ch;
                if del {
                    *it = V::Null; // unequip (item is removed)
                    c = true;
                }
            }
        }
    });
    egui::CollapsingHeader::new(RichText::new("Inventory").heading()).default_open(true).show(ui, |ui| {
        if !matches!(h.get("equipment"), Some(V::Arr(_))) {
            if let Some(v) = h.get_mut("equipment") {
                *v = V::Arr(vec![]);
            }
        }
        let lvl = h.get("level").and_then(|v| v.int()).unwrap_or(1) as i32;
        if let Some(a) = h.get_mut("equipment").and_then(|m| m.arr_mut()) {
            let mut kill = None;
            for (i, it) in a.iter_mut().enumerate() {
                let (ch, del) = equip_row(ui, it, names);
                c |= ch;
                if del {
                    kill = Some(i);
                }
            }
            if let Some(i) = kill {
                a.remove(i);
                c = true;
            }
            if let Some(n) = names {
                if let Some(id) = picker(ui, "eqp", n, Kind::Equipment, filter) {
                    a.push(V::Arr(vec![
                        V::Int(16), V::Int(hps::hash_string(&id) as i32), V::Float(1.0),
                        V::Int(lvl.max(1)), V::Int(0), V::Int(0), V::Int(0),
                    ]));
                    c = true;
                }
            }
        }
    });
    egui::CollapsingHeader::new(RichText::new("Trinkets").heading()).show(ui, |ui| {
        if !matches!(h.get("trinkets"), Some(V::Arr(_))) {
            if let Some(v) = h.get_mut("trinkets") {
                *v = V::Arr(vec![]);
            }
        }
        if let Some(a) = h.get_mut("trinkets").and_then(|m| m.arr_mut()) {
            let mut kill = None;
            for (i, it) in a.iter().enumerate() {
                ui.horizontal(|ui| {
                    let s = it.str().map(|s| names.map(|n| n.id_label(s)).unwrap_or(s.to_string())).unwrap_or_else(|| format!("{:?}", it));
                    ui.label(s);
                    if ui.small_button("🗑").clicked() {
                        kill = Some(i);
                    }
                });
            }
            if let Some(i) = kill {
                a.remove(i);
                c = true;
            }
            if let Some(n) = names {
                if let Some(id) = picker(ui, "trk", n, Kind::Trinket, filter) {
                    a.push(V::Str(id));
                    c = true;
                }
            }
        }
    });
    egui::CollapsingHeader::new(RichText::new("Skills").heading()).show(ui, |ui| {
        if let Some(V::Dict(d)) = h.get_mut("picked-skills") {
            egui::Grid::new("skills").num_columns(2).show(ui, |ui| {
                for (k, v) in d.iter_mut() {
                    ui.label(names.map(|n| n.id_label(k)).unwrap_or(k.clone()));
                    c |= int_edit(ui, v);
                    ui.end_row();
                }
            });
        }
    });
    if c {
        blk.dirty = true;
    }
    c
}

/// Generic tree editor for anything the friendly tabs don't cover.
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
                        if ui.small_button("✖").on_hover_text("Delete element").clicked() {
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
                    V::Bool(b) => c |= ui.checkbox(b, "").changed(),
                    V::Int(i) => {
                        c |= ui.add(egui::DragValue::new(i)).changed();
                        if i.unsigned_abs() > 100_000 {
                            if let Some(n) = names {
                                let l = n.label(*i as u32);
                                if !l.starts_with('#') {
                                    ui.label(RichText::new(l).weak());
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
                        ui.label(RichText::new(other.kind()).weak());
                    }
                }
            });
        }
    }
    c
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
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

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Saves folder:");
                let r = ui.add(egui::TextEdit::singleline(&mut self.dir).desired_width(430.0));
                if ui.button("Scan").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    self.names = None;
                    self.scan();
                }
                let cur = self.file.as_ref().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).unwrap_or_default();
                let mut pick = None;
                egui::ComboBox::from_label("slot").selected_text(cur).show_ui(ui, |ui| {
                    for (i, (p, _)) in self.files.iter().enumerate() {
                        let n = p.file_name().unwrap().to_string_lossy().to_string();
                        let lbl = if i == 0 { format!("{n}  (newest)") } else { n };
                        if ui.selectable_label(Some(p) == self.file.as_ref(), lbl).clicked() {
                            pick = Some(p.clone());
                        }
                    }
                });
                if let Some(p) = pick {
                    self.open(p);
                }
            });
            ui.horizontal(|ui| {
                let dirty = self.dirty();
                if ui.add_enabled(dirty, egui::Button::new(RichText::new("💾 Save changes").strong())).clicked() {
                    self.write();
                }
                if ui.add_enabled(dirty, egui::Button::new("↺ Discard")).clicked() {
                    if let Some(p) = self.file.clone() {
                        self.open(p);
                    }
                }
                if ui.button("🗄 Backups").clicked() {
                    self.show_backups = !self.show_backups;
                }
                if self.names_rx.is_some() {
                    ui.spinner();
                    ui.label("reading game data…");
                }
                ui.label(RichText::new("Close the game before saving.").weak());
            });
            let (s, err) = &self.status;
            if !s.is_empty() {
                ui.colored_label(if *err { Color32::LIGHT_RED } else { Color32::LIGHT_GREEN }, s);
            }
            ui.add_space(2.0);
        });

        if self.show_backups {
            let mut open = true;
            let mut restore = None;
            egui::Window::new("Backups").open(&mut open).default_width(480.0).show(ctx, |ui| {
                let bdir = backup_dir(Path::new(&self.dir));
                ui.label(RichText::new(bdir.display().to_string()).weak().small());
                let mut v: Vec<PathBuf> = std::fs::read_dir(&bdir).map(|r| r.flatten().map(|e| e.path()).collect()).unwrap_or_default();
                v.sort();
                v.reverse();
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    if v.is_empty() {
                        ui.label("No editor backups yet. One is made automatically every time you save.");
                    }
                    for p in v {
                        ui.horizontal(|ui| {
                            ui.label(p.file_name().unwrap().to_string_lossy());
                            if ui.button("Restore").clicked() {
                                restore = Some(p.clone());
                            }
                        });
                    }
                });
            });
            self.show_backups = open;
            if let (Some(b), Some(f)) = (restore, self.file.clone()) {
                let r = std::fs::read(&b).map_err(|e| e.to_string()).and_then(|d| Save::parse(&d).map(|_| d)).and_then(|d| std::fs::write(&f, d).map_err(|e| e.to_string()));
                match r {
                    Ok(()) => {
                        self.open(f.clone());
                        self.msg(format!("Restored {} over {}", b.display(), f.display()), false);
                    }
                    Err(e) => self.msg(format!("Restore failed: {e}"), true),
                }
            }
        }

        let names = self.names.as_ref();
        let Some(save) = self.save.as_mut() else {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("No save loaded");
            });
            return;
        };

        egui::SidePanel::left("nav").resizable(false).exact_width(220.0).show(ctx, |ui| {
            ui.add_space(6.0);
            if ui.selectable_label(self.view == View::Town, RichText::new("🏰 Town").size(16.0)).clicked() {
                self.view = View::Town;
            }
            ui.separator();
            ui.label(RichText::new("Heroes").weak());
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, h) in save.heroes.iter().enumerate() {
                    let v = h.data.val.as_ref();
                    let name = v.and_then(|v| v.get("name")).and_then(|n| n.str()).filter(|n| !n.is_empty()).unwrap_or("(empty hero slot)");
                    let lvl = v.and_then(|v| v.get("level")).and_then(|n| n.int()).unwrap_or(0);
                    let mark = if h.data.dirty { " •" } else { "" };
                    if ui.selectable_label(self.view == View::Hero(i), format!("{name}  (Lv {lvl}){mark}")).clicked() {
                        self.view = View::Hero(i);
                    }
                }
                ui.separator();
                if ui.selectable_label(self.view == View::Raw, "🔧 Advanced (raw data)").clicked() {
                    self.view = View::Raw;
                }
            });
        });

        let filter = &mut self.filter;
        let raw_sel = &mut self.raw_sel;
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| match self.view {
                View::Town => {
                    town_view(ui, save, names);
                }
                View::Hero(i) => {
                    if let Some(h) = save.heroes.get_mut(i) {
                        if h.level.is_some() {
                            ui.colored_label(Color32::YELLOW, "⚠ This hero is mid-run. Changes may be overwritten when the run ends; finish or abandon the run first for best results.");
                        }
                        hero_view(ui, &mut h.data, names, filter);
                    }
                }
                View::Raw => {
                    ui.label("Edit any value directly. Changes here are applied exactly as typed — use with care.");
                    ui.horizontal(|ui| {
                        ui.selectable_value(raw_sel, 0, "Town");
                        for (i, h) in save.heroes.iter().enumerate() {
                            let n = h.data.val.as_ref().and_then(|v| v.get("name")).and_then(|n| n.str()).unwrap_or("?").to_string();
                            ui.selectable_value(raw_sel, i + 1, n);
                        }
                    });
                    ui.separator();
                    let blk = if *raw_sel == 0 { Some(&mut save.town) } else { save.heroes.get_mut(*raw_sel - 1).map(|h| &mut h.data) };
                    if let Some(blk) = blk {
                        if let Some(v) = blk.val.as_mut() {
                            if raw_tree(ui, "root", v, names, 0) {
                                blk.dirty = true;
                            }
                        }
                    }
                }
            });
        });
    }
}
