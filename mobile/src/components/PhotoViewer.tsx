import { useEffect, useRef, useState } from "react";
import {
  ActivityIndicator,
  ActionSheetIOS,
  Platform,
  Alert,
  FlatList,
  Modal,
  PanResponder,
  Pressable,
  ScrollView,
  Text,
  View,
  useWindowDimensions,
} from "react-native";
import {
  SafeAreaProvider,
  useSafeAreaInsets,
} from "react-native-safe-area-context";
import { Image } from "expo-image";
import { StatusBar } from "expo-status-bar";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ApiError, type Photo, type PhotoFace } from "@daily-mirror/api";
import { useSession, type ActiveSession } from "../session";
import {
  captureDate,
  containedRect,
  photoDate,
  rotatedFitScale,
} from "../gallery";
import { useCachedImage } from "../cache/use-cached-image";
import { IconButton, styles } from "../ui";

type PhotoViewerProps = {
  photos: Photo[];
  initialIndex: number;
  session: ActiveSession;
  onClose(): void;
};

/** A rotation shown on the device before the server has saved it. The
 *  server answers with a new media revision (a new URL), which retires it. */
type PendingRotation = { id: string; url: string; degrees: number };

export function PhotoViewer(props: PhotoViewerProps) {
  return (
    <Modal
      visible
      animationType="fade"
      presentationStyle="fullScreen"
      onRequestClose={props.onClose}
      supportedOrientations={["portrait", "landscape-left", "landscape-right"]}
    >
      <SafeAreaProvider>
        <PhotoViewerContent {...props} />
      </SafeAreaProvider>
    </Modal>
  );
}

