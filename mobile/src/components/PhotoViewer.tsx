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
import { useQueryClient } from "@tanstack/react-query";
import { ApiError, type Photo } from "@daily-mirror/api";
import { useSession, type ActiveSession } from "../session";
import { captureDate, photoDate } from "../gallery";
import { useCachedImage } from "../cache/use-cached-image";
import { IconButton, styles } from "../ui";

type PhotoViewerProps = {
  photos: Photo[];
  initialIndex: number;
  session: ActiveSession;
  onClose(): void;
};

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
  const [busy, setBusy] = useState(false);
  const list = useRef<FlatList<Photo>>(null);
  const queryClient = useQueryClient();
  const { expire } = useSession();
  const photo = photos[index];
  const dismiss = useRef({ zoomed, busy, onClose });
  dismiss.current = { zoomed, busy, onClose };
  const pan = useRef(
    PanResponder.create({
      onMoveShouldSetPanResponder: (_, gesture) =>
        !dismiss.current.zoomed &&
        !dismiss.current.busy &&
        Math.abs(gesture.dy) > 35 &&
        Math.abs(gesture.dx) < 25,
      onPanResponderRelease: (_, gesture) => {
        if (Math.abs(gesture.dy) > 90 && !dismiss.current.busy)
          dismiss.current.onClose();
      },
    }),
  ).current;
  const pageWidth = Math.max(1, width - insets.left - insets.right);
  const pageHeight = Math.max(1, height - insets.top - insets.bottom - 160);
  if (!photo) return null;

  const edit = async (action: "left" | "right" | "delete") => {
    if (busy) return;
    setBusy(true);
    try {
      if (action === "delete") {
        await session.api.deletePhoto(photo.id);
        onClose();
      } else await session.api.rotate(photo.id, action === "left" ? -90 : 90);
    } catch (caught) {
      if (caught instanceof ApiError && caught.status === 401) {
        await expire();
        return;
      }
      Alert.alert(
        "Could not update photograph",
        caught instanceof Error ? caught.message : "Please try again.",
      );
    } finally {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["photos"] }),
        queryClient.invalidateQueries({ queryKey: ["people"] }),
        queryClient.invalidateQueries({ queryKey: ["person-photos"] }),
      ]);
      setBusy(false);
    }
  };
  const confirmDelete = () =>
    Alert.alert(
      "Delete this photograph?",
      "This permanently removes the photograph from your archive and devices.",
      [
        { text: "Cancel", style: "cancel" },
        {
          text: "Delete photo",
          style: "destructive",
          onPress: () => void edit("delete"),
        },
      ],
    );
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
          if (choice === 1) void edit("right");
          if (choice === 2) void edit("left");
          if (choice === 3) confirmDelete();
        },
      );
    } else {
      Alert.alert("Photo options", undefined, [
        { text: "Rotate clockwise", onPress: () => void edit("right") },
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
    if (next < 0 || next >= photos.length || zoomed || busy) return;
    list.current?.scrollToIndex({ index: next, animated: true });
    setIndex(next);
    setZoomed(false);
  };
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
            disabled={busy}
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
            scrollEnabled={!zoomed && !busy}
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
                onZoom={setZoomed}
                onToggleControls={() => setControls((value) => !value)}
              />
            )}
          />
        </View>
        <View
          style={[
            styles.row,
            {
              height: 80,
              justifyContent: "space-evenly",
              opacity: controls ? 1 : 0,
            },
          ]}
          pointerEvents={controls ? "auto" : "none"}
        >
          <IconButton
            icon="chevron-back"
            label="Newer photograph"
            color="white"
            disabled={index === 0 || zoomed || busy}
            onPress={() => navigate(index - 1)}
          />
          {busy ? (
            <ActivityIndicator color="white" />
          ) : (
            <Text style={{ color: "#9AAA9E", fontSize: 12 }}>
              Pinch to explore
            </Text>
          )}
          <IconButton
            icon="chevron-forward"
            label="Older photograph"
            color="white"
            disabled={index === photos.length - 1 || zoomed || busy}
            onPress={() => navigate(index + 1)}
          />
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
  onZoom,
  onToggleControls,
}: {
  photo: Photo;
  active: boolean;
  session: ActiveSession;
  width: number;
  height: number;
  onZoom(value: boolean): void;
  onToggleControls(): void;
}) {
  const scroll = useRef<ScrollView>(null);
  const scale = useRef(1);
  const lastTap = useRef(0);
  const tapTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [loaded, setLoaded] = useState(false);
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
          {thumbnail ? (
            <Image
              source={{ uri: thumbnail }}
              cachePolicy="memory"
              contentFit="contain"
              style={{ position: "absolute", width, height }}
            />
          ) : null}
          {original.uri ? (
            <Image
              source={{ uri: original.uri }}
              cachePolicy="memory"
              contentFit="contain"
              transition={180}
              style={{ width, height }}
              onLoad={() => {
                setLoaded(true);
              }}
              onError={original.onError}
            />
          ) : null}
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
