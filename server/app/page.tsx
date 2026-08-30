import { useEffect, useState } from "react";
import { useGetApiPhotos } from "@server/client/react-query";

export default function Page() {
  const photos = useGetApiPhotos({ query: { refetchInterval: 10_000 } });
  const rawItems = photos.data?.data.photos ?? [];
  const [startDate, setStartDate] = useState("");
  const [endDate, setEndDate] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editVersion, setEditVersion] = useState(0);
  const [editError, setEditError] = useState("");
  const items = [...rawItems]
    .filter((photo) => isInDateRange(photo.id, startDate, endDate))
    .sort(compareNewestFirst);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const selectedPhoto = items.find((photo) => photo.id === selectedId);

  useEffect(() => {
    if (!selectedId) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setSelectedId(null);
    };
    document.body.classList.add("lightbox-open");
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      document.body.classList.remove("lightbox-open");
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [selectedId]);

  return (
    <main className="gallery-page">
      <header className="gallery-heading">
        <div>
          <p className="eyebrow">A photograph at a time</p>
          <h1>Daily Mirror</h1>
        </div>
        <p className="photo-count">
          {photos.isLoading ? "Loading…" : `${items.length} ${items.length === 1 ? "photo" : "photos"}`}
        </p>
      </header>

      <section className="gallery-tools" aria-label="Filter photographs">
        <div className="date-range">
          <label>
            <span>From</span>
            <input
              type="date"
              value={startDate}
              max={endDate || undefined}
              onChange={(event) => setStartDate(event.target.value)}
            />
          </label>
          <span className="date-separator" aria-hidden="true">—</span>
          <label>
            <span>To</span>
            <input
              type="date"
              value={endDate}
              min={startDate || undefined}
              onChange={(event) => setEndDate(event.target.value)}
            />
          </label>
          {startDate || endDate ? (
            <button className="clear-filter" type="button" onClick={() => { setStartDate(""); setEndDate(""); }}>
              Clear
            </button>
          ) : null}
        </div>
        {editError ? <p className="edit-error" role="alert">{editError}</p> : null}
      </section>

      {photos.isError ? (
        <p className="empty-state">The photo archive could not be loaded.</p>
      ) : items.length === 0 && !photos.isLoading ? (
        <p className="empty-state">
          {rawItems.length ? "No photographs were taken in this date range." : "The first photograph will appear here after the button is pressed."}
        </p>
      ) : (
        <section className="photo-grid" aria-label="Photograph archive">
          {items.map((photo) => (
            <figure className="photo-card" key={photo.id}>
              <button
                className="photo-open"
                type="button"
                aria-label={`Open photograph from ${formatCaptureId(photo.id)}`}
                onClick={() => setSelectedId(photo.id)}
              >
                <img src={`${photo.url}?v=${editVersion}`} alt="Daily Mirror capture" loading="lazy" />
              </button>
              <figcaption>{formatCaptureId(photo.id)}</figcaption>
            </figure>
          ))}
        </section>
      )}

      {selectedPhoto ? (
        <div
          className="lightbox"
          role="dialog"
          aria-modal="true"
          aria-label={`Photograph from ${formatCaptureId(selectedPhoto.id)}`}
          onClick={(event) => {
            if (event.target === event.currentTarget) setSelectedId(null);
          }}
        >
          <figure className="lightbox-frame">
            <button
              className="lightbox-close"
              type="button"
              aria-label="Close expanded photograph"
              onClick={() => setSelectedId(null)}
            >
              ×
            </button>
            <img
              src={`${selectedPhoto.url}?v=${editVersion}`}
              alt={`Daily Mirror capture from ${formatCaptureId(selectedPhoto.id)}`}
            />
            <figcaption>{formatCaptureId(selectedPhoto.id)}</figcaption>
            <div className="photo-actions" aria-label="Edit photograph">
              <button type="button" disabled={editingId === selectedPhoto.id} onClick={() => editPhoto(selectedPhoto.id, "rotate-left")}>↶ Rotate left</button>
              <button type="button" disabled={editingId === selectedPhoto.id} onClick={() => editPhoto(selectedPhoto.id, "rotate-right")}>↷ Rotate right</button>
              <button className="delete-photo" type="button" disabled={editingId === selectedPhoto.id} onClick={() => editPhoto(selectedPhoto.id, "delete")}>Delete</button>
            </div>
          </figure>
        </div>
      ) : null}
    </main>
  );

  async function editPhoto(id: string, action: "rotate-left" | "rotate-right" | "delete") {
    if (action === "delete" && !window.confirm("Permanently delete this photograph?")) return;
    setEditingId(id);
    setEditError("");
    try {
      const response = await fetch(`/api/photos/${encodeURIComponent(id)}`, action === "delete" ? {
        method: "DELETE",
      } : {
        method: "PATCH",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ degrees: action === "rotate-left" ? -90 : 90 }),
      });
      if (!response.ok) throw new Error(`The server returned ${response.status}.`);
      if (action === "delete") {
        setSelectedId(null);
        await photos.refetch();
      } else {
        setEditVersion((version) => version + 1);
      }
    } catch (error) {
      setEditError(error instanceof Error ? `The photograph could not be edited. ${error.message}` : "The photograph could not be edited.");
    } finally {
      setEditingId(null);
    }
  }
}

function isInDateRange(id: string, startDate: string, endDate: string) {
  const timestamp = captureTime(id);
  if (!timestamp) return !startDate && !endDate;
  const start = startDate ? Date.parse(`${startDate}T00:00:00`) : Number.NEGATIVE_INFINITY;
  const end = endDate ? Date.parse(`${endDate}T23:59:59.999`) : Number.POSITIVE_INFINITY;
  return timestamp >= start && timestamp <= end;
}

function compareNewestFirst(left: { id: string }, right: { id: string }) {
  const timeDifference = captureTime(right.id) - captureTime(left.id);
  return timeDifference || right.id.localeCompare(left.id);
}

function captureTime(id: string) {
  const match = id.match(/^(\d{4})(\d{2})(\d{2})T(\d{2})(\d{2})(\d{2})Z/);
  if (!match) return 0;
  const [, year, month, day, hour, minute, second] = match;
  return Date.parse(`${year}-${month}-${day}T${hour}:${minute}:${second}Z`);
}

function formatCaptureId(id: string) {
  const match = id.match(/^(\d{4})(\d{2})(\d{2})T(\d{2})(\d{2})(\d{2})Z/);
  if (!match) return id;
  const [, year, month, day, hour, minute, second] = match;
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(`${year}-${month}-${day}T${hour}:${minute}:${second}Z`));
}
