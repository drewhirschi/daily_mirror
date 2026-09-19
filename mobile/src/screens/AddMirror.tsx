import {
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  ActivityIndicator,
  Linking,
  Pressable,
  ScrollView,
  Text,
  TextInput,
  View,
} from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";
import { useQueryClient } from "@tanstack/react-query";
import Ionicons from "@expo/vector-icons/Ionicons";
import type { ActiveSession } from "../session";
import { mintClaimToken } from "../pairing/api";
import {
  awaitConfirmation,
  initialPairingState,
  pairingReducer,
  provisionDevice,
  stepErrorMessage,
  type SendingStage,
} from "../pairing/machine";
import { espProvisioner, type Provisioner } from "../pairing/provisioning";
import { Button, styles, useColors } from "../ui";

const stageLabels: Record<SendingStage, string> = {
  token: "Getting a pairing code…",
  payload: "Telling the mirror where home is…",
  credentials: "Sending your Wi-Fi details…",
};

export function AddMirror({
  session,
  onDone,
}: {
  session: ActiveSession;
  onDone(): void;
}) {
  const c = useColors();
  const queryClient = useQueryClient();
  const [state, dispatch] = useReducer(pairingReducer, initialPairingState);
  const [password, setPassword] = useState("");
  const [manualSsid, setManualSsid] = useState("");
  const provisioner = useMemo<Provisioner>(() => espProvisioner(), []);
  const cancelled = useRef({ aborted: false });

  useEffect(() => {
    dispatch({ type: "begin", needsManualJoin: provisioner.needsManualJoin });
    const flag = cancelled.current;
    return () => {
      flag.aborted = true;
      provisioner.stopSearch();
    };
  }, [provisioner]);

  const { step, busy, device, ssid, attempt } = state;
  useEffect(() => {
    if (!busy) return;
    let live = true;
    const fail = (error: unknown, retryStep: typeof step) => {
      if (live)
        dispatch({
          type: "failed",
          message: stepErrorMessage(error, retryStep),
          retryStep,
        });
    };
    void (async () => {
      try {
        if (step === "discover") {
          const devices = await provisioner.search();
          if (!devices.length)
            throw new Error("No mirror answered on this network.");
          if (live) dispatch({ type: "found", devices });
        } else if (step === "wifi" && device) {
          await device.connect();
          const networks = await device.scanWifi().catch(() => []);
          if (live) dispatch({ type: "networks", networks });
        } else if (step === "sending" && device) {
          await provisionDevice({
            device,
            ssid,
            passphrase: password,
            mint: () => mintClaimToken(session.api),
            onStage: (stage) => live && dispatch({ type: "stage", stage }),
          });
          if (live) dispatch({ type: "awaiting" });
        } else if (step === "confirm" && device) {
          const result = await awaitConfirmation({
            device,
            signal: cancelled.current,
          });
          if (!live) return;
          if (result) dispatch({ type: "result", result });
          else dispatch({ type: "timeout" });
        }
      } catch (error) {
        fail(
          error,
          step === "sending" || step === "confirm" ? "sending" : step,
        );
      }
    })();
    return () => {
      live = false;
    };
    // `attempt` re-runs the same step after a retry.
  }, [step, busy, device, ssid, password, attempt, provisioner, session]);

  useEffect(() => {
    if (state.step === "success")
      void queryClient.invalidateQueries({ queryKey: ["devices"] });
  }, [state.step, queryClient]);

  return (
    <SafeAreaView
      edges={["left", "right"]}
      style={{ flex: 1, backgroundColor: c.background }}
    >
      <ScrollView contentContainerStyle={{ padding: 22, gap: 18 }}>
        {step === "instructions" ? (
          <Card title="Wake the mirror up">
            <Step
              n={1}
              text="Plug the mirror in and wait for its ring to glow."
            />
            <Step
              n={2}
              text="Hold the button for five seconds. The ring fills amber as you hold, then chases around in amber."
            />
            <Step
              n={3}
              text="Let go. The mirror stays ready to pair for five minutes."
            />
            <Button
              title="The ring is chasing amber"
              onPress={() =>
                dispatch({
                  type: "begin",
                  needsManualJoin: provisioner.needsManualJoin,
                })
              }
            />
          </Card>
        ) : step === "join" ? (
          <Card title="Join the mirror's Wi-Fi">
            <Text style={{ color: c.secondary, lineHeight: 23 }}>
              Open Settings › Wi-Fi on this iPhone and join the network that
              starts with “Mirror-”. Come back here once it is connected.
            </Text>
            <Button
              title="Open Wi-Fi settings"
              quiet
              onPress={() => void Linking.openSettings().catch(() => undefined)}
            />
            <Button
              title="I've joined it"
              onPress={() => dispatch({ type: "joined" })}
            />
          </Card>
        ) : step === "discover" ? (
          <Card title="Looking for your mirror">
            {busy ? (
              <Waiting label="Scanning for mirrors nearby…" />
            ) : (
              state.found.map((found) => (
                <Pressable
                  key={found.name}
                  accessibilityRole="button"
                  onPress={() => dispatch({ type: "select", device: found })}
                  style={({ pressed }) => [
                    styles.row,
                    {
                      gap: 12,
                      paddingVertical: 12,
                      opacity: pressed ? 0.6 : 1,
                    },
                  ]}
                >
                  <Ionicons name="tv-outline" size={22} color={c.accent} />
                  <Text style={{ color: c.text, fontSize: 17, flex: 1 }}>
                    {found.name}
                  </Text>
                  <Ionicons
                    name="chevron-forward"
                    size={18}
                    color={c.secondary}
                  />
                </Pressable>
              ))
            )}
            <Problem state={state} dispatch={dispatch} />
            {!busy && (
              <Button
                title="Scan again"
                quiet
                onPress={() => dispatch({ type: "scan" })}
              />
            )}
          </Card>
        ) : step === "wifi" ? (
          <Card title="Your home Wi-Fi">
            <Text style={{ color: c.secondary, lineHeight: 23 }}>
              The mirror uses this network from now on. It cannot use networks
              that need a browser sign-in.
            </Text>
            {busy ? (
              <Waiting label="Asking the mirror what it can see…" />
            ) : (
              <>
                {state.networks.map((network) => (
                  <Pressable
                    key={network.ssid}
                    accessibilityRole="button"
                    onPress={() =>
                      dispatch({ type: "ssid", ssid: network.ssid })
                    }
                    style={({ pressed }) => [
                      styles.row,
                      {
                        gap: 10,
                        paddingVertical: 10,
                        opacity: pressed ? 0.6 : 1,
                      },
                    ]}
                  >
                    <Ionicons
                      name={
                        network.ssid === ssid
                          ? "checkmark-circle"
                          : "wifi-outline"
                      }
                      size={20}
                      color={network.ssid === ssid ? c.accent : c.secondary}
                    />
                    <Text style={{ color: c.text, fontSize: 16, flex: 1 }}>
                      {network.ssid}
                    </Text>
                  </Pressable>
                ))}
                <Field
                  label="Network name"
                  value={ssid || manualSsid}
                  onChange={(text) => {
                    setManualSsid(text);
                    dispatch({ type: "ssid", ssid: text });
                  }}
                />
                <Field
                  label="Password"
                  value={password}
                  secure
                  onChange={setPassword}
                />
                <Problem state={state} dispatch={dispatch} />
                <Button
                  title="Send to the mirror"
                  onPress={() => ssid && dispatch({ type: "send" })}
                />
              </>
            )}
          </Card>
        ) : step === "sending" ? (
          <Card title="Setting up">
            {busy ? (
              <Waiting label={stageLabels[state.stage]} />
            ) : (
              <Problem state={state} dispatch={dispatch} />
            )}
          </Card>
        ) : step === "confirm" ? (
          <Card title="Press the button on the mirror now">
            <Text style={{ color: c.secondary, lineHeight: 23 }}>
              The ring is pulsing amber. One short press within thirty seconds
              tells the mirror this phone is yours.
            </Text>
            {busy ? <Waiting label="Waiting for the press…" /> : null}
            <Problem state={state} dispatch={dispatch} />
          </Card>
        ) : (
          <Card title="Your mirror is ready">
            <Text style={{ color: c.secondary, lineHeight: 23 }}>
              {state.deviceName} joined your household. Press its button any
              time to take a photograph.
            </Text>
            <Button title="Done" onPress={onDone} />
          </Card>
        )}
      </ScrollView>
    </SafeAreaView>
  );
}

