//! Minimal reader for the game's SValue XML definition files (.sval):
//! strips comments, expands %defblock/%block templates and builds a tree.
//! Used to find blueprints, item modifiers, base items, buildings and icons.

#[derive(Debug, Default, Clone)]
pub struct Node {
    pub tag: String,
    pub name: Option<String>,
    pub text: String,
    pub kids: Vec<Node>,
}

impl Node {
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.kids.iter().find(|k| k.name.as_deref() == Some(name))
    }
    pub fn s(&self, name: &str) -> Option<&str> {
        self.child(name).map(|n| n.text.trim()).filter(|t| !t.is_empty())
    }
    pub fn int(&self, name: &str) -> Option<i32> {
        self.s(name).and_then(|t| t.parse().ok())
    }
    pub fn flag(&self, name: &str, default: bool) -> bool {
        match self.s(name) {
            Some("true") => true,
            Some("false") => false,
            _ => default,
        }
    }
    pub fn strs(&self, name: &str) -> Vec<String> {
        self.child(name).map(|a| a.kids.iter().map(|k| k.text.trim().to_string()).collect()).unwrap_or_default()
    }
    /// `<array name="icon"><s>sheet.png</s><i>100</i><vec4>x y w h</vec4></array>`
    pub fn icon(&self) -> Option<IconRef> {
        let a = self.child("icon")?;
        let sheet = a.kids.first()?.text.trim().to_string();
        let v = a.kids.iter().find(|k| k.tag == "vec4")?;
        let n: Vec<f32> = v.text.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        (n.len() == 4 && sheet.ends_with(".png")).then(|| IconRef { sheet, rect: [n[0], n[1], n[2], n[3]] })
    }
    /// First descendant (depth-first) with the given name.
    pub fn find(&self, name: &str) -> Option<&Node> {
        for k in &self.kids {
            if k.name.as_deref() == Some(name) {
                return Some(k);
            }
            if let Some(f) = k.find(name) {
                return Some(f);
            }
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct IconRef {
    pub sheet: String,
    pub rect: [f32; 4],
}

/// Remove comments and expand templates the way the game's loader does.
pub fn preprocess(src: &str) -> String {
    // <!-- --> comments
    let mut s = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(i) = rest.find("<!--") {
        s.push_str(&rest[..i]);
        rest = match rest[i..].find("-->") {
            Some(j) => &rest[i + j + 3..],
            None => "",
        };
    }
    s.push_str(rest);
    // %defblock NAME ARGS…  body  %endblock
    let mut defs: Vec<(String, Vec<String>, String)> = vec![];
    let mut out = String::with_capacity(s.len());
    let mut lines = s.lines();
    while let Some(line) = lines.next() {
        let t = line.trim_start();
        if let Some(h) = t.strip_prefix("%defblock") {
            let mut it = h.split_whitespace();
            let name = it.next().unwrap_or("").to_string();
            let args: Vec<String> = it.map(|a| a.to_string()).collect();
            let mut body = String::new();
            for l in lines.by_ref() {
                if l.trim_start().starts_with("%endblock") {
                    break;
                }
                body.push_str(l);
                body.push('\n');
            }
            defs.push((name, args, body));
        } else if let Some(b) = t.strip_prefix("%block") {
            let mut it = b.split_whitespace();
            let name = it.next().unwrap_or("");
            let vals: Vec<&str> = it.collect();
            if let Some((_, args, body)) = defs.iter().find(|d| d.0 == name) {
                let mut e = body.clone();
                for (k, a) in args.iter().enumerate() {
                    e = e.replace(&format!("%%{a}%%"), vals.get(k).copied().unwrap_or(""));
                }
                out.push_str(&e);
            }
        } else if t.starts_with('%') {
            // %// comments, %include, %if … are not needed for the data we read
            continue;
        } else {
            match line.find("%//") {
                Some(i) => out.push_str(&line[..i]),
                None => out.push_str(line),
            }
            out.push('\n');
        }
    }
    out
}

/// Parse XML-ish markup into a list of top-level nodes.
pub fn parse(src: &str) -> Vec<Node> {
    let mut stack: Vec<Node> = vec![Node::default()];
    let b = src.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            let j = src[i..].find('<').map(|x| x + i).unwrap_or(b.len());
            stack.last_mut().unwrap().text.push_str(&src[i..j]);
            i = j;
            continue;
        }
        let Some(end) = src[i..].find('>').map(|x| x + i) else { break };
        let inner = &src[i + 1..end];
        i = end + 1;
        if inner.starts_with('?') || inner.starts_with('!') {
            continue;
        }
        if let Some(tag) = inner.strip_prefix('/') {
            let tag = tag.trim();
            // close the nearest matching open tag (tolerates stray tags)
            if let Some(pos) = stack.iter().rposition(|n| n.tag == tag) {
                if pos == 0 {
                    continue;
                }
                while stack.len() > pos {
                    let n = stack.pop().unwrap();
                    stack.last_mut().unwrap().kids.push(n);
                }
            }
            continue;
        }
        let self_close = inner.ends_with('/');
        let inner = inner.trim_end_matches('/');
        let tag = inner.split_whitespace().next().unwrap_or("").to_string();
        let name = inner.find("name=\"").and_then(|p| {
            let r = &inner[p + 6..];
            r.find('"').map(|q| r[..q].to_string())
        });
        let node = Node { tag, name, ..Default::default() };
        if self_close {
            stack.last_mut().unwrap().kids.push(node);
        } else {
            stack.push(node);
        }
    }
    while stack.len() > 1 {
        let n = stack.pop().unwrap();
        stack.last_mut().unwrap().kids.push(n);
    }
    stack.pop().unwrap().kids
}

/// Entries of a definition file: the dicts of the root array (LoadMultiple)
/// or the root dict itself (LoadSingle).
pub fn entries(src: &str) -> Vec<Node> {
    let nodes = parse(&preprocess(src));
    let mut out = vec![];
    for n in nodes {
        match n.tag.as_str() {
            "array" | "a" => out.extend(n.kids.into_iter().filter(|k| k.tag == "dict" || k.tag == "d")),
            "dict" | "d" => out.push(n),
            _ => {}
        }
    }
    out
}

pub fn quality_code(q: &str) -> i32 {
    match q {
        "common" => 1,
        "uncommon" => 2,
        "rare" => 4,
        "epic" => 8,
        "legendary" => 16,
        "quest" => 32,
        "unique" => 64,
        "cursed" => 128,
        _ => 0,
    }
}

/// Modifiers an item of this quality gets (the game's EquipmentModifiersPerQuality).
pub fn mods_per_quality(q: i32) -> usize {
    match q {
        2 => 1,
        4 => 2,
        8 => 3,
        16 => 4,
        _ => 0,
    }
}
