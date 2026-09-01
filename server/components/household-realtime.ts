import { useEffect, useRef, useState } from "react";

export type RealtimeStatus = "disabled" | "connecting" | "connected" | "reconnecting";

type RealtimeSession = {
  household_id: string;
  websocket_url: string;
  expires_at: number;
};

type RealtimeEvent = {
  type?: string;
  household_id?: string;
  sequence?: number;
  data?: unknown;
};

const PHOTO_EVENTS = new Set(["photo.created", "photo.updated", "photo.deleted", "photos.reconciled"]);

export function useHouseholdRealtime(enabled: boolean, refreshPhotos: () => void) {
  const [status, setStatus] = useState<RealtimeStatus>(enabled ? "connecting" : "disabled");
  const [notice, setNotice] = useState<string | null>(null);
  const refreshRef = useRef(refreshPhotos);
  refreshRef.current = refreshPhotos;

  useEffect(() => {
    if (!enabled) {
      setStatus("disabled");
      return;
    }

    let stopped = false;
    let socket: WebSocket | null = null;
    let reconnectTimer: number | null = null;
    let attempt = 0;

    const scheduleReconnect = () => {
      if (stopped || reconnectTimer !== null) return;
      setStatus("reconnecting");
      const delay = Math.min(30_000, 1_000 * (2 ** Math.min(attempt, 5)));
      attempt += 1;
      reconnectTimer = window.setTimeout(() => {
        reconnectTimer = null;
        void connect();
      }, delay);
    };

    const connect = async () => {
      if (stopped || !navigator.onLine) {
        scheduleReconnect();
        return;
      }
      setStatus(attempt ? "reconnecting" : "connecting");
      try {
        const response = await fetch("/api/realtime/session", {
          headers: { accept: "application/json" },
          cache: "no-store",
        });
        if (response.status === 404) {
          setStatus("disabled");
          return;
        }
        if (!response.ok) throw new Error(`session endpoint returned ${response.status}`);
        const session = await response.json() as RealtimeSession;
        if (stopped) return;

        socket = new WebSocket(session.websocket_url);
        socket.addEventListener("open", () => {
          attempt = 0;
          setStatus("connected");
        });
        socket.addEventListener("message", (message) => {
          try {
            const event = JSON.parse(String(message.data)) as RealtimeEvent;
            if (event.type && PHOTO_EVENTS.has(event.type)) refreshRef.current();
            if (event.type === "household.notice" && typeof event.data === "object" && event.data !== null && "message" in event.data) {
              const messageText = (event.data as { message?: unknown }).message;
              if (typeof messageText === "string") setNotice(messageText);
            }
          } catch {
            // A malformed optional event must not break gallery refreshes or reconnection.
          }
        });
        socket.addEventListener("close", scheduleReconnect);
        socket.addEventListener("error", () => socket?.close());
      } catch {
        scheduleReconnect();
      }
    };

    const reconnectWhenOnline = () => {
      if (socket?.readyState === WebSocket.OPEN || socket?.readyState === WebSocket.CONNECTING) return;
      if (reconnectTimer !== null) window.clearTimeout(reconnectTimer);
      reconnectTimer = null;
      void connect();
    };

    window.addEventListener("online", reconnectWhenOnline);
    void connect();
    return () => {
      stopped = true;
      window.removeEventListener("online", reconnectWhenOnline);
      if (reconnectTimer !== null) window.clearTimeout(reconnectTimer);
      socket?.close(1000, "gallery closed");
    };
  }, [enabled]);

  return { status, notice };
}
