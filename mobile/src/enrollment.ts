import {
  ApiError,
  REQUIRED_ENROLLMENT_PHOTOS,
  type EnrollmentStatus,
  type MirrorApi,
  type UploadGrant,
} from "@daily-mirror/api";

/**
 * Guided face enrollment: capture ids, the five poses, and the upload chain.
 *
 * Everything here is plain TypeScript so `node:test` can cover the grant, PUT,
 * finalize and polling behaviour without rendering the camera screen.
 */

/** One of the five directions the person is asked to look toward. */
export type EnrollmentPose = {
  id: string;
  label: string;
  /** Spoken guidance shown under the viewfinder. */
  caption: string;
  /** Outline centre as a fraction of the preview width. */
  x: number;
  /** Outline centre as a fraction of the preview height. */
  y: number;
};

/**
 * Capture order: lower left, upper left, centre, upper right, lower right.
 * The sweep keeps the head moving through a wide range of angles instead of
 * returning to centre between poses.
 */
export const ENROLLMENT_POSES: readonly EnrollmentPose[] = [
  {
    id: "lower-left",
    label: "Lower left",
    caption: "Turn slightly and look toward the lower left",
    x: 0.28,
    y: 0.68,
  },
  {
    id: "upper-left",
    label: "Upper left",
    caption: "Now look up toward the upper left",
    x: 0.28,
    y: 0.32,
  },
  {
    id: "center",
    label: "Centre",
    caption: "Look straight at the camera",
    x: 0.5,
    y: 0.5,
  },
  {
    id: "upper-right",
    label: "Upper right",
    caption: "Look up toward the upper right",
    x: 0.72,
    y: 0.32,
  },
  {
    id: "lower-right",
    label: "Lower right",
    caption: "Last one: look down toward the lower right",
    x: 0.72,
    y: 0.68,
  },
];

if (ENROLLMENT_POSES.length !== REQUIRED_ENROLLMENT_PHOTOS)
  throw new Error("The pose list must match the server's required photo count");

export type SlotStatus =
  "empty" | "uploading" | "processing" | "enrolled" | "retake" | "failed";

export type EnrollmentSlot = {
  pose: EnrollmentPose;
  status: SlotStatus;
  /** The capture id, which is also the photo id the server reports back. */
  photoId?: string;
  /** Local capture, kept so a failed upload can be retried without recapturing. */
  fileUri?: string;
  byteLength?: number;
  contentType?: string;
  faceCount?: number | null;
  thumbnailUrl?: string | null;
  message?: string;
};

const pad = (value: number, width = 2) => String(value).padStart(width, "0");

function defaultRandomBytes(length: number): Uint8Array {
  const bytes = new Uint8Array(length);
  try {
    // expo-crypto is the device source of randomness.
    const crypto = require("expo-crypto") as typeof import("expo-crypto");
    return crypto.getRandomBytes(length);
  } catch {
    // Web Crypto covers the test runner and any build without the native module.
    globalThis.crypto.getRandomValues(bytes);
    return bytes;
  }
}

/**
 * Build a capture id in the `YYYYMMDDTHHMMSSZ-<8 hex>` form the Pi uses, so the
 * server and gallery parse the capture date the same way for every source.
 */
