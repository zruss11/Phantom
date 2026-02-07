import clsx from 'clsx';
import type { AgentId, TaskStatusState } from '../../lib/types';
import { AGENTS } from '../../lib/constants';

interface StatusBadgeProps {
  status: TaskStatusState;
}

const statusStyles: Record<TaskStatusState, string> = {
  running: 'bg-accent-success/15 text-accent-success',
  completed: 'bg-accent-success/15 text-accent-success',
  error: 'bg-accent-error/15 text-accent-error',
  stopped: 'bg-bg-surface text-text-tertiary',
  idle: 'bg-accent-info/15 text-accent-info',
  queued: 'bg-accent-warning/15 text-accent-warning',
};

export function StatusBadge({ status }: StatusBadgeProps) {
  return (
    <span className={clsx('inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[11px] font-medium', statusStyles[status])}>
      {status === 'running' && <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-current" />}
      {status.charAt(0).toUpperCase() + status.slice(1)}
    </span>
  );
}

interface AgentBadgeProps {
  agentId: AgentId;
}

export function AgentBadge({ agentId }: AgentBadgeProps) {
  const agent = AGENTS[agentId];
  if (!agent) return null;
  return (
    <span
      className="inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[11px] font-medium"
      style={{ backgroundColor: `${agent.color}20`, color: agent.color }}
    >
      {agent.name}
    </span>
  );
}
