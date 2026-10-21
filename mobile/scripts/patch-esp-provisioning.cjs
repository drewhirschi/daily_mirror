/*
 * Patch the ESP provisioning bridge so SoftAP pairing works on iOS without the
 * Hotspot Configuration entitlement. Runs as this workspace's postinstall, so
 * every `npm ci` (including the one on the Mac that builds the app) applies it.
 *
 * Why: ESPProvision's ESPDevice.connect() for SoftAP always goes through
 * NEHotspotConfigurationManager to join the camera's network, and that fails
 * outright unless the app is signed with
 * com.apple.developer.networking.HotspotConfiguration. The ESP32-P4 camera has
 * no Bluetooth, so SoftAP is its only pairing transport. With this patch the
 * person joins the camera's `mirror-<hex>` network in Settings first, and the
 * bridge opens the provisioning session over HTTP directly: it reads
 * `proto-ver` (which carries the security scheme and capabilities that
 * connect() would have read) and then calls the public initialiseSession().
 * Everything after that - sendData, scanWifiList, provision - is unchanged.
 *
 * The address comes from the WifiBaseUrl Info.plist key, as ESPProvision's own
 * SoftAP transport does (see app.config.ts).
 *
 * Idempotent, and loud when the library has changed shape: a silent no-op
 * would ship an app whose SoftAP pairing quietly fails again.
 */
const fs = require("node:fs");
const path = require("node:path");

const MARKER = "daily-mirror patch: SoftAP without NEHotspotConfiguration";

let pkg;
try {
  pkg = require.resolve(
    "@orbital-systems/react-native-esp-idf-provisioning/package.json",
    { paths: [path.join(__dirname, "..")] },
  );
} catch {
  // Not installed (for instance a server-only install); nothing to patch.
  process.exit(0);
}
const file = path.join(path.dirname(pkg), "ios", "EspIdfProvisioning.swift");
let source = fs.readFileSync(file, "utf8");
if (source.includes(MARKER)) process.exit(0);

const anchor = `    @objc(connect:resolve:reject:)
    func connect(deviceName: String, resolve: @escaping RCTPromiseResolveBlock, reject: @escaping RCTPromiseRejectBlock) {
        guard let espDevice = self.espDevices[deviceName] else {
            reject("error", "No ESP device found. Call createESPDevice first.", nil)
            return
        }
`;
if (!source.includes(anchor)) {
  console.error(
    `patch-esp-provisioning: ${file} no longer matches; update mobile/scripts/patch-esp-provisioning.cjs`,
  );
  process.exit(1);
}

const helper = `    // ${MARKER}.
    // See mobile/scripts/patch-esp-provisioning.cjs.
    private func connectJoinedSoftAP(espDevice: ESPDevice, resolve: @escaping RCTPromiseResolveBlock, reject: @escaping RCTPromiseRejectBlock) {
        let baseUrl = Bundle.main.infoDictionary?["WifiBaseUrl"] as? String ?? "192.168.4.1:80"
        let transport = ESPSoftAPTransport(baseUrl: baseUrl)
        espDevice.espSoftApTransport = transport
        transport.SendConfigData(path: "proto-ver", data: Data("ESP".utf8)) { response, error in
            guard error == nil, let response = response else {
                reject("error", "Could not reach the camera at \\(baseUrl). Is this phone on its Wi-Fi? \\(error?.localizedDescription ?? "")", nil)
                return
            }
            if let info = try? JSONSerialization.jsonObject(with: response, options: []) as? NSDictionary {
                espDevice.versionInfo = info
                if let prov = info["prov"] as? NSDictionary, let caps = prov["cap"] as? [String] {
                    espDevice.capabilities = caps
                }
            }
            DispatchQueue.main.async {
                var invoked = false
                espDevice.initialiseSession(sessionPath: nil) { status in
                    guard !invoked else { return }
                    invoked = true
                    switch status {
                    case .connected:
                        resolve(["status": "connected"])
                    case .failedToConnect(let error):
                        reject("error", error.description, nil)
                    case .disconnected:
                        reject("error", "Device disconnected.", nil)
                    }
                }
            }
        }
    }

`;

const branch = `        if espDevice.transport == .softap {
            // ${MARKER}.
            connectJoinedSoftAP(espDevice: espDevice, resolve: resolve, reject: reject)
            return
        }
`;

source = source.replace(anchor, helper + anchor + branch);
fs.writeFileSync(file, source);
console.log(`patch-esp-provisioning: patched ${file}`);
