import { useEffect, useMemo, useRef, useState } from "react";
import type { TouchEvent, WheelEvent } from "react";
import { useGetApiPhotos } from "@server/client/react-query";

type Density = "year" | "month" | "day";
type GalleryPhoto = { id: string; url: string; thumbnail_url?: string };
type PhotoGroup = { key: string; label: string; photos: GalleryPhoto[]; headingCount?: number };
const DENSITIES: Density[] = ["year", "month", "day"];

export default function Page() {
  const photos = useGetApiPhotos({ query: { refetchInterval: 10_000 } });
  const [demoMode, setDemoMode] = useState(false);
  const [density, setDensity] = useState<Density>("month");
  const [startDate, setStartDate] = useState("");
  const [endDate, setEndDate] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editVersion, setEditVersion] = useState(0);
  const [editError, setEditError] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [anchorId, setAnchorId] = useState<string | null>(null);
  const pinch = useRef<{ distance: number; anchorId: string | null } | null>(null);
  const viewportWidth = useViewportWidth();

  useEffect(() => setDemoMode(new URLSearchParams(window.location.search).get("demo") === "1"), []);

  const rawItems: GalleryPhoto[] = demoMode ? demoPhotos() : ((photos.data?.data.photos ?? []) as GalleryPhoto[]);
  const items = useMemo(
    () => [...rawItems].filter((photo) => isInDateRange(photo.id, startDate, endDate)).sort(compareNewestFirst),
    [rawItems, startDate, endDate],
  );
  const groups = useMemo(() => groupPhotos(items, density), [items, density]);
  const selectedPhoto = items.find((photo) => photo.id === selectedId);
  const selectedIndex = selectedPhoto ? items.indexOf(selectedPhoto) : -1;

  useEffect(() => {
    if (!selectedId) return;
    const closeOrNavigate = (event: KeyboardEvent) => {
      if (event.key === "Escape") setSelectedId(null);
      if (event.key === "ArrowLeft") selectNeighbor(-1);
      if (event.key === "ArrowRight") selectNeighbor(1);
    };
    document.body.classList.add("lightbox-open");
    window.addEventListener("keydown", closeOrNavigate);
    return () => {
      document.body.classList.remove("lightbox-open");
      window.removeEventListener("keydown", closeOrNavigate);
    };
  }, [selectedId, selectedIndex, items.length]);

  useEffect(() => {
    if (!anchorId) return;
    const frame = requestAnimationFrame(() => {
      document.querySelector<HTMLElement>(`[data-photo-id="${anchorId}"]`)?.scrollIntoView({ block: "center" });
      setAnchorId(null);
    });
    return () => cancelAnimationFrame(frame);
  }, [density, anchorId]);

  return (
    <main className="gallery-page">
      <header className="archive-heading">
        <h1>Archive</h1>
        <p>{photos.isLoading && !demoMode ? "Loading…" : `${items.length.toLocaleString()} photos`}</p>
      </header>

      <section className="gallery-toolbar" aria-label="Gallery controls">
        <div className="density-control" role="group" aria-label="Gallery density">
          {DENSITIES.map((value) => (
            <button key={value} type="button" aria-pressed={density === value} onClick={() => setDensity(value)}>
              {value === "year" ? "Years" : value === "month" ? "Months" : "Days"}
            </button>
          ))}
        </div>
        <details className="date-filter">
          <summary><span>{startDate || endDate ? "Date range" : "All dates"}</span><span aria-hidden="true">⌄</span></summary>
          <div className="date-range">
            <label><span>From</span><input type="date" value={startDate} max={endDate || undefined} onChange={(event) => setStartDate(event.target.value)} /></label>
            <label><span>To</span><input type="date" value={endDate} min={startDate || undefined} onChange={(event) => setEndDate(event.target.value)} /></label>
            {startDate || endDate ? <button className="clear-filter" type="button" onClick={() => { setStartDate(""); setEndDate(""); }}>Show all photos</button> : null}
          </div>
        </details>
        {demoMode ? <span className="demo-badge">Demo archive</span> : null}
      </section>

      {editError ? <p className="edit-error" role="alert">{editError}</p> : null}
      {photos.isError && !demoMode ? <p className="empty-state">The photo archive could not be loaded.</p>
        : items.length === 0 && !(photos.isLoading && !demoMode) ? <p className="empty-state">{rawItems.length ? "No photographs were taken in this date range." : "The first photograph will appear here after the button is pressed."}</p>
        : <div className="photo-archive" data-density={density} aria-label="Photograph archive" onTouchStart={startPinch} onTouchMove={movePinch} onTouchEnd={() => { pinch.current = null; }} onWheel={zoomWithTrackpad}>
          {groups.map((group) => <VirtualGroup key={group.key} group={group} density={density} viewportWidth={viewportWidth} anchorId={anchorId} editVersion={editVersion} onSelect={setSelectedId} />)}
        </div>}

      {selectedPhoto ? (
        <div className="lightbox" role="dialog" aria-modal="true" aria-label={`Photograph from ${formatCaptureId(selectedPhoto.id)}`} onClick={(event) => { if (event.target === event.currentTarget) setSelectedId(null); }}>
          <button className="lightbox-nav lightbox-newer" type="button" aria-label="Newer photograph" disabled={selectedIndex <= 0} onClick={() => selectNeighbor(-1)}>‹</button>
          <button className="lightbox-nav lightbox-older" type="button" aria-label="Older photograph" disabled={selectedIndex >= items.length - 1} onClick={() => selectNeighbor(1)}>›</button>
          <figure className="lightbox-frame">
            <button className="lightbox-close" type="button" aria-label="Close expanded photograph" onClick={() => setSelectedId(null)}>×</button>
            <details className="lightbox-tools">
              <summary aria-label="Photo actions">•••</summary>
              <div className="lightbox-tools-menu">
                <p>Photo actions</p>
                <div className="photo-actions" aria-label="Edit photograph">
                  <button type="button" disabled={demoMode || editingId === selectedPhoto.id} onClick={() => editPhoto(selectedPhoto.id, "rotate-left")}><span aria-hidden="true">↶</span> Rotate left</button>
                  <button type="button" disabled={demoMode || editingId === selectedPhoto.id} onClick={() => editPhoto(selectedPhoto.id, "rotate-right")}><span aria-hidden="true">↷</span> Rotate right</button>
                  <button className="delete-photo" type="button" disabled={demoMode || editingId === selectedPhoto.id} onClick={() => editPhoto(selectedPhoto.id, "delete")}>Delete photo</button>
                </div>
                {demoMode ? <small>Editing is disabled in the demo.</small> : null}
              </div>
            </details>
            <img src={`${selectedPhoto.url}?v=${editVersion}`} alt={`Daily Mirror capture from ${formatCaptureId(selectedPhoto.id)}`} />
            <div className="lightbox-meta">
              <figcaption>{formatCaptureId(selectedPhoto.id)}</figcaption>
            </div>
          </figure>
        </div>
      ) : null}
    </main>
  );

  function zoom(direction: 1 | -1, photoId: string | null = null) {
    const current = DENSITIES.indexOf(density);
    const next = Math.max(0, Math.min(DENSITIES.length - 1, current + direction));
    if (next === current) return;
    setAnchorId(photoId);
    setDensity(DENSITIES[next]);
  }

  function startPinch(event: TouchEvent) {
    if (event.touches.length !== 2) return;
    const first = event.touches[0];
    const second = event.touches[1];
    const x = (first.clientX + second.clientX) / 2;
    const y = (first.clientY + second.clientY) / 2;
    pinch.current = { distance: Math.hypot(first.clientX - second.clientX, first.clientY - second.clientY), anchorId: document.elementFromPoint(x, y)?.closest<HTMLElement>("[data-photo-id]")?.dataset.photoId ?? null };
  }

  function movePinch(event: TouchEvent) {
    if (event.touches.length !== 2 || !pinch.current) return;
    const distance = Math.hypot(event.touches[0].clientX - event.touches[1].clientX, event.touches[0].clientY - event.touches[1].clientY);
    const ratio = distance / pinch.current.distance;
    if (ratio > 1.18) { zoom(1, pinch.current.anchorId); pinch.current.distance = distance; }
    else if (ratio < 0.84) { zoom(-1, pinch.current.anchorId); pinch.current.distance = distance; }
  }

  function zoomWithTrackpad(event: WheelEvent) {
    if (!event.ctrlKey) return;
    event.preventDefault();
    const targetId = (event.target as HTMLElement).closest<HTMLElement>("[data-photo-id]")?.dataset.photoId ?? null;
    zoom(event.deltaY < 0 ? 1 : -1, targetId);
  }

  async function editPhoto(id: string, action: "rotate-left" | "rotate-right" | "delete") {
    if (action === "delete" && !window.confirm("Permanently delete this photograph?")) return;
    setEditingId(id); setEditError("");
    try {
      const response = await fetch(`/api/photos/${encodeURIComponent(id)}`, action === "delete" ? { method: "DELETE" } : { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify({ degrees: action === "rotate-left" ? -90 : 90 }) });
      if (!response.ok) throw new Error(`The server returned ${response.status}.`);
      if (action === "delete") { setSelectedId(null); await photos.refetch(); } else setEditVersion((version) => version + 1);
    } catch (error) {
      setEditError(error instanceof Error ? `The photograph could not be edited. ${error.message}` : "The photograph could not be edited.");
    } finally { setEditingId(null); }
  }

  function selectNeighbor(offset: number) {
    const next = items[selectedIndex + offset];
    if (next) setSelectedId(next.id);
  }
}

