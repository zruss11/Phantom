import clsx from 'clsx';
import { useTasksStore } from '../../stores/tasks';
import { AGENTS, AGENT_IDS } from '../../lib/constants';

const STATUS_OPTIONS = ['all', 'running', 'completed', 'error', 'idle', 'queued'] as const;

export function TaskFilters() {
  const filter = useTasksStore((s) => s.filter);
  const setFilter = useTasksStore((s) => s.setFilter);

  return (
    <div className="flex items-center gap-2 px-3 py-2">
      {/* Agent filter */}
      <select
        value={filter.agent || ''}
        onChange={(e) => setFilter({ agent: e.target.value || null })}
        className="h-7 rounded-md border border-border-default bg-bg-input px-2 text-xs text-text-primary"
      >
        <option value="">All agents</option>
        {AGENT_IDS.map((id) => (
          <option key={id} value={id}>{AGENTS[id].name}</option>
        ))}
      </select>

      {/* Status pills */}
      <div className="flex gap-1">
        {STATUS_OPTIONS.map((status) => (
          <button
            key={status}
            onClick={() => setFilter({ status: status === 'all' ? null : status })}
            className={clsx(
              'rounded-full px-2 py-0.5 text-[11px] font-medium capitalize transition-colors',
              (status === 'all' && !filter.status) || filter.status === status
                ? 'bg-accent-primary-muted text-accent-primary'
                : 'text-text-tertiary hover:text-text-secondary'
            )}
          >
            {status}
          </button>
        ))}
      </div>
    </div>
  );
}
