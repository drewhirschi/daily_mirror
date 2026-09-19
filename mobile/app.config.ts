import type { ExpoConfig } from "expo/config";

const config: ExpoConfig = {
  name: "Daily Mirror",
  slug: "daily-mirror",
  scheme: "dailymirror",
  version: "0.1.0",
  orientation: "default",
  userInterfaceStyle: "automatic",
  icon: "./assets/icon.png",
  ios: {
    bundleIdentifier: "app.dailymirror.ios",
    appleTeamId: "C9P58ZP4AQ",
    supportsTablet: true,
    associatedDomains: ["webcredentials:daily-mirror-pearl.vercel.app"],
    infoPlist: {
      ITSAppUsesNonExemptEncryption: false,
      NSLocalNetworkUsageDescription:
        "Connect to your Daily Mirror development server and to a mirror you are setting up on your local network.",
      NSLocationWhenInUseUsageDescription:
        "iOS needs location access to confirm this iPhone is joined to the mirror's own Wi-Fi network while you set it up.",
      NSCameraUsageDescription:
        "Take enrollment photos so Daily Mirror can recognise the people in your household.",
    },
  },
  android: {
    package: "app.dailymirror.android",
    permissions: ["CAMERA"],
    adaptiveIcon: {
      foregroundImage: "./assets/icon.png",
      backgroundColor: "#275D3B",
    },
  },
  plugins: [
    "expo-image",
    "expo-secure-store",
    "expo-status-bar",
    "expo-dev-client",
    "@react-native-community/datetimepicker",
    // SoftAP provisioning today; the same plugin covers BLE when the ESP32-P4
    // supports it upstream, so both permission sets are requested now.
    [
      "@orbital-systems/react-native-esp-idf-provisioning",
      {
        transport: "both",
        neverForLocation: true,
        bluetoothAlwaysPermission:
          "Allow Daily Mirror to find a nearby mirror while you set it up.",
        locationWhenInUsePermission:
          "iOS needs location access to confirm this iPhone is joined to the mirror's own Wi-Fi network while you set it up.",
        localNetworkPermission:
          "Allow Daily Mirror to talk to a mirror on your local network while you set it up.",
      },
    ],
    [
      "expo-camera",
      {
        cameraPermission:
          "Take enrollment photos so Daily Mirror can recognise the people in your household.",
        // Enrollment takes stills only, so no microphone or barcode support.
        microphonePermission: false,
        recordAudioAndroid: false,
        barcodeScannerEnabled: false,
      },
    ],
  ],
};

export default config;