function PhotoViewerContent({
  photos,
  initialIndex,
  session,
  onClose,
}: PhotoViewerProps) {
  const { width, height } = useWindowDimensions();
  const insets = useSafeAreaInsets();
  const [index, setIndex] = useState(initialIndex);
  const [zoomed, setZoomed] = useState(false);
  const [controls, setControls] = useState(true);
  const [showFaces, setShowFaces] = useState(false);
  const [rotations, setRotations] = useState<PendingRotation[]>([]);
  const [saving, setSaving] = useState(0);
  const list = useRef<FlatList<Photo>>(null);
  const queryClient = useQueryClient();
  const { expire } = useSession();
  const photo = photos[index];
  const dismiss = useRef({ zoomed, onClose });
  dismiss.current = { zoomed, onClose };
  const pan = useRef(
    PanResponder.create({
      onMoveShouldSetPanResponder: (_, gesture) =>
        !dismiss.current.zoomed &&
        Math.abs(gesture.dy) > 35 &&
        Math.abs(gesture.dx) < 25,
      onPanResponderRelease: (_, gesture) => {
        if (Math.abs(gesture.dy) > 90) dismiss.current.onClose();
      },
    }),
  ).current;
  const pageWidth = Math.max(1, width - insets.left - insets.right);
  const pageHeight = Math.max(1, height - insets.top - insets.bottom - 160);
  const faces = useQuery({
    queryKey: ["photo-faces", photo?.id, photo?.url],
    queryFn: ({ signal }) => session.api.photoFaces(photo!.id, signal),
    enabled: showFaces && !!photo,
    staleTime: 60_000,
  });
  useEffect(() => {
    if (faces.error instanceof ApiError && faces.error.status === 401)
      void expire();
  }, [faces.error, expire]);
  if (!photo) return null;

  const failed = (caught: unknown, title: string) => {
    if (caught instanceof ApiError && caught.status === 401) {
      void expire();
      return;
    }
    Alert.alert(
      title,
      caught instanceof Error ? caught.message : "Please try again.",
    );
  };
  const refreshArchive = () =>
    Promise.all([
      queryClient.invalidateQueries({ queryKey: ["photos"] }),
      queryClient.invalidateQueries({ queryKey: ["people"] }),
      queryClient.invalidateQueries({ queryKey: ["person-photos"] }),
    ]);

  // Turn the picture now; the server catches up in the background.
  const rotate = (degrees: -90 | 90) => {
    const target = photo;
    setRotations((current) => {
      const existing = current.find(
        (item) => item.id === target.id && item.url === target.url,
      );
      const rest = current.filter((item) => item !== existing);
      return [
        ...rest,
        {
          id: target.id,
          url: target.url,
          degrees: (existing?.degrees ?? 0) + degrees,
        },
      ];
    });
    setSaving((count) => count + 1);
    void (async () => {
      try {
        await session.api.rotate(target.id, degrees);
        await refreshArchive();
      } catch (caught) {
        setRotations((current) =>
          current.filter((item) => item.id !== target.id),
        );
        failed(caught, "Could not rotate photograph");
      } finally {
        setSaving((count) => count - 1);
      }
    })();
  };

  // Leave the viewer and the grid at once; the request finishes on its own.
  const remove = () => {
    const target = photo;
    queryClient.setQueryData<Photo[]>(["photos"], (current) =>
      current?.filter((item) => item.id !== target.id),
    );
    onClose();
    void (async () => {
      try {
        await session.api.deletePhoto(target.id);
      } catch (caught) {
        if (!(caught instanceof ApiError && caught.status === 404))
          failed(caught, "Could not delete photograph");
      } finally {
        await refreshArchive();
      }
    })();
  };
  const confirmDelete = () =>
    Alert.alert(
      "Delete this photograph?",
      "This permanently removes the photograph from your archive and devices.",
      [
        { text: "Cancel", style: "cancel" },
        { text: "Delete photo", style: "destructive", onPress: remove },
      ],
    );

  const excluded = !!photo.flipbook_excluded;
  const setExcluded = (next: boolean) => {
    const target = photo;
    const update = (value: boolean) =>
      queryClient.setQueryData<Photo[]>(["photos"], (current) =>
        current?.map((item) =>
          item.id === target.id ? { ...item, flipbook_excluded: value } : item,
        ),
      );
    update(next);
    void (async () => {
      try {
        await session.api.setFlipbookMembership(target.id, !next);
        await Promise.all([
          queryClient.invalidateQueries({ queryKey: ["people"] }),
          queryClient.invalidateQueries({ queryKey: ["person-photos"] }),
        ]);
      } catch (caught) {
        update(!next);
        failed(caught, "Could not update flipbooks");
      }
    })();
  };

  const showOptions = () => {
    if (Platform.OS === "ios") {
      ActionSheetIOS.showActionSheetWithOptions(
        {
          options: [
            "Cancel",
            "Rotate clockwise",
            "Rotate counterclockwise",
            "Delete photograph…",
          ],
          cancelButtonIndex: 0,
          destructiveButtonIndex: 3,
        },
        (choice) => {
          if (choice === 1) rotate(90);
          if (choice === 2) rotate(-90);
          if (choice === 3) confirmDelete();
        },
      );
    } else {
      Alert.alert("Photo options", undefined, [
        { text: "Rotate clockwise", onPress: () => rotate(90) },
        { text: "Rotate counterclockwise", onPress: () => rotate(-90) },
        {
          text: "Delete photograph…",
          style: "destructive",
          onPress: confirmDelete,
        },
        { text: "Cancel", style: "cancel" },
      ]);
    }
  };
  const navigate = (next: number) => {
    if (next < 0 || next >= photos.length || zoomed) return;
    list.current?.scrollToIndex({ index: next, animated: true });
    setIndex(next);
    setZoomed(false);
  };
  const status = saving
    ? "Saving rotation…"
    : showFaces
      ? faces.isPending
        ? "Looking for faces…"
        : faces.isError
          ? "Faces unavailable"
          : !faces.data?.analyzed
            ? "Not analyzed yet"
            : faces.data.faces.length === 0
              ? "No faces found"
              : `${faces.data.faces.length} ${faces.data.faces.length === 1 ? "face" : "faces"}`
      : excluded
        ? "Left out of flipbooks"
        : "";
  return (
    <>
      <StatusBar style="light" />
      <View
        style={{
          flex: 1,
          backgroundColor: "#070A08",
          paddingTop: insets.top,
          paddingBottom: insets.bottom,
          paddingLeft: insets.left,
          paddingRight: insets.right,
        }}
      >
        <View
          style={[
            styles.row,
            {
              height: 80,
              justifyContent: "space-between",
              paddingHorizontal: 14,
              opacity: controls ? 1 : 0,
            },
          ]}
          pointerEvents={controls ? "auto" : "none"}
        >
          <IconButton
            icon="close"
            label="Close photo viewer"
            onPress={onClose}
            color="white"
          />
          <View style={{ flex: 1, alignItems: "center" }}>
            <Text style={{ color: "white", fontSize: 16, fontWeight: "600" }}>
              {photoDate(photo.id)}
            </Text>
            <Text style={{ color: "#9AAA9E", fontSize: 13, marginTop: 5 }}>
              {captureDate(photo.id)?.toLocaleTimeString(undefined, {
                hour: "numeric",
                minute: "2-digit",
              })}{" "}
              · {index + 1} of {photos.length}
            </Text>
          </View>
          <IconButton
            icon="ellipsis-horizontal"
            label="Photo options"
            color="white"
            onPress={showOptions}
          />
        </View>
        <View style={{ flex: 1 }} {...pan.panHandlers}>
          <FlatList
            ref={list}
            key={`${pageWidth}-${pageHeight}`}
            data={photos}
            horizontal
            pagingEnabled
            scrollEnabled={!zoomed}
            initialScrollIndex={index}
            initialNumToRender={1}
            maxToRenderPerBatch={2}
            windowSize={3}
            getItemLayout={(_, i) => ({
              index: i,
              length: pageWidth,
              offset: pageWidth * i,
            })}
            keyExtractor={(item) => item.id}
            showsHorizontalScrollIndicator={false}
            onMomentumScrollEnd={(event) => {
              setIndex(
                Math.round(event.nativeEvent.contentOffset.x / pageWidth),
              );
              setZoomed(false);
            }}
            renderItem={({ item, index: itemIndex }) => (
              <ZoomPhoto
                key={`${item.id}:${item.url}`}
                photo={item}
                active={itemIndex === index}
                session={session}
                width={pageWidth}
                height={pageHeight}
                rotation={
                  rotations.find(
                    (pending) =>
                      pending.id === item.id && pending.url === item.url,
                  )?.degrees ?? 0
                }
                faces={
                  showFaces && itemIndex === index && faces.data?.analyzed
                    ? faces.data.faces
                    : undefined
                }
                onZoom={setZoomed}
                onToggleControls={() => setControls((value) => !value)}
              />
            )}
          />
        </View>
        <View
          style={{ height: 80, opacity: controls ? 1 : 0 }}
          pointerEvents={controls ? "auto" : "none"}
        >
          <View style={[styles.row, { justifyContent: "space-evenly" }]}>
            <IconButton
              icon="chevron-back"
              label="Newer photograph"
              color="white"
              disabled={index === 0 || zoomed}
              onPress={() => navigate(index - 1)}
            />
            <IconButton
              icon={excluded ? "book-outline" : "book"}
              label={
                excluded ? "Include in flipbooks" : "Leave out of flipbooks"
              }
              color={excluded ? "#9AAA9E" : "white"}
              onPress={() => setExcluded(!excluded)}
            />
            <IconButton
              icon={showFaces ? "people" : "people-outline"}
              label={
                showFaces ? "Hide face labels" : "Show who is in this photo"
              }
              color={showFaces ? "#A9D8B2" : "white"}
              onPress={() => setShowFaces((value) => !value)}
            />
            <IconButton
              icon="chevron-forward"
              label="Older photograph"
              color="white"
              disabled={index === photos.length - 1 || zoomed}
              onPress={() => navigate(index + 1)}
            />
          </View>
          <View style={[styles.row, { justifyContent: "center", gap: 8 }]}>
            {saving || (showFaces && faces.isPending) ? (
              <ActivityIndicator color="white" size="small" />
            ) : null}
            <Text
              accessibilityLiveRegion="polite"
              style={{ color: "#9AAA9E", fontSize: 12 }}
            >
              {status}
            </Text>
          </View>
        </View>
      </View>
    </>
  );
}