function VirtualGroup({ group, density, viewportWidth, anchorId, editVersion, onSelect }: { group: PhotoGroup; density: Density; viewportWidth: number; anchorId: string | null; editVersion: number; onSelect: (id: string) => void }) {
  const section = useRef<HTMLElement>(null);
  const [nearViewport, setNearViewport] = useState(false);
  const containsAnchor = anchorId ? group.photos.some((photo) => photo.id === anchorId) : false;
  useEffect(() => {
    const node = section.current;
    if (!node) return;
    const observer = new IntersectionObserver(([entry]) => setNearViewport(entry.isIntersecting), { rootMargin: "900px 0px" });
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  return <section className="photo-period" ref={section} aria-labelledby={density !== "day" && group.label ? `period-${group.key}` : undefined} aria-label={!group.label || density === "day" ? "Photographs" : undefined}>
    {density !== "day" && group.label ? <header className="period-heading"><h2 id={`period-${group.key}`}>{group.label}</h2><span>{group.headingCount ?? group.photos.length}</span></header> : null}
    {nearViewport || containsAnchor ? <div className="photo-grid">{group.photos.map((photo) => <figure className="photo-card" key={photo.id} data-photo-id={photo.id}>
      <button className="photo-open" type="button" aria-label={`Open photograph from ${formatCaptureId(photo.id)}`} onClick={() => onSelect(photo.id)}>
        <img src={`${photo.thumbnail_url ?? photo.url}?v=${editVersion}`} alt="" width="320" height="240" loading="lazy" decoding="async" />
      </button>
      {density === "day" ? <figcaption>{formatCaptureDay(photo.id)}</figcaption> : null}
    </figure>)}</div> : <div className="virtual-spacer" style={{ height: estimateGridHeight(group.photos.length, density, viewportWidth) }} aria-hidden="true" />}
  </section>;
}

function groupPhotos(photos: GalleryPhoto[], density: Density): PhotoGroup[] {
  const groups: PhotoGroup[] = [];
  const labeledYears = new Set<number>();
  const yearCounts = new Map<number, number>();
  if (density === "year") {
    for (const photo of photos) {
      const year = captureDate(photo.id)?.getFullYear();
      if (year !== undefined) yearCounts.set(year, (yearCounts.get(year) ?? 0) + 1);
    }
  }
  for (const photo of photos) {
    const date = captureDate(photo.id);
    const key = date ? `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}` : "unknown";
    const previous = groups[groups.length - 1];
    if (previous?.key === key) previous.photos.push(photo);
    else {
      let label = "Earlier";
      if (date && density === "year") {
        label = labeledYears.has(date.getFullYear()) ? "" : String(date.getFullYear());
        labeledYears.add(date.getFullYear());
      } else if (date) {
        label = new Intl.DateTimeFormat(undefined, { month: "long", year: "numeric" }).format(date);
      }
      groups.push({ key, label, photos: [photo], headingCount: date && label && density === "year" ? yearCounts.get(date.getFullYear()) : undefined });
    }
  }
  return groups;
}

function estimateGridHeight(count: number, density: Density, viewportWidth: number) {
  const width = Math.min(1480, Math.max(320, viewportWidth - 32));
  const mobile = viewportWidth < 640;
  const columns = density === "year" ? (mobile ? 12 : 28) : density === "month" ? (mobile ? 5 : 14) : (mobile ? 2 : 4);
  const gap = density === "day" ? 4 : 3;
  const cardWidth = (width - gap * (columns - 1)) / columns;
  const imageHeight = density === "day" ? cardWidth * 0.75 : cardWidth;
  return Math.ceil(count / columns) * (imageHeight + gap + (density === "day" ? 34 : 0));
}

function useViewportWidth() {
  const [width, setWidth] = useState(1024);
  useEffect(() => { const update = () => setWidth(window.innerWidth); update(); window.addEventListener("resize", update); return () => window.removeEventListener("resize", update); }, []);
  return width;
}

let demoCache: GalleryPhoto[] | null = null;
function demoPhotos() {
  if (demoCache) return demoCache;
  const result: GalleryPhoto[] = [];
  const cursor = new Date(); cursor.setUTCHours(18, 30, 0, 0);
  for (let day = 0; day < 3650; day += 1) {
    const id = `${cursor.getUTCFullYear()}${String(cursor.getUTCMonth() + 1).padStart(2, "0")}${String(cursor.getUTCDate()).padStart(2, "0")}T183000Z-demo${String(day).padStart(4, "0")}`;
    result.push({ id, url: "/demo-photo.svg", thumbnail_url: "/demo-thumbnail.webp" });
    cursor.setUTCDate(cursor.getUTCDate() - 1);
  }
  demoCache = result;
  return result;
}

function isInDateRange(id: string, startDate: string, endDate: string) {
  const timestamp = captureTime(id);
  if (!timestamp) return !startDate && !endDate;
  const start = startDate ? Date.parse(`${startDate}T00:00:00`) : Number.NEGATIVE_INFINITY;
  const end = endDate ? Date.parse(`${endDate}T23:59:59.999`) : Number.POSITIVE_INFINITY;
  return timestamp >= start && timestamp <= end;
}
function compareNewestFirst(left: { id: string }, right: { id: string }) { return captureTime(right.id) - captureTime(left.id) || right.id.localeCompare(left.id); }
function captureTime(id: string) { return captureDate(id)?.getTime() ?? 0; }
function captureDate(id: string) {
  const match = id.match(/^(\d{4})(\d{2})(\d{2})T(\d{2})(\d{2})(\d{2})Z/);
  if (!match) return null;
  const [, year, month, day, hour, minute, second] = match;
  return new Date(`${year}-${month}-${day}T${hour}:${minute}:${second}Z`);
}
function formatCaptureId(id: string) { const date = captureDate(id); return date ? new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(date) : id; }
function formatCaptureDay(id: string) { const date = captureDate(id); return date ? new Intl.DateTimeFormat(undefined, { weekday: "short", day: "numeric" }).format(date) : id; }
