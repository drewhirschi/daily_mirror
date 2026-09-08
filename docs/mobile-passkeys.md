# Native iOS passkey login

The existing web login and passkey registration remain unchanged. Native login
uses the same registered credentials and WebAuthn verification with single-use
server ceremonies, then returns a bearer session instead of a browser cookie.

- `POST /api/auth/login/native/passkey/start`: username → ceremony ID and WebAuthn options.
- `POST /api/auth/login/native/passkey/finish`: ceremony ID and signed assertion → native session.
- Both routes disable response caching. Challenge issuance and invalid finish
  attempts use the existing authentication rate limiter.
- `GET /.well-known/apple-app-site-association` is public JSON, authorizing
  `C9P58ZP4AQ.app.dailymirror.ios` for web credentials on the deployment domain.

The mobile app must include `react-native-passkey` and the entitlement
`webcredentials:daily-mirror-pearl.vercel.app`. Use an Apple Developer Program
team that supports Associated Domains; the free Personal Team does not.
When changing the signing team, update the App ID in the association route and
its test to match the actual signed application identifier, and redeploy before
installing the app. Keep the relying-party domain unchanged to reuse existing
website passkeys. Native passkeys need a fresh app build, not a Metro reload.

Tests exercise a software authenticator's registration and signed login,
session authentication/revocation, replay rejection, public association JSON,
and rate limiting. A real-device Face ID test is still required after installing
an eligible signed build. Software authenticator code is a test-only dependency.
