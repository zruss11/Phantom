import { useEffect, useRef } from 'react';
import { MessageSquare } from 'lucide-react';
import type { ChatMessageData } from '../../lib/types';
import { ChatMessage } from './ChatMessage';
import { EmptyState } from './EmptyState';

interface ChatMessageListProps {
  messages: ChatMessageData[];
  taskId: string;
}

export function ChatMessageList({ messages, taskId }: ChatMessageListProps) {
  const bottomRef = useRef<HTMLDivElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    // Auto-scroll to bottom when messages change, but only if already near bottom
    const container = containerRef.current;
    if (!container) return;
    const { scrollTop, scrollHeight, clientHeight } = container;
    const isNearBottom = scrollHeight - scrollTop - clientHeight < 100;
    if (isNearBottom) {
      bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
    }
  }, [messages.length]);

  if (messages.length === 0) {
    return (
      <EmptyState
        icon={MessageSquare}
        title="No messages yet"
        description="Start the task or send a message to begin"
        className="flex-1"
      />
    );
  }

  return (
    <div ref={containerRef} className="flex-1 overflow-y-auto py-3">
      {messages.map((msg, i) => (
        <ChatMessage key={`${msg.timestamp || i}-${i}`} message={msg} taskId={taskId} />
      ))}
      <div ref={bottomRef} />
    </div>
  );
}
