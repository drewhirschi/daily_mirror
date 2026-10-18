import { Platform } from "react-native";
import type {
  ESPDevice,
  ESPWifiList,
} from "@orbital-systems/react-native-esp-idf-provisioning";
import {
  DEVICE_PREFIX,
  PROVISIONING_ENDPOINT,
  PROVISIONING_POP,
  PROVISIONING_USERNAME,
  SOFTAP_PASSWORD,
  parseProvisioningResult,
  type ProvisioningPayload,
  type ProvisioningResult,
} from "./contract";

/** How long a SoftAP credential send is waited on before moving to confirm. */
const SOFTAP_PROVISION_WAIT_MS = 6_000;

/**
 * The only part of the Espressif library the pairing flow touches. Keeping it
 * behind an interface means the step machine can be driven by a fake in tests,
 * and that the BLE/SoftAP choice lives entirely in `provisionerFor()`.
 */
export type PairingDevice = {
  readonly name: string;
  /**
   * SoftAP means the phone is on the camera's own network, with no internet,
   * from `connect()` until `disconnect()`.
   */
  readonly transport: "softap" | "ble";
  connect(): Promise<void>;
  scanWifi(): Promise<WifiNetwork[]>;
  sendPayload(payload: ProvisioningPayload): Promise<ProvisioningResult>;
  sendWifiCredentials(ssid: string, passphrase: string): Promise<void>;
  readResult(): Promise<ProvisioningResult>;
  disconnect(): void;
};

export type WifiNetwork = { ssid: string; rssi: number; open: boolean };

export type Provisioner = {
  /** BLE is the default; SoftAP stays available as a bench fallback. */
  readonly transport: "softap" | "ble";
  /** True when the user must join the camera's Wi-Fi in Settings first. */
  readonly needsManualJoin: boolean;
  search(): Promise<PairingDevice[]>;
  stopSearch(): void;
};

/** Loaded lazily: the native module is absent in Node tests and in Expo Go. */
function lib() {
  return require("@orbital-systems/react-native-esp-idf-provisioning") as typeof import("@orbital-systems/react-native-esp-idf-provisioning");
}

function wrap(
  device: ESPDevice,
  softApPassword: string | null,
  transport: "softap" | "ble",
): PairingDevice {
  return {
    name: device.name,
    transport,
    // Security 2 (SRP6a) needs both halves of the agreed identity; the library
    // throws if either is null. The SoftAP password is unused over BLE.
    connect: () =>
      device.connect(PROVISIONING_POP, softApPassword, PROVISIONING_USERNAME),
    async scanWifi() {
      const list: ESPWifiList[] = await device.scanWifiList();
      const best = new Map<string, WifiNetwork>();
      for (const network of list) {
        if (!network.ssid) continue;
        const seen = best.get(network.ssid);
        if (!seen || seen.rssi < network.rssi)
          best.set(network.ssid, {
            ssid: network.ssid,
            rssi: network.rssi,
            open: network.auth === 0,
          });
      }
      return [...best.values()].sort((a, b) => b.rssi - a.rssi);
    },
    // The endpoint name is passed bare, with no leading slash. Over BLE
    // ESPProvision looks the path up in `configUUIDMap`, whose keys are the
    // characteristic user descriptions the device publishes ("prov-config",
    // "proto-ver", "daily-mirror"); "/daily-mirror" misses and the transport
    // fails with "BLE characteristic does not exist."
    async sendPayload(payload) {
      return parseProvisioningResult(
        await device.sendData(PROVISIONING_ENDPOINT, JSON.stringify(payload)),
      );
    },
    async sendWifiCredentials(ssid, passphrase) {
      if (transport === "softap") {
        // Over SoftAP the camera's access point follows its station onto the
        // home network's channel as it joins, so the phone drops off it for a
        // few seconds and the library's status poll fails with a transport
        // error. That says nothing about the join. Only a verdict from the
        // device itself is final here; anything else falls through to the
        // daily-mirror poll, which reports a failed join in its own words.
        // The library's status poll can also hang on the dead socket instead
        // of failing, which left the app on "Sending" while the camera's
        // 30 s button window ran out. The credentials are written in the
        // first second, so stop waiting after a few and let the confirm
        // screen ask the camera directly.
        const sent = device.provision(ssid, passphrase);
        sent.catch(() => undefined);
        try {
          await Promise.race([
            sent,
            new Promise((resolve) =>
              setTimeout(resolve, SOFTAP_PROVISION_WAIT_MS),
            ),
          ]);
        } catch (error) {
          const text = error instanceof Error ? error.message : String(error);
          if (/auth|not ?found/i.test(text))
            throw new Error(`The camera could not join Wi-Fi (${text}).`);
        }
        return;
      }
      const status = await device.provision(ssid, passphrase);
      // The library reports the device's own view of the join attempt.
      if (status?.status && !/success|connected|ok/i.test(status.status))
        throw new Error(`The camera could not join Wi-Fi (${status.status}).`);
    },
    // A poll carries "{}", never an empty string: protocomm decrypts the
    // request before the firmware ever sees it, and a zero-length security2
    // payload fails that decrypt and makes the device drop the BLE link
    // ("Invalid content received, killing connection").
    async readResult() {
      return parseProvisioningResult(
        await device.sendData(PROVISIONING_ENDPOINT, "{}"),
      );
    },
    disconnect: () => device.disconnect(),
  };
}

