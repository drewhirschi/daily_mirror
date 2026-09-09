export type CacheEntry = {
  key: string;
  uri: string;
  bytes: number;
  touched: number;
};
export interface CacheStorage {
  load(): CacheEntry[];
  save(entries: CacheEntry[]): void;
  exists(entry: CacheEntry): boolean;
  remove(entry: CacheEntry): void;
  download(
    key: string,
    signal: AbortSignal,
  ): Promise<{ uri: string; bytes: number }>;
  clear(): void;
}

export const IMAGE_CACHE_BUDGET = 1024 * 1024 * 1024;
export const MAX_PHOTO_BYTES = 32 * 1024 * 1024;
export const MAX_THUMBNAIL_BYTES = 512 * 1024;

// Full photos are downloaded only on demand; previews retain their small limit.
export const maxImageBytes = (key: string) =>
  /^\/api\/photos\/[^/?]+(?:\?[^#]*)?$/.test(key)
    ? MAX_PHOTO_BYTES
    : MAX_THUMBNAIL_BYTES;

/** Persistent LRU, de-duplicated requests, and bounded network concurrency. */
export class DiskCache {
  private entries = new Map<string, CacheEntry>();
  private pending = new Map<string, Promise<string>>();
  private controllers = new Set<AbortController>();
  private waiting: (() => void)[] = [];
  private running = 0;
  private clearing = false;
  private closed = false;
  private validKeys = new Map<string, Set<string>>();

  constructor(
    private storage: CacheStorage,
    private budget = IMAGE_CACHE_BUDGET,
    private concurrency = 3,
  ) {
    for (const entry of storage.load()) {
      if (
        entry.bytes > 0 &&
        entry.bytes <= maxImageBytes(entry.key) &&
        storage.exists(entry)
      )
        this.entries.set(entry.key, entry);
      else storage.remove(entry);
    }
    this.prune();
  }

  get stats() {
    return {
      count: this.entries.size,
      bytes: [...this.entries.values()].reduce(
        (sum, entry) => sum + entry.bytes,
        0,
      ),
    };
  }

  peek(key: string): string | undefined {
    const entry = this.entries.get(key);
    if (!entry) return;
    if (!this.storage.exists(entry)) {
      this.entries.delete(key);
      return;
    }
    entry.touched = Date.now();
    return entry.uri;
  }

  get(key: string): Promise<string> {
    if (this.closed || this.clearing)
      return Promise.reject(new Error("Cache is unavailable."));
    const cached = this.peek(key);
    if (cached) return Promise.resolve(cached);
    const existing = this.pending.get(key);
    if (existing) return existing;
    const promise = this.fetch(key).finally(() => this.pending.delete(key));
    this.pending.set(key, promise);
    return promise;
  }

  private async fetch(key: string): Promise<string> {
    if (this.running >= this.concurrency)
      await new Promise<void>((resolve) => this.waiting.push(resolve));
    else this.running++;
    const controller = new AbortController();
    this.controllers.add(controller);
    try {
      if (this.closed || this.clearing)
        throw new Error("Cache request cancelled.");
      const file = await this.storage.download(key, controller.signal);
      const entry = { ...file, key, touched: Date.now() };
      if (
        this.closed ||
        this.clearing ||
        [...this.validKeys].some(
          ([prefix, keys]) => key.startsWith(prefix) && !keys.has(key),
        ) ||
        file.bytes <= 0 ||
        file.bytes > maxImageBytes(key) ||
        file.bytes > this.budget
      ) {
        this.storage.remove(entry);
        throw new Error("Image could not be cached.");
      }
      this.entries.set(key, entry);
      this.prune();
      return entry.uri;
    } finally {
      this.controllers.delete(controller);
      const next = this.waiting.shift();
      if (next) next();
      else this.running--;
    }
  }

  private prune() {
    let bytes = this.stats.bytes;
    for (const entry of [...this.entries.values()].sort(
      (a, b) => a.touched - b.touched,
    )) {
      if (bytes <= this.budget) break;
      this.storage.remove(entry);
      this.entries.delete(entry.key);
      bytes -= entry.bytes;
    }
    this.storage.save([...this.entries.values()]);
  }

  /** Purge revisions/deletions only after a successful server catalog refresh. */
  reconcile(validKeys: Set<string>, prefix = "") {
    this.validKeys.set(prefix, validKeys);
    for (const [key, entry] of this.entries) {
      if (key.startsWith(prefix) && !validKeys.has(key)) {
        this.storage.remove(entry);
        this.entries.delete(key);
      }
    }
    this.storage.save([...this.entries.values()]);
  }

  forget(key: string) {
    const entry = this.entries.get(key);
    if (entry) {
      this.storage.remove(entry);
      this.entries.delete(key);
    }
    this.storage.save([...this.entries.values()]);
  }

  async clear(close = false) {
    this.closed ||= close;
    this.clearing = true;
    for (const controller of this.controllers) controller.abort();
    await Promise.allSettled([...this.pending.values()]);
    this.entries.clear();
    this.storage.clear();
    this.clearing = false;
  }
}
