import clsx from 'clsx';
import type { TaskRecord, AgentId } from '../../lib/types';
import { AGENTS } from '../../lib/constants';

interface TaskListItemProps {
  task: TaskRecord;
  selected: boolean;
  onClick: () => void;
}

function timeAgo(timestamp: number): string {
  const seconds = Math.floor(Date.now() / 1000 - timestamp);
  if (seconds < 60) return 'just now';
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

function formatCost(cost: number): string {
  if (cost < 0.01) return '< $0.01';
  return `$${cost.toFixed(2)}`;
}

const statusDot: Record<string, string> = {
  running: 'bg-accent-success animate-pulse',
  completed: 'bg-accent-success',
  error: 'bg-accent-error',
  stopped: 'bg-text-tertiary',
  idle: 'bg-accent-info',
  queued: 'bg-accent-warning',
};

export function TaskListItem({ task, selected, onClick }: TaskListItemProps) {
  const agent = AGENTS[task.agent_id as AgentId];
  const title = task.titleSummary || task.prompt?.slice(0, 60) || 'Untitled task';

  return (
    <button
      onClick={onClick}
      className={clsx(
        'flex w-full items-start gap-3 rounded-lg px-3 py-2.5 text-left transition-colors',
        selected
          ? 'bg-bg-surface-active'
          : 'hover:bg-bg-surface-hover'
      )}
    >
      {/* Agent avatar */}
      <div
        className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-xs font-semibold"
        style={{ backgroundColor: agent ? `${agent.color}20` : undefined, color: agent?.color }}
      >
        {agent?.name.charAt(0) || '?'}
      </div>

      {/* Content */}
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="truncate text-sm font-medium text-text-primary">{title}</span>
        </div>
        <div className="mt-0.5 flex items-center gap-2 text-xs text-text-tertiary">
          <span className="flex items-center gap-1">
            <span className={clsx('inline-block h-1.5 w-1.5 rounded-full', statusDot[task.status_state] || 'bg-text-tertiary')} />
            {task.status_state}
          </span>
          <span>&middot;</span>
          <span>{agent?.name || task.agent_id}</span>
          <span>&middot;</span>
          <span>{formatCost(task.cost)}</span>
        </div>
      </div>

      {/* Time */}
      <span className="shrink-0 text-[11px] text-text-tertiary">{timeAgo(task.created_at)}</span>
    </button>
  );
}
