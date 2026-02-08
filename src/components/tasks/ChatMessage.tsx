import { Bot, Lightbulb, AlertCircle } from 'lucide-react';
import type { ChatMessageData } from '../../lib/types';
import { ToolCallCard } from './ToolCallCard';
import { PermissionCard } from './PermissionCard';
import { UserInputCard } from './UserInputCard';

interface ChatMessageProps {
  message: ChatMessageData;
  taskId: string;
}

export function ChatMessage({ message, taskId }: ChatMessageProps) {
  const type = message.type || message.message_type || 'system';
  const content = message.content || message.text || '';

  switch (type) {
    case 'user':
      return (
        <div className="flex justify-end px-4 py-1.5">
          <div className="max-w-[80%] rounded-xl rounded-br-sm bg-accent-primary-muted px-3.5 py-2.5">
            <p className="whitespace-pre-wrap text-sm text-text-primary">{content}</p>
            {message.attachments?.map((att) => (
              <div key={att.id} className="mt-2">
                {att.mimeType?.startsWith('image/') && att.dataUrl ? (
                  <img src={att.dataUrl} alt={att.fileName || 'attachment'} className="max-h-48 rounded-lg" />
                ) : (
                  <span className="text-xs text-text-secondary">{att.fileName || att.relativePath}</span>
                )}
              </div>
            ))}
          </div>
        </div>
      );

    case 'assistant':
      return (
        <div className="flex gap-2.5 px-4 py-1.5">
          <div className="mt-1 flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-bg-surface">
            <Bot size={14} className="text-text-secondary" />
          </div>
          <div className="min-w-0 flex-1">
            <p className="whitespace-pre-wrap text-sm leading-relaxed text-text-primary">{content}</p>
          </div>
        </div>
      );

    case 'reasoning':
      return (
        <div className="px-4 py-1.5">
          <details className="group rounded-lg border border-border-subtle bg-bg-surface">
            <summary className="flex cursor-pointer items-center gap-2 px-3 py-2 text-xs font-medium text-text-secondary">
              <Lightbulb size={14} className="text-accent-warning" />
              Thinking
            </summary>
            <div className="border-t border-border-subtle px-3 py-2">
              <p className="whitespace-pre-wrap text-xs leading-relaxed text-text-tertiary">
                {message.reasoning || content}
              </p>
            </div>
          </details>
        </div>
      );

    case 'tool_call':
      return <ToolCallCard message={message} />;

    case 'tool_return':
      // Tool returns are usually merged into ToolCallCard, but render standalone if needed
      return (
        <div className="px-4 py-1.5">
          <div className="rounded-lg border border-border-subtle bg-bg-surface px-3 py-2">
            <p className="whitespace-pre-wrap font-mono text-xs text-text-secondary">
              {message.tool_return || message.result || message.output || content}
            </p>
          </div>
        </div>
      );

    case 'permission_request':
      return <PermissionCard message={message} taskId={taskId} />;

    case 'user_input_request':
      return <UserInputCard message={message} taskId={taskId} />;

    case 'error':
      return (
        <div className="flex items-start gap-2 px-4 py-1.5">
          <AlertCircle size={14} className="mt-0.5 shrink-0 text-accent-error" />
          <p className="text-sm text-accent-error">{content}</p>
        </div>
      );

    case 'system':
      return (
        <div className="px-4 py-1">
          <p className="text-center text-xs text-text-tertiary">{content}</p>
        </div>
      );

    default:
      if (!content) return null;
      return (
        <div className="px-4 py-1.5">
          <p className="text-sm text-text-secondary">{content}</p>
        </div>
      );
  }
}
