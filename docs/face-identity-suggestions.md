# Face identity suggestions

In Faces → Processing, click **Find suggestions** to compare existing
unconfirmed faces against known people. New image results are matched when
the processor completes them. Saving manual assignments refreshes suggestions
using the updated examples.

A suggested face shows the person's name and cosine similarity. This score
is not a calibrated probability. Click **Confirm [name]** to save immediately.
You can also choose a different person in the dropdown, then use **Save changes**
in the toolbar. Only after saving does
the face become confirmed, appear in that person's flipbook, and contribute to
future matching. Discard cancels staged edits.

Choosing Unknown and saving explicitly rejects automatic assignment for that
face. Future suggestion refreshes preserve that decision. Assigning a person
manually remains possible at any time.

Unconfirmed faces offer **Remove bad detection** for a box/mesh that is not a usable face, then
**Save changes**. Undo removal or Discard cancels before saving. Saved removals
are hidden from diagnostics and excluded from matching, counts and flipbooks;
the original photograph and detection record remain intact. An administrator
can restore a saved detection by assigning its ID through the existing face
assignment API. Explicitly reprocessing a photo may create new detections;
removal is attached to the current detection record.

To refresh from a server shell with the catalog environment already loaded,
run `cargo run --locked --bin suggest_faces` from `server/`.

## Matching policy

- Enrollment requires five distinct manually confirmed photos across three
  capture dates (using the stored date in `photos.captured_at`).
- Profiles use normalized centroids of normalized SFace embeddings. One face
  per person per photo contributes, preventing duplicate detections from
  counting as independent examples.
- Comparisons stay within the same pipeline, embedding model and dimension.
- Minimum cosine similarity is 0.65, with a 0.15 lead over the next person.
  These are initial conservative thresholds, not validated accuracy guarantees.
- People with fewer examples still compete; they cannot receive suggestions
  until enrolled, but prevent weak matches being assigned to other people.
- Only `confirmed` faces with source `manual` teach profiles. Confirming a
  suggestion uses this same manual path. Proposed faces never teach themselves.
- Suggestions store state `proposed`, source `centroid-v1`, and the similarity
  in `identity_score`. Manual confirmation clears that score and stores source
  `manual`. Manual Unknown stores source `manual-rejected`.
- Recomputing suggestions replaces or clears obsolete proposals while
  preserving confirmed faces and explicit rejections. All reads and writes
  run within the same write transaction to avoid races with label changes.

The server performs matching using stored embeddings; reprocessing JPEGs or
loading OpenCV/MediaPipe into the web server is unnecessary. The explicit
refresh endpoint is `POST /api/admin/faces/suggest`, protected by the existing
application session middleware. It does not run on dashboard GET requests.

Validate suggestions against held-out days and unknown visitors before
considering automatic confirmation. Correct detector/landmark failures before
treating a resulting embedding as a trusted example.
