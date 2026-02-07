import { useState, useCallback } from 'react';
import { tauriInvoke } from '../lib/tauri';

interface UseTauriInvokeResult<T> {
  data: T | null;
  loading: boolean;
  error: string | null;
  invoke: (args?: Record<string, unknown>) => Promise<T | null>;
}

export function useTauriInvoke<T>(cmd: string): UseTauriInvokeResult<T> {
  const [data, setData] = useState<T | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = useCallback(async (args?: Record<string, unknown>): Promise<T | null> => {
    setLoading(true);
    setError(null);
    try {
      const result = await tauriInvoke<T>(cmd, args);
      setData(result);
      return result;
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
      return null;
    } finally {
      setLoading(false);
    }
  }, [cmd]);

  return { data, loading, error, invoke: run };
}
