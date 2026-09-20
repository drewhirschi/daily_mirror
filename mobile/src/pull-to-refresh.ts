import { useCallback, useEffect, useRef, useState } from "react";

/**
 * iOS hides the refresh control with an animation, and a `refreshing` flag
 * that goes true -> false inside a single frame leaves that animation stuck
 * half-open: the spinner sits under the header until the next touch. Holding
 * the flag for this long gives UIRefreshControl a settled state to animate
 * out of.
 */
export const MIN_SPINNER_MS = 450;

const wait = (ms: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));

/**
 * Runs one user-initiated refresh: the flag stays up until the refetch has
 * settled *and* the spinner has been visible long enough to animate away.
 * Kept separate from the hook so the timing is testable without a renderer.
 */
export async function runPullToRefresh(
  refetch: () => unknown,
  {
    setRefreshing,
    now = Date.now,
    sleep = wait,
    minimumMs = MIN_SPINNER_MS,
  }: {
    setRefreshing(refreshing: boolean): void;
    now?: () => number;
    sleep?: (ms: number) => Promise<void>;
    minimumMs?: number;
  },
): Promise<void> {
  const started = now();
  setRefreshing(true);
  try {
    await refetch();
  } catch {
    // A failed refresh is the screen's business: the queries carry the error
    // state, the spinner only has to come down.
  } finally {
    const left = minimumMs - (now() - started);
    if (left > 0) await sleep(left);
    setRefreshing(false);
  }
}

/**
 * Drives a `RefreshControl` from the pull itself rather than from query state.
 * `query.isRefetching` also goes true for focus, stale-time and polling
 * refetches, which showed the spinner on screens the user had only just
 * opened; this only ever reports a refresh the user asked for.
 */
export function usePullToRefresh(refetch: () => unknown): {
  refreshing: boolean;
  onRefresh(): void;
} {
  const [refreshing, setRefreshing] = useState(false);
  const mounted = useRef(true);
  const running = useRef(false);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const onRefresh = useCallback(() => {
    if (running.current) return;
    running.current = true;
    void runPullToRefresh(refetch, {
      setRefreshing: (value) => {
        if (mounted.current) setRefreshing(value);
      },
    }).finally(() => {
      running.current = false;
    });
  }, [refetch]);
  return { refreshing, onRefresh };
}
