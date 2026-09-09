import { useCallback, useMemo, useState } from "react";
import {
  ActivityIndicator,
  Modal,
  Platform,
  Pressable,
  RefreshControl,
  ScrollView,
  SectionList,
  Text,
  View,
  useWindowDimensions,
} from "react-native";
import {
  SafeAreaProvider,
  SafeAreaView,
  useSafeAreaInsets,
} from "react-native-safe-area-context";
import { Gesture, GestureDetector } from "react-native-gesture-handler";
import DateTimePicker from "@react-native-community/datetimepicker";
import { useQuery } from "@tanstack/react-query";
import { useSession, type ActiveSession } from "../session";
import { writeCatalog } from "../cache/thumbnails";
import {
  columnsFor,
  DEFAULT_DENSITY,
  densityAfterPinch,
  densityLabels,
  photoSections,
  selectPhotos,
  type Density,
} from "../gallery";
import { Thumbnail } from "../components/Thumbnail";
import { PhotoViewer } from "../components/PhotoViewer";
import { Button, Empty, IconButton, styles, useColors } from "../ui";

export function Gallery({
  session,
  cacheVersion,
}: {
  session: ActiveSession;
  cacheVersion: number;
}) {
  const c = useColors();
  const { width } = useWindowDimensions();
  const insets = useSafeAreaInsets();
  const { expire } = useSession();
  const unauthorized = useCallback(() => {
    void expire();
  }, [expire]);
  const [density, setDensity] = useState<Density>(DEFAULT_DENSITY);
  const gridGesture = useMemo(
    () =>
      Gesture.Simultaneous(
        Gesture.Native(),
        Gesture.Pinch()
          .runOnJS(true)
          .onEnd((event, success) => {
            if (success)
              setDensity((current) => densityAfterPinch(current, event.scale));
          }),
      ),
    [],
  );
  const [from, setFrom] = useState<Date>();
  const [to, setTo] = useState<Date>();
  const [personId, setPersonId] = useState<string>();
  const [filterOpen, setFilterOpen] = useState(false);
  const [selectedId, setSelectedId] = useState<string>();
  const photos = useQuery({
    queryKey: ["photos"],
    queryFn: async ({ signal }) => {
      const result = await session.api.photos(signal);
      if (signal.aborted) throw new Error("Request cancelled");
      session.cache.reconcile(
        new Set(
          result.photos.flatMap((photo) => [
            photo.url,
            ...(photo.thumbnail_url ? [photo.thumbnail_url] : []),
          ]),
        ),
        "/api/photos/",
      );
      writeCatalog(session.directory, result.photos);
      return result.photos;
    },
    initialData: session.initialPhotos,
    initialDataUpdatedAt: 0,
    staleTime: 60_000,
  });
  const people = useQuery({
    queryKey: ["people"],
    queryFn: ({ signal }) => session.api.people(signal),
    enabled: filterOpen || !!personId,
    staleTime: 60_000,
  });
  const personPhotos = useQuery({
    queryKey: ["person-photos", personId],
    queryFn: ({ signal }) => session.api.personPhotos(personId!, signal),
    enabled: !!personId,
    staleTime: 60_000,
  });
  const personPhotoIds = useMemo(
    () => (personId ? new Set(personPhotos.data?.photo_ids ?? []) : undefined),
    [personId, personPhotos.data],
  );
  const personName = people.data?.people.find(
    (person) => person.id === personId,
  )?.display_name;
  const hasFilters = !!(from || to || personId);
  const refresh = () => {
    void photos.refetch();
    if (personId) void personPhotos.refetch();
  };
  const selectedPhotos = useMemo(
    () => selectPhotos(photos.data || [], from, to, personPhotoIds),
    [photos.data, from, to, personPhotoIds],
  );
  const columns = columnsFor(density, width);
  const sections = useMemo(
    () => photoSections(selectedPhotos, density, columns),
    [selectedPhotos, density, columns],
  );
  const cellSize =
    (width - insets.left - insets.right - 12 - (columns - 1) * 3) / columns;
  const selected = selectedId
    ? selectedPhotos.findIndex((photo) => photo.id === selectedId)
    : -1;

  return (
    <SafeAreaView
      edges={["top", "left", "right"]}
      style={{ flex: 1, backgroundColor: c.background }}
    >
      <View
        style={{
          paddingHorizontal: 22,
          paddingTop: 12,
          paddingBottom: 18,
          gap: 18,
        }}
      >
        <View style={[styles.row, { justifyContent: "space-between" }]}>
          <View>
            <Text style={[styles.title, { color: c.text }]}>Archive</Text>
            <Text style={{ color: c.secondary, marginTop: 5, fontSize: 15 }}>
              {selectedPhotos.length.toLocaleString()}{" "}
              {selectedPhotos.length === 1 ? "moment" : "moments"}, collected
              over time
            </Text>
          </View>
          <IconButton
            icon={hasFilters ? "filter" : "filter-outline"}
            label="Filter photographs"
            onPress={() => setFilterOpen(true)}
          />
        </View>
        <Text
          accessibilityRole="adjustable"
          accessibilityLabel="Archive detail"
          accessibilityValue={{ text: densityLabels[density] }}
          accessibilityHint="Swipe up for more detail or down for less detail."
          accessibilityActions={[
            { name: "increment", label: "More detail" },
            { name: "decrement", label: "Less detail" },
          ]}
          onAccessibilityAction={({ nativeEvent }) => {
            if (nativeEvent.actionName === "increment")
              setDensity((current) => densityAfterPinch(current, 2));
            if (nativeEvent.actionName === "decrement")
              setDensity((current) => densityAfterPinch(current, 0.5));
          }}
          style={{ color: c.secondary, fontSize: 14 }}
        >
          {densityLabels[density]} · Pinch to change the view
        </Text>
        {hasFilters ? (
          <Pressable
            accessibilityRole="button"
            onPress={() => setFilterOpen(true)}
          >
            <Text style={{ color: c.accent }}>
              {personId ? `${personName || "Selected person"} · ` : ""}
              {from?.toLocaleDateString() || "Beginning"} —{" "}
              {to?.toLocaleDateString() || "Today"}
            </Text>
          </Pressable>
        ) : null}
      </View>
      {photos.isError ? (
        <Pressable
          onPress={() => void photos.refetch()}
          accessibilityRole="button"
          style={{ padding: 14, backgroundColor: c.tint }}
        >
          <Text style={{ color: c.secondary, textAlign: "center" }}>
            {photos.data
              ? "Showing your saved archive. Tap to reconnect."
              : "Could not reach your archive. Tap to retry."}
          </Text>
        </Pressable>
      ) : null}
      {personId && personPhotos.isError ? (
        <Button
          title="Couldn’t load person filter. Retry"
          quiet
          onPress={() => void personPhotos.refetch()}
        />
      ) : null}
      {photos.isPending || (personId && personPhotos.isPending) ? (
        <View style={{ paddingTop: 90 }}>
          <ActivityIndicator size="large" color={c.accent} />
        </View>
      ) : (
        <GestureDetector gesture={gridGesture}>
          <SectionList
            key={`${density}-${columns}`}
            sections={sections}
            keyExtractor={(row) => row[0].id}
            extraData={cacheVersion}
            stickySectionHeadersEnabled
            initialNumToRender={12}
            maxToRenderPerBatch={12}
            removeClippedSubviews={false}
            windowSize={9}
            contentContainerStyle={{ paddingHorizontal: 6, paddingBottom: 24 }}
            refreshControl={
              <RefreshControl
                refreshing={
                  photos.isRefetching ||
                  (!!personId && personPhotos.isRefetching)
                }
                onRefresh={refresh}
                tintColor={c.accent}
              />
            }
            renderSectionHeader={({ section }) => (
              <View
                style={[
                  styles.row,
                  {
                    justifyContent: "space-between",
                    paddingHorizontal: 16,
                    paddingTop: 18,
                    paddingBottom: 12,
                    backgroundColor: c.background,
                  },
                ]}
              >
                <Text
                  accessibilityRole="header"
                  style={[styles.subtitle, { color: c.text }]}
                >
                  {section.title}
                </Text>
                <Text style={{ color: c.secondary }}>{section.count}</Text>
              </View>
            )}
            renderItem={({ item }) => (
              <View style={{ flexDirection: "row", gap: 3, marginBottom: 3 }}>
                {item.map((photo) => (
                  <Thumbnail
                    key={`${photo.id}:${photo.thumbnail_url}:${cacheVersion}`}
                    photo={photo}
                    cache={session.cache}
                    size={cellSize}
                    onPress={setSelectedId}
                    onUnauthorized={unauthorized}
                  />
                ))}
              </View>
            )}
            ListEmptyComponent={
              <Empty
                title={
                  personId && personPhotos.isError
                    ? "Person filter is unavailable"
                    : hasFilters
                      ? "No matching moments"
                      : photos.isError
                        ? "Your archive is out of reach"
                        : "Every day starts somewhere"
                }
                detail={
                  personId && personPhotos.isError
                    ? "Reconnect and retry. This filter requires the updated server."
                    : hasFilters
                      ? "Try another person or a different date range."
                      : photos.isError
                        ? "Connect to your server and pull down to try again."
                        : "Take a photograph with your Daily Mirror. It will be waiting here."
                }
              >
                {hasFilters ? (
                  <Button
                    title="Show all photographs"
                    quiet
                    onPress={() => {
                      setFrom(undefined);
                      setTo(undefined);
                      setPersonId(undefined);
                    }}
                  />
                ) : null}
              </Empty>
            }
          />
        </GestureDetector>
      )}
      {selected >= 0 ? (
        <PhotoViewer
          photos={selectedPhotos}
          initialIndex={selected}
          session={session}
          onClose={() => setSelectedId(undefined)}
        />
      ) : null}
      <Modal
        visible={filterOpen}
        animationType="slide"
        presentationStyle="pageSheet"
        onRequestClose={() => setFilterOpen(false)}
      >
        <SafeAreaProvider>
          <SafeAreaView
            style={{
              flex: 1,
              backgroundColor: c.background,
              padding: 24,
              gap: 24,
            }}
          >
            <View style={[styles.row, { justifyContent: "space-between" }]}>
              <Text style={[styles.title, { color: c.text }]}>Filters</Text>
              <IconButton
                icon="close"
                label="Close filters"
                onPress={() => setFilterOpen(false)}
              />
            </View>
            <ScrollView contentContainerStyle={{ gap: 24, paddingBottom: 24 }}>
              <View style={{ gap: 10 }}>
                <Text style={{ color: c.secondary, fontWeight: "600" }}>
                  Person
                </Text>
                <View
                  style={{ flexDirection: "row", flexWrap: "wrap", gap: 8 }}
                >
                  <Button
                    title="Everyone"
                    quiet={!!personId}
                    onPress={() => setPersonId(undefined)}
                  />
                  {people.data?.people.map((person) => (
                    <Button
                      key={person.id}
                      title={person.display_name}
                      quiet={person.id !== personId}
                      onPress={() => setPersonId(person.id)}
                    />
                  ))}
                </View>
                {people.isPending ? <ActivityIndicator /> : null}
                {people.isError ? (
                  <Button
                    title="Retry loading people"
                    quiet
                    onPress={() => void people.refetch()}
                  />
                ) : null}
                <Text style={{ color: c.secondary }}>
                  Includes suggested and confirmed matches.
                </Text>
              </View>
              <DateField
                title="From"
                value={from}
                maximumDate={to || new Date()}
                onChange={setFrom}
              />
              <DateField
                title="To"
                value={to}
                minimumDate={from}
                maximumDate={new Date()}
                onChange={setTo}
              />
              <Button
                title="Show photographs"
                onPress={() => setFilterOpen(false)}
              />
              <Button
                title="Clear filters"
                quiet
                onPress={() => {
                  setFrom(undefined);
                  setTo(undefined);
                  setPersonId(undefined);
                  setFilterOpen(false);
                }}
              />
            </ScrollView>
          </SafeAreaView>
        </SafeAreaProvider>
      </Modal>
    </SafeAreaView>
  );
}

function DateField({
  title,
  value,
  minimumDate,
  maximumDate,
  onChange,
}: {
  title: string;
  value?: Date;
  minimumDate?: Date;
  maximumDate?: Date;
  onChange(value: Date): void;
}) {
  const c = useColors();
  const [open, setOpen] = useState(false);
  return (
    <View style={{ gap: 10 }}>
      <Text style={{ color: c.secondary, fontWeight: "600" }}>{title}</Text>
      <Pressable
        accessibilityRole="button"
        onPress={() => setOpen(!open)}
        style={{ paddingVertical: 16 }}
      >
        <Text style={{ color: c.text, fontSize: 18 }}>
          {value?.toLocaleDateString() || "Any date"}
        </Text>
      </Pressable>
      {open ? (
        <DateTimePicker
          value={value || minimumDate || new Date()}
          mode="date"
          display={Platform.OS === "ios" ? "spinner" : "default"}
          minimumDate={minimumDate}
          maximumDate={maximumDate}
          themeVariant={c.dark ? "dark" : "light"}
          onChange={(event, date) => {
            if (Platform.OS !== "ios") setOpen(false);
            if (event.type === "set" && date) onChange(date);
          }}
        />
      ) : null}
    </View>
  );
}
