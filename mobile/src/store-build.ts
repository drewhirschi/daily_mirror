/**
 * True in any Release build, false when Metro is serving a development build.
 *
 * App Review rejects placeholder features and raw diagnostic text (guidelines
 * 2.1 and 4.2.3), so the few affordances that exist only for us — the invite
 * dialog that admits invites are not built, the unedited error line under a
 * failed pairing step, and the field that points the app at another server —
 * are hidden from a shipped build and kept in the development one, where they
 * are the whole diagnosis.
 *
 * Flip this to `false` by hand if you ever need those affordances in a Release
 * build on your own phone.
 */
export const STORE_BUILD = !__DEV__;
