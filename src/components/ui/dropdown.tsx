import { useState, useRef, useEffect } from 'react';
import { ChevronDown, Search } from 'lucide-react';
import clsx from 'clsx';

interface DropdownOption {
  value: string;
  label: string;
  description?: string;
}

interface DropdownProps {
  options: DropdownOption[];
  value?: string;
  onChange: (value: string) => void;
  placeholder?: string;
  searchable?: boolean;
}

export function Dropdown({ options, value, onChange, placeholder = 'Select...', searchable }: DropdownProps) {
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState('');
  const [highlightedIndex, setHighlightedIndex] = useState(0);
  const ref = useRef<HTMLDivElement>(null);

  const filtered = searchable
    ? options.filter((o) => o.label.toLowerCase().includes(search.toLowerCase()))
    : options;

  const selected = options.find((o) => o.value === value);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, []);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (!open) { if (e.key === 'Enter' || e.key === ' ') setOpen(true); return; }
    if (e.key === 'ArrowDown') { e.preventDefault(); setHighlightedIndex((i) => Math.min(i + 1, filtered.length - 1)); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); setHighlightedIndex((i) => Math.max(i - 1, 0)); }
    else if (e.key === 'Enter' && filtered[highlightedIndex]) { onChange(filtered[highlightedIndex].value); setOpen(false); }
    else if (e.key === 'Escape') setOpen(false);
  };

  return (
    <div ref={ref} className="relative" onKeyDown={handleKeyDown}>
      <button
        onClick={() => setOpen(!open)}
        className="flex h-9 w-full items-center justify-between rounded-md border border-border-default bg-bg-input px-3 text-sm transition-colors hover:bg-bg-surface-hover"
      >
        <span className={selected ? 'text-text-primary' : 'text-text-tertiary'}>
          {selected?.label || placeholder}
        </span>
        <ChevronDown size={14} className={clsx('text-text-tertiary transition-transform', open && 'rotate-180')} />
      </button>

      {open && (
        <div className="absolute left-0 top-full z-50 mt-1 w-full rounded-lg border border-border-default bg-bg-elevated shadow-lg">
          {searchable && (
            <div className="flex items-center gap-2 border-b border-border-subtle px-3 py-2">
              <Search size={14} className="text-text-tertiary" />
              <input
                autoFocus
                value={search}
                onChange={(e) => { setSearch(e.target.value); setHighlightedIndex(0); }}
                placeholder="Search..."
                className="flex-1 bg-transparent text-sm text-text-primary outline-none placeholder:text-text-tertiary"
              />
            </div>
          )}
          <div className="max-h-60 overflow-auto py-1">
            {filtered.map((option, i) => (
              <button
                key={option.value}
                onClick={() => { onChange(option.value); setOpen(false); }}
                className={clsx(
                  'flex w-full flex-col px-3 py-2 text-left transition-colors',
                  i === highlightedIndex ? 'bg-bg-surface-hover' : 'hover:bg-bg-surface-hover',
                  option.value === value && 'text-accent-primary',
                )}
              >
                <span className="text-sm">{option.label}</span>
                {option.description && <span className="text-xs text-text-tertiary">{option.description}</span>}
              </button>
            ))}
            {filtered.length === 0 && <p className="px-3 py-2 text-sm text-text-tertiary">No results</p>}
          </div>
        </div>
      )}
    </div>
  );
}
