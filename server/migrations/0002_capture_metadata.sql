-- Per-photo capture metadata, so a bad photograph can be traced to the camera
-- and the settings that took it. Every column is nullable: each sensor
-- reports what it can, and photographs taken before this release report
-- nothing. Units are documented in docs/capture-metadata.md.
--
-- Expand-only: the previously deployed build keeps serving while this runs,
-- and it simply never reads these columns.

ALTER TABLE photos ADD COLUMN firmware_version TEXT;
ALTER TABLE photos ADD COLUMN sensor TEXT;
ALTER TABLE photos ADD COLUMN width INTEGER;
ALTER TABLE photos ADD COLUMN height INTEGER;
ALTER TABLE photos ADD COLUMN jpeg_quality INTEGER;
ALTER TABLE photos ADD COLUMN exposure_us INTEGER;
ALTER TABLE photos ADD COLUMN analog_gain REAL;
ALTER TABLE photos ADD COLUMN digital_gain REAL;
ALTER TABLE photos ADD COLUMN af_state TEXT;
ALTER TABLE photos ADD COLUMN lens_position REAL;
ALTER TABLE photos ADD COLUMN colour_temperature_k INTEGER;
ALTER TABLE photos ADD COLUMN mean_luma INTEGER;
ALTER TABLE photos ADD COLUMN focus_score INTEGER;
ALTER TABLE photos ADD COLUMN trigger TEXT;
ALTER TABLE photos ADD COLUMN capture_source TEXT;
