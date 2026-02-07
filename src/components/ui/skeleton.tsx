import clsx from 'clsx';

interface SkeletonProps {
  className?: string;
  width?: string | number;
  height?: string | number;
  rounded?: boolean;
}

export function Skeleton({ className, width, height, rounded }: SkeletonProps) {
  return (
    <div
      className={clsx('animate-pulse bg-bg-surface-hover', rounded ? 'rounded-full' : 'rounded-md', className)}
      style={{ width, height }}
    />
  );
}
