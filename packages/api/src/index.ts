import type { components } from "./schema";
import type {
  CreatePersonRequest,
  DeletionRequest,
  EnrollmentStatus,
  HouseholdPerson,
  HouseholdSummary,
  SignupRequest,
} from "./onboarding-types";

export * from "./onboarding-types";

export type UploadGrant = components["schemas"]["UploadGrant"];
export type UploadRequest = components["schemas"]["UploadRequest"];
export type PersonFlipbook = components["schemas"]["PersonFlipbook"];
export type FlipbookFrame = components["schemas"]["FlipbookFrame"];
export type Photo = components["schemas"]["Photo"];
/** What took a photograph, as the gallery shows it. See docs/capture-metadata.md. */
export type PhotoCapture = components["schemas"]["PhotoCapture"];
/** What a camera reports with its upload grant. */
export type CaptureMetadata = components["schemas"]["CaptureMetadata"];
export type User = components["schemas"]["User"];
export type Passkey = components["schemas"]["PasskeySummary"];
export type NativeSession = components["schemas"]["NativeSession"];
export type PasswordLogin = components["schemas"]["PasswordLogin"];
export type PhotoList = components["schemas"]["PhotoList"];
export type PhotoFace = components["schemas"]["PhotoFace"];
export type PhotoFaces = components["schemas"]["PhotoFacesResponse"];

