-- Add ACP session ID for session/load support
-- This allows us to restore agent context when reconnecting to a session

ALTER TABLE tasks ADD COLUMN acp_session_id TEXT;

-- Create index for looking up tasks by their ACP session ID
CREATE INDEX IF NOT EXISTS idx_tasks_acp_session_id ON tasks(acp_session_id);