export function createCaptureId(
  at: Date = new Date(),
  randomBytes: (length: number) => Uint8Array = defaultRandomBytes,
): string {
  const stamp =
    `${at.getUTCFullYear()}${pad(at.getUTCMonth() + 1)}${pad(at.getUTCDate())}` +
    `T${pad(at.getUTCHours())}${pad(at.getUTCMinutes())}${pad(at.getUTCSeconds())}Z`;
  const suffix = Array.from(randomBytes(4), (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
  return `${stamp}-${suffix}`;
}

export const CAPTURE_ID_PATTERN = /^\d{8}T\d{6}Z-[0-9a-f]{8}$/;

/** A file body for the signed PUT. On device this is an `expo-file-system` `File`. */
export type UploadBody = (fileUri: string) => BodyInit | Promise<BodyInit>;

function defaultOpenBody(fileUri: string): BodyInit {
  // `File` implements `Blob`, so fetch streams it with a known length.
  const { File } =
    require("expo-file-system") as typeof import("expo-file-system");
  return new File(fileUri) as unknown as BodyInit;
}

export type UploaderOptions = {
  /** Injected so tests can stub the network and the screen can keep the default. */
  fetchImpl?: typeof fetch;
  openBody?: UploadBody;
};

const isUnresolved = (slot: EnrollmentSlot) =>
  slot.status === "uploading" || slot.status === "processing";

const sleep = (ms: number) =>
  new Promise<void>((resolve) => setTimeout(resolve, ms));

/**
 * A network failure deserves one retry; a refusal from the server does not.
 * An `ApiError` means the server answered, so retrying would fail the same way.
 */
const isRetryable = (error: unknown) => !(error instanceof ApiError);

export class EnrollmentUploader {
  private slots: EnrollmentSlot[] = ENROLLMENT_POSES.map((pose) => ({
    pose,
    status: "empty",
  }));
  private listeners = new Set<(slots: EnrollmentSlot[]) => void>();
  private polling = false;
  private readonly fetchImpl: typeof fetch;
  private readonly openBody: UploadBody;

  constructor(
    private readonly api: MirrorApi,
    private readonly personId: string,
    options: UploaderOptions = {},
  ) {
    this.fetchImpl = options.fetchImpl ?? ((...args) => fetch(...args));
    this.openBody = options.openBody ?? defaultOpenBody;
  }

  get state(): EnrollmentSlot[] {
    return this.slots;
  }

  get enrolledCount(): number {
    return this.slots.filter((slot) => slot.status === "enrolled").length;
  }

  get complete(): boolean {
    return this.enrolledCount === ENROLLMENT_POSES.length;
  }

  /** The next slot that still needs a usable photo, or -1 when finished. */
  nextSlotIndex(from = 0): number {
    const order = [
      ...this.slots.slice(from),
      ...this.slots.slice(0, from),
    ].filter(
      (slot) => !["enrolled", "uploading", "processing"].includes(slot.status),
    );
    return order.length ? this.slots.indexOf(order[0]) : -1;
  }

  subscribe(listener: (slots: EnrollmentSlot[]) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private update(index: number, patch: Partial<EnrollmentSlot>) {
    this.slots = this.slots.map((slot, at) =>
      at === index ? { ...slot, ...patch } : slot,
    );
    for (const listener of this.listeners) listener(this.slots);
  }

  /** Forget a slot's photo so the pose can be captured again. */
  retake(index: number) {
    this.update(index, {
      status: "empty",
      photoId: undefined,
      fileUri: undefined,
      byteLength: undefined,
      contentType: undefined,
      faceCount: undefined,
      thumbnailUrl: undefined,
      message: undefined,
    });
  }

  /**
   * Upload one captured pose. Resolves once the server has accepted the photo
   * for processing; the enrolled or retake verdict arrives through polling.
   */
  async upload(
    slotIndex: number,
    fileUri: string,
    byteLength: number,
    contentType = "image/jpeg",
  ): Promise<void> {
    const photoId = this.slots[slotIndex]?.photoId ?? createCaptureId();
    this.update(slotIndex, {
      status: "uploading",
      photoId,
      fileUri,
      byteLength,
      contentType,
      message: undefined,
    });
    try {
      try {
        await this.send(photoId, fileUri, byteLength, contentType);
      } catch (error) {
        if (!isRetryable(error)) throw error;
        await this.send(photoId, fileUri, byteLength, contentType);
      }
      this.update(slotIndex, { status: "processing" });
    } catch (error) {
      this.update(slotIndex, {
        status: "failed",
        message:
          error instanceof ApiError
            ? error.message
            : "That photo could not be sent. Tap to try again.",
      });
    }
  }

  /** Grant, signed PUT, finalize — the same three steps the Pi performs. */
  private async send(
    photoId: string,
    fileUri: string,
    byteLength: number,
    contentType: string,
  ) {
    const grant = await this.api.createEnrollmentUpload(this.personId, {
      capture_id: photoId,
      content_type: contentType,
      content_length: byteLength,
    });
    await this.put(grant, fileUri, byteLength, contentType);
    await this.api.finalizeEnrollmentUpload(this.personId, photoId);
  }

  private async put(
    grant: UploadGrant,
    fileUri: string,
    byteLength: number,
    contentType: string,
  ) {
    const target = new URL(grant.url, this.api.origin);
    if (target.protocol !== "https:" && target.protocol !== "http:")
      throw new Error("The server returned an invalid upload address.");
    const method = grant.method.toUpperCase();
    if (method !== "PUT" && method !== "POST")
      throw new Error("The server returned an unsupported upload method.");
    // Blob storage rejects an unexpected Authorization header, so the session
    // token only travels when the upload target is the Daily Mirror server.
    const auth = target.origin === this.api.origin ? this.api.headers : {};
    const response = await this.fetchImpl(target.href, {
      method,
      credentials: "omit",
      headers: {
        "Content-Type": contentType,
        "Content-Length": String(byteLength),
        ...auth,
        ...grant.headers,
      },
      body: await this.openBody(fileUri),
    });
    if (!response.ok)
      throw new ApiError(
        response.status,
        "That photo could not be uploaded. Please try again.",
      );
  }

  /** Fold a server status response into the slots, matching on the capture id. */
  mergeStatus(status: EnrollmentStatus) {
    const byId = new Map(status.photos.map((photo) => [photo.photo_id, photo]));
    this.slots = this.slots.map((slot) => {
      const photo = slot.photoId ? byId.get(slot.photoId) : undefined;
      if (!photo) return slot;
      return {
        ...slot,
        status: photo.status,
        faceCount: photo.face_count,
        thumbnailUrl: photo.thumbnail_url,
        message:
          photo.status === "retake"
            ? photo.face_count === 0
              ? "No face was found. Tap to retake."
              : "More than one face was in frame. Tap to retake."
            : photo.status === "failed"
              ? "The server could not process that photo. Tap to retake."
              : undefined,
      };
    });
    for (const listener of this.listeners) listener(this.slots);
  }

  stopPolling() {
    this.polling = false;
  }

  /**
   * Ask the server for per-photo status until nothing is left in flight.
   * Resolves when every slot has a verdict, so the screen can await the done
   * state and the test can assert that polling stops.
   */
  async pollStatus(intervalMs = 2000): Promise<void> {
    if (this.polling) return;
    this.polling = true;
    try {
      while (this.polling && this.slots.some(isUnresolved)) {
        try {
          this.mergeStatus(await this.api.enrollmentStatus(this.personId));
        } catch (error) {
          if (error instanceof ApiError && error.status < 500) throw error;
          // A transient network error just means waiting for the next tick.
        }
        if (this.polling && this.slots.some(isUnresolved))
          await sleep(intervalMs);
      }
    } finally {
      this.polling = false;
    }
  }
}
