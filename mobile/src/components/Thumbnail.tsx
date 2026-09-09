import { memo } from "react";
import { ActivityIndicator, Pressable, Text, View } from "react-native";
import { Image } from "expo-image";
import Ionicons from "@expo/vector-icons/Ionicons";
import { type Photo } from "@daily-mirror/api";
import type { DiskCache } from "../cache/disk-cache";
import { photoDate } from "../gallery";
import { useCachedImage } from "../cache/use-cached-image";
import { useColors } from "../ui";

export const Thumbnail = memo(function Thumbnail({
  photo,
  cache,
  size,
  onPress,
  onUnauthorized,
}: {
  photo: Photo;
  cache: DiskCache;
  size: number;
  onPress(id: string): void;
  onUnauthorized(): void;
}) {
  const c = useColors();
  const image = useCachedImage(
    cache,
    photo.thumbnail_url ?? undefined,
    onUnauthorized,
  );
  const { uri, failed: error } = image;
  return (
    <Pressable
      accessibilityRole="imagebutton"
      accessibilityLabel={`Open photograph from ${photoDate(photo.id)}`}
      onPress={() => onPress(photo.id)}
      style={({ pressed }) => ({
        width: size,
        height: size,
        backgroundColor: c.tint,
        opacity: pressed ? 0.7 : 1,
        overflow: "hidden",
        alignItems: "center",
        justifyContent: "center",
      })}
    >
      {uri ? (
        <Image
          source={{ uri }}
          recyclingKey={`${photo.id}:${photo.thumbnail_url}`}
          cachePolicy="memory"
          contentFit="cover"
          transition={0}
          style={{ width: size, height: size }}
          onError={image.onError}
        />
      ) : error ? (
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Retry thumbnail"
          onPress={image.retry}
          style={{ padding: 12 }}
        >
          <Ionicons
            name="cloud-offline-outline"
            size={20}
            color={c.secondary}
          />
        </Pressable>
      ) : photo.thumbnail_url ? (
        <ActivityIndicator color={c.secondary} size="small" />
      ) : (
        <View style={{ alignItems: "center", padding: 8, gap: 4 }}>
          <Ionicons name="image-outline" size={20} color={c.secondary} />
          {size > 90 ? (
            <Text style={{ color: c.secondary, fontSize: 11 }}>
              Preview processing
            </Text>
          ) : null}
        </View>
      )}
    </Pressable>
  );
});
