import type { AgentId } from './types';

export const AGENTS: Record<AgentId, { name: string; color: string; icon: string }> = {
  codex:          { name: 'Codex',       color: '#22c55e', icon: 'codex.svg' },
  'claude-code':  { name: 'Claude Code', color: '#d97757', icon: 'claude-code.svg' },
  'factory-droid':{ name: 'Droid',       color: '#06b6d4', icon: 'factorydroid.png' },
  'gemini-cli':   { name: 'GeminiCLI',   color: '#4285f4', icon: 'gemini.svg' },
  opencode:       { name: 'OpenCode',    color: '#f59e0b', icon: 'opencode.png' },
} as const;

export const AGENT_IDS = Object.keys(AGENTS) as AgentId[];
