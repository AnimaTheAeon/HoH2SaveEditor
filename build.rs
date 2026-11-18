// Embeds the app icon into the Windows .exe (Explorer, taskbar, shortcuts).
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    #[cfg(windows)]
    {
        let mut r = winresource::WindowsResource::new();
        r.set_icon("assets/icon.ico");
        // File > Properties > Details: anonymous executables look more suspicious to antivirus heuristics
        r.set("ProductName", "Hammersmith");
        r.set("FileDescription", "Hammersmith - Save Editor for Heroes of Hammerwatch II");
        r.set("CompanyName", "AnimaTheAeon");
        r.set("LegalCopyright", "Copyright (c) 2026 AnimaTheAeon");
        r.set("OriginalFilename", "Hammersmith.exe");
        r.set("InternalName", "hammersmith");
        r.set("Comments", "Open source: https://github.com/AnimaTheAeon/HoH2SaveEditor");
        r.compile().expect("embedding icon");
    }
}
