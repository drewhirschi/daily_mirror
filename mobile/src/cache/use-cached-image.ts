import { useEffect, useState } from "react";
import { ApiError } from "@daily-mirror/api";
import type { DiskCache } from "./disk-cache";

/** Every image surface uses the session's shared, persistent image cache. */
export function useCachedImage(
  cache: DiskCache,
  key: string | undefined,
  onUnauthorized: () => void,
  enabled = true,
) {
  const [attempt, setAttempt] = useState(0);
  const [state, setState] = useState(() => ({
    key,
    uri: key ? cache.peek(key) : undefined,
    failed: false,
  }));
  useEffect(() => {
    let mounted = true;
    setState({ key, uri: key ? cache.peek(key) : undefined, failed: false });
    if (key && enabled) {
      void cache.get(key).then(
        (uri) => {
          if (mounted) setState({ key, uri, failed: false });
        },
        (error) => {
          if (!mounted) return;
          setState({ key, uri: undefined, failed: true });
          if (error instanceof ApiError && error.status === 401)
            onUnauthorized();
        },
      );
    }
    return () => {
      mounted = false;
    };
  }, [cache, key, enabled, attempt, onUnauthorized]);
  return {
    uri: state.key === key ? state.uri : key ? cache.peek(key) : undefined,
    failed: state.key === key && state.failed,
    retry: () => setAttempt((value) => value + 1),
    onError: () => {
      if (key) cache.forget(key);
      setState({ key, uri: undefined, failed: true });
    },
  };
}
