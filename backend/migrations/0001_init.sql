PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS sessions (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  agent_type TEXT NOT NULL,
  status TEXT NOT NULL,
  plan_mode INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  last_event_at INTEGER,
  archived_at INTEGER,
  worktree_path TEXT NOT NULL,
  metadata_json TEXT
);

CREATE INDEX IF NOT EXISTS idx_sessions_status
  ON sessions(status);

CREATE INDEX IF NOT EXISTS idx_sessions_updated_at
  ON sessions(updated_at);

CREATE TABLE IF NOT EXISTS messages (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  role TEXT NOT NULL,
  content TEXT NOT NULL,
  content_format TEXT NOT NULL DEFAULT 'markdown',
  meta_json TEXT,
  meta_model TEXT GENERATED ALWAYS AS (json_extract(meta_json, '$.model')) VIRTUAL,
  meta_token_count INTEGER GENERATED ALWAYS AS (json_extract(meta_json, '$.token_count')) VIRTUAL,
  meta_tool_call_id TEXT GENERATED ALWAYS AS (json_extract(meta_json, '$.tool_call_id')) VIRTUAL,
  ts INTEGER NOT NULL,
  seq INTEGER NOT NULL,
  tool_name TEXT,
  tool_call_id TEXT,
  FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_messages_session_ts
  ON messages(session_id, ts);

CREATE INDEX IF NOT EXISTS idx_messages_session_seq
  ON messages(session_id, seq);

CREATE INDEX IF NOT EXISTS idx_messages_meta_model
  ON messages(meta_model);

CREATE INDEX IF NOT EXISTS idx_messages_meta_tool_call_id
  ON messages(meta_tool_call_id);

CREATE TABLE IF NOT EXISTS tool_calls (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  name TEXT NOT NULL,
  input_json TEXT NOT NULL,
  output_json TEXT,
  success INTEGER,
  should_continue INTEGER,
  ts INTEGER NOT NULL,
  seq INTEGER NOT NULL,
  FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_tool_calls_session_ts
  ON tool_calls(session_id, ts);

CREATE INDEX IF NOT EXISTS idx_tool_calls_session_seq
  ON tool_calls(session_id, seq);

CREATE TABLE IF NOT EXISTS checkpoints (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  iteration INTEGER NOT NULL,
  state_json TEXT NOT NULL,
  ts INTEGER NOT NULL,
  FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_checkpoints_session_ts
  ON checkpoints(session_id, ts);

CREATE TABLE IF NOT EXISTS plan_files (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id TEXT NOT NULL,
  path TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  ts INTEGER NOT NULL,
  FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_plan_files_session_ts
  ON plan_files(session_id, ts);
