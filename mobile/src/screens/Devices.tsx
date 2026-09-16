import { RefreshControl, ScrollView, Text, View } from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";
import { useQuery } from "@tanstack/react-query";
import Ionicons from "@expo/vector-icons/Ionicons";
import type { ActiveSession } from "../session";
import { listDevices } from "../pairing/api";
import type { DeviceSummary } from "../pairing/contract";
import { Button, Empty, styles, useColors } from "../ui";

export function lastSeenLabel(
  device: Pick<DeviceSummary, "last_seen_at">,
  now = Date.now(),
): string {
  if (!device.last_seen_at) return "Not seen yet";
  const seen = Date.parse(`${device.last_seen_at.replace(/Z$/, "")}Z`);
  if (!Number.isFinite(seen)) return "Not seen yet";
  const minutes = Math.round((now - seen) / 60_000);
  if (minutes < 2) return "Online now";
  if (minutes < 60) return `Last seen ${minutes} minutes ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `Last seen ${hours} hour${hours === 1 ? "" : "s"} ago`;
  return `Last seen ${new Date(seen).toLocaleDateString()}`;
}

export function Devices({
  session,
  onAdd,
}: {
  session: ActiveSession;
  onAdd(): void;
}) {
  const c = useColors();
  const devices = useQuery({
    queryKey: ["devices"],
    queryFn: ({ signal }) => listDevices(session.api, signal),
    staleTime: 30_000,
  });
  return (
    <SafeAreaView
      edges={["left", "right"]}
      style={{ flex: 1, backgroundColor: c.background }}
    >
      <ScrollView
        contentContainerStyle={{ padding: 22, gap: 16 }}
        refreshControl={
          <RefreshControl
            refreshing={devices.isRefetching}
            onRefresh={() => void devices.refetch()}
            tintColor={c.accent}
          />
        }
      >
        {devices.isPending ? (
          <Text style={{ color: c.secondary }}>Loading your mirrors…</Text>
        ) : devices.isError ? (
          <View style={[styles.card, { backgroundColor: c.card }]}>
            <Text style={[styles.subtitle, { color: c.text }]}>
              Could not load your mirrors
            </Text>
            <Text style={{ color: c.secondary, lineHeight: 23 }}>
              {devices.error instanceof Error
                ? devices.error.message
                : "Please try again."}
            </Text>
            <Button
              title="Try again"
              quiet
              onPress={() => void devices.refetch()}
            />
          </View>
        ) : devices.data.length === 0 ? (
          <Empty
            title="No mirrors yet"
            detail="Add your first Daily Mirror to start collecting photographs at home."
          />
        ) : (
          devices.data.map((device) => (
            <View
              key={device.device_id}
              style={[styles.card, { backgroundColor: c.card, gap: 8 }]}
            >
              <View style={[styles.row, { gap: 12 }]}>
                <Ionicons name="tv-outline" size={22} color={c.accent} />
                <Text style={[styles.subtitle, { color: c.text, flex: 1 }]}>
                  {device.device_name}
                </Text>
              </View>
              <Text style={{ color: c.secondary }}>
                {lastSeenLabel(device)}
              </Text>
              <Text style={{ color: c.secondary, fontSize: 13 }}>
                {device.hardware} · firmware {device.firmware_version}
              </Text>
            </View>
          ))
        )}
        <Button title="Add a mirror" onPress={onAdd} />
        <Text style={{ color: c.secondary, lineHeight: 22, fontSize: 13 }}>
          Every mirror belongs to one household. Moving one to another home
          needs a full reset on the mirror itself.
        </Text>
      </ScrollView>
    </SafeAreaView>
  );
}
