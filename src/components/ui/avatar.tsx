import clsx from 'clsx';
import type { AgentId } from '../../lib/types';
import { AGENTS } from '../../lib/constants';

interface AvatarProps {
  agentId?: AgentId;
  name?: string;
  size?: 'sm' | 'md' | 'lg';
  className?: string;
}

const sizeMap = { sm: 'h-6 w-6 text-[10px]', md: 'h-8 w-8 text-xs', lg: 'h-10 w-10 text-sm' };

export function Avatar({ agentId, name, size = 'md', className }: AvatarProps) {
  if (agentId) {
    const agent = AGENTS[agentId];
    return (
      <div
        className={clsx('flex items-center justify-center rounded-full', sizeMap[size], className)}
        style={{ backgroundColor: `${agent.color}20` }}
      >
        <span className="font-semibold" style={{ color: agent.color }}>
          {agent.name.charAt(0)}
        </span>
      </div>
    );
  }

  const initials = name ? name.split(' ').map((n) => n[0]).join('').slice(0, 2).toUpperCase() : '?';
  return (
    <div className={clsx('flex items-center justify-center rounded-full bg-accent-primary-muted text-accent-primary font-semibold', sizeMap[size], className)}>
      {initials}
    </div>
  );
}
