-- Add agent session ID for CLI resume support
ALTER TABLE tasks ADD COLUMN agent_session_id TEXT;

-- Backfill legacy ACP session ID if present
UPDATE tasks SET agent_session_id = acp_session_id WHERE agent_session_id IS NULL;

-- Create index for looking up tasks by agent session ID
CREATE INDEX IF NOT EXISTS idx_tasks_agent_session_id ON tasks(agent_session_id);
