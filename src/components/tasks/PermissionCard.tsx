import { useState } from 'react';
import { Shield, Terminal } from 'lucide-react';
import { tauriInvoke } from '../../lib/tauri';
import type { ChatMessageData } from '../../lib/types';

interface PermissionCardProps {
  message: ChatMessageData;
  taskId: string;
}

export function PermissionCard({ message, taskId }: PermissionCardProps) {
  const [responded, setResponded] = useState(false);
  const [selectedOption, setSelectedOption] = useState<string | null>(null);

  const toolName = message.tool_name || 'Unknown tool';
  const description = message.description || '';
  const options = message.options || [];

  // Parse command from raw_input if it's a Bash tool
  let commandPreview = '';
  if (message.raw_input) {
    try {
      const raw = typeof message.raw_input === 'string' ? JSON.parse(message.raw_input) : message.raw_input;
      if (raw.command) commandPreview = raw.command;
    } catch { /* ignore */ }
  }

  const handleRespond = async (optionId: string) => {
    setSelectedOption(optionId);
    setResponded(true);
    try {
      await tauriInvoke('respond_to_permission', {
        taskId,
        requestId: message.request_id || '',
        responseId: optionId,
      });
    } catch (err) {
      console.error('Failed to respond to permission:', err);
      setResponded(false);
      setSelectedOption(null);
    }
  };

  return (
    <div className="px-4 py-1.5">
      <div className="rounded-lg border border-accent-warning/30 bg-accent-warning/5">
        <div className="flex items-center gap-2 px-3 py-2">
          <Shield size={14} className="text-accent-warning" />
          <span className="text-xs font-medium text-text-primary">Permission Request</span>
          <code className="rounded bg-bg-surface px-1.5 py-0.5 text-[11px] text-text-secondary">{toolName}</code>
        </div>

        {description && (
          <div className="border-t border-accent-warning/10 px-3 py-2">
            <p className="text-xs text-text-secondary">{description}</p>
          </div>
        )}

        {commandPreview && (
          <div className="border-t border-accent-warning/10 px-3 py-2">
            <div className="flex items-center gap-1.5 text-[11px] text-text-tertiary">
              <Terminal size={12} />
              <code className="truncate">{commandPreview}</code>
            </div>
          </div>
        )}

        <div className="flex items-center gap-2 border-t border-accent-warning/10 px-3 py-2">
          {responded ? (
            <span className="text-xs text-text-secondary">
              Responded: {options.find((o) => o.id === selectedOption)?.label || selectedOption}
            </span>
          ) : (
            options.map((option) => (
              <button
                key={option.id}
                onClick={() => handleRespond(option.id)}
                className="rounded-md bg-bg-surface px-2.5 py-1 text-xs font-medium text-text-primary transition-colors hover:bg-bg-surface-hover"
              >
                {option.label}
              </button>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
