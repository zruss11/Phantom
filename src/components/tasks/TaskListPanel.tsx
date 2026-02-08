import { Plus, ClipboardList } from 'lucide-react';
import { useTasksStore } from '../../stores/tasks';
import { TaskListItem } from './TaskListItem';
import { TaskFilters } from './TaskFilters';
import { EmptyState } from './EmptyState';
import type { TaskRecord } from '../../lib/types';

export function TaskListPanel() {
  const tasks = useTasksStore((s) => s.tasks);
  const selectedTaskId = useTasksStore((s) => s.selectedTaskId);
  const selectTask = useTasksStore((s) => s.selectTask);
  const filter = useTasksStore((s) => s.filter);
  const creatingTask = useTasksStore((s) => s.creatingTask);
  const setCreatingTask = useTasksStore((s) => s.setCreatingTask);

  const filtered = tasks.filter((t: TaskRecord) => {
    if (filter.agent && t.agent_id !== filter.agent) return false;
    if (filter.status && t.status_state !== filter.status) return false;
    return true;
  });

  return (
    <div className="flex h-full flex-col border-r border-border-subtle bg-bg-base">
      {/* Header */}
      <div className="flex items-center justify-between border-b border-border-subtle px-4 py-3">
        <h2 className="text-sm font-semibold text-text-primary">Tasks</h2>
        <button
          onClick={() => setCreatingTask(!creatingTask)}
          className="flex h-7 items-center gap-1.5 rounded-md bg-accent-primary px-2.5 text-xs font-medium text-white transition-colors hover:bg-accent-primary-hover"
        >
          <Plus size={14} />
          New Task
        </button>
      </div>

      {/* Filters */}
      <TaskFilters />

      {/* Task list */}
      <div className="flex-1 overflow-y-auto px-1.5 py-1">
        {filtered.length === 0 ? (
          <EmptyState
            icon={ClipboardList}
            title="No tasks"
            description={filter.agent || filter.status ? 'Try changing filters' : 'Create your first task to get started'}
          />
        ) : (
          <div className="flex flex-col gap-0.5">
            {filtered.map((task) => (
              <TaskListItem
                key={task.id}
                task={task}
                selected={task.id === selectedTaskId}
                onClick={() => selectTask(task.id)}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
