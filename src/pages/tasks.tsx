import { useEffect, useCallback } from 'react';
import { tauriInvoke } from '../lib/tauri';
import { useTauriEvent } from '../hooks/useTauriEvent';
import { useTasksStore } from '../stores/tasks';
import { TaskListPanel } from '../components/tasks/TaskListPanel';
import { TaskCreationForm } from '../components/tasks/TaskCreationForm';
import { TaskDetailPanel } from '../components/tasks/TaskDetailPanel';
import type { TaskRecord, ChatMessageData } from '../lib/types';

export function TasksPage() {
  const setTasks = useTasksStore((s) => s.setTasks);
  const addTask = useTasksStore((s) => s.addTask);
  const updateTask = useTasksStore((s) => s.updateTask);
  const appendMessage = useTasksStore((s) => s.appendMessage);
  const setMessages = useTasksStore((s) => s.setMessages);
  const creatingTask = useTasksStore((s) => s.creatingTask);

  // Load tasks on mount
  useEffect(() => {
    tauriInvoke<TaskRecord[]>('load_tasks')
      .then((tasks) => setTasks(tasks || []))
      .catch((err) => console.error('Failed to load tasks:', err));
  }, [setTasks]);

  // Subscribe to AddTask event
  // The event payload format from Rust is an array: [null, taskId, taskRecord]
  // But Tauri v2 listen gives the payload directly
  useTauriEvent<any>('AddTask', useCallback((payload: any) => {
    // Handle both array format [null, id, task] and direct task format
    if (Array.isArray(payload)) {
      const task = payload[2] || payload[1];
      if (task?.id) addTask(task);
    } else if (payload?.id) {
      addTask(payload);
    }
  }, [addTask]));

  // Subscribe to StatusUpdate
  useTauriEvent<any>('StatusUpdate', useCallback((payload: any) => {
    let id: string, status: string, statusState: string;
    if (Array.isArray(payload)) {
      [, id, status, , statusState] = payload;
    } else {
      id = payload?.id || payload?.task_id;
      status = payload?.status || payload?.message;
      statusState = payload?.status_state || payload?.state;
    }
    if (id) updateTask(id, { status, status_state: statusState });
  }, [updateTask]));

  // Subscribe to CostUpdate
  useTauriEvent<any>('CostUpdate', useCallback((payload: any) => {
    let id: string, cost: number;
    if (Array.isArray(payload)) {
      [, id, cost] = payload;
    } else {
      id = payload?.id || payload?.task_id;
      cost = payload?.cost;
    }
    if (id) updateTask(id, { cost });
  }, [updateTask]));

  // Subscribe to ChatLogBatch (bulk message load)
  useTauriEvent<any>('ChatLogBatch', useCallback((payload: any) => {
    let taskId: string, messages: ChatMessageData[];
    if (Array.isArray(payload)) {
      [, taskId, messages] = payload;
    } else {
      taskId = payload?.task_id;
      messages = payload?.messages;
    }
    if (taskId && messages) setMessages(taskId, messages);
  }, [setMessages]));

  // Subscribe to ChatLogStreaming (real-time message)
  useTauriEvent<any>('ChatLogStreaming', useCallback((payload: any) => {
    let taskId: string, message: ChatMessageData;
    if (Array.isArray(payload)) {
      [, taskId, message] = payload;
      // Sometimes message is embedded differently
      if (!message && payload[1]) {
        taskId = payload[0];
        message = payload[1];
      }
    } else {
      taskId = payload?.task_id;
      message = payload;
    }
    if (taskId && message) appendMessage(taskId, message);
  }, [appendMessage]));

  return (
    <div className="flex h-full">
      {/* Left panel: task list */}
      <div className="flex w-[35%] min-w-[280px] max-w-[440px] flex-col">
        {creatingTask && <TaskCreationForm />}
        <TaskListPanel />
      </div>

      {/* Right panel: task detail */}
      <TaskDetailPanel />
    </div>
  );
}
