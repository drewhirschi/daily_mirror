-- Account deletion is requested from the app or the website and carried out
-- by an operator with `daily-mirror-onboarding delete-account`, never
-- automatically. The row outlives the account as a record of what was asked
-- for and when it was done.
--
-- Expand-only: the previously deployed build never reads this table.

CREATE TABLE IF NOT EXISTS account_deletion_requests (
    user_id TEXT PRIMARY KEY,
    username TEXT NOT NULL,
    requested_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    fulfilled_at TEXT
);
