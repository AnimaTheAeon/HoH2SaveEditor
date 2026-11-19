#![cfg_attr(windows, windows_subsystem = "windows")]
mod gamedata;
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
use theme::{section, Section, gray, semibold, ACCENT, GRAY, LABEL, RED};

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
    art: Option<Art>,
    game_running: bool,
    game_check: f64,
    confirm: Option<Pending>,
    allow_close: bool,
}

/// Something that would throw away unsaved edits; asked about first.
#[derive(Clone)]
enum Pending {
    Open(PathBuf),
    Rescan,
    RestoreZip(PathBuf),
    RestoreSlot(PathBuf),
    Close,
}

/// Sprite sheets taken from the player's own game install at runtime.
struct Art {
    classes: egui::TextureHandle, // players/class_showcase.png, 32x32 per class
    town: egui::TextureHandle,    // gui/icons_town.png, 24x24 per icon
    sheets: std::collections::HashMap<String, egui::TextureHandle>,
}

impl Art {
    /// The game's own icon for an item, trinket or building (by id hash).
    fn icon(&self, names: Option<&Names>, h: u32) -> Option<(egui::TextureId, egui::Rect)> {
        let i = names?.icons.get(&h)?;
        let t = self.sheets.get(&i.sheet)?;
        let [w, hh] = t.size();
        let (w, hh) = (w as f32, hh as f32);
        let r = i.rect;
        Some((t.id(), egui::Rect::from_min_size(egui::pos2(r[0] / w, r[1] / hh), egui::vec2(r[2] / w, r[3] / hh))))
    }
}

/// Is Heroes of Hammerwatch II running? (Windows process list; nothing is read from the game.)
fn game_is_running() -> bool {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct Entry {
            size: u32,
            usage: u32,
            pid: u32,
            heap: usize,
            module: u32,
            threads: u32,
            parent: u32,
            prio: i32,
            flags: u32,
            exe: [u16; 260],
        }
        unsafe extern "system" {
            fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> isize;
            fn Process32FirstW(snap: isize, e: *mut Entry) -> i32;
            fn Process32NextW(snap: isize, e: *mut Entry) -> i32;
            fn CloseHandle(h: isize) -> i32;
        }
        unsafe {
            let snap = CreateToolhelp32Snapshot(0x2, 0);
            if snap == -1 {
                return false;
            }
            let mut e: Entry = std::mem::zeroed();
            e.size = std::mem::size_of::<Entry>() as u32;
            let mut found = false;
            let mut ok = Process32FirstW(snap, &mut e);
            while ok != 0 {
                let len = e.exe.iter().position(|&c| c == 0).unwrap_or(260);
                if String::from_utf16_lossy(&e.exe[..len]).eq_ignore_ascii_case("HWR2.exe") {
                    found = true;
                    break;
                }
                ok = Process32NextW(snap, &mut e);
            }
            CloseHandle(snap);
            found
        }
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HAMMERSMITH_FAKE_GAME").is_some()
    }
}

fn sheet_uv(sheet: f32, x: f32, y: f32, s: f32) -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(x / sheet, y / sheet), egui::vec2(s / sheet, s / sheet))
}

/// Where each class's figure sits on the game's class sheet.
fn class_uv(class: &str) -> Option<egui::Rect> {
    let (x, y) = match class.split('_').next().unwrap_or("") {
        "wizard" => (0.0, 0.0),
        "warrior" => (32.0, 0.0),
        "ranger" => (64.0, 0.0),
        "paladin" => (96.0, 0.0),
        "warlock" => (0.0, 32.0),
        "rogue" => (32.0, 32.0),
        "sorcerer" => (64.0, 32.0),
        "priest" => (0.0, 64.0),
        _ => return None,
    };
    Some(sheet_uv(128.0, x, y, 32.0))
}

// ---- dates for the save picker (no date library: keeps the exe small) ----
fn local_offset_secs(utc: i64) -> i64 {
    #[cfg(windows)]
    {
        #[repr(C)]
        #[derive(Default, Clone, Copy)]
        struct St { y: u16, m: u16, dow: u16, d: u16, h: u16, mi: u16, s: u16, ms: u16 }
        unsafe extern "system" {
            fn SystemTimeToTzSpecificLocalTime(tz: *const u8, utc: *const St, local: *mut St) -> i32;
        }
        let (y, mo, d, h, mi, s) = civil(utc);
        let u = St { y: y as u16, m: mo as u16, dow: 0, d: d as u16, h: h as u16, mi: mi as u16, s: s as u16, ms: 0 };
        let mut l = St::default();
        if unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &u, &mut l) } != 0 {
            let lt = days_from_civil(l.y as i64, l.m as i64, l.d as i64) * 86400 + l.h as i64 * 3600 + l.mi as i64 * 60 + l.s as i64;
            return lt - utc;
        }
        0
    }
    #[cfg(not(windows))]
    {
        let _ = utc;
        std::env::var("HAMMERSMITH_TZ_MIN").ok().and_then(|v| v.parse::<i64>().ok()).unwrap_or(0) * 60
    }
}
#[cfg_attr(not(windows), allow(dead_code))]
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468
}
fn civil(t: i64) -> (i64, i64, i64, i64, i64, i64) {
    let (days, secs) = (t.div_euclid(86400), t.rem_euclid(86400));
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d, secs / 3600, secs / 60 % 60, secs % 60)
}
/// "Oct 18, 11:13 AM" in the player's local time.
/// Save date for the picker; adds seconds when two saves share the same minute.
fn when(files: &[(PathBuf, u64)], t: u64) -> String {
    let base = friendly_time(t, false);
    if files.iter().filter(|(_, u)| friendly_time(*u, false) == base).count() > 1 { friendly_time(t, true) } else { base }
}

