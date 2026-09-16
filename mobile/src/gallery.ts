import type { Photo } from "@daily-mirror/api";

export type Density = "year" | "month" | "day";
export const DEFAULT_DENSITY: Density = "day";
export const densityLabels = { day: "Days", month: "Months", year: "Years" };

// A deliberate pinch changes one level; small movements do not regroup the grid.
export function densityAfterPinch(density: Density, scale: number): Density {
  const levels: Density[] = ["year", "month", "day"];
  const step = scale < 0.8 ? -1 : scale > 1.25 ? 1 : 0;
  return levels[Math.max(0, Math.min(2, levels.indexOf(density) + step))];
}

export const columnsFor = (density: Density, width: number) =>
  density === "year"
    ? width > 700
      ? 10
      : 6
    : density === "month"
      ? width > 700
        ? 6
        : 3
      : width > 700
        ? 4
        : 2;

export function captureDate(id: string): Date | null {
  const match = /^(\d{4})(\d{2})(\d{2})T(\d{2})(\d{2})(\d{2})Z/.exec(id);
  if (!match) return null;
  const [, y, m, d, h, min, s] = match;
  const date = new Date(`${y}-${m}-${d}T${h}:${min}:${s}Z`);
  return Number.isNaN(date.getTime()) ? null : date;
}

export function photoDate(id: string) {
  const date = captureDate(id);
  return date
    ? date.toLocaleDateString(undefined, {
        month: "long",
        day: "numeric",
        year: "numeric",
      })
    : "Earlier photograph";
}

export function selectPhotos(
  photos: Photo[],
  from?: Date,
  to?: Date,
  personPhotoIds?: ReadonlySet<string>,
) {
  const start = from ? new Date(from).setHours(0, 0, 0, 0) : -Infinity;
  const end = to ? new Date(to).setHours(23, 59, 59, 999) : Infinity;
  return photos
    .filter((photo) => {
      if (personPhotoIds && !personPhotoIds.has(photo.id)) return false;
      const date = captureDate(photo.id);
      return date
        ? date.getTime() >= start && date.getTime() <= end
        : !from && !to;
    })
    .sort(
      (a, b) =>
        (captureDate(b.id)?.getTime() ?? 0) -
          (captureDate(a.id)?.getTime() ?? 0) || b.id.localeCompare(a.id),
    );
}

export function photoSections(
  photos: Photo[],
  density: Density,
  columns: number,
) {
  const groups: {
    key: string;
    title: string;
    count: number;
    data: Photo[][];
  }[] = [];
  for (const photo of photos) {
    const date = captureDate(photo.id);
    const key = date
      ? `${date.getFullYear()}${density !== "year" ? `-${date.getMonth()}` : ""}${density === "day" ? `-${date.getDate()}` : ""}`
      : "earlier";
    let group = groups[groups.length - 1];
    if (!group || group.key !== key) {
      group = {
        key,
        title: date
          ? date.toLocaleDateString(
              undefined,
              density === "year"
                ? { year: "numeric" }
                : density === "month"
                  ? { month: "long", year: "numeric" }
                  : { month: "long", day: "numeric", year: "numeric" },
            )
          : "Earlier",
        count: 0,
        data: [],
      };
      groups.push(group);
    }
    group.count++;
    let row = group.data[group.data.length - 1];
    if (!row || row.length === columns) {
      row = [];
      group.data.push(row);
    }
    row.push(photo);
  }
  return groups;
}

const shortDate = (date: Date) =>
  date.toLocaleDateString(undefined, { month: "short", day: "numeric" });

/** The Archive header chip reads as a date range, with the person in front. */
export function filterSummary(from?: Date, to?: Date, person?: string) {
  const range =
    !from && !to
      ? "All dates"
      : `${from ? shortDate(from) : "Start"} – ${to ? shortDate(to) : "Today"}`;
  return person ? `${person} · ${range}` : range;
}

/** Where a contain-fit image lands inside its frame, in frame coordinates. */
export function containedRect(
  frame: { width: number; height: number },
  image: { width: number; height: number },
) {
  if (image.width <= 0 || image.height <= 0)
    return { x: 0, y: 0, width: frame.width, height: frame.height };
  const scale = Math.min(
    frame.width / image.width,
    frame.height / image.height,
  );
  const width = image.width * scale;
  const height = image.height * scale;
  return {
    x: (frame.width - width) / 2,
    y: (frame.height - height) / 2,
    width,
    height,
  };
}

/** Shrink so a quarter-turned image still fits the same frame. */
export function rotatedFitScale(
  frame: { width: number; height: number },
  image: { width: number; height: number } | undefined,
  degrees: number,
) {
  if (degrees % 180 === 0) return 1;
  if (!image)
    return (
      Math.min(frame.width, frame.height) / Math.max(frame.width, frame.height)
    );
  const shown = containedRect(frame, image);
  return Math.min(1, frame.width / shown.height, frame.height / shown.width);
}
