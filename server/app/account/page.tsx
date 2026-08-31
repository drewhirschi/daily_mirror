import { useEffect, useState } from "react";
import { createPasskey, supportsPasskeys } from "../../components/webauthn";

type User = { id: string; username: string; display_name: string };
type Passkey = { credential_id: string; label: string; created_at: string; last_used_at?: string };

export default function AccountPage() {
  const [user, setUser] = useState<User | null>(null);
  const [passkeys, setPasskeys] = useState<Passkey[]>([]);
  const [label, setLabel] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");

  useEffect(() => { void refresh(); }, []);

  return (
    <main className="account-page">
      <header className="account-heading">
        <div>
          <p className="eyebrow">Account</p>
          <h1>{user?.display_name ?? "Your account"}</h1>
          {user ? <p>@{user.username}</p> : null}
        </div>
        <button className="quiet-button" type="button" onClick={logout}>Sign out</button>
      </header>

      <section className="account-card">
        <h2>Passkeys</h2>
        <p>Use Face ID, Touch ID, Windows Hello, your phone, or a hardware security key instead of typing your password.</p>
        <div className="passkey-enroll">
          <label className="auth-field">
            <span>Name this passkey</span>
            <input value={label} maxLength={100} placeholder="Phone, laptop, security key…" onChange={(event) => setLabel(event.target.value)} />
          </label>
          <button className="auth-primary" type="button" disabled={busy || !label.trim() || !supportsPasskeys()} onClick={addPasskey}>
            Add passkey
          </button>
        </div>
        {message ? <p className="account-message" role="status">{message}</p> : null}
        {passkeys.length ? (
          <ul className="passkey-list">
            {passkeys.map((passkey) => (
              <li key={passkey.credential_id}>
                <strong>{passkey.label}</strong>
                <span>Added {new Date(`${passkey.created_at}Z`).toLocaleDateString()}</span>
                <span>{passkey.last_used_at ? `Last used ${new Date(`${passkey.last_used_at}Z`).toLocaleDateString()}` : "Not used yet"}</span>
              </li>
            ))}
          </ul>
        ) : <p className="auth-note">No passkeys enrolled yet. Your password remains the recovery method.</p>}
      </section>

      <section className="account-card install-card">
        <img src="/icons/icon-192.png" alt="" width="72" height="72" />
        <div>
          <h2>Add to Home Screen</h2>
          <p>On your iPhone or iPad, open Daily Mirror, tap the Share button, then choose <strong>Add to Home Screen</strong>. It will launch like an app and keep your 30-day sign-in.</p>
        </div>
      </section>
    </main>
  );

  async function refresh() {
    const [userResponse, passkeyResponse] = await Promise.all([
      fetch("/api/auth/me"),
      fetch("/api/auth/passkeys"),
    ]);
    if (userResponse.status === 401) return window.location.assign("/login?next=/account");
    if (userResponse.ok) setUser(await userResponse.json());
    if (passkeyResponse.ok) setPasskeys((await passkeyResponse.json()).passkeys);
  }

  async function addPasskey() {
    setBusy(true);
    setMessage("");
    try {
      const startResponse = await fetch("/api/auth/passkeys/register/start", { method: "POST" });
      if (!startResponse.ok) throw new Error("The server could not start passkey enrollment.");
      const start = await startResponse.json() as { ceremony_id: string; options: Record<string, unknown> };
      const credential = await createPasskey(start.options);
      const finishResponse = await fetch("/api/auth/passkeys/register/finish", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ ceremony_id: start.ceremony_id, label, credential }),
      });
      if (!finishResponse.ok) throw new Error("Passkey enrollment could not be completed.");
      setLabel("");
      setMessage("Passkey added. You can use it the next time you sign in.");
      await refresh();
    } catch (caught) {
      setMessage(caught instanceof Error ? caught.message : "Passkey enrollment failed.");
    } finally {
      setBusy(false);
    }
  }

  async function logout() {
    await fetch("/api/auth/logout", { method: "POST" });
    window.location.assign("/login");
  }
}
