import { FormEvent, useEffect, useState } from "react";
import { getPasskey, supportsPasskeys } from "../../components/webauthn";

export default function LoginPage() {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [passkeysAvailable, setPasskeysAvailable] = useState(false);

  useEffect(() => {
    setPasskeysAvailable(supportsPasskeys());
    setUsername(window.localStorage.getItem("daily-mirror-username") ?? "");
  }, []);

  return (
    <main className="auth-page">
      <section className="auth-card">
        <p className="eyebrow">Private archive</p>
        <h1>Welcome back</h1>
        <p className="auth-intro">Sign in once and this browser will stay signed in for 30 days.</p>
        <form onSubmit={passwordLogin}>
          <label className="auth-field">
            <span>Username</span>
            <input
              name="username"
              autoComplete="username webauthn"
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              required
              autoFocus
            />
          </label>
          <label className="auth-field">
            <span>Password</span>
            <input
              name="password"
              type="password"
              autoComplete="current-password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              required
            />
          </label>
          <button className="auth-primary" type="submit" disabled={busy}>Sign in</button>
        </form>
        <div className="auth-divider"><span>or</span></div>
        <button
          className="auth-passkey"
          type="button"
          disabled={busy || !passkeysAvailable || !username.trim()}
          onClick={passkeyLogin}
        >
          Sign in with a passkey
        </button>
        {!passkeysAvailable ? <p className="auth-note">This browser does not expose passkey support.</p> : null}
        {error ? <p className="auth-error" role="alert">{error}</p> : null}
      </section>
    </main>
  );

  async function passwordLogin(event: FormEvent) {
    event.preventDefault();
    await run(async () => {
      const response = await fetch("/api/auth/login/password", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ username, password }),
      });
      await requireOk(response);
      finishLogin();
    });
  }

  async function passkeyLogin() {
    await run(async () => {
      const startResponse = await fetch("/api/auth/login/passkey/start", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ username }),
      });
      const start = await requireJson(startResponse) as { ceremony_id: string; options: Record<string, unknown> };
      const credential = await getPasskey(start.options);
      const finishResponse = await fetch("/api/auth/login/passkey/finish", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ ceremony_id: start.ceremony_id, credential }),
      });
      await requireOk(finishResponse);
      finishLogin();
    });
  }

  async function run(operation: () => Promise<void>) {
    setBusy(true);
    setError("");
    try {
      await operation();
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : "Sign-in failed.");
    } finally {
      setBusy(false);
    }
  }

  function finishLogin() {
    window.localStorage.setItem("daily-mirror-username", username.trim());
    const next = new URLSearchParams(window.location.search).get("next") ?? "/";
    window.location.assign(next.startsWith("/") && !next.startsWith("//") ? next : "/");
  }
}

async function requireOk(response: Response) {
  if (response.ok) return;
  let message = "Sign-in failed.";
  try {
    const body = await response.json() as { error?: string };
    if (body.error) message = body.error;
  } catch { /* keep the safe message */ }
  throw new Error(message);
}

async function requireJson(response: Response) {
  await requireOk(response);
  return response.json();
}
