import { useState } from 'react';
import { HelpCircle } from 'lucide-react';
import { tauriInvoke } from '../../lib/tauri';
import type { ChatMessageData } from '../../lib/types';

interface UserInputCardProps {
  message: ChatMessageData;
  taskId: string;
}

export function UserInputCard({ message, taskId }: UserInputCardProps) {
  const [responded, setResponded] = useState(false);
  const [answers, setAnswers] = useState<Record<string, string>>({});

  const questions = message.questions || [];

  const handleSubmit = async () => {
    setResponded(true);
    try {
      await tauriInvoke('respond_to_user_input', {
        taskId,
        requestId: message.request_id || '',
        answers,
      });
    } catch (err) {
      console.error('Failed to respond to user input:', err);
      setResponded(false);
    }
  };

  if (responded) {
    return (
      <div className="px-4 py-1.5">
        <div className="rounded-lg border border-accent-info/30 bg-accent-info/5 px-3 py-2">
          <p className="text-xs text-text-secondary">Answered</p>
        </div>
      </div>
    );
  }

  return (
    <div className="px-4 py-1.5">
      <div className="rounded-lg border border-accent-info/30 bg-accent-info/5">
        <div className="flex items-center gap-2 px-3 py-2">
          <HelpCircle size={14} className="text-accent-info" />
          <span className="text-xs font-medium text-text-primary">Agent needs your input</span>
        </div>

        {questions.map((q) => (
          <div key={q.id} className="border-t border-accent-info/10 px-3 py-2">
            {q.header && (
              <p className="mb-1 text-[11px] font-medium uppercase tracking-wider text-text-tertiary">{q.header}</p>
            )}
            <p className="mb-2 text-xs text-text-primary">{q.question}</p>

            {q.options && q.options.length > 0 ? (
              <div className="flex flex-wrap gap-1.5">
                {q.options.map((opt) => (
                  <button
                    key={opt.value}
                    onClick={() => setAnswers((prev) => ({ ...prev, [q.id]: opt.value }))}
                    className={`rounded-md px-2.5 py-1 text-xs font-medium transition-colors ${
                      answers[q.id] === opt.value
                        ? 'bg-accent-primary text-white'
                        : 'bg-bg-surface text-text-primary hover:bg-bg-surface-hover'
                    }`}
                  >
                    {opt.label}
                  </button>
                ))}
              </div>
            ) : (
              <input
                type="text"
                value={answers[q.id] || ''}
                onChange={(e) => setAnswers((prev) => ({ ...prev, [q.id]: e.target.value }))}
                placeholder="Type your answer..."
                className="w-full rounded-md border border-border-default bg-bg-input px-2.5 py-1.5 text-xs text-text-primary placeholder:text-text-tertiary focus:outline-none focus:ring-1 focus:ring-accent-primary"
              />
            )}
          </div>
        ))}

        <div className="border-t border-accent-info/10 px-3 py-2">
          <button
            onClick={handleSubmit}
            disabled={questions.some((q) => !answers[q.id])}
            className="rounded-md bg-accent-primary px-3 py-1 text-xs font-medium text-white transition-colors hover:bg-accent-primary-hover disabled:opacity-50"
          >
            Submit
          </button>
        </div>
      </div>
    </div>
  );
}
