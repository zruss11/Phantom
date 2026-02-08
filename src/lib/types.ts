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

// --- Chat & Task Creation Types ---

export type ChatMessageType =
  | 'user'
  | 'assistant'
  | 'reasoning'
  | 'tool_call'
  | 'tool_return'
  | 'permission_request'
  | 'user_input_request'
  | 'plan_update'
  | 'plan_content'
  | 'file_edit'
  | 'diff'
  | 'error'
  | 'system';

export interface ChatMessageData {
  type: ChatMessageType;
  message_type?: string;
  content?: string;
  text?: string;
  timestamp?: string;
  clientMessageId?: string;
  client_message_id?: string;
  queuedDisposition?: 'queue' | 'steer' | null;

  // Tool call
  tool_call?: { name: string; arguments: string | object };
  name?: string;
  arguments?: string | object;

  // Tool return
  tool_return?: string;
  result?: string;
  output?: string;

  // Permission request
  request_id?: string;
  tool_name?: string;
  description?: string;
  raw_input?: string | object;
  options?: Array<{ id: string; label: string; description?: string }>;

  // User input request
  questions?: Array<{
    id: string;
    header: string;
    question: string;
    options?: Array<{ label: string; description?: string; value: string }>;
    multiSelect?: boolean;
  }>;

  // Attachments
  attachments?: Array<{
    id: string;
    relativePath: string;
    fileName?: string;
    mimeType?: string;
    dataUrl?: string;
  }>;

  // Reasoning
  reasoning?: string;

  // File edit / diff
  file_path?: string;
  path?: string;
  edit_content?: string;
  diff?: string;
  title?: string;

  // Plan
  explanation?: string;
  plan?: Array<{ step: string; status: 'pending' | 'inProgress' | 'completed' }>;
}

export interface CreateAgentPayload {
  agentId: string;
  prompt: string;
  projectPath?: string;
  baseBranch?: string;
  planMode: boolean;
  thinking: boolean;
  useWorktree: boolean;
  permissionMode: string;
  execModel: string;
  reasoningEffort?: string;
  agentMode?: string;
  codexMode?: string;
  claudeRuntime?: string;
  attachments?: Array<{ id: string; relativePath: string; mimeType: string }>;
  multiCreate: boolean;
  suppressNotifications: boolean;
}

export interface CreateAgentResult {
  task_id: string;
  session_id: string;
  worktreePath?: string;
}

export interface EnrichedModel {
  value: string;
  name?: string;
  description?: string;
  supportedReasoningEfforts: Array<{ value: string; name?: string }>;
  defaultReasoningEffort?: string;
  isDefault: boolean;
}

export interface TaskHistory {
  task_id: string;
  agent_id: string;
  pending_prompt?: string;
  status_state: string;
  title_summary?: string;
  worktree_path?: string;
  project_path?: string;
  branch?: string;
  messages: ChatMessageData[];
}
