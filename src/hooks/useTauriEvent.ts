import { useEffect } from 'react';
import { tauriListen } from '../lib/tauri';

export function useTauriEvent<T>(event: string, handler: (payload: T) => void) {
  useEffect(() => {
    const unlisten = tauriListen<T>(event, handler);
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [event, handler]);
}
