import { useEffect, useState } from "react";
import { ActivityIndicator, AppState, View } from "react-native";
import { GestureHandlerRootView } from "react-native-gesture-handler";
import { SafeAreaProvider } from "react-native-safe-area-context";
import {
  NavigationContainer,
  DarkTheme,
  DefaultTheme,
} from "@react-navigation/native";
import { createNativeBottomTabNavigator } from "@react-navigation/bottom-tabs/unstable";
import {
  QueryCache,
  QueryClient,
  QueryClientProvider,
  focusManager,
} from "@tanstack/react-query";
import { StatusBar } from "expo-status-bar";
import { Image } from "expo-image";
import { ApiError } from "@daily-mirror/api";
import { SessionProvider, useSession, type ActiveSession } from "./session";
import { Flipbooks } from "./components/Flipbooks";
import { Gallery } from "./screens/Gallery";
import { Account } from "./screens/Account";
import { SignIn } from "./screens/SignIn";
import { useColors } from "./ui";

const Tab = createNativeBottomTabNavigator();
export default function App() {
  return (
    <GestureHandlerRootView style={{ flex: 1 }}>
      <SafeAreaProvider>
        <SessionProvider>
          <Root />
        </SessionProvider>
      </SafeAreaProvider>
    </GestureHandlerRootView>
  );
}

function Root() {
  const { active, loading } = useSession();
  const c = useColors();
  return (
    <>
      <StatusBar style={c.dark ? "light" : "dark"} />
      {loading ? (
        <View
          style={{
            flex: 1,
            backgroundColor: c.background,
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          <ActivityIndicator size="large" color={c.accent} />
        </View>
      ) : active ? (
        <SignedIn
          key={`${active.stored.origin}:${active.stored.user.id}:${active.stored.expiresAt}`}
          session={active}
        />
      ) : (
        <SignIn />
      )}
    </>
  );
}

function SignedIn({ session }: { session: ActiveSession }) {
  const { expire } = useSession();
  const c = useColors();
  const [cacheVersion, setCacheVersion] = useState(0);
  const [client] = useState(
    () =>
      new QueryClient({
        queryCache: new QueryCache({
          onError: (error) => {
            if (error instanceof ApiError && error.status === 401)
              void expire();
          },
        }),
        defaultOptions: {
          queries: {
            retry: (count, error) =>
              !(error instanceof ApiError && error.status < 500) && count < 1,
            gcTime: 10 * 60_000,
          },
        },
      }),
  );
  useEffect(() => {
    const subscription = AppState.addEventListener("change", (state) =>
      focusManager.setFocused(state === "active"),
    );
    return () => {
      subscription.remove();
      void client.cancelQueries();
      client.clear();
    };
  }, [client]);
  const clearCache = async () => {
    await client.cancelQueries();
    await session.cache.clear();
    await Image.clearMemoryCache();
    setCacheVersion((version) => version + 1);
    await Promise.all([
      client.invalidateQueries({ queryKey: ["photos"] }),
      client.invalidateQueries({ queryKey: ["people"] }),
    ]);
  };
  return (
    <QueryClientProvider client={client}>
      <NavigationContainer
        theme={{
          ...(c.dark ? DarkTheme : DefaultTheme),
          colors: {
            ...(c.dark ? DarkTheme.colors : DefaultTheme.colors),
            background: c.background,
            card: c.card,
            text: c.text,
            primary: c.accent,
            border: c.border,
          },
        }}
      >
        <Tab.Navigator
          screenOptions={({ route }) => ({
            headerShown: false,
            tabBarActiveTintColor: c.accent,
            tabBarIcon: ({ focused }) => ({
              type: "sfSymbol",
              name:
                route.name === "Archive"
                  ? focused
                    ? "photo.fill.on.rectangle.fill"
                    : "photo.on.rectangle"
                  : route.name === "Flipbooks"
                    ? focused
                      ? "rectangle.stack.fill"
                      : "rectangle.stack"
                    : focused
                      ? "person.crop.circle.fill"
                      : "person.crop.circle",
            }),
          })}
        >
          <Tab.Screen name="Archive">
            {() => <Gallery session={session} cacheVersion={cacheVersion} />}
          </Tab.Screen>
          <Tab.Screen name="Flipbooks">
            {() => <Flipbooks session={session} cacheVersion={cacheVersion} />}
          </Tab.Screen>
          <Tab.Screen name="Account">
            {() => <Account session={session} onClearCache={clearCache} />}
          </Tab.Screen>
        </Tab.Navigator>
      </NavigationContainer>
    </QueryClientProvider>
  );
}
