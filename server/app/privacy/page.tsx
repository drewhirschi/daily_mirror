// The privacy policy URL the App Store listing points at. It must be reachable
// without signing in; `bypasses_authentication` in src/view_auth.rs allows it.
// Everything below describes what the system actually does today, taken from
// the schema and the storage code. Keep it in step with the App Privacy
// answers in App Store Connect (docs/app-store-submission.md).
import { CONTACT_EMAIL } from "../../components/contact";

export default function PrivacyPage() {
  return (
    <main className="prose-page">
      <p className="eyebrow">Daily Mirror</p>
      <h1>Privacy Policy</h1>
      <p className="prose-meta">Last updated: 22 September 2026</p>

      <h2>Who runs Daily Mirror</h2>
      <p>
        Daily Mirror is an independent product run by its developer. It is not
        backed by an advertising business, and there is no third party whose
        interest in your household is different from yours.
      </p>

      <h2>What Daily Mirror collects</h2>
      <ul>
        <li>
          <strong>Photographs.</strong> The pictures a Daily Mirror camera takes
          in your home, and the enrollment photographs you take with your phone.
        </li>
        <li>
          <strong>Face data.</strong> For each photograph, Daily Mirror detects
          faces and stores where each face is, a set of facial landmarks, and a
          numeric face signature (an <em>embedding</em>) that lets it tell one
          person from another. This is the sensitive part of the system and it
          is treated as such below.
        </li>
        <li>
          <strong>Your household.</strong> The household name, the names of the
          people in it, and which face belongs to which person.
        </li>
        <li>
          <strong>Your account.</strong> Username, display name, a hashed
          password, and any passkeys you enrol. Passwords are stored only as
          Argon2 hashes; they are never stored or transmitted in the clear.
        </li>
        <li>
          <strong>Your cameras.</strong> Each camera&rsquo;s identifier, model
          and firmware version, when it was paired, and when it was last heard
          from.
        </li>
        <li>
          <strong>Camera settings for each photograph.</strong> Exposure, gain,
          focus, sensor model and similar values, kept so pictures can be
          improved over time.
        </li>
      </ul>
      <p>
        Daily Mirror does not collect your location, your contacts, your
        browsing, or anything from your phone beyond what is listed here.
      </p>

      <h2>How face data is used</h2>
      <p>
        Face signatures exist for one purpose: to group the photographs of your
        household by the person in them, so the archive is worth looking at.
        They are never used for advertising or marketing, never sold, never
        shared with a third party, and never used to identify anyone outside
        your own household. No face data leaves the storage described below.
      </p>
      <p>
        Face signatures are derived from your photographs and are deleted when
        the photographs they came from are deleted.
      </p>

      <h2>Where it is stored</h2>
      <p>
        Photographs are stored as files in Cloudflare R2 object storage.
        Everything else &mdash; accounts, households, people, faces, cameras
        &mdash; is stored in a Turso (libSQL) database. Both are reached only by
        the Daily Mirror server, over HTTPS. Your phone also keeps a copy of
        images you have looked at, so they open without downloading again; that
        copy is removed when you clear it on the Account screen or sign out.
      </p>

      <h2>Who it is shared with</h2>
      <p>
        Nobody. Daily Mirror contains no advertising, no analytics service and
        no third-party tracking of any kind, and it does not sell or share
        personal information. The only companies involved are the ones that
        store and serve the data on the developer&rsquo;s behalf &mdash;
        Cloudflare, Turso and Vercel &mdash; and they act only as processors.
      </p>

      <h2>Deleting your account and your data</h2>
      <p>
        You can ask for your account to be deleted from inside the app, on the
        Account screen, or from the account page on this site. Deletion
        requests are carried out by hand within 30 days, and the app shows your
        request as pending until then. Deleting your account removes your
        login, your password hash, your passkeys and your sessions.
      </p>
      <p>
        If you are the only member of your household, deleting your account also
        erases the household: every photograph and its stored file, every face
        and face signature derived from them, the people, and the cameras&rsquo;
        claims on the household. If other people still share your household,
        their archive is theirs and stays; only your account is removed. An
        administrator who is the last one left is asked to make somebody else an
        administrator first, so the household is not stranded.
      </p>
      <p>
        You can also delete an individual photograph at any time; its stored
        file and the faces derived from it go with it.
      </p>

      <h2>How long it is kept</h2>
      <p>
        Photographs and the face data derived from them are kept until you
        delete them or delete your household. Sessions expire after 30 days.
        Pairing codes for a new camera expire within minutes of being issued.
      </p>

      <h2>Children</h2>
      <p>
        Daily Mirror photographs the people who live in a home, and in most
        homes that includes children. Those photographs and the face data
        derived from them belong to the household and are visible only to the
        accounts in it. Daily Mirror is not directed to children as users; the
        adult who sets up the household is responsible for it.
      </p>

      <h2>Security</h2>
      <p>
        All traffic between the app, the cameras and the server uses HTTPS.
        Passwords are hashed with Argon2, session tokens are stored hashed, and
        a camera authenticates with a token of its own rather than with your
        account.
      </p>

      <h2>Contact</h2>
      <p>
        Questions, a copy of your data, or a request to have something removed:
        write to <a href={`mailto:${CONTACT_EMAIL}`}>{CONTACT_EMAIL}</a>, or see{" "}
        <a href="/support">the support page</a>.
      </p>
    </main>
  );
}
