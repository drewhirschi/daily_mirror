import { useEffect } from "react";

export function PwaMetadata() {
  useEffect(() => {
    if (!("serviceWorker" in navigator)) return;
    void navigator.serviceWorker.register("/sw.js", { scope: "/" }).catch(() => {
      // The gallery remains fully usable when a browser disables service workers.
    });
  }, []);

  return (
    <>
      <title>Daily Mirror</title>
      <meta name="description" content="A private daily photo journal." />
      <meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover" />
      <meta name="theme-color" content="#f2efe8" />
      <meta name="apple-mobile-web-app-capable" content="yes" />
      <meta name="apple-mobile-web-app-status-bar-style" content="default" />
      <meta name="apple-mobile-web-app-title" content="Daily Mirror" />
      <link rel="manifest" href="/manifest.webmanifest" />
      <link rel="icon" href="/icons/icon-192.png" sizes="192x192" type="image/png" />
      <link rel="apple-touch-icon" href="/icons/apple-touch-icon.png" />
    </>
  );
}
