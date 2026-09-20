import { useEffect, useMemo, useRef, useState } from "react";
import {
  ActivityIndicator,
  Animated,
  Easing,
  Linking,
  Pressable,
  Text,
  View,
  type LayoutChangeEvent,
} from "react-native";
import { SafeAreaProvider, SafeAreaView } from "react-native-safe-area-context";
import { CameraView, useCameraPermissions, type CameraType } from "expo-camera";
import { File } from "expo-file-system";
import Ionicons from "@expo/vector-icons/Ionicons";
import type { HouseholdPerson } from "@daily-mirror/api";
import type { ActiveSession } from "../session";
import {
  ENROLLMENT_POSES,
  EnrollmentUploader,
  type EnrollmentSlot,
} from "../enrollment";
import { Button, IconButton, styles, useColors } from "../ui";

/** The outline is a little narrower than half the preview, roughly head-shaped. */
const OUTLINE_WIDTH = 0.44;
const OUTLINE_ASPECT = 1.3;

export function EnrollmentCapture({
  session,
  person,
  onDone,
}: {
  session: ActiveSession;
  person: HouseholdPerson;
  onDone(): void;
}) {
  const c = useColors();
  const [permission, requestPermission] = useCameraPermissions();
  const [facing, setFacing] = useState<CameraType>("front");
  const camera = useRef<CameraView>(null);
  const [preview, setPreview] = useState({ width: 0, height: 0 });
  const [target, setTarget] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const uploader = useMemo(
    () => new EnrollmentUploader(session.api, person.id),
    [session.api, person.id],
  );
  const [slots, setSlots] = useState<EnrollmentSlot[]>(uploader.state);

  useEffect(() => {
    const unsubscribe = uploader.subscribe(setSlots);
    return () => {
      unsubscribe();
      uploader.stopPolling();
    };
  }, [uploader]);

  useEffect(() => {
    if (!permission) return;
    if (!permission.granted && permission.canAskAgain) void requestPermission();
  }, [permission, requestPermission]);

  // The outline glides to the next pose and breathes so it reads as guidance.
  const offset = useRef(new Animated.ValueXY({ x: 0, y: 0 })).current;
  const pulse = useRef(new Animated.Value(1)).current;
  const pose = ENROLLMENT_POSES[target] ?? ENROLLMENT_POSES[0];

  useEffect(() => {
    if (!preview.width || !preview.height) return;
    Animated.spring(offset, {
      toValue: {
        x: (pose.x - 0.5) * preview.width,
        y: (pose.y - 0.5) * preview.height,
      },
      friction: 7,
      tension: 42,
      useNativeDriver: true,
    }).start();
  }, [pose, preview, offset]);

  useEffect(() => {
    const loop = Animated.loop(
      Animated.sequence([
        Animated.timing(pulse, {
          toValue: 1.05,
          duration: 1100,
          easing: Easing.inOut(Easing.quad),
          useNativeDriver: true,
        }),
        Animated.timing(pulse, {
          toValue: 1,
          duration: 1100,
          easing: Easing.inOut(Easing.quad),
          useNativeDriver: true,
        }),
      ]),
    );
    loop.start();
    return () => loop.stop();
  }, [pulse]);

  const complete = slots.every((slot) => slot.status === "enrolled");

  const capture = async (slotIndex: number) => {
    if (busy || !camera.current) return;
    setBusy(true);
    setError("");
    try {
      const photo = await camera.current.takePictureAsync({
        quality: 0.85,
        skipProcessing: false,
      });
      if (!photo?.uri) throw new Error("The camera returned no photo.");
      // `File` implements `Blob`, so `size` is the exact body length to declare.
      const size = new File(photo.uri).size ?? 0;
      if (!size) throw new Error("The captured photo was empty.");
      // Advance straight away; the upload continues in the background.
      const next = uploader.nextSlotIndex(slotIndex + 1);
      setTarget(next === -1 ? slotIndex : next);
      void uploader
        .upload(slotIndex, photo.uri, size, "image/jpeg", {
          // Cheap to know here, and it is what the tuning view wants first.
          ...(photo.width && photo.height
            ? { width: photo.width, height: photo.height }
            : {}),
          jpeg_quality: 85,
        })
        .then(() => uploader.pollStatus());
    } catch (caught) {
      setError(
        caught instanceof Error
          ? caught.message
          : "That photo could not be taken. Please try again.",
      );
    } finally {
      setBusy(false);
    }
  };

  if (!permission || (!permission.granted && permission.canAskAgain))
    return (
      <Shell>
        <ActivityIndicator size="large" color={c.accent} />
      </Shell>
    );

  if (!permission.granted)
    return (
      <Shell>
        <Ionicons name="camera-outline" size={44} color={c.secondary} />
        <Text style={[styles.subtitle, { color: c.text, textAlign: "center" }]}>
          Camera access is off
        </Text>
        <Text
          style={{ color: c.secondary, textAlign: "center", lineHeight: 23 }}
        >
          Daily Mirror needs the camera to take enrollment photos of{" "}
          {person.display_name}. You can turn it on in Settings.
        </Text>
        <Button
          title="Open Settings"
          onPress={() => void Linking.openSettings()}
        />
        <Button title="Not now" quiet onPress={onDone} />
      </Shell>
    );

  if (complete)
    return (
      <Shell>
        <Ionicons name="checkmark-circle" size={54} color={c.accent} />
        <Text style={[styles.title, { color: c.text, textAlign: "center" }]}>
          Ready
        </Text>
        <Text
          style={{ color: c.secondary, textAlign: "center", lineHeight: 24 }}
        >
          New camera photos of {person.display_name} will be tagged
          automatically.
        </Text>
        <Button title="Back to household" onPress={onDone} />
      </Shell>
    );

  const outlineWidth = preview.width * OUTLINE_WIDTH;
  return (
    <SafeAreaProvider>
      <SafeAreaView style={{ flex: 1, backgroundColor: "#000" }}>
        <View
          style={[
            styles.row,
            { justifyContent: "space-between", paddingHorizontal: 16 },
          ]}
        >
          <IconButton icon="close" label="Close capture" onPress={onDone} />
          <Text style={{ color: "#FFF", fontWeight: "600", flexShrink: 1 }}>
            {person.display_name} · {uploader.enrolledCount} of{" "}
            {ENROLLMENT_POSES.length}
          </Text>
          <IconButton
            icon="camera-reverse-outline"
            label="Switch camera"
            color="#FFF"
            onPress={() =>
              setFacing((current) => (current === "front" ? "back" : "front"))
            }
          />
        </View>
        <View
          style={{ flex: 1, overflow: "hidden" }}
          onLayout={(event: LayoutChangeEvent) =>
            setPreview({
              width: event.nativeEvent.layout.width,
              height: event.nativeEvent.layout.height,
            })
          }
        >
          <CameraView ref={camera} style={{ flex: 1 }} facing={facing} />
          {preview.width ? (
            <Animated.View
              pointerEvents="none"
              style={{
                position: "absolute",
                left: (preview.width - outlineWidth) / 2,
                top: (preview.height - outlineWidth * OUTLINE_ASPECT) / 2,
                width: outlineWidth,
                height: outlineWidth * OUTLINE_ASPECT,
                borderWidth: 3,
                borderColor: "#FFFFFFDD",
                borderRadius: outlineWidth / 2,
                transform: [
                  { translateX: offset.x },
                  { translateY: offset.y },
                  { scale: pulse },
                ],
              }}
            />
          ) : null}
        </View>
        <View style={{ padding: 16, gap: 14 }}>
          <Text
            accessibilityLiveRegion="polite"
            style={{
              color: "#FFF",
              textAlign: "center",
              fontSize: 17,
              lineHeight: 24,
            }}
          >
            {error || pose.caption}
          </Text>
          <View
            style={[styles.row, { justifyContent: "space-between", gap: 8 }]}
          >
            {slots.map((slot, index) => (
              <Slot
                key={slot.pose.id}
                slot={slot}
                active={index === target}
                onPress={() => {
                  if (
                    slot.status === "uploading" ||
                    slot.status === "processing"
                  )
                    return;
                  if (slot.status !== "empty") uploader.retake(index);
                  setTarget(index);
                }}
              />
            ))}
          </View>
          <Pressable
            accessibilityRole="button"
            accessibilityLabel={`Take the ${pose.label.toLowerCase()} photo`}
            disabled={busy}
            onPress={() => void capture(target)}
            style={({ pressed }) => ({
              alignSelf: "center",
              width: 74,
              height: 74,
              borderRadius: 37,
              borderWidth: 4,
              borderColor: "#FFF",
              backgroundColor: pressed ? "#FFFFFF66" : "transparent",
              opacity: busy ? 0.5 : 1,
              alignItems: "center",
              justifyContent: "center",
            })}
          >
            {busy ? <ActivityIndicator color="#FFF" /> : null}
          </Pressable>
        </View>
      </SafeAreaView>
    </SafeAreaProvider>
  );
}

