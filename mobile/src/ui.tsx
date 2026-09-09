import type { ComponentProps, ReactNode } from "react";
import {
  ActivityIndicator,
  Pressable,
  StyleSheet,
  Text,
  View,
  useColorScheme,
} from "react-native";
import Ionicons from "@expo/vector-icons/Ionicons";

export function useColors() {
  const dark = useColorScheme() === "dark";
  return {
    dark,
    background: dark ? "#121513" : "#F7F8F5",
    card: dark ? "#1F2521" : "#FFFFFF",
    text: dark ? "#F3F5F0" : "#1C2923",
    secondary: dark ? "#A5B1A8" : "#647469",
    border: dark ? "#343E36" : "#E0E6DE",
    accent: dark ? "#A9D8B2" : "#275D3B",
    tint: dark ? "#2D3F31" : "#E8F0E5",
    danger: dark ? "#FF9D97" : "#B83532",
  };
}

export function IconButton({
  icon,
  label,
  onPress,
  disabled,
  color,
}: {
  icon: ComponentProps<typeof Ionicons>["name"];
  label: string;
  onPress(): void;
  disabled?: boolean;
  color?: string;
}) {
  const colors = useColors();
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      disabled={disabled}
      onPress={onPress}
      style={({ pressed }) => [
        styles.iconButton,
        { opacity: disabled ? 0.3 : pressed ? 0.5 : 1 },
      ]}
    >
      <Ionicons name={icon} size={24} color={color || colors.accent} />
    </Pressable>
  );
}

export function Button({
  title,
  onPress,
  busy,
  quiet,
  danger,
}: {
  title: string;
  onPress(): void;
  busy?: boolean;
  quiet?: boolean;
  danger?: boolean;
}) {
  const c = useColors();
  return (
    <Pressable
      accessibilityRole="button"
      disabled={busy}
      onPress={onPress}
      style={({ pressed }) => [
        styles.button,
        {
          backgroundColor: quiet ? c.tint : c.accent,
          opacity: pressed || busy ? 0.65 : 1,
        },
      ]}
    >
      {busy ? (
        <ActivityIndicator color={quiet ? c.accent : c.background} />
      ) : (
        <Text
          style={{
            color: danger ? c.danger : quiet ? c.accent : c.background,
            fontSize: 16,
            fontWeight: "600",
          }}
        >
          {title}
        </Text>
      )}
    </Pressable>
  );
}

export function Empty({
  title,
  detail,
  children,
}: {
  title: string;
  detail: string;
  children?: ReactNode;
}) {
  const c = useColors();
  return (
    <View style={styles.empty}>
      <Ionicons name="images-outline" size={42} color={c.secondary} />
      <Text style={[styles.subtitle, { color: c.text, textAlign: "center" }]}>
        {title}
      </Text>
      <Text
        style={{
          color: c.secondary,
          textAlign: "center",
          fontSize: 16,
          lineHeight: 24,
        }}
      >
        {detail}
      </Text>
      {children}
    </View>
  );
}

export const styles = StyleSheet.create({
  title: { fontSize: 34, fontWeight: "700", letterSpacing: -1.1 },
  subtitle: { fontSize: 20, fontWeight: "600", letterSpacing: -0.3 },
  iconButton: {
    minWidth: 46,
    minHeight: 46,
    alignItems: "center",
    justifyContent: "center",
  },
  button: {
    minHeight: 50,
    borderRadius: 15,
    alignItems: "center",
    justifyContent: "center",
    paddingHorizontal: 20,
  },
  empty: { padding: 32, gap: 16, alignItems: "center", paddingTop: 80 },
  row: { flexDirection: "row", alignItems: "center" },
  card: { borderRadius: 20, padding: 20, gap: 14 },
});
