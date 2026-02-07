import { useState, useEffect } from 'react';
import { CheckCircle, XCircle, Info, X } from 'lucide-react';
import clsx from 'clsx';

type ToastVariant = 'success' | 'error' | 'info';

interface Toast {
  id: string;
  message: string;
  variant: ToastVariant;
}

let toasts: Toast[] = [];
let listeners: Array<() => void> = [];

function emit() { listeners.forEach((l) => l()); }

export function toast(message: string, variant: ToastVariant = 'info') {
  const id = `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  toasts = [...toasts, { id, message, variant }];
  emit();
  setTimeout(() => { dismissToast(id); }, 4000);
}

function dismissToast(id: string) {
  toasts = toasts.filter((t) => t.id !== id);
  emit();
}

function useToasts() {
  const [, forceUpdate] = useState(0);
  useEffect(() => {
    const listener = () => forceUpdate((n) => n + 1);
    listeners.push(listener);
    return () => { listeners = listeners.filter((l) => l !== listener); };
  }, []);
  return toasts;
}

const icons = { success: CheckCircle, error: XCircle, info: Info };
const variantStyles = {
  success: 'border-accent-success/30 bg-accent-success/10',
  error: 'border-accent-error/30 bg-accent-error/10',
  info: 'border-accent-info/30 bg-accent-info/10',
};

export function ToastContainer() {
  const items = useToasts();
  if (items.length === 0) return null;

  return (
    <div className="fixed bottom-4 right-4 z-[600] flex flex-col gap-2">
      {items.map((t) => {
        const Icon = icons[t.variant];
        return (
          <div key={t.id} className={clsx('flex items-center gap-3 rounded-lg border px-4 py-3 shadow-lg', variantStyles[t.variant])}>
            <Icon size={16} className="shrink-0" />
            <span className="text-sm text-text-primary">{t.message}</span>
            <button onClick={() => dismissToast(t.id)} className="ml-2 text-text-tertiary hover:text-text-primary">
              <X size={14} />
            </button>
          </div>
        );
      })}
    </div>
  );
}
