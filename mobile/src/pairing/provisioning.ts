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
  parseProvisioningResult,
  type ProvisioningPayload,
  type ProvisioningResult,
} from "./contract";

/**
 * The only part of the Espressif library the pairing flow touches. Keeping it
 * behind an interface means the step machine can be driven by a fake in tests,
 * and that the BLE/SoftAP choice lives entirely in `provisionerFor()`.
 */
export type PairingDevice = {
  readonly name: string;
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

function wrap(device: ESPDevice, softApPassword: string | null): PairingDevice {
  return {
    name: device.name,
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
  return {
    transport,
    // Only SoftAP on iOS makes the user leave the app for Settings.
    needsManualJoin: transport === "softap" && Platform.OS === "ios",
    async search() {
      const { ESPProvisionManager, ESPSecurity, ESPTransport } = lib();
      const devices = await ESPProvisionManager.searchESPDevices(
        DEVICE_PREFIX,
        transport === "ble" ? ESPTransport.ble : ESPTransport.softap,
        ESPSecurity.secure2,
      );
      return devices.map((device) => wrap(device, softApPassword));
    },
    stopSearch() {
      try {
        lib().ESPProvisionManager.stopESPDevicesSearch();
      } catch {
        // Stopping a search that never started is not worth surfacing.
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
