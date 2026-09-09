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
    },
  },
  plugins: [
    "expo-image",
    "expo-secure-store",
    "expo-status-bar",
    "expo-dev-client",
    "@react-native-community/datetimepicker",
  ],
};

export default config;
