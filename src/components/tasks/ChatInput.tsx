import { useState, useRef, useCallback } from 'react';
import { Send, Loader2 } from 'lucide-react';
import clsx from 'clsx';
import { tauriInvoke } from '../../lib/tauri';

interface ChatInputProps {
  taskId: string;
  statusState: string;
}

export function ChatInput({ taskId, statusState }: ChatInputProps) {
  const [message, setMessage] = useState('');
  const [sending, setSending] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const isRunning = statusState === 'running';

  const resize = useCallback(() => {
    const el = textareaRef.current;
    if (el) {
      el.style.height = 'auto';
      el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
    }
  }, []);

  const handleSend = async () => {
    const text = message.trim();
    if (!text || sending) return;

    setSending(true);
    try {
      if (isRunning) {
        // Queue the message for next turn
        const clientMessageId = `${Date.now()}-${Math.random().toString(36).slice(2)}`;
        await tauriInvoke('enqueue_chat_message', {
          taskId,
          message: text,
          clientMessageId,
          disposition: 'queue',
        });
      } else {
        await tauriInvoke('send_chat_message', {
          taskId,
          message: text,
        });
      }
      setMessage('');
      if (textareaRef.current) {
        textareaRef.current.style.height = 'auto';
      }
    } catch (err) {
      console.error('Failed to send message:', err);
    } finally {
      setSending(false);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  return (
    <div className="border-t border-border-subtle px-4 py-3">
      {isRunning && (
        <div className="mb-2 flex items-center gap-1.5 text-[11px] text-accent-warning">
          <Loader2 size={12} className="animate-spin" />
          Agent is working — messages will be queued
        </div>
      )}
      <div className="flex items-end gap-2">
        <textarea
          ref={textareaRef}
          value={message}
          onChange={(e) => {
            setMessage(e.target.value);
            resize();
          }}
          onKeyDown={handleKeyDown}
          placeholder={isRunning ? 'Queue a follow-up message...' : 'Send a message...'}
          rows={1}
          className={clsx(
            'flex-1 resize-none rounded-lg border bg-bg-input px-3 py-2 text-sm text-text-primary placeholder:text-text-tertiary',
            'focus:outline-none focus:ring-2 focus:ring-accent-primary',
            'border-border-default'
          )}
        />
        <button
          onClick={handleSend}
          disabled={!message.trim() || sending}
          className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-accent-primary text-white transition-colors hover:bg-accent-primary-hover disabled:opacity-50"
        >
          {sending ? <Loader2 size={16} className="animate-spin" /> : <Send size={16} />}
        </button>
      </div>
    </div>
  );
}
