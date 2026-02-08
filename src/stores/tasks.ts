import { create } from 'zustand';
import type { TaskRecord, ChatMessageData } from '../lib/types';

interface TasksStore {
  tasks: TaskRecord[];
  selectedTaskId: string | null;
  messages: Record<string, ChatMessageData[]>;
  filter: { agent: string | null; status: string | null };
  creatingTask: boolean;

  setTasks: (tasks: TaskRecord[]) => void;
  addTask: (task: TaskRecord) => void;
  removeTask: (id: string) => void;
  updateTask: (id: string, updates: Partial<TaskRecord>) => void;
  selectTask: (id: string | null) => void;
  setMessages: (taskId: string, messages: ChatMessageData[]) => void;
  appendMessage: (taskId: string, message: ChatMessageData) => void;
  setFilter: (filter: Partial<TasksStore['filter']>) => void;
  setCreatingTask: (creating: boolean) => void;
}

export const useTasksStore = create<TasksStore>((set) => ({
  tasks: [],
  selectedTaskId: null,
  messages: {},
  filter: { agent: null, status: null },
  creatingTask: false,

  setTasks: (tasks) => set({ tasks }),

  addTask: (task) =>
    set((state) => ({ tasks: [task, ...state.tasks] })),

  removeTask: (id) =>
    set((state) => ({
      tasks: state.tasks.filter((t) => t.id !== id),
      selectedTaskId: state.selectedTaskId === id ? null : state.selectedTaskId,
      messages: (() => {
        const { [id]: _, ...rest } = state.messages;
        return rest;
      })(),
    })),

  updateTask: (id, updates) =>
    set((state) => ({
      tasks: state.tasks.map((t) => (t.id === id ? { ...t, ...updates } : t)),
    })),

  selectTask: (id) => set({ selectedTaskId: id }),

  setMessages: (taskId, messages) =>
    set((state) => ({ messages: { ...state.messages, [taskId]: messages } })),

  appendMessage: (taskId, message) =>
    set((state) => ({
      messages: {
        ...state.messages,
        [taskId]: [...(state.messages[taskId] || []), message],
      },
    })),

  setFilter: (filter) =>
    set((state) => ({ filter: { ...state.filter, ...filter } })),

  setCreatingTask: (creating) => set({ creatingTask: creating }),
}));
