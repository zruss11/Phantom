import { type TextareaHTMLAttributes, forwardRef, useCallback, useEffect, useRef } from 'react';
import clsx from 'clsx';

interface TextareaProps extends TextareaHTMLAttributes<HTMLTextAreaElement> {
  label?: string;
  error?: string;
}

export const Textarea = forwardRef<HTMLTextAreaElement, TextareaProps>(
  ({ label, error, className, id, onChange, ...props }, ref) => {
    const internalRef = useRef<HTMLTextAreaElement | null>(null);
    const textareaId = id || label?.toLowerCase().replace(/\s+/g, '-');

    const resize = useCallback(() => {
      const el = internalRef.current;
      if (el) {
        el.style.height = 'auto';
        el.style.height = `${el.scrollHeight}px`;
      }
    }, []);

    useEffect(() => { resize(); }, [resize]);

    return (
      <div className="flex flex-col gap-1.5">
        {label && (
          <label htmlFor={textareaId} className="text-sm font-medium text-text-primary">
            {label}
          </label>
        )}
        <textarea
          ref={(node) => {
            internalRef.current = node;
            if (typeof ref === 'function') ref(node);
            else if (ref) ref.current = node;
          }}
          id={textareaId}
          className={clsx(
            'min-h-[80px] resize-none rounded-md border bg-bg-input px-3 py-2 text-sm text-text-primary placeholder:text-text-tertiary',
            'transition-colors focus:outline-none focus:ring-2 focus:ring-accent-primary focus:border-border-strong',
            error ? 'border-accent-error' : 'border-border-default',
            className,
          )}
          onChange={(e) => {
            onChange?.(e);
            resize();
          }}
          {...props}
        />
        {error && <p className="text-xs text-accent-error">{error}</p>}
      </div>
    );
  },
);
Textarea.displayName = 'Textarea';