function ZoomPhoto({
  photo,
  active,
  session,
  width,
  height,
  rotation,
  faces,
  onZoom,
  onToggleControls,
}: {
  photo: Photo;
  active: boolean;
  session: ActiveSession;
  width: number;
  height: number;
  rotation: number;
  faces?: PhotoFace[];
  onZoom(value: boolean): void;
  onToggleControls(): void;
}) {
  const scroll = useRef<ScrollView>(null);
  const scale = useRef(1);
  const lastTap = useRef(0);
  const tapTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [loaded, setLoaded] = useState(false);
  const [natural, setNatural] = useState<{ width: number; height: number }>();
  const { expire } = useSession();
  const preview = useCachedImage(
    session.cache,
    photo.thumbnail_url ?? undefined,
    expire,
  );
  const original = useCachedImage(session.cache, photo.url, expire, active);
  const thumbnail = preview.uri;
  const error = original.failed;
  useEffect(() => () => clearTimeout(tapTimer.current), []);
  useEffect(() => {
    if (!active) {
      scroll.current?.scrollResponderZoomTo({
        x: 0,
        y: 0,
        width,
        height,
        animated: false,
      });
      scale.current = 1;
    } else onZoom(false);
  }, [active, width, height, onZoom]);
  const frame = { width, height };
  const shown = natural ? containedRect(frame, natural) : undefined;

  return (
    <View
      style={{ width, height, alignItems: "center", justifyContent: "center" }}
    >
      <ScrollView
        ref={scroll}
        style={{ width, height }}
        contentContainerStyle={{ width, height }}
        minimumZoomScale={1}
        maximumZoomScale={5}
        pinchGestureEnabled
        centerContent
        bouncesZoom
        bounces={false}
        alwaysBounceHorizontal={false}
        alwaysBounceVertical={false}
        showsHorizontalScrollIndicator={false}
        showsVerticalScrollIndicator={false}
        scrollEventThrottle={16}
        onScroll={(event) => {
          scale.current = event.nativeEvent.zoomScale || 1;
          if (active) onZoom(scale.current > 1.02);
        }}
      >
        <Pressable
          accessibilityRole="image"
          accessibilityLabel={`Photograph from ${photoDate(photo.id)}. Double tap or pinch to zoom.`}
          style={{ width, height }}
          onPress={(event) => {
            const now = Date.now();
            clearTimeout(tapTimer.current);
            if (now - lastTap.current < 300) {
              const zoom = scale.current > 1.02 ? 1 : 2.5;
              const w = width / zoom,
                h = height / zoom;
              scroll.current?.scrollResponderZoomTo({
                x:
                  zoom === 1
                    ? 0
                    : Math.max(0, event.nativeEvent.locationX - w / 2),
                y:
                  zoom === 1
                    ? 0
                    : Math.max(0, event.nativeEvent.locationY - h / 2),
                width: w,
                height: h,
                animated: true,
              });
              lastTap.current = 0;
            } else {
              lastTap.current = now;
              tapTimer.current = setTimeout(onToggleControls, 300);
            }
          }}
        >
          <View
            style={{
              width,
              height,
              transform: [
                { rotate: `${rotation}deg` },
                { scale: rotatedFitScale(frame, natural, rotation) },
              ],
            }}
          >
            {thumbnail ? (
              <Image
                source={{ uri: thumbnail }}
                cachePolicy="memory"
                contentFit="contain"
                style={{ position: "absolute", width, height }}
                onLoad={(event) => {
                  if (!natural) setNatural(event.source);
                }}
              />
            ) : null}
            {original.uri ? (
              <Image
                source={{ uri: original.uri }}
                cachePolicy="memory"
                contentFit="contain"
                transition={180}
                style={{ width, height }}
                onLoad={(event) => {
                  setLoaded(true);
                  setNatural(event.source);
                }}
                onError={original.onError}
              />
            ) : null}
            {faces && shown ? (
              <View
                pointerEvents="none"
                style={{
                  position: "absolute",
                  left: shown.x,
                  top: shown.y,
                  width: shown.width,
                  height: shown.height,
                }}
              >
                {faces.map((face) => (
                  <FaceLabel key={face.id} face={face} area={shown} />
                ))}
              </View>
            ) : null}
          </View>
        </Pressable>
      </ScrollView>
      {active && !loaded && !error ? (
        <ActivityIndicator
          pointerEvents="none"
          color="white"
          style={{ position: "absolute" }}
        />
      ) : null}
      {active && error ? (
        <Pressable
          accessibilityRole="button"
          onPress={() => {
            setLoaded(false);
            original.retry();
          }}
          style={{
            position: "absolute",
            padding: 18,
            backgroundColor: "#1E2922EE",
            borderRadius: 14,
          }}
        >
          <Text style={{ color: "white", textAlign: "center" }}>
            Full photograph unavailable.{"\n"}Tap to retry when connected.
          </Text>
        </Pressable>
      ) : null}
    </View>
  );
}

