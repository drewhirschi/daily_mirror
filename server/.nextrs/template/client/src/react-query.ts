import { useQueryClient } from "@tanstack/react-query";
import { useParams as useRouterParams } from "@tanstack/react-router";

export function useSeed<T>(key: unknown[]): T | undefined {
  return useQueryClient().getQueryData<{ data: T }>(key)?.data;
}

// Matched route params ([seg] segments). Pages get them as a `params` prop;
// deep components can call this. Backed by the app shell's TanStack Router so
// the values stay LIVE across soft navigation — the server's __nx_params__
// tag is only the boot-time snapshot and goes stale after a client-side nav.
export function useParams<T extends Record<string, string> = Record<string, string>>(): T {
  return useRouterParams({ strict: false }) as T;
}

// React Query hooks, option factories, query keys, and URL-bound helpers.
export * from "./generated/react-query";
