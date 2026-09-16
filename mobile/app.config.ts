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
        "Connect to your Daily Mirror development server on your local network.",
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
  android: {
    package: "app.dailymirror.android",
  },
  plugins: [
    "expo-image",
    "expo-secure-store",
    "expo-status-bar",
    "expo-dev-client",
    "@react-native-community/datetimepicker",
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
