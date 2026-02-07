interface PlaceholderPageProps {
  title: string;
}

export function PlaceholderPage({ title }: PlaceholderPageProps) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-4">
      <h1 className="text-3xl font-semibold text-text-primary">{title}</h1>
      <p className="text-text-secondary">Coming soon</p>
    </div>
  );
}
