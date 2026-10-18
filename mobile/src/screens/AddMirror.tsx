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
  KeyboardAvoidingView,
  Platform,
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
import { listDevices, mintClaimToken } from "../pairing/api";
import { SOFTAP_PASSWORD, type ClaimTokenGrant } from "../pairing/contract";
import {
  HomeWifiNeeded,
  awaitConfirmation,
  awaitServerClaim,
  deviceBaseline,
  heldGrantUsable,
  type DeviceBaseline,
  initialPairingState,
  pairingReducer,
  provisionDevice,
  stepErrorDetail,
  stepErrorMessage,
  type SendingStage,
} from "../pairing/machine";
import {
  espProvisioner,
  softApDevice,
  type Provisioner,
} from "../pairing/provisioning";
import { Button, styles, useColors } from "../ui";
import { STORE_BUILD } from "../store-build";

const stageLabels: Record<SendingStage, string> = {
  token: "Getting a pairing code…",
  payload: "Telling the camera where home is…",
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
  /* Over SoftAP the phone has no internet from the moment it joins the
   * camera's network, so the claim token is minted on the home Wi-Fi in the
   * `prepare` step and held here, together with the household's devices as
   * they were then, until the server shows the camera has redeemed it. It is
   * never minted from the camera's network. */
  const heldGrant = useRef<ClaimTokenGrant | null>(null);
  const baseline = useRef<{ devices: DeviceBaseline | null; since: number }>({
    devices: null,
    since: 0,
  });
  const provisioner = useMemo<Provisioner>(() => espProvisioner(), []);
  const cancelled = useRef({ aborted: false });

  useEffect(() => {
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
          detail: stepErrorDetail(error, step),
          retryStep,
        });
    };
    void (async () => {
      try {
        if (step === "prepare") {
          heldGrant.current = null;
          const since = Date.now();
          const [grant, devices] = await Promise.all([
            mintClaimToken(session.api),
            // Not fatal: without it, "claimed since the code was minted" is
            // used instead.
            listDevices(session.api).catch(() => null),
          ]);
          heldGrant.current = grant;
          baseline.current = {
            devices: devices ? deviceBaseline(devices) : null,
            since,
          };
          if (live) dispatch({ type: "prepared" });
        } else if (step === "discover") {
          const devices = await provisioner.search();
          if (!devices.length) throw new Error("No camera answered.");
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
            mint: async () => {
              if (device.transport === "softap") {
                // Kept until the server shows it redeemed, so a resend after a
                // wrong password reuses it.
                const held = heldGrant.current;
                if (heldGrantUsable(held)) return held;
                throw new HomeWifiNeeded();
              }
              return mintClaimToken(session.api);
            },
            onStage: (stage) => live && dispatch({ type: "stage", stage }),
          });
          if (live) dispatch({ type: "awaiting" });
        } else if (step === "confirm" && device?.transport === "softap") {
          const result = await awaitServerClaim({
            listDevices: () => {
              // Short, so a request stuck on the network switch is retried.
              const controller = new AbortController();
              const timer = setTimeout(() => controller.abort(), 8_000);
              return listDevices(session.api, controller.signal).finally(() =>
                clearTimeout(timer),
              );
            },
            baseline: baseline.current.devices,
            since: baseline.current.since,
            device,
            signal: cancelled.current,
          });
          if (!live) return;
          if (result?.status === "claimed") heldGrant.current = null;
          if (result) dispatch({ type: "result", result });
          else dispatch({ type: "timeout" });
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
          error instanceof HomeWifiNeeded
            ? "prepare"
            : step === "sending" || step === "confirm"
              ? device?.transport === "softap"
                ? "join"
                : "sending"
              : step === "wifi" && device?.transport === "softap"
                ? "join"
                : step,
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
      <KeyboardAvoidingView
        style={{ flex: 1 }}
        behavior={Platform.OS === "ios" ? "padding" : undefined}
      >
        <ScrollView
          keyboardShouldPersistTaps="handled"
          contentContainerStyle={{
            padding: 22,
            paddingBottom: 40,
            gap: 14,
          }}
        >
          {step === "choose" ? (
            <>
              <Lede>Which camera do you have?</Lede>
              <Choice
                icon="bluetooth"
                title="The Bluetooth camera"
                detail="Set up from this app over Bluetooth."
                onPress={() =>
                  dispatch({
                    type: "begin",
                    needsManualJoin: provisioner.needsManualJoin,
                  })
                }
              />
              <Choice
                icon="wifi"
                title="ESP32-P4 (Wi-Fi setup)"
                detail="Set up over the camera's own Wi-Fi network."
                onPress={() => dispatch({ type: "useWifi" })}
              />
            </>
          ) : step === "prepare" ? (
            <>
              <WifiSteps current={0} />
              <Card title="Getting ready">
                {busy ? (
                  <Waiting label="Getting a pairing code… Stay on your home Wi-Fi." />
                ) : (
                  <Problem state={state} dispatch={dispatch} />
                )}
                <Button
                  title="Choose a different camera"
                  quiet
                  onPress={() => dispatch({ type: "choose" })}
                />
              </Card>
            </>
          ) : step === "join" ? (
            // A camera with no Bluetooth (the ESP32-P4) is set up over its
            // own Wi-Fi network, which the person joins in Settings.
            <>
              <WifiSteps current={1} />
              <Card title="Join the camera's Wi-Fi">
                <Text style={{ color: c.secondary, lineHeight: 23 }}>
                  Pairing code ready. Open Settings › Wi-Fi and join the network
                  that starts with “mirror-”, then come back here.
                </Text>
                <Text style={{ color: c.secondary }}>
                  Password:{" "}
                  <Text selectable style={{ color: c.text, fontWeight: "600" }}>
                    {SOFTAP_PASSWORD}
                  </Text>
                </Text>
                <Problem state={state} dispatch={dispatch} />
                <Button
                  title="I've joined it"
                  onPress={() =>
                    dispatch({ type: "select", device: softApDevice() })
                  }
                />
                <Button
                  title="Choose a different camera"
                  quiet
                  onPress={() => dispatch({ type: "choose" })}
                />
              </Card>
            </>
          ) : step === "discover" ? (
            <>
              <Lede>
                Plug the camera in, then hold its button for five seconds until
                the ring chases amber. It stays ready to pair for five minutes.
              </Lede>
              {busy ? (
                <Card title="Select your camera">
                  <Waiting label="Looking for cameras over Bluetooth…" />
                </Card>
              ) : (
                <>
                  {state.found.map((found) => (
                    <Pressable
                      key={found.name}
                      accessibilityRole="button"
                      onPress={() =>
                        dispatch({ type: "select", device: found })
                      }
                      style={({ pressed }) => [
                        styles.card,
                        styles.row,
                        {
                          backgroundColor: c.card,
                          gap: 14,
                          opacity: pressed ? 0.6 : 1,
                        },
                      ]}
                    >
                      <Ionicons
                        name="camera-outline"
                        size={24}
                        color={c.accent}
                      />
                      <Text
                        style={[styles.subtitle, { color: c.text, flex: 1 }]}
                      >
                        {found.name}
                      </Text>
                      <Ionicons
                        name="chevron-forward"
                        size={20}
                        color={c.secondary}
                      />
                    </Pressable>
                  ))}
                  <Problem state={state} dispatch={dispatch} />
                  <Button
                    title="Scan again"
                    quiet
                    onPress={() => dispatch({ type: "scan" })}
                  />
                  <Button
                    title="Choose a different camera"
                    quiet
                    onPress={() => dispatch({ type: "choose" })}
                  />
                </>
              )}
            </>
          ) : step === "wifi" ? (
            <>
              <Lede>
                The camera uses this network from now on. It cannot use networks
                that need a browser sign-in.
              </Lede>
              <Card title="Your home Wi-Fi">
                {busy ? (
                  <Waiting label="Asking the camera what it can see…" />
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
                      title="Send to the camera"
                      onPress={() => ssid && dispatch({ type: "send" })}
                    />
                  </>
                )}
              </Card>
            </>
          ) : step === "sending" ? (
            <Card title="Setting up">
              {busy ? (
                <Waiting label={stageLabels[state.stage]} />
              ) : (
                <Problem state={state} dispatch={dispatch} />
              )}
            </Card>
          ) : step === "confirm" && device?.transport === "softap" ? (
            <>
              <Lede>
                Your phone will reconnect to your home Wi-Fi. Press the camera's
                button once when its light blinks fast amber; you have thirty
                seconds from then. Keep this screen open.
              </Lede>
              <Card title="Press the button on the camera now">
                {busy ? (
                  <Waiting label="Waiting for the camera to show up in your household…" />
                ) : null}
                <Problem state={state} dispatch={dispatch} />
              </Card>
            </>
          ) : step === "confirm" ? (
            <>
              <Lede>
                The ring is pulsing amber. One short press within thirty seconds
                tells the camera this phone is yours.
              </Lede>
              <Card title="Press the button on the camera now">
                {busy ? <Waiting label="Waiting for the press…" /> : null}
                <Problem state={state} dispatch={dispatch} />
              </Card>
            </>
          ) : (
            <Card title="Your camera is ready">
              <Text style={{ color: c.secondary, lineHeight: 23 }}>
                {state.deviceName} joined your household. Press its button any
                time to take a photograph.
              </Text>
              <Button title="Done" onPress={onDone} />
            </Card>
          )}
        </ScrollView>
      </KeyboardAvoidingView>
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
      {state.detail && !STORE_BUILD ? (
        // Kept deliberately in development: on real hardware the wording above
        // hides the one line that says what actually failed. A shipped build
        // shows only the plain-language message.
        <Text selectable style={{ color: c.secondary, fontSize: 12 }}>
          {state.detail}
        </Text>
      ) : null}
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

const wifiSteps = [
  "Get a pairing code (stay on your home Wi-Fi).",
  `Open Settings › Wi-Fi and join “mirror-…” with the password ${SOFTAP_PASSWORD}, then come back.`,
  "Pick your home Wi-Fi and enter its password.",
  "When the camera's light blinks fast amber, press its button once.",
];

/** The four steps of a Wi-Fi setup, with the current one highlighted. */
function WifiSteps({ current }: { current: number }) {
  const c = useColors();
  return (
    <View style={{ gap: 8 }}>
      {wifiSteps.map((text, index) => (
        <Text
          key={text}
          style={{
            color: index === current ? c.text : c.secondary,
            fontWeight: index === current ? "600" : "400",
            lineHeight: 22,
          }}
        >
          {index + 1}. {text}
        </Text>
      ))}
    </View>
  );
}

function Choice({
  icon,
  title,
  detail,
  onPress,
}: {
  icon: "bluetooth" | "wifi";
  title: string;
  detail: string;
  onPress(): void;
}) {
  const c = useColors();
  return (
    <Pressable
      accessibilityRole="button"
      onPress={onPress}
      style={({ pressed }) => [
        styles.card,
        styles.row,
        { backgroundColor: c.card, gap: 14, opacity: pressed ? 0.6 : 1 },
      ]}
    >
      <Ionicons name={icon} size={24} color={c.accent} />
      <View style={{ flex: 1, gap: 4 }}>
        <Text style={[styles.subtitle, { color: c.text }]}>{title}</Text>
        <Text style={{ color: c.secondary }}>{detail}</Text>
      </View>
      <Ionicons name="chevron-forward" size={20} color={c.secondary} />
    </Pressable>
  );
}

/** The plain paragraph the household screen opens with. */
function Lede({ children }: { children: ReactNode }) {
  const c = useColors();
  return <Text style={{ color: c.secondary, lineHeight: 23 }}>{children}</Text>;
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
