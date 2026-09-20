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
import { STORE_BUILD } from "../store-build";

/** The small pill used for "You", "Admin" and "Has account". */
function Badge({
  label,
  color,
  background,
}: {
  label: string;
  color: string;
  background: string;
}) {
  return (
    <View
      style={{
        backgroundColor: background,
        borderRadius: 8,
        paddingHorizontal: 8,
        paddingVertical: 3,
      }}
    >
      <Text style={{ color, fontSize: 12, fontWeight: "700" }}>{label}</Text>
    </View>
  );
}

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
  /** The household name being edited, or null when the title is just read. */
  const [householdName, setHouseholdName] = useState<string | null>(null);
  const [renameError, setRenameError] = useState("");
  /** The person an admin tried to invite, while invites are still unbuilt. */
  const [inviting, setInviting] = useState<string | null>(null);
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

  const rename = useMutation({
    mutationFn: (displayName: string) =>
      session.api.renameHousehold(displayName),
    onSuccess: async () => {
      setHouseholdName(null);
      setRenameError("");
      await client.invalidateQueries({ queryKey: ["household"] });
    },
    onError: (caught) =>
      setRenameError(
        caught instanceof ApiError
          ? caught.status === 403
            ? "Only an administrator can rename this household."
            : caught.message
          : "Could not rename this household. Please try again.",
      ),
  });

  const submitHouseholdName = () => {
    const trimmed = (householdName ?? "").trim();
    if (!trimmed) {
      setRenameError("Enter a name for your household.");
      return;
    }
    setRenameError("");
    rename.mutate(trimmed);
  };

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
  /**
   * A 404 means this server build has no household route at all, which is a
   * stale deployment rather than anything the person can fix by retrying.
   */
  const serverTooOld =
    household.error instanceof ApiError && household.error.status === 404;
  const isAdmin = household.data?.role === "admin";

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
            <View style={[styles.row, { gap: 10, flexShrink: 1 }]}>
              <Text
                accessibilityRole="header"
                style={[styles.title, { color: c.text, flexShrink: 1 }]}
              >
                {household.data?.display_name ?? "Your household"}
              </Text>
              {isAdmin ? (
                <Badge label="Admin" color={c.accent} background={c.tint} />
              ) : null}
            </View>
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
              Five photos from five angles teach the cameras a face, so new
              photos of that person are tagged automatically.
            </Text>

            {/* Only an administrator may rename, and the server enforces it. */}
            {household.isSuccess && isAdmin ? (
              householdName === null ? (
                <Pressable
                  accessibilityRole="button"
                  accessibilityLabel="Rename household"
                  onPress={() => {
                    setHouseholdName(household.data.display_name);
                    setRenameError("");
                  }}
                  style={({ pressed }) => [
                    styles.row,
                    { gap: 8, opacity: pressed ? 0.6 : 1 },
                  ]}
                >
                  <Ionicons name="create-outline" size={18} color={c.accent} />
                  <Text style={{ color: c.accent, fontWeight: "600" }}>
                    Rename household
                  </Text>
                </Pressable>
              ) : (
                <View style={[styles.card, { backgroundColor: c.card }]}>
                  <Text style={{ color: c.text, fontWeight: "600" }}>
                    Household name
                  </Text>
                  <TextInput
                    accessibilityLabel="Household name"
                    style={input}
                    value={householdName}
                    onChangeText={setHouseholdName}
                    editable={!rename.isPending}
                    autoCapitalize="words"
                    autoCorrect={false}
                    autoFocus
                    maxLength={80}
                    returnKeyType="done"
                    onSubmitEditing={submitHouseholdName}
                    placeholder="Your household's name"
                    placeholderTextColor={c.secondary}
                  />
                  {renameError ? (
                    <Text accessibilityRole="alert" style={{ color: c.danger }}>
                      {renameError}
                    </Text>
                  ) : null}
                  <Button
                    title="Save"
                    busy={rename.isPending}
                    onPress={submitHouseholdName}
                  />
                  <Button
                    title="Cancel"
                    quiet
                    onPress={() => {
                      setHouseholdName(null);
                      setRenameError("");
                    }}
                  />
                </View>
              )
            ) : null}

            {household.isPending ? (
              <Text style={{ color: c.secondary }}>
                Loading your household…
              </Text>
            ) : household.isError ? (
              <View style={[styles.card, { backgroundColor: c.card }]}>
                <Text style={{ color: c.text, lineHeight: 23 }}>
                  {missingHousehold
                    ? "This account is not linked to a household yet. Ask an administrator to add you, or create a new account."
                    : serverTooOld
                      ? "This server is running an older version of Daily Mirror that does not have households yet. Update the server, then try again."
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
                        <Badge
                          label="You"
                          color={c.accent}
                          background={c.tint}
                        />
                      ) : person.account === "linked" ? (
                        <Badge
                          label="Has account"
                          color={c.accent}
                          background={c.tint}
                        />
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
                      accessibilityLabel={`Take photos of ${person.display_name}`}
                      onPress={() => setCapturing(person)}
                    />
                    {/*
                      Someone without a login is perfectly normal here — young
                      children never get one. Invites are not built yet, so an
                      admin is told plainly rather than shown a dead end. A
                      shipped build hides the button entirely: App Review reads
                      an affordance whose only outcome is "coming soon" as an
                      unfinished feature.
                    */}
                    {!STORE_BUILD &&
                    isAdmin &&
                    !you &&
                    person.account !== "linked" ? (
                      <Button
                        title="Invite to sign in"
                        quiet
                        accessibilityLabel={`Invite ${person.display_name} to sign in`}
                        onPress={() => setInviting(person.display_name)}
                      />
                    ) : null}
                  </View>
                );
              })
            )}

            {household.isSuccess && isAdmin ? (
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

        <Modal
          visible={!!inviting}
          animationType="fade"
          transparent
          onRequestClose={() => setInviting(null)}
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
                Invites are coming soon
              </Text>
              <Text style={{ color: c.secondary, lineHeight: 23 }}>
                {inviting} does not have their own sign-in yet. Inviting someone
                by email is not built, so for now they can create an account and
                an administrator can link them.
              </Text>
              <Button title="Got it" onPress={() => setInviting(null)} />
            </View>
          </View>
        </Modal>

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
