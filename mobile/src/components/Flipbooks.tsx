import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type ComponentProps,
} from "react";
import {
  ActivityIndicator,
  Alert,
  Linking,
  Modal,
  Pressable,
  RefreshControl,
  ScrollView,
  Text,
  View,
  UIManager,
} from "react-native";
import { SafeAreaProvider, SafeAreaView } from "react-native-safe-area-context";
import { Image } from "expo-image";
import { useQuery } from "@tanstack/react-query";
import { useFocusEffect } from "@react-navigation/native";
import Ionicons from "@expo/vector-icons/Ionicons";
import { useCachedImage } from "../cache/use-cached-image";
import { selectedFrameIndex } from "../flipbook";
import { ApiError, type PersonFlipbook } from "@daily-mirror/api";
import { useSession, type ActiveSession } from "../session";
import { Button, IconButton, styles, useColors } from "../ui";

const NativeSlider:
  typeof import("@react-native-community/slider").default | null =
  UIManager.hasViewManagerConfig("RNCSlider")
    ? require("@react-native-community/slider").default
    : null;

export function Flipbooks({
  session,
  cacheVersion,
}: {
  session: ActiveSession;
  cacheVersion: number;
}) {
  const c = useColors();
  const [personId, setPersonId] = useState<string>();
  const [pickerOpen, setPickerOpen] = useState(false);
  const people = useQuery({
    queryKey: ["people"],
    queryFn: async ({ signal }) => {
      const result = await session.api.people(signal);
      if (signal.aborted) throw new Error("Request cancelled");
      session.cache.reconcile(
        new Set(
          result.people.flatMap((person) =>
            person.frames.map((frame) => frame.crop_url),
          ),
        ),
        "/api/admin/faces/",
      );
      return result;
    },
    staleTime: 60_000,
  });
  const { refetch } = people;
  useFocusEffect(
    useCallback(() => {
      void refetch({ cancelRefetch: false });
    }, [refetch]),
  );
  const refresh = () => void refetch({ cancelRefetch: false });
  const selected =
    people.data?.people.find((person) => person.id === personId) ??
    people.data?.people[0];
  return (
    <SafeAreaView
      edges={["top", "left", "right"]}
      style={{ flex: 1, backgroundColor: c.background }}
    >
      <ScrollView
        alwaysBounceVertical
        refreshControl={
          <RefreshControl
            refreshing={people.isRefetching}
            onRefresh={refresh}
            tintColor={c.accent}
          />
        }
        contentContainerStyle={{ padding: 22, paddingBottom: 40, gap: 18 }}
      >
        <View style={[styles.row, { justifyContent: "space-between" }]}>
          <Text
            accessibilityRole="header"
            style={[styles.title, { color: c.text }]}
          >
            Flipbooks
          </Text>
          {people.isRefetching ? (
            <ActivityIndicator
              accessibilityLabel="Refreshing flipbooks"
              color={c.accent}
            />
          ) : (
            <IconButton
              icon="refresh"
              label="Refresh flipbooks"
              onPress={refresh}
              disabled={people.isPending}
            />
          )}
        </View>
        {people.isError ? (
          <Pressable accessibilityRole="button" onPress={refresh}>
            <Text style={{ color: c.secondary }}>
              {people.data
                ? "Couldn’t refresh. Showing the last loaded flipbooks. Tap to retry."
                : "Couldn’t load flipbooks. Tap to retry."}
            </Text>
          </Pressable>
        ) : null}
        {people.isPending ? (
          <ActivityIndicator />
        ) : selected ? (
          <>
            <Pressable
              accessibilityRole="button"
              accessibilityLabel={`Person: ${selected.display_name}. Change person`}
              accessibilityState={{ expanded: pickerOpen }}
              onPress={() => setPickerOpen(true)}
              style={[
                styles.row,
                {
                  justifyContent: "space-between",
                  padding: 16,
                  borderRadius: 16,
                  backgroundColor: c.card,
                },
              ]}
            >
              <Text style={[styles.subtitle, { color: c.text, flexShrink: 1 }]}>
                {selected.display_name}
              </Text>
              <Ionicons name="chevron-down" size={20} color={c.accent} />
            </Pressable>
            <PersonBook
              key={`${selected.id}:${cacheVersion}`}
              person={selected}
              session={session}
            />
          </>
        ) : !people.isError ? (
          <Text style={{ color: c.secondary }}>
            People will appear here once faces have been assigned in your
            archive.
          </Text>
        ) : null}
        <View style={{ gap: 8 }}>
          <Text style={{ color: c.secondary, lineHeight: 21 }}>
            One photograph per day, including suggested face matches. Only
            matches you confirm help recognize that person in future photos.
          </Text>
          <Button
            title="Review face matches"
            quiet
            onPress={() => {
              void Linking.openURL(`${session.api.origin}/admin`).catch(() => {
                Alert.alert("Couldn’t open face review", "Please try again.");
              });
            }}
          />
        </View>
      </ScrollView>
      <Modal
        visible={pickerOpen}
        animationType="slide"
        presentationStyle="pageSheet"
        onRequestClose={() => setPickerOpen(false)}
      >
        <SafeAreaProvider>
          <SafeAreaView style={{ flex: 1, backgroundColor: c.background }}>
            <View
              style={[
                styles.row,
                { justifyContent: "space-between", padding: 22 },
              ]}
            >
              <Text
                accessibilityRole="header"
                style={[styles.title, { color: c.text, flexShrink: 1 }]}
              >
                Choose a person
              </Text>
              <IconButton
                icon="close"
                label="Close people picker"
                onPress={() => setPickerOpen(false)}
              />
            </View>
            <ScrollView
              contentContainerStyle={{
                paddingHorizontal: 22,
                paddingBottom: 32,
                gap: 8,
              }}
            >
              {people.data?.people.map((person) => (
                <Pressable
                  key={person.id}
                  accessibilityRole="button"
                  accessibilityState={{ selected: person.id === selected?.id }}
                  onPress={() => {
                    setPersonId(person.id);
                    setPickerOpen(false);
                  }}
                  style={[
                    styles.row,
                    {
                      justifyContent: "space-between",
                      padding: 18,
                      borderRadius: 16,
                      backgroundColor:
                        person.id === selected?.id ? c.tint : c.card,
                    },
                  ]}
                >
                  <View style={{ gap: 5, flexShrink: 1 }}>
                    <Text style={[styles.subtitle, { color: c.text }]}>
                      {person.display_name}
                    </Text>
                    <Text style={{ color: c.secondary }}>
                      {person.frames.length}{" "}
                      {person.frames.length === 1 ? "day" : "days"}
                    </Text>
                  </View>
                  {person.id === selected?.id ? (
                    <Ionicons name="checkmark" size={24} color={c.accent} />
                  ) : null}
                </Pressable>
              ))}
            </ScrollView>
          </SafeAreaView>
        </SafeAreaProvider>
      </Modal>
    </SafeAreaView>
  );
}

