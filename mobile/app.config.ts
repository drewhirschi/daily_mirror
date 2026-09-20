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
    // Signed by the paid Apple Developer Program team, so Associated Domains is
    // available. Keep this in step with NATIVE_PASSKEYS_ENABLED in
    // src/auth-features.ts and the App ID served by the server's
    // /.well-known/apple-app-site-association route.
    associatedDomains: ["webcredentials:daily-mirror-pearl.vercel.app"],
    infoPlist: {
      ITSAppUsesNonExemptEncryption: false,
      NSLocalNetworkUsageDescription:
        "Connect to your Daily Mirror development server and to a camera you are setting up on your local network.",
      NSBluetoothAlwaysUsageDescription:
        "Daily Mirror uses Bluetooth to find and set up a nearby camera.",
      NSBluetoothPeripheralUsageDescription:
        "Daily Mirror uses Bluetooth to find and set up a nearby camera.",
      NSLocationWhenInUseUsageDescription:
        "iOS needs location access to confirm this iPhone is joined to a camera's own Wi-Fi network while you set it up.",
      NSCameraUsageDescription:
        "Take enrollment photos so Daily Mirror can recognise the people in your household.",
    },
  },
  android: {
    package: "app.dailymirror.android",
    // BLUETOOTH_SCAN (with neverForLocation) and BLUETOOTH_CONNECT are added by
    // the esp-idf-provisioning config plugin below; declaring them again here
    // would fight its manifest attributes.
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
    // BLE is the provisioning transport the app uses; SoftAP stays available as
    // a bench fallback, so both permission sets are requested.
    [
      "@orbital-systems/react-native-esp-idf-provisioning",
      {
        transport: "both",
        neverForLocation: true,
        bluetoothAlwaysPermission:
          "Allow Daily Mirror to find a nearby camera while you set it up.",
        locationWhenInUsePermission:
          "iOS needs location access to confirm this iPhone is joined to a camera's own Wi-Fi network while you set it up.",
        localNetworkPermission:
          "Allow Daily Mirror to talk to a camera on your local network while you set it up.",
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
