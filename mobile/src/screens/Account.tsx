import { useCallback, useState } from "react";
import { Alert, Linking, ScrollView, Text, View } from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";
import { useFocusEffect } from "@react-navigation/native";
import { useQuery } from "@tanstack/react-query";
import Ionicons from "@expo/vector-icons/Ionicons";
import { useSession, type ActiveSession } from "../session";
import { IMAGE_CACHE_BUDGET } from "../cache/disk-cache";
import { Button, styles, useColors } from "../ui";

export function Account({
  session,
  onClearCache,
}: {
  session: ActiveSession;
  onClearCache(): Promise<void>;
}) {
  const c = useColors();
  const { signOut } = useSession();
  const [busy, setBusy] = useState(false);
  const [stats, setStats] = useState(session.cache.stats);
  const passkeys = useQuery({
    queryKey: ["passkeys"],
    queryFn: ({ signal }) => session.api.passkeys(signal),
    staleTime: 60_000,
  });
  useFocusEffect(
    useCallback(() => {
      setStats(session.cache.stats);
    }, [session]),
  );
  const logout = async (localOnly = false) => {
    setBusy(true);
    try {
      await signOut(localOnly);
    } catch {
      Alert.alert(
        "The server could not be reached",
        "You can sign out on this iPhone now. Your server session will expire automatically.",
        [
          { text: "Try later", style: "cancel" },
          {
            text: "Sign out on this iPhone",
            style: "destructive",
            onPress: () => void logout(true),
          },
        ],
      );
    } finally {
      setBusy(false);
    }
  };
  return (
    <SafeAreaView
      edges={["top", "left", "right"]}
      style={{ flex: 1, backgroundColor: c.background }}
    >
      <ScrollView contentContainerStyle={{ padding: 22, gap: 20 }}>
        <Text style={[styles.title, { color: c.text, marginVertical: 12 }]}>
          Your account
        </Text>
        <View style={[styles.card, { backgroundColor: c.card }]}>
          <View style={[styles.row, { gap: 16 }]}>
            <View
              style={{
                width: 58,
                height: 58,
                borderRadius: 29,
                backgroundColor: c.tint,
                alignItems: "center",
                justifyContent: "center",
              }}
            >
              <Ionicons name="person-outline" color={c.accent} size={26} />
            </View>
            <View style={{ flex: 1 }}>
              <Text style={[styles.subtitle, { color: c.text }]}>
                {session.stored.user.display_name}
              </Text>
              <Text style={{ color: c.secondary, marginTop: 4 }}>
                @{session.stored.user.username}
              </Text>
            </View>
          </View>
          <Text selectable style={{ color: c.secondary, lineHeight: 22 }}>
            {session.api.origin}
          </Text>
        </View>
        <View style={[styles.card, { backgroundColor: c.card }]}>
          <Text style={[styles.subtitle, { color: c.text }]}>
            On this iPhone
          </Text>
          <Text style={{ color: c.text, fontSize: 32, fontWeight: "600" }}>
            {(stats.bytes / 1024 / 1024).toFixed(1)}{" "}
            <Text style={{ fontSize: 17, color: c.secondary }}>MB</Text>
          </Text>
          <Text style={{ color: c.secondary, lineHeight: 23 }}>
            {stats.count.toLocaleString()} saved images. Up to{" "}
            {IMAGE_CACHE_BUDGET / 1024 / 1024 / 1024} GB stays on your phone so
            familiar moments open without downloading again. The least recently
            viewed images make room for new ones.
          </Text>
          <Text style={{ color: c.secondary, lineHeight: 23 }}>
            Thumbnails and face crops share this space with full photographs you
            open. Signing out removes all saved images from this device.
          </Text>
          <Button
            title="Clear saved images"
            quiet
            busy={busy}
            onPress={() =>
              Alert.alert(
                "Clear saved images?",
                "Your photographs remain in your archive. Images will download again when you browse.",
                [
                  { text: "Cancel", style: "cancel" },
                  {
                    text: "Clear",
                    onPress: () => {
                      setBusy(true);
                      void onClearCache()
                        .then(() => setStats(session.cache.stats))
                        .catch(() =>
                          Alert.alert(
                            "Could not clear cache",
                            "Please try again.",
                          ),
                        )
                        .finally(() => setBusy(false));
                    },
                  },
                ],
              )
            }
          />
        </View>
        <View style={[styles.card, { backgroundColor: c.card }]}>
          <Text style={[styles.subtitle, { color: c.text }]}>Passkeys</Text>
          {passkeys.isPending ? (
            <Text style={{ color: c.secondary }}>Loading passkeys…</Text>
          ) : passkeys.isError ? (
            <Button
              title="Retry loading passkeys"
              quiet
              onPress={() => void passkeys.refetch()}
            />
          ) : passkeys.data.passkeys.length ? (
            passkeys.data.passkeys.map((passkey) => (
              <View
                key={passkey.credential_id}
                style={[styles.row, { gap: 12 }]}
              >
                <Ionicons name="key-outline" color={c.accent} size={20} />
                <View style={{ flex: 1 }}>
                  <Text style={{ color: c.text, fontWeight: "600" }}>
                    {passkey.label}
                  </Text>
                  <Text style={{ color: c.secondary, marginTop: 4 }}>
                    Added{" "}
                    {new Date(
                      `${passkey.created_at.replace(/Z$/, "")}Z`,
                    ).toLocaleDateString()}
                  </Text>
                </View>
              </View>
            ))
          ) : (
            <Text style={{ color: c.secondary }}>
              No passkeys enrolled yet.
            </Text>
          )}
          <Text style={{ color: c.secondary, lineHeight: 23 }}>
            Add and manage passkeys in your web account. Use a saved passkey or
            your password to sign in; your session stays in Keychain.
          </Text>
          <Button
            title="Open web account"
            quiet
            onPress={() => {
              void Linking.openURL(`${session.api.origin}/account`).catch(() =>
                Alert.alert("Could not open browser"),
              );
            }}
          />
        </View>
        <Button
          title="Sign out"
          quiet
          danger
          busy={busy}
          onPress={() => void logout()}
        />
        <Text
          style={{
            color: c.secondary,
            fontSize: 12,
            textAlign: "center",
            marginVertical: 10,
          }}
        >
          Daily Mirror · 0.1.0
        </Text>
      </ScrollView>
    </SafeAreaView>
  );
}
