# Decisions

- Curated only: sources default to `.lnk`, non-recursive; the first run (or reset / corrupt config) registers only `%APPDATA%\ApolloLauncher\Shortcuts`. Bare exes enter via Add app / drag-drop, or by adding `exe` to a source's include list.
- Custom icons accept png/jpg/bmp/gif/webp/tiff/ico/exe/dll, resized to 128 px; priority custom > lnk icon > exe icon > letter tile.
- Edits (name, description, args, icon, accent, category, favorite, hidden) are overrides keyed by app id; the scanner never overwrites them.
- Dedup identity includes arguments.
- Entries whose target is missing are kept and shown greyed.
- Glyphs are painter-drawn; bold is simulated by double-drawing.
- File dialogs run on a thread and return `Option<Picked>` over a channel; cancel is a no-op.
- UI functions return events; the app applies them.
- Uncategorized tab appears only when non-empty; source default categories are auto-added.
- Recent shows up to 20. Window default 1100x700, minimum 480x360.
- Icons load lazily for visible cards on 3 worker threads.
- `APOLLOLAUNCHER_DATA_DIR` overrides the data directory (used for testing).
