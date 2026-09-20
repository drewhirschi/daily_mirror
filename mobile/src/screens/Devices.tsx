import {
  RefreshControl,
  ScrollView,
  Text,
  View,
  Pressable,
} from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";
import { useQuery } from "@tanstack/react-query";
import Ionicons from "@expo/vector-icons/Ionicons";
import type { ActiveSession } from "../session";
import { listDevices } from "../pairing/api";
import type { DeviceSummary } from "../pairing/contract";
import { Button, styles, useColors } from "../ui";
import { usePullToRefresh } from "../pull-to-refresh";

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

/**
 * Presented like the household screen: an opening paragraph, one card per
 * camera, and an "Add camera" row in the same shape as "Add person".
 */
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
  const pull = usePullToRefresh(devices.refetch);
  return (
    <SafeAreaView
      edges={["left", "right"]}
      style={{ flex: 1, backgroundColor: c.background }}
    >
      <ScrollView
        contentContainerStyle={{ padding: 22, paddingBottom: 40, gap: 14 }}
        refreshControl={
          <RefreshControl
            refreshing={pull.refreshing}
            onRefresh={pull.onRefresh}
            tintColor={c.accent}
          />
        }
      >
        <Text style={{ color: c.secondary, lineHeight: 23 }}>
          Every camera belongs to one household. Moving one to another home
          needs a full reset on the camera itself.
        </Text>

        {devices.isPending ? (
          <Text style={{ color: c.secondary }}>Loading your cameras…</Text>
        ) : devices.isError ? (
          <View style={[styles.card, { backgroundColor: c.card }]}>
            <Text style={{ color: c.text, lineHeight: 23 }}>
              Your cameras could not be loaded.
            </Text>
            <Button
              title="Try again"
              quiet
              onPress={() => void devices.refetch()}
            />
          </View>
        ) : devices.data.length === 0 ? (
          <View style={[styles.card, { backgroundColor: c.card }]}>
            <Text style={[styles.subtitle, { color: c.text }]}>
              No cameras yet
            </Text>
            <Text style={{ color: c.secondary, lineHeight: 23 }}>
              Add your first camera to start collecting photographs at home.
            </Text>
          </View>
        ) : (
          devices.data.map((device) => (
            <View
              key={device.device_id}
              style={[styles.card, { backgroundColor: c.card }]}
            >
              <View style={[styles.row, { gap: 10 }]}>
                <Text
                  style={[styles.subtitle, { color: c.text, flexShrink: 1 }]}
                >
                  {device.device_name}
                </Text>
              </View>
              <View style={[styles.row, { gap: 8 }]}>
                <Ionicons
                  name={
                    lastSeenLabel(device) === "Online now"
                      ? "checkmark-circle"
                      : "ellipse-outline"
                  }
                  size={18}
                  color={
                    lastSeenLabel(device) === "Online now"
                      ? c.accent
                      : c.secondary
                  }
                />
                <Text
                  style={{
                    color:
                      lastSeenLabel(device) === "Online now"
                        ? c.accent
                        : c.secondary,
                  }}
                >
                  {lastSeenLabel(device)}
                </Text>
              </View>
              <Text style={{ color: c.secondary, fontSize: 13 }}>
                {device.hardware} · firmware {device.firmware_version}
              </Text>
            </View>
          ))
        )}

        <Pressable
          accessibilityRole="button"
          onPress={onAdd}
          style={({ pressed }) => [
            styles.card,
            styles.row,
            { backgroundColor: c.card, gap: 12, opacity: pressed ? 0.6 : 1 },
          ]}
        >
          <Ionicons name="add-circle-outline" size={24} color={c.accent} />
          <Text style={{ color: c.accent, fontWeight: "600" }}>Add camera</Text>
        </Pressable>
      </ScrollView>
    </SafeAreaView>
  );
}
