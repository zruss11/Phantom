import { type ReactNode, useState } from 'react';
import { ChevronRight } from 'lucide-react';
import clsx from 'clsx';

interface AccordionProps {
  title: string;
  children: ReactNode;
  defaultOpen?: boolean;
}

export function Accordion({ title, children, defaultOpen = false }: AccordionProps) {
  const [open, setOpen] = useState(defaultOpen);

  return (
    <div className="border-b border-border-subtle">
      <button
        onClick={() => setOpen(!open)}
        className="flex w-full items-center gap-2 px-3 py-2.5 text-sm font-medium text-text-primary transition-colors hover:bg-bg-surface-hover"
      >
        <ChevronRight size={14} className={clsx('shrink-0 text-text-tertiary transition-transform duration-200', open && 'rotate-90')} />
        {title}
      </button>
      {open && <div className="px-3 pb-3">{children}</div>}
    </div>
  );
}
