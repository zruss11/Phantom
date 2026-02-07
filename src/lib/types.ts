export interface TaskRecord {
  id: string;
  agent_id: string;
  codexAccountId: string | null;
  model: string;
  prompt: string | null;
  project_path: string | null;
  worktreePath: string | null;
  branch: string | null;
  status: string;
  status_state: string;
  cost: number;
  created_at: number;
  updated_at: number;
  titleSummary: string | null;
  agentSessionId: string | null;
  totalTokens: number | null;
  contextWindow: number | null;
  claudeRuntime: string | null;
}

export interface AutomationRecord {
  id: string;
  name: string;
  enabled: boolean;
  agentId: string;
  execModel: string;
  prompt: string;
  projectPath: string | null;
  baseBranch: string | null;
  planMode: boolean;
  thinking: boolean;
  useWorktree: boolean;
  permissionMode: string;
  reasoningEffort: string | null;
  agentMode: string | null;
  codexMode: string | null;
  claudeRuntime: string | null;
  cron: string;
  nextRunAt: number | null;
  lastRunAt: number | null;
  lastError: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface AutomationRunRecord {
  id: string;
  automationId: string;
  taskId: string | null;
  scheduledFor: number;
  createdAt: number;
  error: string | null;
}

export interface CachedModel {
  value: string;
  name: string | null;
  description: string | null;
}

export interface CachedMode {
  value: string;
  name: string | null;
  description: string | null;
}

export interface CodexAccountRecord {
  id: string;
  label: string | null;
  codexHome: string;
  email: string | null;
  planType: string | null;
  created_at: number;
  updated_at: number;
}

export interface AttachmentRecord {
  id: string;
  fileName: string | null;
  mimeType: string | null;
  relativePath: string;
  byteSize: number;
}

export interface MessageRecord {
  id: number;
  message_type: string;
  content: string | null;
  reasoning: string | null;
  tool_name: string | null;
  tool_arguments: string | null;
  tool_return: string | null;
  timestamp: string;
}

export interface MeetingSessionRecord {
  id: string;
  title: string | null;
  status: string;
  capture_mic: boolean;
  capture_system: boolean;
  started_at: number | null;
  stopped_at: number | null;
  duration_ms: number;
  created_at: number;
  updated_at: number;
}

export interface MeetingSegmentRecord {
  id: number;
  session_id: string;
  text: string;
  start_ms: number;
  end_ms: number;
  speaker: string | null;
  created_at: number;
}

export type AgentId = 'codex' | 'claude-code' | 'factory-droid' | 'gemini-cli' | 'opencode';

export type TaskStatusState = 'idle' | 'running' | 'completed' | 'error' | 'stopped' | 'queued';

export type ThemeMode = 'dark' | 'light' | 'system';

export interface Settings {
  theme: ThemeMode;
  sidebarCollapsed: boolean;
}
