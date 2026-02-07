import { type InputHTMLAttributes, forwardRef } from 'react';
import clsx from 'clsx';

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  label?: string;
  error?: string;
  helperText?: string;
}

export const Input = forwardRef<HTMLInputElement, InputProps>(
  ({ label, error, helperText, className, id, ...props }, ref) => {
    const inputId = id || label?.toLowerCase().replace(/\s+/g, '-');
    return (
      <div className="flex flex-col gap-1.5">
        {label && (
          <label htmlFor={inputId} className="text-sm font-medium text-text-primary">
            {label}
          </label>
        )}
        <input
          ref={ref}
          id={inputId}
          className={clsx(
            'h-9 rounded-md border bg-bg-input px-3 text-sm text-text-primary placeholder:text-text-tertiary',
            'transition-colors focus:outline-none focus:ring-2 focus:ring-accent-primary focus:border-border-strong',
            error ? 'border-accent-error' : 'border-border-default',
            'disabled:opacity-50 disabled:cursor-not-allowed',
            className,
          )}
          {...props}
        />
        {error && <p className="text-xs text-accent-error">{error}</p>}
        {helperText && !error && <p className="text-xs text-text-tertiary">{helperText}</p>}
      </div>
    );
  },
);
Input.displayName = 'Input';
