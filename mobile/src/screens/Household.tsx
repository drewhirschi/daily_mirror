import { useState } from "react";
import {
  KeyboardAvoidingView,
  Modal,
  Platform,
  Pressable,
  ScrollView,
  Text,
  TextInput,
  View,
} from "react-native";
import { SafeAreaProvider, SafeAreaView } from "react-native-safe-area-context";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import Ionicons from "@expo/vector-icons/Ionicons";
import {
  ApiError,
  type HouseholdPerson,
  type PersonEnrollment,
} from "@daily-mirror/api";
import type { ActiveSession } from "../session";
import { EnrollmentCapture } from "./EnrollmentCapture";
import { Button, IconButton, styles, useColors, useInputStyle } from "../ui";

function enrollmentLabel(enrollment: PersonEnrollment) {
  if (enrollment.enrolled) return "Enrolled";
  if (enrollment.enrolled_photos > 0)
    return `${enrollment.enrolled_photos} of ${enrollment.required_photos}`;
  return "Not set up";
}

export function Household({
  session,
  firstRun,
  onClose,
}: {
  session: ActiveSession;
  firstRun?: boolean;
  onClose(): void;
}) {
  const c = useColors();
  const input = useInputStyle();
  const client = useQueryClient();
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState("");
  const [error, setError] = useState("");
  /** The person just added, so we can offer their capture straight away. */
  const [offer, setOffer] = useState<HouseholdPerson | null>(null);
  const [capturing, setCapturing] = useState<HouseholdPerson | null>(null);

  const household = useQuery({
    queryKey: ["household"],
    queryFn: ({ signal }) => session.api.household(signal),
    staleTime: 15_000,
  });

  const addPerson = useMutation({
    mutationFn: (displayName: string) =>
      session.api.addHouseholdPerson({ display_name: displayName }),
    onSuccess: async (person) => {
      setName("");
      setAdding(false);
      setError("");
      setOffer(person);
      await client.invalidateQueries({ queryKey: ["household"] });
    },
    onError: (caught) =>
      setError(
        caught instanceof ApiError
          ? caught.status === 409
            ? "This household is full."
            : caught.message
          : "Could not add that person. Please try again.",
      ),
  });

  const submitName = () => {
    const trimmed = name.trim();
    if (!trimmed) {
      setError("Enter the person's name.");
      return;
    }
    setError("");
    addPerson.mutate(trimmed);
  };

  const missingHousehold =
    household.error instanceof ApiError && household.error.status === 409;

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
              Your household
            </Text>
            <IconButton
              icon="close"
              label="Close household"
              onPress={onClose}
            />
          </View>
          <ScrollView
            keyboardShouldPersistTaps="handled"
            contentContainerStyle={{ padding: 22, paddingBottom: 40, gap: 14 }}
          >
            {firstRun ? (
              <View
                style={[
                  styles.card,
                  { backgroundColor: c.tint, flexDirection: "row", gap: 14 },
                ]}
              >
                <Ionicons name="sparkles-outline" size={22} color={c.accent} />
                <Text style={{ color: c.text, lineHeight: 23, flex: 1 }}>
                  Welcome. Add everyone who lives here, then take five quick
                  photos of each face.
                </Text>
              </View>
            ) : null}
            <Text style={{ color: c.secondary, lineHeight: 23 }}>
              Five photos from five angles teach the mirror a face, so new
              photos of that person are tagged automatically.
            </Text>

            {household.isPending ? (
              <Text style={{ color: c.secondary }}>
                Loading your household…
              </Text>
            ) : household.isError ? (
              <View style={[styles.card, { backgroundColor: c.card }]}>
                <Text style={{ color: c.text, lineHeight: 23 }}>
                  {missingHousehold
                    ? "This account is not linked to a household yet. Ask an administrator to add you, or create a new account."
                    : "Your household could not be loaded."}
                </Text>
                {missingHousehold ? null : (
                  <Button
                    title="Try again"
                    quiet
                    onPress={() => void household.refetch()}
                  />
                )}
              </View>
            ) : (
              household.data.people.map((person) => {
                const you = person.id === household.data.self_person_id;
                return (
                  <View
                    key={person.id}
                    style={[styles.card, { backgroundColor: c.card }]}
                  >
                    <View style={[styles.row, { gap: 10 }]}>
                      <Text
                        style={[
                          styles.subtitle,
                          { color: c.text, flexShrink: 1 },
                        ]}
                      >
                        {person.display_name}
                      </Text>
                      {you ? (
                        <View
                          style={{
                            backgroundColor: c.tint,
                            borderRadius: 8,
                            paddingHorizontal: 8,
                            paddingVertical: 3,
                          }}
                        >
                          <Text
                            style={{
                              color: c.accent,
                              fontSize: 12,
                              fontWeight: "700",
                            }}
                          >
                            You
                          </Text>
                        </View>
                      ) : null}
                    </View>
                    <View style={[styles.row, { gap: 8 }]}>
                      <Ionicons
                        name={
                          person.enrollment.enrolled
                            ? "checkmark-circle"
                            : "ellipse-outline"
                        }
                        size={18}
                        color={
                          person.enrollment.enrolled ? c.accent : c.secondary
                        }
                      />
                      <Text
                        style={{
                          color: person.enrollment.enrolled
                            ? c.accent
                            : c.secondary,
                        }}
                      >
                        {enrollmentLabel(person.enrollment)}
                      </Text>
                    </View>
                    <Button
                      title={
                        person.enrollment.enrolled
                          ? "Take more photos"
                          : "Take photos"
                      }
                      quiet
                      onPress={() => setCapturing(person)}
                    />
                  </View>
                );
              })
            )}

            {household.isSuccess ? (
              adding ? (
                <View style={[styles.card, { backgroundColor: c.card }]}>
                  <Text style={{ color: c.text, fontWeight: "600" }}>
                    Who else lives here?
                  </Text>
                  <TextInput
                    accessibilityLabel="New person's name"
                    style={input}
                    value={name}
                    onChangeText={setName}
                    editable={!addPerson.isPending}
                    autoCapitalize="words"
                    autoCorrect={false}
                    autoFocus
                    returnKeyType="done"
                    onSubmitEditing={submitName}
                    placeholder="Their name"
                    placeholderTextColor={c.secondary}
                  />
                  {error ? (
                    <Text accessibilityRole="alert" style={{ color: c.danger }}>
                      {error}
                    </Text>
                  ) : null}
                  <Button
                    title="Add"
                    busy={addPerson.isPending}
                    onPress={submitName}
                  />
                  <Button
                    title="Cancel"
                    quiet
                    onPress={() => {
                      setAdding(false);
                      setName("");
                      setError("");
                    }}
                  />
                </View>
              ) : (
                <Pressable
                  accessibilityRole="button"
                  onPress={() => setAdding(true)}
                  style={({ pressed }) => [
                    styles.card,
                    styles.row,
                    {
                      backgroundColor: c.card,
                      gap: 12,
                      opacity: pressed ? 0.6 : 1,
                    },
                  ]}
                >
                  <Ionicons
                    name="add-circle-outline"
                    size={24}
                    color={c.accent}
                  />
                  <Text style={{ color: c.accent, fontWeight: "600" }}>
                    Add person
                  </Text>
                </Pressable>
              )
            ) : null}
          </ScrollView>
        </KeyboardAvoidingView>

        {/* Offer the guided capture immediately after a person is added. */}
        <Modal
          visible={!!offer}
          animationType="fade"
          transparent
          onRequestClose={() => setOffer(null)}
        >
          <View
            style={{
              flex: 1,
              backgroundColor: "#00000088",
              justifyContent: "center",
              padding: 28,
            }}
          >
            <View style={[styles.card, { backgroundColor: c.card }]}>
              <Text style={[styles.subtitle, { color: c.text }]}>
                Add photos of {offer?.display_name} now?
              </Text>
              <Text style={{ color: c.secondary, lineHeight: 23 }}>
                Five photos take about a minute and can be done any time.
              </Text>
              <Button
                title="Take photos"
                onPress={() => {
                  const person = offer;
                  setOffer(null);
                  setCapturing(person);
                }}
              />
              <Button title="Later" quiet onPress={() => setOffer(null)} />
            </View>
          </View>
        </Modal>

        <Modal
          visible={!!capturing}
          animationType="slide"
          presentationStyle="fullScreen"
          onRequestClose={() => setCapturing(null)}
        >
          {capturing ? (
            <EnrollmentCapture
              session={session}
              person={capturing}
              onDone={() => {
                setCapturing(null);
                void client.invalidateQueries({ queryKey: ["household"] });
              }}
            />
          ) : null}
        </Modal>
      </SafeAreaView>
    </SafeAreaProvider>
  );
}