function Slot({
  slot,
  active,
  onPress,
}: {
  slot: EnrollmentSlot;
  active: boolean;
  onPress(): void;
}) {
  const c = useColors();
  const busy = slot.status === "uploading" || slot.status === "processing";
  const icon =
    slot.status === "enrolled"
      ? "checkmark"
      : slot.status === "retake" || slot.status === "failed"
        ? "refresh"
        : null;
  const tint =
    slot.status === "enrolled"
      ? c.accent
      : slot.status === "retake" || slot.status === "failed"
        ? c.danger
        : "#FFFFFF99";
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={`${slot.pose.label}: ${statusLabel(slot.status)}`}
      accessibilityState={{ disabled: busy, selected: active }}
      onPress={onPress}
      style={{ flex: 1, alignItems: "center", gap: 5 }}
    >
      <View
        style={{
          width: "100%",
          aspectRatio: 0.8,
          borderRadius: 10,
          borderWidth: active ? 2 : 1,
          borderColor: active ? "#FFF" : tint,
          backgroundColor: "#FFFFFF1A",
          alignItems: "center",
          justifyContent: "center",
        }}
      >
        {busy ? (
          <ActivityIndicator size="small" color="#FFF" />
        ) : icon ? (
          <Ionicons name={icon} size={20} color={tint} />
        ) : null}
      </View>
      <Text style={{ color: tint, fontSize: 10 }} numberOfLines={1}>
        {statusLabel(slot.status)}
      </Text>
    </Pressable>
  );
}

function statusLabel(status: EnrollmentSlot["status"]) {
  switch (status) {
    case "uploading":
      return "Uploading";
    case "processing":
      return "Processing";
    case "enrolled":
      return "Enrolled";
    case "retake":
      return "Retake";
    case "failed":
      return "Retry";
    default:
      return "Empty";
  }
}

function Shell({ children }: { children: React.ReactNode }) {
  const c = useColors();
  return (
    <SafeAreaProvider>
      <SafeAreaView style={{ flex: 1, backgroundColor: c.background }}>
        <View
          style={{
            flex: 1,
            padding: 32,
            gap: 18,
            alignItems: "center",
            justifyContent: "center",
          }}
        >
          {children}
        </View>
      </SafeAreaView>
    </SafeAreaProvider>
  );
}
