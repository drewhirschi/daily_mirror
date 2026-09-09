import { Directory, File, Paths } from "expo-file-system";
import * as Crypto from "expo-crypto";
import { fetch } from "expo/fetch";
import { ApiError, type MirrorApi, type Photo } from "@daily-mirror/api";
import {
  DiskCache,
  maxImageBytes,
  type CacheEntry,
  type CacheStorage,
} from "./disk-cache";

const root = () => new Directory(Paths.cache, "daily-mirror");
const hash = (value: string) =>
  Crypto.digestStringAsync(Crypto.CryptoDigestAlgorithm.SHA256, value);

export async function createImageCache(api: MirrorApi, userId: string) {
  const directory = new Directory(
    root(),
    await hash(`${api.origin}\n${userId}`),
  );
  directory.create({ intermediates: true, idempotent: true });
  const manifest = new File(directory, "index.json");
  const prefix = `${directory.uri.replace(/\/$/, "")}/`;
  const storage: CacheStorage = {
    load() {
      try {
        const value: unknown = JSON.parse(manifest.textSync());
        if (!Array.isArray(value)) return [];
        return value.filter(
          (entry): entry is CacheEntry =>
            !!entry &&
            typeof entry.key === "string" &&
            typeof entry.uri === "string" &&
            entry.uri.startsWith(prefix) &&
            !entry.uri.slice(prefix.length).includes("/") &&
            Number.isFinite(entry.bytes) &&
            Number.isFinite(entry.touched),
        );
      } catch {
        return [];
      }
    },
    save(entries) {
      const temporary = new File(directory, "index.tmp");
      temporary.write(JSON.stringify(entries));
      if (manifest.exists) manifest.delete();
      temporary.move(manifest);
    },
    exists: (entry) => new File(entry.uri).exists,
    remove(entry) {
      const file = new File(entry.uri);
      if (file.exists) file.delete();
    },
    async download(key, signal) {
      const file = new File(directory, `${await hash(key)}.image`);
      const limit = maxImageBytes(key);
      const controller = new AbortController();
      const abort = () => controller.abort();
      if (signal.aborted) abort();
      signal.addEventListener("abort", abort);
      const timer = setTimeout(abort, 30_000);
      try {
        const response = await fetch(api.media(key), {
          headers: api.headers,
          credentials: "omit",
          signal: controller.signal,
        });
        if (!response.ok)
          throw new ApiError(response.status, "This image is unavailable.");
        if (
          !["image/webp", "image/jpeg"].some((type) =>
            response.headers.get("content-type")?.includes(type),
          )
        )
          throw new Error("Invalid image format.");
        if (Number(response.headers.get("content-length")) > limit)
          throw new Error("Image is too large.");
        const reader = response.body?.getReader();
        if (!reader) throw new Error("Empty image response.");
        let size = 0;
        const chunks: Uint8Array[] = [];
        while (true) {
          const chunk = await reader.read();
          if (chunk.done) break;
          size += chunk.value.length;
          if (size > limit) {
            await reader.cancel();
            throw new Error("Image is too large.");
          }
          chunks.push(chunk.value);
        }
        if (controller.signal.aborted) throw new Error("Download cancelled.");
        const bytes = new Uint8Array(size);
        let offset = 0;
        for (const chunk of chunks) {
          bytes.set(chunk, offset);
          offset += chunk.length;
        }
        // Reject a proxy/login document even if its Content-Type was incorrect.
        if (
          !(
            String.fromCharCode(...bytes.slice(0, 4)) === "RIFF" &&
            String.fromCharCode(...bytes.slice(8, 12)) === "WEBP"
          ) &&
          !(bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff)
        )
          throw new Error("Invalid image data.");
        file.write(bytes);
        return { uri: file.uri, bytes: size };
      } finally {
        clearTimeout(timer);
        signal.removeEventListener("abort", abort);
      }
    },
    clear() {
      if (directory.exists) directory.delete();
      directory.create({ intermediates: true, idempotent: true });
    },
  };
  // Recover from an interrupted manifest write and remove orphaned downloads.
  const entries = storage.load();
  const known = new Set(entries.map((entry) => entry.uri));
  for (const file of directory.list()) {
    if (
      file instanceof File &&
      !["index.json", "catalog.json"].includes(file.name) &&
      !known.has(file.uri)
    )
      file.delete();
  }
  return { cache: new DiskCache(storage), directory };
}

export function readCatalog(directory: Directory): Photo[] | undefined {
  try {
    const value: unknown = JSON.parse(
      new File(directory, "catalog.json").textSync(),
    );
    if (
      !Array.isArray(value) ||
      !value.every(
        (photo) =>
          typeof photo?.id === "string" &&
          typeof photo?.url === "string" &&
          (photo.thumbnail_url == null ||
            typeof photo.thumbnail_url === "string"),
      )
    )
      return;
    return value;
  } catch {
    return;
  }
}

export function writeCatalog(directory: Directory, photos: Photo[]) {
  new File(directory, "catalog.json").write(JSON.stringify(photos));
}

export function clearPrivateFiles() {
  const directory = root();
  if (directory.exists) directory.delete();
}
