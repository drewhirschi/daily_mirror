-- Baseline: the schema this server shipped with before versioned migrations
-- existed. Production already has every object below, so each statement is
-- written to be a no-op against a database that is already current, and the
-- runner records the migration either way.
--
-- Every statement here must stay idempotent. See docs/deployment.md.

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL COLLATE NOCASE UNIQUE,
    display_name TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    household_id TEXT,
    person_id TEXT,
    household_role TEXT NOT NULL DEFAULT 'member',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Databases created by the pre-migration server built `users` without the
-- household columns and added them later. ADD COLUMN is idempotent here: the
-- runner ignores "duplicate column name".
ALTER TABLE users ADD COLUMN household_id TEXT;
ALTER TABLE users ADD COLUMN person_id TEXT;
ALTER TABLE users ADD COLUMN household_role TEXT NOT NULL DEFAULT 'member';

CREATE TABLE IF NOT EXISTS auth_sessions (
    token_hash TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS auth_sessions_user_expires
    ON auth_sessions(user_id, expires_at);

CREATE TABLE IF NOT EXISTS passkeys (
    credential_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    label TEXT NOT NULL,
    passkey_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_used_at TEXT,
    FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS passkeys_user
    ON passkeys(user_id, created_at);

CREATE TABLE IF NOT EXISTS auth_ceremonies (
    token_hash TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    user_id TEXT NOT NULL,
    state_json TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(user_id) REFERENCES users(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS auth_discoverable_ceremonies (
    token_hash TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    state_json TEXT NOT NULL,
    expires_at INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS auth_login_limits (
    key_hash TEXT PRIMARY KEY,
    failures INTEGER NOT NULL,
    window_started_at INTEGER NOT NULL,
    blocked_until INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS photos (
    id TEXT PRIMARY KEY,
    storage_key TEXT NOT NULL,
    captured_at TEXT NOT NULL,
    content_type TEXT NOT NULL DEFAULT 'image/jpeg',
    byte_size INTEGER,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'ready')),
    rotation_degrees INTEGER NOT NULL DEFAULT 0,
    thumbnail_status TEXT NOT NULL DEFAULT 'pending',
    media_revision INTEGER NOT NULL DEFAULT 0,
    flipbook_excluded INTEGER NOT NULL DEFAULT 0,
    device_id TEXT,
    source TEXT NOT NULL DEFAULT 'device',
    enrollment_person_id TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

ALTER TABLE photos ADD COLUMN thumbnail_status TEXT NOT NULL DEFAULT 'pending';
ALTER TABLE photos ADD COLUMN media_revision INTEGER NOT NULL DEFAULT 0;
ALTER TABLE photos ADD COLUMN flipbook_excluded INTEGER NOT NULL DEFAULT 0;
ALTER TABLE photos ADD COLUMN device_id TEXT;
ALTER TABLE photos ADD COLUMN source TEXT NOT NULL DEFAULT 'device';
ALTER TABLE photos ADD COLUMN enrollment_person_id TEXT;

CREATE INDEX IF NOT EXISTS photos_ready_captured_at
    ON photos(status, captured_at DESC);

CREATE TABLE IF NOT EXISTS photo_processing (
    photo_id TEXT NOT NULL,
    pipeline_version TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK(status IN ('pending', 'leased', 'complete', 'failed')),
    available_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    lease_token TEXT,
    leased_by TEXT,
    lease_expires_at TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    completed_at TEXT,
    PRIMARY KEY(photo_id, pipeline_version)
);

CREATE INDEX IF NOT EXISTS photo_processing_claim
    ON photo_processing(pipeline_version, status, available_at, lease_expires_at);

CREATE TABLE IF NOT EXISTS photo_analyses (
    photo_id TEXT NOT NULL,
    pipeline_version TEXT NOT NULL,
    oriented_width INTEGER NOT NULL,
    oriented_height INTEGER NOT NULL,
    original_sha256 TEXT,
    face_count INTEGER NOT NULL,
    processing_millis INTEGER NOT NULL,
    completed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY(photo_id, pipeline_version)
);

CREATE TABLE IF NOT EXISTS faces (
    id TEXT PRIMARY KEY,
    photo_id TEXT NOT NULL,
    pipeline_version TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    detector_confidence REAL NOT NULL,
    bounds_x REAL NOT NULL,
    bounds_y REAL NOT NULL,
    bounds_width REAL NOT NULL,
    bounds_height REAL NOT NULL,
    landmark_model TEXT NOT NULL,
    landmark_schema TEXT NOT NULL,
    landmarks_json TEXT NOT NULL,
    embedding_model TEXT NOT NULL,
    embedding BLOB NOT NULL,
    embedding_dimension INTEGER NOT NULL,
    person_id TEXT,
    identity_state TEXT NOT NULL DEFAULT 'unknown'
        CHECK(identity_state IN ('unknown', 'proposed', 'confirmed')),
    identity_source TEXT,
    identity_score REAL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(photo_id, pipeline_version, ordinal)
);

CREATE INDEX IF NOT EXISTS faces_photo
    ON faces(photo_id, pipeline_version, ordinal);

CREATE TABLE IF NOT EXISTS people (
    id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS people_display_name
    ON people(display_name COLLATE NOCASE);

CREATE INDEX IF NOT EXISTS faces_person
    ON faces(person_id, pipeline_version, photo_id);

CREATE TABLE IF NOT EXISTS households (
    id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    grid_size INTEGER NOT NULL DEFAULT 4
        CHECK(grid_size IN (4, 6)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS households_display_name
    ON households(display_name COLLATE NOCASE);

CREATE TABLE IF NOT EXISTS household_members (
    household_id TEXT NOT NULL,
    person_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    PRIMARY KEY(household_id, person_id),
    UNIQUE(household_id, position)
);

CREATE INDEX IF NOT EXISTS household_members_person
    ON household_members(person_id, household_id);

CREATE TABLE IF NOT EXISTS devices (
    device_id TEXT PRIMARY KEY,
    household_id TEXT NOT NULL,
    device_name TEXT NOT NULL,
    hardware TEXT NOT NULL,
    firmware_version TEXT NOT NULL,
    device_token_hash TEXT NOT NULL UNIQUE,
    claimed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_seen_at TEXT,
    released_at TEXT
);

CREATE INDEX IF NOT EXISTS devices_household
    ON devices(household_id, claimed_at DESC);

CREATE TABLE IF NOT EXISTS device_claim_tokens (
    token_hash TEXT PRIMARY KEY,
    household_id TEXT NOT NULL,
    created_by_user TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    consumed_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS device_claim_tokens_household
    ON device_claim_tokens(household_id, expires_at);

CREATE TABLE IF NOT EXISTS household_users (
    user_id TEXT PRIMARY KEY,
    household_id TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