export class ApiError extends Error {
  constructor(
    public readonly status: number,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

export function serverOrigin(input: string, allowHttp = false): string {
  const url = new URL(input.trim());
  if (
    url.username ||
    url.password ||
    url.search ||
    url.hash ||
    url.pathname !== "/"
  ) {
    throw new Error(
      "Enter the server origin only, without a path or credentials.",
    );
  }
  if (url.protocol !== "https:" && !(allowHttp && url.protocol === "http:")) {
    throw new Error("Use an HTTPS server address.");
  }
  return url.origin;
}

/** Never attach account credentials to an origin supplied by a media response. */
export function mediaUrl(origin: string, path: string): string {
  const url = new URL(path, origin);
  if (
    url.origin !== origin ||
    !(
      url.pathname.startsWith("/api/photos/") ||
      /^\/api\/admin\/faces\/[a-zA-Z0-9_-]+\/crop$/.test(url.pathname)
    ) ||
    url.username ||
    url.password
  ) {
    throw new Error("The server returned an invalid photo address.");
  }
  return url.href;
}

export class MirrorApi {
  readonly origin: string;
  constructor(
    origin: string,
    private readonly token?: string,
    allowHttp = false,
  ) {
    this.origin = serverOrigin(origin, allowHttp);
  }

  get headers(): Record<string, string> {
    return this.token
      ? {
          Authorization: `Bearer ${this.token}`,
          "Cache-Control": "no-cache, no-store",
        }
      : {};
  }

  private async request<T>(path: string, init: RequestInit = {}): Promise<T> {
    const controller = new AbortController();
    const abort = () => controller.abort();
    if (init.signal?.aborted) abort();
    init.signal?.addEventListener("abort", abort);
    const timer = setTimeout(abort, 30_000);
    try {
      const response = await fetch(`${this.origin}${path}`, {
        ...init,
        signal: controller.signal,
        credentials: "omit",
        headers: {
          Accept: "application/json",
          ...this.headers,
          ...(init.body ? { "Content-Type": "application/json" } : {}),
        },
      });
      if (!response.ok) {
        const message =
          response.status === 401
            ? "Your session has expired. Please sign in again."
            : response.status === 429
              ? "Too many sign-in attempts. Please try again later."
              : response.status === 404
                ? "This item is no longer available."
                : `The server could not complete this request (${response.status}).`;
        throw new ApiError(response.status, message);
      }
      return response.status === 204
        ? (undefined as T)
        : ((await response.json()) as T);
    } finally {
      clearTimeout(timer);
      init.signal?.removeEventListener("abort", abort);
    }
  }

  login(input: PasswordLogin) {
    return this.request<NativeSession>("/api/auth/login/native", {
      method: "POST",
      body: JSON.stringify(input),
    });
  }
  /** Omit the username to let the platform offer its own passkey picker. */
  passkeyStart(username?: string) {
    const trimmed = username?.trim();
    const input: components["schemas"]["NativePasskeyLoginStart"] = trimmed
      ? { username: trimmed }
      : {};
    return this.request<components["schemas"]["NativePasskeyChallenge"]>(
      "/api/auth/login/native/passkey/start",
      { method: "POST", body: JSON.stringify(input) },
    );
  }
  passkeyFinish(input: components["schemas"]["NativePasskeyLoginFinish"]) {
    return this.request<NativeSession>(
      "/api/auth/login/native/passkey/finish",
      { method: "POST", body: JSON.stringify(input) },
    );
  }
  signup(input: SignupRequest) {
    return this.request<NativeSession>("/api/auth/signup", {
      method: "POST",
      body: JSON.stringify(input),
    });
  }
  household(signal?: AbortSignal) {
    return this.request<HouseholdSummary>("/api/household", { signal });
  }
  renameHousehold(displayName: string) {
    const input: components["schemas"]["RenameHouseholdRequest"] = {
      display_name: displayName,
    };
    return this.request<HouseholdSummary>("/api/household", {
      method: "PATCH",
      body: JSON.stringify(input),
    });
  }
  addHouseholdPerson(input: CreatePersonRequest) {
    return this.request<HouseholdPerson>("/api/household/people", {
      method: "POST",
      body: JSON.stringify(input),
    });
  }
  createEnrollmentUpload(personId: string, input: UploadRequest) {
    return this.request<UploadGrant>(
      `/api/household/people/${encodeURIComponent(personId)}/enrollment/uploads`,
      { method: "POST", body: JSON.stringify(input) },
    );
  }
  finalizeEnrollmentUpload(personId: string, photoId: string) {
    return this.request<void>(
      `/api/household/people/${encodeURIComponent(personId)}/enrollment/uploads/${encodeURIComponent(photoId)}`,
      { method: "POST" },
    );
  }
  enrollmentStatus(personId: string, signal?: AbortSignal) {
    return this.request<EnrollmentStatus>(
      `/api/household/people/${encodeURIComponent(personId)}/enrollment`,
      { signal },
    );
  }
  me(signal?: AbortSignal) {
    return this.request<User>("/api/auth/me", { signal });
  }
  photos(signal?: AbortSignal) {
    return this.request<PhotoList>("/api/photos", { signal });
  }
  people(signal?: AbortSignal) {
    return this.request<components["schemas"]["PeopleResponse"]>(
      "/api/admin/people",
      { signal },
    );
  }
  personPhotos(personId: string, signal?: AbortSignal) {
    return this.request<components["schemas"]["PersonPhotosResponse"]>(
      `/api/admin/people/${encodeURIComponent(personId)}/photos`,
      { signal },
    );
  }
  passkeys(signal?: AbortSignal) {
    return this.request<components["schemas"]["PasskeyList"]>(
      "/api/auth/passkeys",
      { signal },
    );
  }
  logout() {
    return this.request<void>("/api/auth/logout", { method: "POST" });
  }
  /**
   * Ask for the account and its data to be deleted, which App Review
   * guideline 5.1.1(v) requires the app to offer. Nothing is removed by this
   * call: an operator carries the request out. Repeating it returns the
   * original request.
   */
  requestAccountDeletion() {
    return this.request<DeletionRequest>("/api/auth/account/deletion-request", {
      method: "POST",
    });
  }
  /** The open deletion request for this account, or null when there is none. */
  async accountDeletionRequest(
    signal?: AbortSignal,
  ): Promise<DeletionRequest | null> {
    try {
      return await this.request<DeletionRequest>(
        "/api/auth/account/deletion-request",
        { signal },
      );
    } catch (caught) {
      if (caught instanceof ApiError && caught.status === 404) return null;
      throw caught;
    }
  }
  rotate(id: string, degrees: -90 | 90) {
    const edit: components["schemas"]["RotatePhoto"] = { degrees };
    return this.request<void>(`/api/photos/${encodeURIComponent(id)}`, {
      method: "PATCH",
      body: JSON.stringify(edit),
    });
  }
  deletePhoto(id: string) {
    return this.request<void>(`/api/photos/${encodeURIComponent(id)}`, {
      method: "DELETE",
    });
  }
  photoFaces(id: string, signal?: AbortSignal) {
    return this.request<PhotoFaces>(
      `/api/photos/${encodeURIComponent(id)}/faces`,
      { signal },
    );
  }
  setFlipbookMembership(id: string, included: boolean) {
    const membership: components["schemas"]["FlipbookMembership"] = {
      included,
    };
    return this.request<components["schemas"]["FlipbookMembership"]>(
      `/api/photos/${encodeURIComponent(id)}/flipbook`,
      { method: "PUT", body: JSON.stringify(membership) },
    );
  }
  media(path: string) {
    return mediaUrl(this.origin, path);
  }
}