function Problem({
  state,
  dispatch,
}: {
  state: ReturnType<typeof pairingReducer>;
  dispatch: (event: { type: "retry" }) => void;
}) {
  const c = useColors();
  if (!state.error) return null;
  return (
    <View style={{ gap: 12 }}>
      <Text style={{ color: c.danger, lineHeight: 23 }}>{state.error}</Text>
      {state.retryStep ? (
        <Button
          title="Try again"
          quiet
          onPress={() => dispatch({ type: "retry" })}
        />
      ) : null}
    </View>
  );
}

function Card({ title, children }: { title: string; children: ReactNode }) {
  const c = useColors();
  return (
    <View style={[styles.card, { backgroundColor: c.card }]}>
      <Text style={[styles.subtitle, { color: c.text }]}>{title}</Text>
      {children}
    </View>
  );
}

function Step({ n, text }: { n: number; text: string }) {
  const c = useColors();
  return (
    <View style={[styles.row, { gap: 12, alignItems: "flex-start" }]}>
      <Text style={{ color: c.accent, fontWeight: "700", fontSize: 16 }}>
        {n}
      </Text>
      <Text style={{ color: c.secondary, lineHeight: 23, flex: 1 }}>
        {text}
      </Text>
    </View>
  );
}

function Waiting({ label }: { label: string }) {
  const c = useColors();
  return (
    <View style={[styles.row, { gap: 12 }]}>
      <ActivityIndicator color={c.accent} />
      <Text style={{ color: c.secondary, flex: 1 }}>{label}</Text>
    </View>
  );
}

function Field({
  label,
  value,
  onChange,
  secure,
}: {
  label: string;
  value: string;
  onChange(text: string): void;
  secure?: boolean;
}) {
  const c = useColors();
  return (
    <View style={{ gap: 6 }}>
      <Text style={{ color: c.secondary, fontSize: 13 }}>{label}</Text>
      <TextInput
        value={value}
        onChangeText={onChange}
        autoCapitalize="none"
        autoCorrect={false}
        secureTextEntry={secure}
        style={{
          borderWidth: 1,
          borderColor: c.border,
          borderRadius: 12,
          paddingHorizontal: 14,
          paddingVertical: 12,
          color: c.text,
          fontSize: 16,
        }}
      />
    </View>
  );
}
