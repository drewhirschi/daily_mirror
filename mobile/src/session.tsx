import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { AppState, NativeModules, Platform } from "react-native";
import { loginWithPasskey } from "./passkey-login";
import * as SecureStore from "expo-secure-store";
import { Image } from "expo-image";
import {
  ApiError,
  MirrorApi,
  type NativeSession,
  type PasswordLogin,
  type Photo,
} from "@daily-mirror/api";
import {
  createImageCache,
  clearPrivateFiles,
  readCatalog,
} from "./cache/thumbnails";

const SESSION_KEY = "daily-mirror.session.v1";
const SERVER_KEY = "daily-mirror.server.v1";
export const DEFAULT_SERVER = "https://daily-mirror-pearl.vercel.app";
type StoredSession = NativeSession & { origin: string; expiresAt: number };
type Resources = Awaited<ReturnType<typeof createImageCache>>;
export type ActiveSession = Resources & {
  stored: StoredSession;
  api: MirrorApi;
  initialPhotos?: Photo[];
};
type SessionContext = {
  active: ActiveSession | null;
  loading: boolean;
  error: string;
  lastServer: string;
  signIn(origin: string, credentials: PasswordLogin): Promise<void>;
  signInWithPasskey(origin: string, username: string): Promise<void>;
  signOut(localOnly?: boolean): Promise<void>;
  expire(): Promise<void>;
};
const Context = createContext<SessionContext | null>(null);
export const useSession = () => {
  const value = useContext(Context);
  if (!value) throw new Error("Missing session provider");
  return value;
};

export function SessionProvider({ children }: { children: ReactNode }) {
  const [active, setActive] = useState<ActiveSession | null>(null);
  const activeRef = useRef<ActiveSession | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [lastServer, setLastServer] = useState(DEFAULT_SERVER);
  const ending = useRef<Promise<void> | null>(null);

  const open = useCallback(async (stored: StoredSession) => {
    const api = new MirrorApi(stored.origin, stored.token, __DEV__);
    const resources = await createImageCache(api, stored.user.id);
    const next = {
      stored,
      api,
      ...resources,
      initialPhotos: readCatalog(resources.directory),
    };
    activeRef.current = next;
    setActive(next);
  }, []);

  const expire = useCallback(async () => {
    if (ending.current) return ending.current;
    const session = activeRef.current;
    activeRef.current = null;
    setActive(null);
    const cleanup = async () => {
      try {
        await SecureStore.deleteItemAsync(SESSION_KEY);
      } finally {
        try {
          await session?.cache.clear(true);
        } finally {
          clearPrivateFiles();
          await Image.clearMemoryCache();
        }
      }
    };
    ending.current = cleanup().finally(() => {
      ending.current = null;
    });
    return ending.current;
  }, []);

  useEffect(() => {
    void (async () => {
      try {
        setLastServer(
          (await SecureStore.getItemAsync(SERVER_KEY)) || DEFAULT_SERVER,
        );
        const raw = await SecureStore.getItemAsync(SESSION_KEY);
        if (raw) {
          const stored: StoredSession = JSON.parse(raw);
          if (
            typeof stored.token !== "string" ||
            typeof stored.user?.id !== "string" ||
            !Number.isFinite(stored.expiresAt) ||
            stored.expiresAt <= Date.now()
          )
            await expire();
          else await open(stored);
        } else clearPrivateFiles();
      } catch {
        setError(
          "Your saved session could not be restored. Please sign in again.",
        );
        await expire().catch(() => undefined);
      } finally {
        setLoading(false);
      }
    })();
  }, [open, expire]);

  useEffect(() => {
    if (!active) return;
    const validate = async () => {
      if (active.stored.expiresAt <= Date.now()) {
        await expire();
        return;
      }
      try {
        await active.api.me();
      } catch (caught) {
        if (
          activeRef.current === active &&
          caught instanceof ApiError &&
          caught.status === 401
        )
          await expire();
        // A transient network error preserves the cached archive for offline use.
      }
    };
    void validate();
    const timer = setInterval(() => {
      if (active.stored.expiresAt <= Date.now()) void expire();
    }, 60_000);
    const subscription = AppState.addEventListener("change", (state) => {
      if (state === "active") void validate();
    });
    return () => {
      clearInterval(timer);
      subscription.remove();
    };
  }, [active, expire]);

  const signIn = async (origin: string, credentials: PasswordLogin) => {
    await ending.current;
    setError("");
    const api = new MirrorApi(origin, undefined, __DEV__);
    let result: NativeSession;
    try {
      result = await api.login(credentials);
    } catch (caught) {
      if (caught instanceof ApiError && caught.status === 401)
        throw new Error("The username or password is incorrect.");
      if (caught instanceof ApiError && caught.status === 404)
        throw new Error(
          "This server needs the mobile API update before you can sign in.",
        );
      throw caught;
    }
    await saveSession(api, result);
  };

  const signInWithPasskey = async (origin: string, username: string) => {
    await ending.current;
    setError("");
    if (Platform.OS !== "ios" || !NativeModules.Passkey) {
      throw new Error(
        "Install the updated iPhone build to use passkeys. Password sign-in still works.",
      );
    }
    const { Passkey } =
      require("react-native-passkey") as typeof import("react-native-passkey");
    if (!Passkey.isSupported())
      throw new Error("Passkeys are not supported on this device.");
    const api = new MirrorApi(origin, undefined, __DEV__);
    const result = await loginWithPasskey(api, username, (request) =>
      Passkey.get(request),
    );
    await saveSession(api, result);
  };

  const saveSession = async (api: MirrorApi, result: NativeSession) => {
    const origin = api.origin;
    const stored = {
      ...result,
      origin: api.origin,
      expiresAt: Date.now() + result.expires_in_seconds * 1000,
    };
    try {
      await SecureStore.setItemAsync(SESSION_KEY, JSON.stringify(stored), {
        keychainAccessible: SecureStore.WHEN_UNLOCKED_THIS_DEVICE_ONLY,
      });
      await SecureStore.setItemAsync(SERVER_KEY, api.origin);
      setLastServer(api.origin);
      await open(stored);
    } catch (caught) {
      await new MirrorApi(api.origin, result.token, __DEV__)
        .logout()
        .catch(() => undefined);
      await expire();
      throw caught;
    }
  };

  const signOut = async (localOnly = false) => {
    if (!localOnly && activeRef.current) {
      try {
        await activeRef.current.api.logout();
      } catch (caught) {
        if (!(caught instanceof ApiError && caught.status === 401))
          throw caught;
      }
    }
    await expire();
  };

  return (
    <Context.Provider
      value={{
        active,
        loading,
        error,
        lastServer,
        signIn,
        signInWithPasskey,
        signOut,
        expire,
      }}
    >
      {children}
    </Context.Provider>
  );
}