function provisionerFor(
  transport: "softap" | "ble",
  softApPassword: string | null,
): Provisioner {
  // The native stop traps (a Swift crash, not a JS error) when no search was
  // ever started, which is every SoftAP-only visit to Add a mirror.
  let searching = false;
  return {
    transport,
    // Only SoftAP on iOS makes the user leave the app for Settings.
    needsManualJoin: transport === "softap" && Platform.OS === "ios",
    async search() {
      const { ESPProvisionManager, ESPSecurity, ESPTransport } = lib();
      searching = true;
      const devices = await ESPProvisionManager.searchESPDevices(
        DEVICE_PREFIX,
        transport === "ble" ? ESPTransport.ble : ESPTransport.softap,
        ESPSecurity.secure2,
      );
      return devices.map((device) => wrap(device, softApPassword, transport));
    },
    stopSearch() {
      if (!searching) return;
      searching = false;
      try {
        lib().ESPProvisionManager.stopESPDevicesSearch();
      } catch {
        // A JS-side failure to stop is not worth surfacing.
      }
    },
  };
}

/**
 * The default: BLE provisioning. The camera advertises as `mirror-<hex>` and
 * the phone never leaves the app, so "select your camera" can be the first
 * step of the flow.
 */
export function espProvisioner(): Provisioner {
  return provisionerFor("ble", null);
}

/**
 * SoftAP provisioning, kept for bench work with firmware that does not raise a
 * BLE service. On iOS the phone has to be on the camera's own Wi-Fi network
 * first, so the flow asks the user to join it in Settings.
 */
export function softApProvisioner(
  softApPassword: string | null = null,
): Provisioner {
  return provisionerFor("softap", softApPassword);
}

/**
 * A camera reached over its own Wi-Fi network instead of Bluetooth: the
 * ESP32-P4, whose radio cannot run BLE provisioning. iOS cannot search for
 * SoftAP devices, and joining one from the app needs an entitlement this
 * build does not carry, so the person joins `mirror-<hex>` in Settings first
 * and `connect()` talks straight to the camera's address (the bridge patch in
 * mobile/scripts/patch-esp-provisioning.cjs). The name is only a handle here.
 */
export function softApDevice(name = `${DEVICE_PREFIX}wifi`): PairingDevice {
  const { ESPDevice, ESPSecurity, ESPTransport } = lib();
  return wrap(
    new ESPDevice({
      name,
      transport: ESPTransport.softap,
      security: ESPSecurity.secure2,
    }),
    SOFTAP_PASSWORD,
    "softap",
  );
}
