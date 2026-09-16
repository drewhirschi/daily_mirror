import { Platform } from "react-native";
import type {
  ESPDevice,
  ESPWifiList,
} from "@orbital-systems/react-native-esp-idf-provisioning";
import {
  DEVICE_PREFIX,
  PROVISIONING_ENDPOINT,
  parseProvisioningResult,
  type ProvisioningPayload,
  type ProvisioningResult,
} from "./contract";

/**
 * The only part of the Espressif library the pairing flow touches. Keeping it
 * behind an interface means the step machine can be driven by a fake in tests,
 * and that switching SoftAP for BLE later is a change in `espTransport()`.
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
  /** SoftAP today; BLE once the P4 supports it upstream. */
  readonly transport: "softap" | "ble";
  /** True when the user must join the mirror's Wi-Fi in Settings first. */
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
    connect: () => device.connect(null, softApPassword, null),
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
    async sendPayload(payload) {
      return parseProvisioningResult(
        await device.sendData(
          `/${PROVISIONING_ENDPOINT}`,
          JSON.stringify(payload),
        ),
      );
    },
    async sendWifiCredentials(ssid, passphrase) {
      const status = await device.provision(ssid, passphrase);
      // The library reports the device's own view of the join attempt.
      if (status?.status && !/success|connected|ok/i.test(status.status))
        throw new Error(`The mirror could not join Wi-Fi (${status.status}).`);
    },
    async readResult() {
      return parseProvisioningResult(
        await device.sendData(`/${PROVISIONING_ENDPOINT}`, ""),
      );
    },
    disconnect: () => device.disconnect(),
  };
}

/**
 * SoftAP provisioning. On iOS the phone has to be on the mirror's own Wi-Fi
 * network before anything here can reach it, so the flow asks the user to join
 * it in Settings; on Android the library joins for us.
 */
export function espProvisioner(
  softApPassword: string | null = null,
): Provisioner {
  return {
    transport: "softap",
    needsManualJoin: Platform.OS === "ios",
    async search() {
      const { ESPProvisionManager, ESPSecurity, ESPTransport } = lib();
      const devices = await ESPProvisionManager.searchESPDevices(
        DEVICE_PREFIX,
        ESPTransport.softap,
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
