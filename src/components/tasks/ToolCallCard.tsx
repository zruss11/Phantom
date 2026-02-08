import { useState } from 'react';
import { ChevronRight, Terminal, FileCode, Pencil, Search, Globe } from 'lucide-react';
import clsx from 'clsx';
import type { ChatMessageData } from '../../lib/types';

const toolIcons: Record<string, typeof Terminal> = {
  Bash: Terminal,
  Read: FileCode,
  Edit: Pencil,
  Write: Pencil,
  Grep: Search,
  Glob: Search,
  WebFetch: Globe,
};

interface ToolCallCardProps {
  message: ChatMessageData;
}

export function ToolCallCard({ message }: ToolCallCardProps) {
  const [open, setOpen] = useState(false);
  const toolName = message.tool_call?.name || message.name || message.tool_name || 'Tool';
  const args = message.tool_call?.arguments || message.arguments;
  const result = message.tool_return || message.result || message.output;
  const Icon = toolIcons[toolName] || Terminal;

  let argsStr = '';
  if (typeof args === 'string') argsStr = args;
  else if (args) {
    try { argsStr = JSON.stringify(args, null, 2); } catch { argsStr = String(args); }
  }

  // Extract a short summary from args
  let summary = '';
  if (typeof args === 'object' && args !== null) {
    const obj = args as Record<string, unknown>;
    if (obj.command) summary = String(obj.command).slice(0, 80);
    else if (obj.file_path || obj.path) summary = String(obj.file_path || obj.path);
    else if (obj.pattern) summary = String(obj.pattern);
  }

  return (
    <div className="px-4 py-1">
      <div className="rounded-lg border border-border-subtle bg-bg-surface">
        <button
          onClick={() => setOpen(!open)}
          className="flex w-full items-center gap-2 px-3 py-2 text-left"
        >
          <ChevronRight
            size={12}
            className={clsx('shrink-0 text-text-tertiary transition-transform', open && 'rotate-90')}
          />
          <Icon size={14} className="shrink-0 text-text-secondary" />
          <span className="text-xs font-medium text-text-secondary">{toolName}</span>
          {summary && (
            <span className="truncate font-mono text-[11px] text-text-tertiary">{summary}</span>
          )}
        </button>

        {open && (
          <div className="border-t border-border-subtle">
            {argsStr && (
              <div className="px-3 py-2">
                <p className="mb-1 text-[10px] font-medium uppercase tracking-wider text-text-tertiary">Arguments</p>
                <pre className="overflow-x-auto whitespace-pre-wrap rounded bg-bg-base p-2 font-mono text-[11px] text-text-secondary">
                  {argsStr}
                </pre>
              </div>
            )}
            {result && (
              <div className="border-t border-border-subtle px-3 py-2">
                <p className="mb-1 text-[10px] font-medium uppercase tracking-wider text-text-tertiary">Result</p>
                <pre className="max-h-40 overflow-auto whitespace-pre-wrap rounded bg-bg-base p-2 font-mono text-[11px] text-text-secondary">
                  {typeof result === 'string' ? result : JSON.stringify(result, null, 2)}
                </pre>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
