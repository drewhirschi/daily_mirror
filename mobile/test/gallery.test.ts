import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_DENSITY,
  containedRect,
  densityAfterPinch,
  filterSummary,
  photoSections,
  rotatedFitScale,
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
test("header filter summary shows dates, with the person in front", () => {
  assert.equal(filterSummary(), "All dates");
  assert.match(filterSummary(new Date(2026, 8, 1)), /^Sep 1 – Today$/);
  assert.match(
    filterSummary(undefined, new Date(2026, 8, 15), "Drew"),
    /^Drew · Start – Sep 15$/,
  );
});
test("face overlays and rotations follow the contain-fit image", () => {
  const frame = { width: 400, height: 800 };
  const landscape = { width: 4000, height: 3000 };
  assert.deepEqual(containedRect(frame, landscape), {
    x: 0,
    y: 250,
    width: 400,
    height: 300,
  });
  assert.equal(rotatedFitScale(frame, landscape, 0), 1);
  // A 400×300 image turned on its side is 300×400, which already fits.
  assert.equal(rotatedFitScale(frame, landscape, 90), 1);
  // A square image at full width must shrink to fit the width when turned.
  assert.equal(
    rotatedFitScale(
      { width: 400, height: 300 },
      { width: 10, height: 10 },
      -90,
    ),
    1,
  );
  // A tall 80×400 image turned on its side is 400 wide, over a 300 frame.
  assert.equal(
    rotatedFitScale(
      { width: 300, height: 400 },
      { width: 20, height: 100 },
      90,
    ),
    0.75,
  );
  assert.equal(
    rotatedFitScale({ width: 300, height: 400 }, undefined, 90),
    0.75,
  );
});
