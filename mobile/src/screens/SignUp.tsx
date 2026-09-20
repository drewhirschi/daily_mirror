import { useState } from "react";
import {
  KeyboardAvoidingView,
  Platform,
  ScrollView,
  Text,
  TextInput,
  View,
} from "react-native";
import { SafeAreaProvider, SafeAreaView } from "react-native-safe-area-context";
import { useSession } from "../session";
import { Button, IconButton, styles, useColors, useInputStyle } from "../ui";

/** The server enforces the same rule in `AuthStore::create_user`. */
export const MINIMUM_PASSWORD_LENGTH = 12;

export function SignUp({
  origin,
  onClose,
}: {
  origin: string;
  onClose(): void;
}) {
  const { signUp } = useSession();
  const c = useColors();
  const input = useInputStyle();
  const [username, setUsername] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const submit = async () => {
    if (busy) return;
    const trimmed = username.trim();
    const name = displayName.trim();
    if (!trimmed || !name) {
      setError("Enter a username and the name you want shown.");
      return;
    }
    if (password.length < MINIMUM_PASSWORD_LENGTH) {
      setError(
        `Choose a password of at least ${MINIMUM_PASSWORD_LENGTH} characters.`,
      );
      return;
    }
    setBusy(true);
    setError("");
    try {
      // On success the session becomes active and this screen unmounts with it.
      await signUp(origin, {
        username: trimmed,
        display_name: name,
        password,
      });
      setPassword("");
    } catch (caught) {
      setError(
        caught instanceof Error
          ? caught.message
          : "Could not create your account. Please try again.",
      );
      setBusy(false);
    }
  };

  const remaining = MINIMUM_PASSWORD_LENGTH - password.length;
  return (
    <SafeAreaProvider>
      <SafeAreaView style={{ flex: 1, backgroundColor: c.background }}>
        <KeyboardAvoidingView
          style={{ flex: 1 }}
          behavior={Platform.OS === "ios" ? "padding" : undefined}
        >
          <View
            style={[
              styles.row,
              { justifyContent: "space-between", paddingHorizontal: 22 },
            ]}
          >
            <Text
              accessibilityRole="header"
              style={[styles.title, { color: c.text, flexShrink: 1 }]}
            >
              Create an account
            </Text>
            <IconButton
              icon="close"
              label="Close sign up"
              onPress={onClose}
              disabled={busy}
            />
          </View>
          <ScrollView
            keyboardShouldPersistTaps="handled"
            contentContainerStyle={{ padding: 22, paddingBottom: 40, gap: 14 }}
          >
            <Text style={{ color: c.secondary, lineHeight: 23 }}>
              Your account creates a household. You can add everyone who lives
              with you next, and take a few photos so the cameras learn their
              faces.
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
              autoComplete="username-new"
              textContentType="username"
              returnKeyType="next"
              placeholder="A name to sign in with"
              placeholderTextColor={c.secondary}
            />
            <Text style={{ color: c.text, fontWeight: "600" }}>
              Display name
            </Text>
            <TextInput
              accessibilityLabel="Display name"
              style={input}
              value={displayName}
              onChangeText={setDisplayName}
              editable={!busy}
              autoCapitalize="words"
              autoComplete="name"
              textContentType="name"
              returnKeyType="next"
              placeholder="How your name appears"
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
                autoComplete="password-new"
                textContentType="newPassword"
                onSubmitEditing={() => void submit()}
                returnKeyType="go"
                placeholder="At least 12 characters"
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
            <Text
              style={{
                color: remaining > 0 ? c.secondary : c.accent,
                lineHeight: 22,
              }}
            >
              {remaining > 0
                ? `Use at least ${MINIMUM_PASSWORD_LENGTH} characters — ${remaining} to go.`
                : `Long enough. ${password.length} characters.`}
            </Text>
            {error ? (
              <Text
                accessibilityRole="alert"
                style={{ color: c.danger, lineHeight: 22 }}
              >
                {error}
              </Text>
            ) : null}
            <Button
              title="Create account"
              busy={busy}
              onPress={() => void submit()}
            />
            <Text
              style={{ color: c.secondary, fontSize: 13, lineHeight: 20 }}
              selectable
            >
              Signing up on {origin}
            </Text>
          </ScrollView>
        </KeyboardAvoidingView>
      </SafeAreaView>
    </SafeAreaProvider>
  );
}
