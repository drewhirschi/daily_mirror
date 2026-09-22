import type { ExpoConfig } from "expo/config";

const config: ExpoConfig = {
  name: "Daily Mirror",
  slug: "daily-mirror",
  scheme: "dailymirror",
  // Marketing version shown on the App Store. Bump the minor/patch for each
  // release Drew announces; `ios.buildNumber` is the number App Store Connect
  // dedupes on and must increase with every upload, even for the same version.
  version: "1.0.0",
  // The gallery, the flipbooks and the enrollment camera are all designed for
  // a phone held upright.
  orientation: "portrait",
  userInterfaceStyle: "automatic",
  icon: "./assets/icon.png",
  ios: {
    bundleIdentifier: "app.dailymirror.ios",
    appleTeamId: "C9P58ZP4AQ",
    // Increase this on every upload to App Store Connect, whatever `version`
    // says. App Store Connect rejects a build whose number it has already seen.
    buildNumber: "1",
    // Nothing here has been laid out or tested for a tablet, and declaring
    // iPad support would oblige us to supply iPad screenshots and survive an
    // iPad review pass. Ship the iPhone app; it still runs on iPad in
    // compatibility mode.
    supportsTablet: false,
    // Signed by the paid Apple Developer Program team, so Associated Domains is
    // available. Keep this in step with NATIVE_PASSKEYS_ENABLED in
    // src/auth-features.ts and the App ID served by the server's
    // /.well-known/apple-app-site-association route.
    associatedDomains: ["webcredentials:daily-mirror-pearl.vercel.app"],
    infoPlist: {
      // The app speaks only standard HTTPS/TLS to its own server, which is
      // exempt encryption. Declaring it here skips the export-compliance
      // questions on every single upload.
      ITSAppUsesNonExemptEncryption: false,
      NSLocalNetworkUsageDescription:
        "Daily Mirror talks to a camera on your local network while you set it up.",
      NSBluetoothAlwaysUsageDescription:
        "Daily Mirror uses Bluetooth to find and set up a nearby camera.",
      NSBluetoothPeripheralUsageDescription:
        "Daily Mirror uses Bluetooth to find and set up a nearby camera.",
      NSLocationWhenInUseUsageDescription:
        "iOS needs location access to confirm this iPhone is joined to a camera's own Wi-Fi network while you set it up.",
      NSCameraUsageDescription:
        "Take enrollment photos so Daily Mirror can recognise the people in your household.",
      // react-native-passkey ships the string "Allow DailyMirror to access
      // your Face ID biometric data", which names no purpose and reads as a
      // placeholder to App Review. Face ID here does one thing: unlock the
      // passkey that signs you in.
      NSFaceIDUsageDescription:
        "Use Face ID to unlock the passkey that signs you in to Daily Mirror.",
    },
    // Apple requires a PrivacyInfo.xcprivacy for the app and for every SDK it
    // embeds. Expo generates the manifests for its own modules; these are the
    // app's own declarations, merged into the generated file by prebuild.
    //
    // Nothing here is used for tracking: the app has no analytics, no ad SDK
    // and no third-party network calls at all, so NSPrivacyTracking is false
    // and the tracking-domain list is empty.
    privacyManifests: {
      NSPrivacyTracking: false,
      NSPrivacyTrackingDomains: [],
      // Data is collected by the server, not by the app on its own behalf; the
      // App Privacy answers in App Store Connect are the authoritative
      // statement and are drafted in docs/app-store-submission.md.
      NSPrivacyCollectedDataTypes: [],
      NSPrivacyAccessedAPITypes: [
        {
          // Reading and writing the on-disk image cache, and reporting its
          // size on the Account screen.
          NSPrivacyAccessedAPIType: "NSPrivacyAccessedAPICategoryFileTimestamp",
          NSPrivacyAccessedAPITypeReasons: ["C617.1"],
        },
        {
          // The cache refuses to grow past the free space on the device.
          NSPrivacyAccessedAPIType: "NSPrivacyAccessedAPICategoryDiskSpace",
          NSPrivacyAccessedAPITypeReasons: ["E174.1"],
        },
        {
          // React Native and Expo keep app-scoped preferences here.
          NSPrivacyAccessedAPIType: "NSPrivacyAccessedAPICategoryUserDefaults",
          NSPrivacyAccessedAPITypeReasons: ["CA92.1"],
        },
        {
          // Monotonic timing inside the app; never sent anywhere.
          NSPrivacyAccessedAPIType:
            "NSPrivacyAccessedAPICategorySystemBootTime",
          NSPrivacyAccessedAPITypeReasons: ["35F9.1"],
        },
      ],
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
