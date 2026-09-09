import { useEffect, useState } from "react";
import {
  KeyboardAvoidingView,
  Platform,
  ScrollView,
  Text,
  TextInput,
  View,
  Pressable,
} from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";
import Ionicons from "@expo/vector-icons/Ionicons";
import { passkeyErrorMessage } from "../passkey-login";
import { useSession } from "../session";
import { Button, IconButton, styles, useColors } from "../ui";

export function SignIn() {
  const {
    signIn,
    signInWithPasskey,
    lastServer,
    error: sessionError,
  } = useSession();
  const c = useColors();
  const [origin, setOrigin] = useState(lastServer);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [settings, setSettings] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => setOrigin(lastServer), [lastServer]);
  const submit = async () => {
    if (busy) return;
    if (!username.trim() || !password) {
      setError("Enter your username and password.");
      return;
    }
    setBusy(true);
    setError("");
    try {
      await signIn(origin, { username: username.trim(), password });
      setPassword("");
      setShowPassword(false);
    } catch (caught) {
      setError(
        caught instanceof Error
          ? caught.message
          : "Could not sign in. Please try again.",
      );
    } finally {
      setBusy(false);
    }
  };
  const submitPasskey = async () => {
    if (busy) return;
    if (!username.trim()) {
      setError("Enter your username to use a saved passkey.");
      return;
    }
    setBusy(true);
    setError("");
    try {
      await signInWithPasskey(origin, username.trim());
      setPassword("");
      setShowPassword(false);
    } catch (error) {
      setError(passkeyErrorMessage(error));
    } finally {
      setBusy(false);
    }
  };
  const input = {
    backgroundColor: c.card,
    color: c.text,
    borderWidth: 1,
    borderColor: c.border,
    borderRadius: 14,
    padding: 16,
    fontSize: 17,
  };
  return (
    <SafeAreaView style={{ flex: 1, backgroundColor: c.background }}>
      <KeyboardAvoidingView
        style={{ flex: 1 }}
        behavior={Platform.OS === "ios" ? "padding" : undefined}
      >
        <ScrollView
          keyboardShouldPersistTaps="handled"
          contentContainerStyle={{
            flexGrow: 1,
            justifyContent: "center",
            padding: 28,
          }}
        >
          <View
            style={{
              width: "100%",
              maxWidth: 420,
              alignSelf: "center",
              gap: 16,
            }}
          >
            <View
              style={{
                backgroundColor: c.tint,
                width: 76,
                height: 76,
                borderRadius: 24,
                alignItems: "center",
                justifyContent: "center",
                marginBottom: 16,
              }}
            >
              <Ionicons name="aperture-outline" color={c.accent} size={42} />
            </View>
            <Text style={[styles.title, { color: c.text }]}>Daily Mirror</Text>
            <Text
              style={{
                fontSize: 18,
                lineHeight: 27,
                color: c.secondary,
                marginBottom: 22,
              }}
            >
              A little moment, every day.{"\n"}Your photo archive, close at
              hand.
            </Text>
            <Text style={{ color: c.text, fontWeight: "600" }}>Username</Text>
            <TextInput
              accessibilityLabel="Username"
              style={input}
              value={username}
              onChangeText={setUsername}
              editable={!busy}
              autoCapitalize="none"
              autoCorrect={false}
              textContentType="username"
              autoComplete="username"
              returnKeyType="next"
              placeholder="Your username"
              placeholderTextColor={c.secondary}
            />
            <Text style={{ color: c.text, fontWeight: "600" }}>Password</Text>
            <View>
              <TextInput
                accessibilityLabel="Password"
                style={[input, { paddingRight: 58 }]}
                value={password}
                onChangeText={setPassword}
                editable={!busy}
                secureTextEntry={!showPassword}
                autoCapitalize="none"
                autoCorrect={false}
                textContentType="password"
                autoComplete="current-password"
                onSubmitEditing={() => void submit()}
                returnKeyType="go"
                placeholder="Your password"
                placeholderTextColor={c.secondary}
              />
              <View
                style={{
                  position: "absolute",
                  right: 6,
                  top: 0,
                  bottom: 0,
                  justifyContent: "center",
                }}
              >
                <IconButton
                  icon={showPassword ? "eye-off-outline" : "eye-outline"}
                  label={showPassword ? "Hide password" : "Show password"}
                  onPress={() => setShowPassword((visible) => !visible)}
                  color={c.secondary}
                />
              </View>
            </View>
            {error || sessionError ? (
              <Text
                accessibilityRole="alert"
                style={{ color: c.danger, lineHeight: 22 }}
              >
                {error || sessionError}
              </Text>
            ) : null}
            <Button title="Sign in" busy={busy} onPress={() => void submit()} />
            {Platform.OS === "ios" ? (
              <Button
                title="Sign in with a passkey"
                quiet
                busy={busy}
                onPress={() => void submitPasskey()}
              />
            ) : null}
            <Pressable
              onPress={() => setSettings(!settings)}
              accessibilityRole="button"
              style={{ padding: 14 }}
            >
              <Text style={{ color: c.secondary, textAlign: "center" }}>
                Server settings
              </Text>
            </Pressable>
            {settings ? (
              <TextInput
                accessibilityLabel="Server address"
                style={input}
                value={origin}
                onChangeText={setOrigin}
                editable={!busy}
                autoCapitalize="none"
                autoCorrect={false}
                keyboardType="url"
                placeholder="https://your-server"
                placeholderTextColor={c.secondary}
              />
            ) : null}
          </View>
        </ScrollView>
      </KeyboardAvoidingView>
    </SafeAreaView>
  );
}