fn friendly_time(ms: u64, secs: bool) -> String {
    let utc = (ms / 1000) as i64;
    let (_, m, d, h, mi, s) = civil(utc + local_offset_secs(utc));
    const MON: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (h12, ap) = match h { 0 => (12, "AM"), 1..=11 => (h, "AM"), 12 => (12, "PM"), _ => (h - 12, "PM") };
    if secs { format!("{} {d}, {h12}:{mi:02}:{s:02} {ap}", MON[(m - 1) as usize]) } else { format!("{} {d}, {h12}:{mi:02} {ap}", MON[(m - 1) as usize]) }
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
            art: None,
            game_running: false,
            game_check: f64::NEG_INFINITY,
            confirm: None,
            allow_close: false,
        };
        a.scan();
        a
    }

    fn msg(&mut self, s: impl Into<String>, err: bool) {
        self.status = (s.into(), err);
        self.status_t = f64::NAN; // stamped with the frame time on next paint
    }

    fn dirty(&self) -> bool {
        self.save.as_ref().map(|s| s.town.dirty || s.heroes.iter().any(|h| h.data.dirty)).unwrap_or(false)
    }

    /// Ask before anything that would throw away unsaved edits.
    fn request(&mut self, p: Pending) {
        if self.dirty() {
            self.confirm = Some(p);
        } else {
            self.perform(p);
        }
    }

    fn perform(&mut self, p: Pending) {
        if self.game_running && matches!(p, Pending::RestoreZip(_) | Pending::RestoreSlot(_)) {
            self.msg("Close Heroes of Hammerwatch II before restoring a backup; the game would overwrite it.", true);
            return;
        }
        match p {
            Pending::Open(p) => self.open(p),
            Pending::Rescan => {
                self.names = None;
                self.art = None;
                self.scan();
            }
            Pending::RestoreZip(b) => match self.restore_zip(&b) {
                Ok(n) => {
                    self.scan();
                    self.msg(format!("Restored {n} save slots from {}", b.file_name().unwrap().to_string_lossy()), false);
                }
                Err(e) => self.msg(format!("Restore failed: {e}"), true),
            },
            Pending::RestoreSlot(b) => {
                if let Some(f) = self.file.clone() {
                    let r = std::fs::read(&b).map_err(|e| e.to_string()).and_then(|d| Save::parse(&d).map(|_| d)).and_then(|d| std::fs::write(&f, d).map_err(|e| e.to_string()));
                    match r {
                        Ok(()) => {
                            self.refresh_list();
                            self.open(f.clone());
                            self.msg(format!("Restored {}", b.file_name().unwrap().to_string_lossy()), false);
                        }
                        Err(e) => self.msg(format!("Restore failed: {e}"), true),
                    }
                }
            }
            Pending::Close => self.allow_close = true,
        }
    }

    /// Re-read the list of save files and their dates without reopening anything.
    fn refresh_list(&mut self) {
        self.files.clear();
        if let Ok(rd) = std::fs::read_dir(PathBuf::from(&self.dir)) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().map(|x| x == "hps").unwrap_or(false) {
                    let t = std::fs::read(&p).ok().filter(|b| b.len() > 16).map(|b| u64::from_le_bytes(b[8..16].try_into().unwrap())).unwrap_or(0);
                    self.files.push((p, t));
                }
            }
        }
        self.files.sort_by(|a, b| b.1.cmp(&a.1));
    }

    fn save_label(&self, p: &Path) -> String {
        match self.files.iter().position(|(f, _)| f == p) {
            Some(0) => format!("latest save ({})", when(&self.files, self.files[0].1)),
            Some(i) => format!("earlier save ({})", when(&self.files, self.files[i].1)),
            None => p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        }
    }

    fn scan(&mut self) {
        let dir = PathBuf::from(&self.dir);
        self.refresh_list();
        if self.files.is_empty() {
            self.save = None;
            self.msg("No saves found in that folder. Open Settings and point it at Steam\\userdata\\<your id>\\619820\\remote\\saves.", true);
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
                let label = self.save_label(&p);
                self.msg(
                    format!("Opened your {label}{}", if run { ". A dungeon run is in progress; its level data is left untouched." } else { "" }),
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

    /// Save to disk. Returns true on success.
    fn write(&mut self) -> bool {
        if self.game_running {
            self.msg("Close Heroes of Hammerwatch II before saving; the game would overwrite your changes.", true);
            return false;
        }
        let (Some(path), Some(save)) = (self.file.clone(), self.save.as_mut()) else { return false };
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
                self.refresh_list(); // the saved file is now the newest
                self.open(path.clone());
                self.msg(format!("Saved and verified. Backup: {}", b.file_name().unwrap().to_string_lossy()), false);
                true
            }
            Err(e) => {
                self.msg(format!("Save failed: {e}"), true);
                false
            }
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

pub fn cap_words(s: &str) -> String {
    s.split(['_', '-']).filter(|w| !w.is_empty()).map(|w| { let mut c = w.chars(); c.next().map(|x| x.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default() }).collect::<Vec<_>>().join(" ")
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

/// Quality a modifier needs to sit in slot `n` (the game's IndexToQuality(n + 2);
/// the first slot also takes Common modifiers).
fn slot_ok(n: usize, q: i32) -> bool {
    if n == 0 { q == 1 || q == 2 } else { q == 1 << (n + 1) }
}

/// Would the game accept modifier `m` in slot `slot` of this item, next to `others`?
/// Mirrors its roll rules: matching tags, one per exclusive group, one skill
/// modifier per item, and the modifier's item-level range.
fn mod_ok(n: &Names, base: u32, ilvl: i32, slot: usize, others: &[u32], m: &names::ModDef) -> bool {
    let Some(b) = n.bases.get(&base) else { return false };
    if !slot_ok(slot, m.quality) || ilvl < m.ilvl || ilvl > m.ilvl_max || !m.tags.iter().any(|t| b.tags.contains(t)) {
        return false;
    }
    let mut skill = b.skill;
    for o in others {
        let Some(od) = n.mods.get(o) else { continue };
        if *o == hps::hash_string(&m.id) || (m.exclusive != 0 && od.exclusive == m.exclusive) {
            return false;
        }
        skill |= od.skill;
    }
    !(skill && m.skill)
}

/// Short explanations for fields whose meaning isn't obvious.
fn tip_for(label: &str) -> Option<&'static str> {
    Some(match label.trim() {
        "New Game+" => "How many times this has been through New Game+. Each tier makes enemies tougher and raises the hero level cap by 5.",
        "Level" => "Changing Level also sets Experience to the start of that level. The cap is 20 plus 5 per New Game+ tier.",
        "Experience" => "Total experience earned. Changing it also updates Level to match.",
        "Unspent stat points" => "Attribute points waiting to be spent in game. Raising the level here doesn't add any, so add them here if you want them.",
        "Unspent skill points" => "Skill points waiting to be spent in game.",
        "Rerolls used" => "How many rerolls this hero has used. Lowering it gives them back.",
        "Dash charges" => "How many dashes the hero can do in a row before waiting for a recharge.",
        "Valor" => "Valor this hero has earned.",
        "Item level" => "Sets the item's power and which modifiers can roll on it. Usually close to the hero's level.",
        "Upgrade level" => "Times the item has been upgraded at the blacksmith (the +N on its name).",
        "Quality" => "Common to Legendary decides how many modifiers an item holds (0 to 4). Lowering it removes extra modifiers.",
        "Modifiers" => "Only modifiers the game itself could roll on this item are offered: matching item type, item level in range, one per group, and at most one skill modifier.",
        "Blueprints unlocked" => "Blueprints let shops and the forge offer modifiers, trinkets, drinks, food and dyes. Unlock All matches the game's own unlock-all cheat.",
        _ => return None,
    })
}

/// Row label with its hover tip, if it has one.
fn label_tip(ui: &mut Ui, label: impl Into<RichText>) {
    let l: RichText = label.into();
    let tip = tip_for(l.text());
    ui.label(l);
    if let Some(t) = tip {
        theme::info(ui, t);
    }
}

/// Number row: label … [Max] [value] [stepper]
fn num_row(s: &mut Section, label: &str, v: &mut V, step: bool, max: Option<i32>) -> bool {
    let mut c = false;
    s.row(|ui| {
    label_tip(ui, label);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
            if theme::text_btn(ui, "Max", ACCENT, false, true) {
                *v = V::Int(m);
                c = true;
            }
        }
    });
    });
    c
}

/// Experience needed to finish level `l` (the game's LevelExperience, in f32 like the game).
fn level_xp(l: i32) -> i64 {
    if l <= 0 { 0 } else { (2000.0f32 * (l as f32).powf(2.2)) as i64 }
}

/// The level a given amount of experience puts a hero at.
fn level_for_xp(xp: i64, cap: i32) -> i32 {
    let mut l = 1;
    while l < cap && xp >= level_xp(l) {
        l += 1;
    }
    l
}

fn as_i64(v: &V) -> i64 {
    match v {
        V::Int(i) => *i as i64,
        V::Long(i) => *i,
        V::Float(f) => *f as i64,
        _ => 0,
    }
}

fn set_num(v: &mut V, x: i64) {
    *v = match v {
        V::Long(_) => V::Long(x),
        V::Float(_) => V::Float(x as f32),
        _ => V::Int(x.clamp(0, i32::MAX as i64) as i32),
    };
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
                let r = ui.add(egui::Button::new(RichText::new(format!("Add {}", d.name)).color(ACCENT)).frame(false));
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

/// Equipment list with expandable rows.
/// Item: [quality, baseHash, intensity, ilvl, upgrade, colorA, colorB, mods…]
fn gear_rows(s: &mut Section, items: &mut Vec<V>, names: Option<&Names>, art: Option<&Art>, salt: &str, equipped: bool) -> bool {
    let mut c = false;
    let mut kill = None;
    for (i, it) in items.iter_mut().enumerate() {
        let Some(a) = it.arr_mut().filter(|a| a.len() >= 7) else {
            s.row(|ui| ui.label(gray(if matches!(it, V::Null) { "Empty slot" } else { "Unrecognised item" })));
            continue;
        };
        let id = egui::Id::new((salt, i));
        let open = s.ui.data(|d| d.get_temp::<bool>(id).unwrap_or(false));
        let q = a[0].int().unwrap_or(0) as i32;
        let base = a[1].int().unwrap_or(0) as u32;
        let (name, base_id) = split_label(&names_label(names, base as i64));
        let mod_ids: Vec<u32> = a.iter().skip(7).map(|m| m.int().unwrap_or(0) as u32).collect();
        let mod_name = |h: u32| names.and_then(|n| n.mods.get(&h)).map(|d| d.name.clone()).unwrap_or_else(|| pretty_id(&split_label(&names_label(names, h as i64)).0));
        let icon = art.and_then(|ar| ar.icon(names, base));
        let clicked = s.row_indent(if icon.is_some() { 62.0 } else { 16.0 }, |ui| {
            if let Some((t, uv)) = icon {
                theme::sprite_tile(ui, t, uv, qual_color(q), 36.0);
                ui.add_space(4.0);
            }
            ui.vertical(|ui| {
                ui.label(semibold(&name, 15.0).color(qual_color(q)));
                ui.label(gray(format!("{} · Item level {} · +{}{}", qual_name(q), a[3].int().unwrap_or(0), a[4].int().unwrap_or(0),
                    match mod_ids.len() { 0 => String::new(), 1 => " · 1 modifier".into(), n => format!(" · {n} modifiers") })).size(12.5));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| theme::chevron(ui, open));
            let full = egui::Rect::from_x_y_ranges(ui.max_rect().x_range(), ui.min_rect().y_range());
            theme::click_area(ui, full, id.with("row")).clicked()
        });
        if clicked {
            s.ui.data_mut(|d| d.insert_temp(id, !open));
        }
        if !open {
            continue;
        }
        s.row(|ui| {
            ui.add_space(16.0);
            label_tip(ui, gray("Quality"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::ComboBox::from_id_salt(id.with("q")).selected_text(RichText::new(qual_name(q)).color(qual_color(q))).show_ui(ui, |ui| {
                    for (v, n) in QUAL {
                        if ui.selectable_label(q == v, RichText::new(n).color(qual_color(v))).clicked() && v != q {
                            a[0] = V::Int(v);
                            // the regular qualities hold a fixed number of modifiers
                            let cap = gamedata::mods_per_quality(v);
                            if v <= 16 && a.len() > 7 + cap {
                                a.truncate(7 + cap);
                            }
                            c = true;
                        }
                    }
                });
            });
        });
        c |= num_row(s, "    Item level", &mut a[3], true, None);
        c |= num_row(s, "    Upgrade level", &mut a[4], true, None);
        // modifiers
        let q = a[0].int().unwrap_or(0) as i32;
        let cap = gamedata::mods_per_quality(q);
        let ilvl = a[3].int().unwrap_or(0) as i32;
        s.row(|ui| {
            ui.add_space(16.0);
            label_tip(ui, gray("Modifiers"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(gray(if q <= 16 { format!("{} of {cap}", mod_ids.len()) } else { format!("{}", mod_ids.len()) }));
            });
        });
        // Each slot only takes modifiers of its own quality, so a modifier is
        // swapped in place; only the last one can be removed (removing one in
        // the middle would shift the rest into slots they don't fit).
        let mut drop_mod = None;
        let mut swap: Option<(usize, u32)> = None;
        let last = mod_ids.len().saturating_sub(1);
        for (k, h) in mod_ids.iter().enumerate() {
            let nm = mod_name(*h);
            let others: Vec<u32> = mod_ids.iter().enumerate().filter(|(j, _)| *j != k).map(|(_, x)| *x).collect();
            let mut alts: Vec<&names::ModDef> = names
                .map(|n| n.mods.values().filter(|m| hps::hash_string(&m.id) != *h && mod_ok(n, base, ilvl, k, &others, m)).collect())
                .unwrap_or_default();
            alts.sort_by(|x, y| x.name.cmp(&y.name));
            let col = names.and_then(|n| n.mods.get(h)).map(|d| qual_color(d.quality)).unwrap_or(LABEL);
            s.row(|ui| {
                ui.add_space(32.0);
                if alts.is_empty() {
                    ui.add(egui::Label::new(RichText::new(&nm).color(col).size(13.5)).truncate());
                } else {
                    egui::ComboBox::from_id_salt(id.with(("mod", k))).width(400.0).height(420.0)
                        .selected_text(RichText::new(&nm).color(col).size(13.5))
                        .show_ui(ui, |ui| {
                            ui.label(gray("Swap for:").size(12.0));
                            for m in &alts {
                                if ui.selectable_label(false, RichText::new(&m.name).color(qual_color(m.quality))).clicked() {
                                    swap = Some((k, hps::hash_string(&m.id)));
                                }
                            }
                        });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if k == last && theme::text_btn(ui, "Remove", RED, false, true) {
                        drop_mod = Some(k);
                    }
                });
            });
        }
        if let Some((k, h)) = swap {
            a[7 + k] = V::Int(h as i32);
            c = true;
        }
        if let Some(k) = drop_mod {
            a.remove(7 + k);
            c = true;
        }
        if let Some(n) = names {
            if q <= 16 && mod_ids.len() < cap && drop_mod.is_none() && swap.is_none() {
                let slot = mod_ids.len();
                let mut opts: Vec<&names::ModDef> = n.mods.values().filter(|m| mod_ok(n, base, ilvl, slot, &mod_ids, m)).collect();
                opts.sort_by(|x, y| x.name.cmp(&y.name));
                s.row(|ui| {
                    ui.add_space(32.0);
                    if opts.is_empty() {
                        ui.label(gray(if n.bases.contains_key(&base) { "No other modifiers fit this item at this item level" } else { "This item's modifiers can't be looked up" }).size(13.0));
                        return;
                    }
                    egui::ComboBox::from_id_salt(id.with("addmod")).width(330.0).height(420.0)
                        .selected_text(RichText::new(format!("Add a modifier ({} fit)", opts.len())).color(ACCENT))
                        .show_ui(ui, |ui| {
                            for m in &opts {
                                if ui.selectable_label(false, RichText::new(&m.name).color(qual_color(m.quality))).clicked() {
                                    a.push(V::Int(hps::hash_string(&m.id) as i32));
                                    c = true;
                                }
                            }
                        });
                });
            }
        }
        if !base_id.is_empty() {
            s.kv(gray("    Base item"), |ui| ui.label(gray(&base_id)));
        }
        s.row(|ui| {
            ui.add_space(16.0);
            if theme::text_btn(ui, if equipped { "Unequip & Delete" } else { "Delete Item" }, RED, false, true) {
                kill = Some(i);
            }
        });
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

fn town_view(ui: &mut Ui, save: &mut Save, names: Option<&Names>, art: Option<&Art>) -> bool {
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
        if let Some(bp) = t.get_mut("blueprints").and_then(|b| b.arr_mut()) {
            let have: std::collections::HashSet<u32> = bp.iter().filter_map(|v| v.str()).map(hps::hash_string).collect();
            let missing: Vec<&String> = names.map(|n| n.blueprint_pool.iter().filter(|(_, h)| !have.contains(h)).map(|(s, _)| s).collect()).unwrap_or_default();
            let n = bp.len();
            let mut add = false;
            s.row(|ui| {
                label_tip(ui, "Blueprints unlocked");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !missing.is_empty() && theme::text_btn(ui, "Unlock All", ACCENT, false, true) {
                        add = true;
                    }
                    let total = names.map(|x| x.blueprint_pool.len()).filter(|t| *t > 0);
                    ui.label(gray(match total { Some(_) if missing.is_empty() => format!("{n} · all unlocked"), Some(_) => format!("{n} · {} to go", missing.len()), None => n.to_string() }));
                });
            });
            if add {
                let k = missing.len();
                bp.extend(missing.into_iter().map(|s| V::Str(s.clone())));
                c = true;
                s.ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("toast_req"), format!("Unlocked {k} blueprints. Save to keep them.")));
            }
        }
        if let Some(st) = t.get("town-stash").and_then(|s| s.arr()) {
            let n: usize = st.iter().filter_map(|tab| tab.get("items").and_then(|i| i.arr())).map(|i| i.len()).sum();
            let tabs = st.len();
            s.kv("Town stash", |ui| ui.label(gray(format!("{n} items in {tabs} tab{}", if tabs == 1 { "" } else { "s" }))));
        }
    });
    section(ui, Some("Buildings"), Some("The game normally needs a higher Guild Hall before other buildings can be upgraded. That isn't checked here."), |s| {
        if let Some(b) = t.get_mut("buildings").and_then(|b| b.arr_mut()) {
            for x in b.iter_mut() {
                let h = x.get("building-def").and_then(|v| v.int()).unwrap_or(0) as u32;
                let def = names.and_then(|n| n.buildings.get(&h));
                let n = def.map(|d| d.name.clone()).unwrap_or_else(|| split_label(&names_label(names, h as i64)).0);
                let icon = art.and_then(|a| a.icon(names, h));
                let max = def.map(|d| d.max_variation);
                s.row_indent(if icon.is_some() { 62.0 } else { 16.0 }, |ui| {
                    if let Some((tx, uv)) = icon {
                        theme::sprite_tile(ui, tx, uv, ACCENT, 36.0);
                        ui.add_space(4.0);
                    }
                    ui.label(&n);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        match (x.get_mut("variation"), max) {
                            (Some(v), Some(mx)) if matches!(v, V::Int(_)) => {
                                let cur = v.int().unwrap_or(0) as i32;
                                let d = theme::stepper(ui);
                                let nv = (cur + d).clamp(0, mx);
                                if nv != cur {
                                    *v = V::Int(nv);
                                    c = true;
                                }
                                ui.label(if nv == mx { RichText::new(format!("Level {} of {} · Max", nv + 1, mx + 1)).color(theme::GREEN) } else { gray(format!("Level {} of {}", nv + 1, mx + 1)) });
                            }
                            _ => {
                                ui.label(RichText::new("Built").color(theme::GREEN));
                            }
                        }
                    });
                });
            }
        }
    });
    if c {
        blk.dirty = true;
    }
    ch | c
}

