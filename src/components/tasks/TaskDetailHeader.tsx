import { Trash2, StopCircle, GitBranch, Cpu, DollarSign } from 'lucide-react';
import type { TaskRecord, AgentId } from '../../lib/types';
import { AGENTS } from '../../lib/constants';
import { tauriInvoke } from '../../lib/tauri';
import { useTasksStore } from '../../stores/tasks';

interface TaskDetailHeaderProps {
  task: TaskRecord;
}

function formatCost(cost: number): string {
  if (cost < 0.01) return '< $0.01';
  return `$${cost.toFixed(2)}`;
}

export function TaskDetailHeader({ task }: TaskDetailHeaderProps) {
  const removeTask = useTasksStore((s) => s.removeTask);
  const agent = AGENTS[task.agent_id as AgentId];
  const title = task.titleSummary || task.prompt?.slice(0, 80) || 'Untitled task';
  const isRunning = task.status_state === 'running';

  const handleStop = async () => {
    try {
      await tauriInvoke('soft_stop_task', { taskId: task.id });
    } catch (err) {
      console.error('Failed to stop task:', err);
    }
  };

  const handleDelete = async () => {
    try {
      await tauriInvoke('delete_task', { taskId: task.id });
      removeTask(task.id);
    } catch (err) {
      console.error('Failed to delete task:', err);
    }
  };

  return (
    <div className="flex items-center gap-3 border-b border-border-subtle px-4 py-2.5">
      {/* Agent badge */}
      <span
        className="inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[11px] font-medium"
        style={{ backgroundColor: agent ? `${agent.color}20` : undefined, color: agent?.color }}
      >
        {agent?.name || task.agent_id}
      </span>

      {/* Title */}
      <h3 className="min-w-0 flex-1 truncate text-sm font-medium text-text-primary">{title}</h3>

      {/* Metadata chips */}
      <div className="flex items-center gap-2 text-[11px] text-text-tertiary">
        {task.model && (
          <span className="flex items-center gap-1 rounded bg-bg-surface px-1.5 py-0.5">
            <Cpu size={10} />
            {task.model}
          </span>
        )}
        {task.branch && (
          <span className="flex items-center gap-1 rounded bg-bg-surface px-1.5 py-0.5">
            <GitBranch size={10} />
            {task.branch}
          </span>
        )}
        <span className="flex items-center gap-1">
          <DollarSign size={10} />
          {formatCost(task.cost)}
        </span>
      </div>

      {/* Actions */}
      <div className="flex items-center gap-1">
        {isRunning && (
          <button
            onClick={handleStop}
            className="flex h-7 items-center gap-1 rounded-md px-2 text-xs text-accent-warning transition-colors hover:bg-bg-surface-hover"
            title="Stop task"
          >
            <StopCircle size={14} />
            Stop
          </button>
        )}
        <button
          onClick={handleDelete}
          className="flex h-7 w-7 items-center justify-center rounded-md text-text-tertiary transition-colors hover:bg-bg-surface-hover hover:text-accent-error"
          title="Delete task"
        >
          <Trash2 size={14} />
        </button>
      </div>
    </div>
  );
}
