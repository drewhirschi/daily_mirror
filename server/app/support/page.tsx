// The support URL the App Store listing points at. Reachable without signing
// in; `bypasses_authentication` in src/view_auth.rs allows it. App Review
// checks that the page works and offers a way to get in touch.
import { CONTACT_EMAIL } from "../../components/contact";

export default function SupportPage() {
  return (
    <main className="prose-page">
      <p className="eyebrow">Daily Mirror</p>
      <h1>Support</h1>
      <h2>Get in touch</h2>
      <p>
        Email <a href={`mailto:${CONTACT_EMAIL}`}>{CONTACT_EMAIL}</a>. Please
        say which iPhone and iOS version you are on, and what you were doing
        when it went wrong.
      </p>

      <h2>Setting up a camera</h2>
      <p>
        Open the app, go to Account &rarr; Cameras &rarr; Add a camera, and hold
        the phone near the camera while it is powered on and its light is
        breathing. The phone finds the camera over Bluetooth, asks for your
        Wi-Fi network and password, and hands them to the camera. Both the
        phone and the camera need to be near the 2.4&nbsp;GHz network you want
        the camera to join.
      </p>
      <p>
        If pairing does not find the camera, power the camera off and on again
        and try once more; it only accepts pairing for a few minutes after it
        starts.
      </p>

      <h2>Using the app without a camera</h2>
      <p>
        The archive, the flipbooks, your household and enrollment photographs
        taken with the phone all work with no camera paired at all. A camera is
        what fills the archive on its own.
      </p>

      <h2>Deleting your account</h2>
      <p>
        Account &rarr; Request account deletion, in the app or on the account
        page of this site. Requests are carried out within 30 days. What that
        removes is set out in the <a href="/privacy">privacy policy</a>.
      </p>

      <h2>Privacy</h2>
      <p>
        Daily Mirror stores photographs of your household and the face data it
        derives from them. The <a href="/privacy">privacy policy</a> says
        exactly what is kept, where, and how to have it deleted.
      </p>
    </main>
  );
}
