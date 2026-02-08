import { useEffect } from 'react';
import { MousePointerClick } from 'lucide-react';
import { useTasksStore } from '../../stores/tasks';
import { tauriInvoke } from '../../lib/tauri';
import type { TaskHistory } from '../../lib/types';
import { TaskDetailHeader } from './TaskDetailHeader';
import { ChatMessageList } from './ChatMessageList';
import { ChatInput } from './ChatInput';
import { EmptyState } from './EmptyState';

export function TaskDetailPanel() {
  const tasks = useTasksStore((s) => s.tasks);
  const selectedTaskId = useTasksStore((s) => s.selectedTaskId);
  const messages = useTasksStore((s) => s.messages);
  const setMessages = useTasksStore((s) => s.setMessages);

  const task = tasks.find((t) => t.id === selectedTaskId);
  const taskMessages = selectedTaskId ? messages[selectedTaskId] || [] : [];

  // Load task history when a task is selected
  useEffect(() => {
    if (!selectedTaskId) return;
    // Only fetch if we don't already have messages
    if (messages[selectedTaskId]?.length) return;

    tauriInvoke<TaskHistory>('get_task_history', { taskId: selectedTaskId })
      .then((history) => {
        if (history?.messages) {
          setMessages(selectedTaskId, history.messages);
        }
      })
      .catch((err) => console.error('Failed to load task history:', err));
  }, [selectedTaskId, messages, setMessages]);

  if (!task) {
    return (
      <div className="flex flex-1 items-center justify-center bg-bg-base">
        <EmptyState
          icon={MousePointerClick}
          title="No task selected"
          description="Select a task from the list to view its details"
        />
      </div>
    );
  }

  return (
    <div className="flex flex-1 flex-col bg-bg-base">
      <TaskDetailHeader task={task} />
      <ChatMessageList messages={taskMessages} taskId={task.id} />
      <ChatInput taskId={task.id} statusState={task.status_state} />
    </div>
  );
}