/** A box around one face with the name the pipeline settled on. Suggested
 *  matches read differently from confirmed ones so they are not mistaken for
 *  reviewed identities. */
function FaceLabel({
  face,
  area,
}: {
  face: PhotoFace;
  area: { width: number; height: number };
}) {
  const confirmed = face.identity_state === "confirmed" && !!face.person_name;
  const color = confirmed
    ? "#A9D8B2"
    : face.person_name
      ? "#F2D27A"
      : "#D7DDD8";
  const name = face.person_name
    ? confirmed
      ? face.person_name
      : `${face.person_name}?`
    : "Unknown";
  const top = face.bounds.y * area.height;
  return (
    <View
      accessible
      accessibilityLabel={`${name}${confirmed ? "" : ", suggested"}`}
      style={{
        position: "absolute",
        left: face.bounds.x * area.width,
        top,
        width: face.bounds.width * area.width,
        height: face.bounds.height * area.height,
        borderWidth: 2,
        borderColor: color,
        borderRadius: 4,
      }}
    >
      <View
        style={{
          position: "absolute",
          left: -2,
          ...(top > 26
            ? { bottom: "100%", marginBottom: 4 }
            : { top: "100%", marginTop: 4 }),
          paddingHorizontal: 7,
          paddingVertical: 3,
          borderRadius: 6,
          backgroundColor: color,
        }}
      >
        <Text
          numberOfLines={1}
          style={{ color: "#070A08", fontSize: 12, fontWeight: "700" }}
        >
          {name}
        </Text>
      </View>
    </View>
  );
}
