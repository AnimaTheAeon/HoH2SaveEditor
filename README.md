# Heroes of Hammerwatch II Save Editor

A tiny, single-file save editor for Heroes of Hammerwatch II, written in Rust (egui).

## Features
- Finds your Steam saves automatically (`Steam\userdata\<id>\619820\remote\saves`) and opens the newest slot.
- Shows real names for items, trinkets, buildings, classes and skills, read live from the game's own `res/assets.bin` and language files. Nothing is bundled, so updates are picked up automatically.
- **Town:** Gold, Wood, Stone, Iron, Crystals, Dust, Ore, plus NG+ level, and an overview of buildings, blueprints and stash.
- **Heroes:** name, level, XP, NG+, stats, elixirs, unspent points, revive, carried materials, equipped gear, inventory, trinkets and skill levels.
- **Advanced:** a raw tree editor for every value in the town and hero data.
- **Safety:** an automatic backup before every save (`%LOCALAPPDATA%\HeroesofHammerwatch2\editor_backups`), one-click restore, and a write-then-verify step so a damaged file is never written. Untouched data is copied byte-for-byte.

## Save format (reverse-engineered)
```
"H2SV" u32 version(4) u64 time_ms  cstr town_name  u16 (mods<<1|flag)  mods*u32  u32 0
block(town) u32 n  n * { u32 key, block(hero), u32 in_run [cstr level, block(level)] }
tail (current level, kept verbatim)  first 4 bytes of MD5(all preceding bytes)
block = u32 raw_size, u32 lz4_size, LZ4 block (lz4_size 0 = stored; raw_size 0 = null)
```
Values use Crackshell's SValue binary form. Type tags: 01 array, 02 dict, 04 float, 05 int, 06 string, 07 bytes, 08/09/0a vec2/3/4, 0b long, 10 double. Compact forms: 65 [] · 66 {} · 67 true · 68 false · 69 0f · 6a 0d · 6e 0 · 73 0L · 78 "" · 79 bytes[] · 7a/7c/7f zero vectors · 7b (x,0) · 8c/8d/8e splat vectors.
IDs are the game's `HashString()`: MurmurHash3 x86_32 with seed `0xF808409C`.

## Build
`cargo build --release` (Windows), or push to GitHub and let the Actions workflow build `HoH2SaveEditor.exe`.
Self-test any save: `hoh2edit --selftest path\to\0.hps`