function PersonBook({
  person,
  session,
}: {
  person: PersonFlipbook;
  session: ActiveSession;
}) {
  const c = useColors();
  const { expire } = useSession();
  const frames = useMemo(
    () =>
      [...person.frames].sort((a, b) =>
        a.capture_day.localeCompare(b.capture_day),
      ),
    [person.frames],
  );
  // The right end is the newest available day; drag left to go back in time.
  const [selectedDay, setSelectedDay] = useState<string>();
  const index = selectedFrameIndex(frames, selectedDay);
  const frame = frames[index];
  useEffect(() => {
    let cancelled = false;
    // Only one background download at a time leaves cache capacity for the
    // selected frame. The shared cache deduplicates foreground/background work.
    void (async () => {
      for (const item of [...frames].reverse()) {
        if (cancelled) break;
        try {
          await session.cache.get(item.crop_url);
        } catch (error) {
          if (error instanceof ApiError && error.status === 401) {
            void expire();
            break;
          }
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [frames, session, expire]);
  if (!frame)
    return (
      <Text style={{ color: c.secondary }}>
        No photographs matched to {person.display_name} yet.
      </Text>
    );
  return (
    <View style={{ gap: 12 }}>
      <CachedFrame
        key={frame.face_id}
        session={session}
        crop={frame.crop_url}
        label={`${person.display_name} on ${frame.capture_day}`}
      />
      <Text style={{ color: c.text, textAlign: "center", fontSize: 17 }}>
        {new Date(`${frame.capture_day}T12:00:00`).toLocaleDateString(
          undefined,
          { month: "long", day: "numeric", year: "numeric" },
        )}
      </Text>
      <Slider
        accessibilityLabel={`Scrub through ${person.display_name}'s photographs`}
        minimumValue={0}
        maximumValue={Math.max(1, frames.length - 1)}
        step={1}
        value={index}
        disabled={frames.length < 2}
        onValueChange={(value) => {
          const next = Math.round(value);
          setSelectedDay(
            next === frames.length - 1 ? undefined : frames[next]?.capture_day,
          );
        }}
        minimumTrackTintColor={c.accent}
      />
      <View style={{ flexDirection: "row", justifyContent: "space-between" }}>
        <Text style={{ color: c.secondary }}>Older</Text>
        <Text style={{ color: c.secondary }}>Latest</Text>
      </View>
      <Text style={{ color: c.secondary, textAlign: "center" }}>
        {index + 1} of {frames.length} · Drag back through the days
      </Text>
    </View>
  );
}

// Use only the server's 384px face thumbnail. An uncropped archive preview
// changes framing on arrival, and originals are unnecessary for playback.
function CachedFrame({
  session,
  crop,
  label,
}: {
  session: ActiveSession;
  crop: string;
  label: string;
}) {
  const c = useColors();
  const { expire } = useSession();
  const image = useCachedImage(session.cache, crop, expire);
  const { uri, failed } = image;
  return (
    <View
      style={{
        width: "100%",
        aspectRatio: 1,
        borderRadius: 20,
        overflow: "hidden",
        backgroundColor: c.tint,
      }}
    >
      {uri ? (
        <Image
          source={{ uri }}
          cachePolicy="memory"
          contentFit="contain"
          transition={0}
          style={{ width: "100%", height: "100%" }}
          accessibilityLabel={label}
          onError={image.onError}
        />
      ) : null}
      {!uri && !failed ? (
        <ActivityIndicator
          style={{ position: "absolute", alignSelf: "center", top: "50%" }}
        />
      ) : null}
      {failed ? (
        <View style={{ position: "absolute", bottom: 12, alignSelf: "center" }}>
          <Button title="Retry photograph" onPress={image.retry} />
        </View>
      ) : null}
    </View>
  );
}

// Preserve scrubbing when Metro updates an older development client.
function Slider(
  props: ComponentProps<
    typeof import("@react-native-community/slider").default
  >,
) {
  const c = useColors();
  const [width, setWidth] = useState(1);
  if (NativeSlider) return <NativeSlider {...props} />;
  const max = props.maximumValue ?? 1;
  const update = (x: number) =>
    props.onValueChange?.(
      Math.round(Math.max(0, Math.min(1, x / width)) * max),
    );
  return (
    <View
      accessibilityRole="adjustable"
      accessibilityLabel={props.accessibilityLabel}
      accessibilityValue={{ min: 0, max, now: props.value ?? 0 }}
      accessibilityActions={[{ name: "increment" }, { name: "decrement" }]}
      onAccessibilityAction={(event) => {
        if (!props.disabled)
          props.onValueChange?.(
            Math.max(
              0,
              Math.min(
                max,
                (props.value ?? 0) +
                  (event.nativeEvent.actionName === "increment" ? 1 : -1),
              ),
            ),
          );
      }}
      onLayout={(event) =>
        setWidth(Math.max(1, event.nativeEvent.layout.width))
      }
      onStartShouldSetResponder={() => !props.disabled}
      onResponderGrant={(event) => update(event.nativeEvent.locationX)}
      onResponderMove={(event) => update(event.nativeEvent.locationX)}
      onResponderTerminationRequest={() => false}
      style={{
        height: 44,
        justifyContent: "center",
        opacity: props.disabled ? 0.4 : 1,
      }}
    >
      <View
        pointerEvents="none"
        style={{ height: 4, backgroundColor: c.tint, borderRadius: 2 }}
      />
      <View
        pointerEvents="none"
        style={{
          position: "absolute",
          left: Math.max(0, ((width - 24) * (props.value ?? 0)) / max),
          width: 24,
          height: 24,
          borderRadius: 12,
          backgroundColor: c.accent,
        }}
      />
    </View>
  );
}
