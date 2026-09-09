import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_DENSITY,
  densityAfterPinch,
  photoSections,
  selectPhotos,
} from "../src/gallery";

const photos = [
  "20260906T120000Z-a",
  "20250905T120000Z-b",
  "20260907T120000Z-c",
].map((id) => ({ id, url: `/api/photos/${id}`, thumbnail_url: null }));
test("gallery orders captures and groups virtualized rows without dropping photos", () => {
  const ordered = selectPhotos(photos);
  assert.equal(ordered[0].id, "20260907T120000Z-c");
  const sections = photoSections(ordered, "year", 1);
  assert.deepEqual(
    sections.map((section) => section.count),
    [2, 1],
  );
  assert.equal(
    sections.flatMap((section) => section.data.flat()).length,
    photos.length,
  );
});
test("date filters include both selected local calendar days", () => {
  const result = selectPhotos(
    photos,
    new Date(2026, 8, 6),
    new Date(2026, 8, 7),
  );
  assert.equal(result.length, 2);
  assert.equal(selectPhotos(photos, new Date(2026, 8, 8)).length, 0);
});

test("person and date filters intersect without altering the full archive", () => {
  const matches = new Set([photos[0].id, photos[1].id]);
  assert.equal(selectPhotos(photos, undefined, undefined, matches).length, 2);
  assert.deepEqual(
    selectPhotos(
      photos,
      new Date(2026, 8, 6),
      new Date(2026, 8, 7),
      matches,
    ).map((p) => p.id),
    [photos[0].id],
  );
  assert.equal(selectPhotos(photos, undefined, undefined, new Set()).length, 0);
  assert.equal(selectPhotos(photos).length, 3);
});

test("archive starts in days and pinches through months and years in both directions", () => {
  assert.equal(DEFAULT_DENSITY, "day");
  const month = densityAfterPinch(DEFAULT_DENSITY, 0.7);
  assert.equal(month, "month");
  const year = densityAfterPinch(month, 0.7);
  assert.equal(year, "year");
  assert.equal(densityAfterPinch(year, 0.7), "year");
  assert.equal(densityAfterPinch(year, 1.4), "month");
  assert.equal(densityAfterPinch(month, 1.4), "day");
  assert.equal(densityAfterPinch("day", 1.4), "day");
  for (const scale of [0.85, 1, 1.2]) {
    assert.equal(densityAfterPinch("month", scale), "month");
  }
});