fn hero_view(ui: &mut Ui, blk: &mut Block, names: Option<&Names>, filter: &mut String, filter2: &mut String, in_run: bool, art: Option<&Art>) -> bool {
    if !blk.editable() {
        ui.colored_label(RED, format!("This hero can't be decoded safely: {}", blk.err.clone().unwrap_or_default()));
        return false;
    }
    let h = blk.val.as_mut().unwrap();
    let mut c = false;
    let class = h.get("class").and_then(|v| v.str()).unwrap_or("?").to_string();
    let class_n = names.map(|n| n.class_name(&class)).unwrap_or_else(|| cap_words(class.split('_').next().unwrap_or("?")));
    let lvl = h.get("level").and_then(|v| v.int()).unwrap_or(1) as i32;
    let title = h.get("name").and_then(|v| v.str()).filter(|s| !s.is_empty()).unwrap_or("Empty Hero Slot").to_string();
    match (art, class_uv(&class)) {
        (Some(a), Some(uv)) => {
            ui.horizontal(|ui| {
                theme::sprite_tile(ui, a.classes.id(), uv, class_style(&class).1, 72.0);
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.add_space(6.0);
                    theme::large_title(ui, &title, &format!("{class_n} · Level {lvl}"));
                });
            });
        }
        _ if class == "?" || class.is_empty() => theme::large_title(ui, &title, "No hero yet"),
        _ => theme::large_title(ui, &title, &format!("{class_n} · Level {lvl}")),
    }

    if h.get("class").and_then(|v| v.str()).filter(|s| !s.is_empty()).is_none() {
        section(ui, None, None, |s| {
            s.row(|ui| ui.label(gray("This slot doesn't have a hero yet. Create one in the game, then come back to edit it.")));
        });
        return false;
    }
    let footer = if in_run { Some("This hero is mid-run. The game may overwrite changes when the run ends.") } else { None };
    section(ui, Some("Profile"), footer, |s| {
        if let Some(V::Str(n)) = h.get_mut("name") {
            s.kv("Name", |ui| c |= theme::text_field(ui, n, 240.0).changed());
        }
        s.kv("Class", |ui| ui.label(gray(&class_n)));
        if let Some(V::Bool(d)) = h.get_mut("dead") {
            s.kv("Status", |ui| {
                if *d {
                    if theme::filled_btn(ui, "Revive", theme::ACCENT_FILL) {
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
    let ngp = h.get("ngp").and_then(|v| v.int()).unwrap_or(0) as i32;
    let cap = 20 + 5 * ngp.max(0);
    let foot = format!("Level and Experience stay in step. This hero's level cap is {cap} (20, plus 5 per New Game+ tier). Levels added here don't grant stat or skill points; add those under Unspent points.");
    section(ui, Some("Progression"), Some(&foot), |s| {
        // Level <-> Experience, kept consistent the way the game computes it
        if h.get("level").is_some() && h.get("experience").is_some() {
            let mut lv = V::Int(lvl);
            if num_row(s, "Level", &mut lv, true, None) {
                let nl = (as_i64(&lv) as i32).clamp(1, cap);
                if let Some(v) = h.get_mut("level") { *v = V::Int(nl); }
                if let Some(v) = h.get_mut("experience") { set_num(v, level_xp(nl - 1)); }
                c = true;
            }
            let mut xp = h.get("experience").cloned().unwrap_or(V::Int(0));
            if num_row(s, "Experience", &mut xp, false, None) {
                let x = as_i64(&xp).clamp(0, level_xp(cap) - 1);
                if let Some(v) = h.get_mut("experience") { set_num(v, x); }
                if let Some(v) = h.get_mut("level") { *v = V::Int(level_for_xp(x, cap)); }
                c = true;
            }
        }
        for (k, label) in [("ngp", "New Game+"), ("bonus-stats", "Unspent stat points"),
            ("bonus-skills", "Unspent skill points"), ("rerolls-used", "Rerolls used"), ("dash-charges", "Dash charges"), ("valor", "Valor")] {
            if let Some(v) = h.get_mut(k) {
                c |= num_row(s, label, v, true, None);
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
    section(ui, Some("Equipped"), Some("Click an item to change its quality, levels and modifiers."), |s| {
        if let Some(a) = h.get_mut("equipped").and_then(|m| m.arr_mut()) {
            c |= gear_rows(s, a, names, art, "eq", true);
        }
    });
    if !matches!(h.get("equipment"), Some(V::Arr(_))) {
        if let Some(v) = h.get_mut("equipment") {
            *v = V::Arr(vec![]);
        }
    }
    section(ui, Some("Inventory"), Some("New items are added as Legendary at the hero's level. Open one to give it modifiers."), |s| {
        if let Some(a) = h.get_mut("equipment").and_then(|m| m.arr_mut()) {
            if a.is_empty() {
                s.row(|ui| ui.label(gray("No items")));
            }
            c |= gear_rows(s, a, names, art, "inv", false);
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
                let icon = it.str().and_then(|x| art.and_then(|ar| ar.icon(names, hps::hash_string(x))));
                s.row_indent(if icon.is_some() { 62.0 } else { 16.0 }, |ui| {
                    if let Some((tx, uv)) = icon {
                        theme::sprite_tile(ui, tx, uv, theme::PURPLE, 36.0);
                        ui.add_space(4.0);
                    }
                    ui.label(n);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::text_btn(ui, "Remove", RED, false, true) {
                            kill = Some(i);
                        }
                    });
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

/// Sidebar row. Returns (tapped, row rect). The selected highlight is
/// drawn separately so it can glide between rows.
fn side_item(ui: &mut Ui, sel: bool, icon: theme::Icon, title: &str, trailing: &str) -> (bool, egui::Rect) {
    let r = egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            theme::icon_tile(ui, icon);
            ui.add(egui::Label::new(RichText::new(title).size(15.0).color(LABEL)).truncate());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(trailing).size(13.5).color(if sel { LABEL.gamma_multiply(0.9) } else { GRAY }));
            });
        });
    });
    let rect = r.response.rect;
    let resp = theme::click_area(ui, rect, ui.id().with(("side", title, rect.top() as i32)));
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
        [0.039, 0.031, 0.035, 1.0]
    }

    fn update(&mut self, ctx: &egui::Context, _f: &mut eframe::Frame) {
        theme::background(ctx);
        ctx.request_repaint_after(std::time::Duration::from_millis(100)); // gentle light drift
        if let Some(rx) = &self.names_rx {
            if let Ok(r) = rx.try_recv() {
                match r {
                    Ok(n) => {
                        let tex = |key: &str| {
                            n.images.get(key).map(|(w, h, px)| {
                                ctx.load_texture(key, egui::ColorImage::from_rgba_unmultiplied([*w, *h], px), egui::TextureOptions::NEAREST)
                            })
                        };
                        if let (Some(classes), Some(town)) = (tex("players/class_showcase.png"), tex("gui/icons_town.png")) {
                            let sheets = n.images.keys().filter_map(|k| tex(k).map(|t| (k.clone(), t))).collect();
                            self.art = Some(Art { classes, town, sheets });
                        }
                        self.names = Some(n);
                    }
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
        if now - self.game_check > 2.0 {
            self.game_check = now;
            self.game_running = game_is_running();
        }
        // closing the window with unsaved edits asks first
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Pending::Close);
        }
        if self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) && self.dirty() {
            self.write();
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
                        if side_item(ui, self.view == View::Settings, theme::Icon::Glyph("S", theme::GRAY3), "Settings", "").0 {
                            self.view = View::Settings;
                        }
                        return;
                    };
                    let hl = ui.painter().add(egui::Shape::Noop);
                    let mut sel_rect = None;
                    let art = self.art.as_ref();
                    let town_icon = |x: f32, y: f32, g: &'static str, c: Color32| match art {
                        Some(a) => theme::Icon::Sprite(a.town.id(), sheet_uv(256.0, x, y, 24.0), c),
                        None => theme::Icon::Glyph(g, c),
                    };
                    let mut item = |ui: &mut Ui, v: View, icon: theme::Icon, title: &str, trail: &str, view: &mut View| {
                        let (clicked, r) = side_item(ui, *view == v, icon, title, trail);
                        if *view == v {
                            sel_rect = Some(r);
                        }
                        if clicked {
                            *view = v;
                        }
                    };
                    item(ui, View::Town, town_icon(0.0, 0.0, "T", theme::GREEN), "Town", if save.town.dirty { "Edited" } else { "" }, &mut self.view);
                    side_header(ui, "Heroes");
                    for (i, h) in save.heroes.iter().enumerate() {
                        let v = h.data.val.as_ref();
                        let name = v.and_then(|v| v.get("name")).and_then(|n| n.str()).filter(|n| !n.is_empty()).unwrap_or("Empty Slot");
                        let lvl = v.and_then(|v| v.get("level")).and_then(|n| n.int()).unwrap_or(0);
                        let class = v.and_then(|v| v.get("class")).and_then(|n| n.str()).unwrap_or("");
                        let (g, col) = class_style(class);
                        let icon = match (art, class_uv(class)) {
                            (Some(a), Some(uv)) => theme::Icon::Sprite(a.classes.id(), uv, col),
                            _ => theme::Icon::Glyph(g, col),
                        };
                        let trail = if h.data.dirty { "Edited".to_string() } else if lvl > 0 { format!("Lv {lvl}") } else { String::new() };
                        item(ui, View::Hero(i), icon, name, &trail, &mut self.view);
                    }
                    side_header(ui, "More");
                    item(ui, View::Raw, town_icon(24.0, 0.0, "{}", theme::INDIGO), "Advanced", "", &mut self.view);
                    item(ui, View::Settings, town_icon(96.0, 96.0, "S", theme::GRAY3), "Settings", "", &mut self.view);
                    // selection: a blue glass lozenge that glides to the selected row
                    if let Some(r) = sel_rect {
                        let ctx = ui.ctx().clone();
                        let y = ctx.animate_value_with_time(egui::Id::new("hl_y"), r.top(), 0.22);
                        let h = ctx.animate_value_with_time(egui::Id::new("hl_h"), r.height(), 0.22);
                        let rr = egui::Rect::from_min_size(egui::pos2(r.left(), y), egui::vec2(r.width(), h));
                        let mut shapes = vec![egui::Shape::rect_filled(rr, 12.0, theme::ACCENT_FILL.gamma_multiply(0.92))];
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
        let dirty = self.dirty();
        let running = self.game_running;
        let filter = &mut self.filter;
        let filter2 = &mut self.filter2;
        let art = self.art.as_ref();
        let raw_sel = &mut self.raw_sel;

        let mut rescan = false;
        egui::CentralPanel::default().frame(egui::Frame::new().inner_margin(egui::Margin { left: 18, right: 20, top: 14, bottom: 0 })).show(ctx, |ui| {
            // nav bar
            ui.horizontal(|ui| {
                // the game keeps ~10 rotating copies of one save (0.hps, 0_1.hps …); show them by date
                let idx = self.files.iter().position(|(p, _)| Some(p) == self.file.as_ref());
                let cur = match idx {
                    Some(0) => format!("Latest save · {}", when(&self.files, self.files[0].1)),
                    Some(i) => format!("Older save · {}", when(&self.files, self.files[i].1)),
                    None => "No save loaded".into(),
                };
                ui.label(gray("Editing"));
                egui::ComboBox::from_id_salt("slot").selected_text(RichText::new(cur).color(ACCENT)).width(250.0).height(560.0).show_ui(ui, |ui| {
                    ui.set_min_width(330.0);
                    ui.label(gray("Your game keeps your last few saves.\nThe latest one is where you'll continue playing.").size(12.5));
                    ui.add_space(4.0);
                    for (i, (p, t)) in self.files.iter().enumerate() {
                        let mut txt = egui::text::LayoutJob::default();
                        txt.append(&when(&self.files, *t), 0.0, egui::TextFormat { font_id: egui::FontId::proportional(14.5), color: LABEL, ..Default::default() });
                        let tag = if i == 0 { "   Latest" } else { "   Earlier" };
                        txt.append(tag, 0.0, egui::TextFormat { font_id: egui::FontId::proportional(12.5), color: if i == 0 { ACCENT } else { GRAY }, ..Default::default() });
                        let file = p.file_name().unwrap().to_string_lossy().to_string();
                        if ui.selectable_label(Some(p) == self.file.as_ref(), txt).on_hover_text(format!("File: {file}")).clicked() {
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
                    if theme::text_btn(ui, "Save", ACCENT, true, dirty && !running) {
                        do_save = true;
                    }
                    ui.add_space(6.0);
                    if theme::text_btn(ui, "Discard", RED, false, dirty) {
                        do_discard = true;
                    }
                    ui.add_space(6.0);
                    if theme::text_btn(ui, "Backups", ACCENT, false, true) {
                        self.show_backups = true;
                    }
                    ui.add_space(6.0);
                    if theme::text_btn(ui, "Back Up All", ACCENT, false, true) {
                        do_backup = true;
                    }
                    });
                    });
                    theme::sheen(ui.painter(), r.response.rect, 16.0);
                });
            });
            if running {
                ui.add_space(4.0);
                theme::pill(ui, "Heroes of Hammerwatch II is running. Close the game before saving; it would overwrite your changes.", theme::WARN);
            }
            ui.add_space(6.0);
            egui::ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
                // keep a comfortable reading width and centre it in wide windows
                let full = ui.available_width();
                let cw = full.min(900.0);
                let x0 = ui.cursor().left() + ((full - cw) / 2.0).floor();
                let col = egui::Rect::from_min_size(egui::pos2(x0, ui.cursor().top()), egui::vec2(cw, f32::INFINITY));
                ui.scope_builder(egui::UiBuilder::new().max_rect(col), |ui| {
                match (self.view, self.save.as_mut()) {
                    (View::Settings, _) | (_, None) => {
                        theme::large_title(ui, "Settings", "");
                        section(ui, Some("Saves folder"), Some("Usually Steam\\userdata\\<your id>\\619820\\remote\\saves. Found automatically."), |s| {
                            s.row(|ui| {
                                let r = ui.add(egui::TextEdit::singleline(&mut self.dir).frame(false).desired_width(ui.available_width() - 70.0));
                                if theme::text_btn(ui, "Scan", ACCENT, true, true) || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
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
                                if theme::text_btn(ui, "Back Up All Saves Now", ACCENT, false, true) {
                                    do_backup = true;
                                }
                            });
                            s.row(|ui| {
                                if theme::text_btn(ui, "Show Backups", ACCENT, false, true) {
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
                        town_view(ui, save, names, art);
                    }
                    (View::Hero(i), Some(save)) => {
                        if let Some(h) = save.heroes.get_mut(i) {
                            let run = h.level.is_some();
                            hero_view(ui, &mut h.data, names, filter, filter2, run, art);
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
                                egui::ComboBox::from_id_salt("rawsel").selected_text(RichText::new(name_of(*raw_sel)).color(ACCENT)).show_ui(ui, |ui| {
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
        });

        if let Some(t) = ctx.data_mut(|d| d.remove_temp::<String>(egui::Id::new("toast_req"))) {
            self.msg(t, false);
        }
        if let Some(p) = pick {
            if Some(&p) != self.file.as_ref() {
                self.request(Pending::Open(p));
            }
        }
        if rescan {
            self.request(Pending::Rescan);
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
                            if theme::text_btn(ui, "Done", ACCENT, true, true) {
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
                                    if theme::text_btn(ui, "Restore All", ACCENT, false, true) {
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
                                    if theme::text_btn(ui, "Restore", ACCENT, false, true) {
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
                    self.request(Pending::RestoreZip(b));
                } else {
                    self.request(Pending::RestoreSlot(b));
                }
            }
        }

        // ---------- unsaved changes sheet ----------
        if let Some(p) = self.confirm.clone() {
            let mut choice = 0; // 1 save, 2 don't save, 3 cancel
            let what = match &p {
                Pending::Close => "closing",
                Pending::Open(_) => "switching saves",
                Pending::Rescan => "rescanning",
                Pending::RestoreZip(_) | Pending::RestoreSlot(_) => "restoring a backup",
            };
            let running = self.game_running;
            let m = egui::Modal::new(egui::Id::new("unsaved"))
                .backdrop_color(Color32::from_black_alpha(130))
                .frame(theme::glass(Color32::from_rgba_unmultiplied(30, 30, 40, 245), 24).inner_margin(egui::Margin::same(22)))
                .show(ctx, |ui| {
                    ui.set_width(380.0);
                    ui.label(RichText::new("Save your changes?").font(theme::bold(18.0)));
                    ui.add_space(4.0);
                    ui.label(gray(format!("You have edits that haven't been saved. They'll be lost if you continue {what} without saving.")));
                    if running {
                        ui.add_space(4.0);
                        ui.label(RichText::new("The game is running, so saving is blocked. Close the game first.").color(theme::WARN).size(13.0));
                    }
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if theme::text_btn(ui, "Cancel", ACCENT, false, true) {
                            choice = 3;
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if !running && theme::filled_btn(ui, "Save", theme::ACCENT_FILL) {
                                choice = 1;
                            }
                            ui.add_space(6.0);
                            if theme::text_btn(ui, "Don't Save", RED, false, true) {
                                choice = 2;
                            }
                        });
                    });
                });
            if m.should_close() && choice == 0 {
                choice = 3;
            }
            match choice {
                1 => {
                    self.confirm = None;
                    if self.write() {
                        self.perform(p);
                    }
                }
                2 => {
                    self.confirm = None;
                    if let Some(s) = self.save.as_mut() {
                        s.town.dirty = false;
                        s.heroes.iter_mut().for_each(|h| h.data.dirty = false);
                    }
                    self.perform(p);
                }
                3 => self.confirm = None,
                _ => {}
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
