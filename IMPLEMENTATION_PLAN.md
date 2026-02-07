# IMPLEMENTATION_PLAN — Local Vector Search + Global Cmd+K

> Goal: fully on-device hybrid search (FTS → vector rerank) across Tasks + Notes/Meetings, with a global Cmd+K command palette available anywhere.

## Constraints / decisions
- Embedding model: **download on first use** (keep DMG lean)
- UX: **semantic-first** search; include an **Exact** (keyword-only) mode
- Search: **Hybrid** keyword (FTS5 when available) → vector rerank
- Privacy: no external embedding APIs

## Milestones

### M1 — DB + plumbing (safe scaffolding)
- [x] Add SQLite schema for `semantic_chunks` (vector storage) (added table + indexes in `src-tauri/src/db.rs`)
- [x] Add FTS virtual table `semantic_fts` (best-effort; do not fail app startup if FTS5 unavailable) (best-effort create in `src-tauri/src/db.rs`)
- [x] Add Rust module for semantic search (types + helpers) (new `src-tauri/src/semantic_search.rs`)
- [ ] Add Tauri commands:
  - [x] `semantic_index_status` (added `semantic_search::semantic_index_status`)
  - [x] `semantic_search({ query, types, limit, exact })` (keyword-only across tasks/messages + meetings/segments for now)
  - [x] `semantic_reindex_all` (rebuilds `semantic_fts` from `semantic_chunks` if available)
  - [x] `semantic_delete_for_entity` (deletes from `semantic_chunks` + `semantic_fts`)

### M2 — Global Cmd+K palette (app-wide)
- [x] Move command palette overlay out of `notesPage` so it exists globally (moved to top-level in `gui/menu.html`)
- [x] Add document-level key handler in `gui/js/application.js` (Cmd/Ctrl+K) (via keybind `action.commandPalette`)
- [x] Render results grouped by type (Tasks / Notes) (global palette UI)
- [x] Selecting result navigates + opens the item (tasks open chat, notes open transcript)

### M3 — Keyword search (MVP fallback)
- [x] Implement keyword-only search via FTS if available (populate `semantic_fts` from tasks/messages + notes/segments on demand)
- [x] If FTS unavailable, fallback to substring match on titles (and body substring fallback)
- [x] Ensure global palette works even before embeddings are downloaded (palette uses keyword `semantic_search`)

### M4 — Local embeddings (ONNX + tokenizer)
- [x] Pick an embedding model (small MiniLM/BGE) and define model assets layout (scaffolded in `src-tauri/src/embedding_model.rs`)
- [ ] Implement download-on-first-use with progress + cancel
- [ ] Implement embedding generation (tokenizer + ORT session)
- [ ] Store embeddings as packed f32 BLOBs

### M5 — Incremental indexing
- [ ] Define chunking for:
  - [ ] tasks: title + `messages` transcript chunks
  - [ ] notes/meetings: title + transcript/text chunks
- [ ] Index triggers (debounced):
  - [ ] task title change
  - [ ] message append
  - [ ] meeting segment append / meeting stop
- [ ] content_hash checks to avoid re-embedding unchanged text

### M6 — Hybrid rerank + UX polish
- [ ] Implement hybrid search: candidates from keyword → rerank with vectors
- [ ] Add “Indexing…” indicator and “Exact” toggle
- [ ] Update Notes search bar to semantic-first (calls `semantic_search`)

## Completion
When everything above is done and working end-to-end, add:

STATUS: COMPLETE
