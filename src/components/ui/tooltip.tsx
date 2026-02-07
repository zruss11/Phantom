import { type ReactNode, useState } from 'react';
import clsx from 'clsx';

interface TooltipProps {
  content: string;
  children: ReactNode;
  side?: 'top' | 'bottom' | 'left' | 'right';
  delay?: number;
}

const positionStyles = {
  top: 'bottom-full left-1/2 -translate-x-1/2 mb-2',
  bottom: 'top-full left-1/2 -translate-x-1/2 mt-2',
  left: 'right-full top-1/2 -translate-y-1/2 mr-2',
  right: 'left-full top-1/2 -translate-y-1/2 ml-2',
};

export function Tooltip({ content, children, side = 'top', delay = 300 }: TooltipProps) {
  const [visible, setVisible] = useState(false);
  let timeout: ReturnType<typeof setTimeout>;

  return (
    <div
      className="relative inline-flex"
      onMouseEnter={() => { timeout = setTimeout(() => setVisible(true), delay); }}
      onMouseLeave={() => { clearTimeout(timeout); setVisible(false); }}
    >
      {children}
      {visible && (
        <div className={clsx('pointer-events-none absolute z-[800] whitespace-nowrap rounded-md bg-bg-elevated px-2 py-1 text-xs text-text-primary shadow-md border border-border-subtle', positionStyles[side])}>
          {content}
        </div>
      )}
    </div>
  );
}
