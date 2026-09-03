import { FormEvent, useEffect, useState } from "react";
import {
  useGetApiAdminFaces,
  useGetApiAdminPeople,
  usePatchApiAdminFaces,
  usePostApiAdminPeople,
} from "@server/client/react-query";
import type { AdminFace, AdminFaceDashboard, AdminPhoto, PersonFlipbook } from "@server/client/react-query";

type AdminView = "processing" | "people" | "flipbooks";
type AssignmentMap = Record<string, string>;

export default function AdminPage() {
  const dashboard = useGetApiAdminFaces({
    query: { refetchInterval: 30_000, refetchIntervalInBackground: false },
  });
  const people = useGetApiAdminPeople({ query: { staleTime: 10_000 } });
  const createPerson = usePostApiAdminPeople();
  const assignFaces = usePatchApiAdminFaces();
  const [view, setView] = useState<AdminView>("processing");
  const [newPersonName, setNewPersonName] = useState("");
  const [selectedPersonId, setSelectedPersonId] = useState("");
  const [frameIndex, setFrameIndex] = useState(0);
  const [message, setMessage] = useState("");
  const [assignmentEdits, setAssignmentEdits] = useState<AssignmentMap>({});
  const [savedAssignments, setSavedAssignments] = useState<AssignmentMap>({});
  const [saveError, setSaveError] = useState("");
  const [showBoundingBoxes, setShowBoundingBoxes] = useState(true);
  const [showLandmarks, setShowLandmarks] = useState(true);

  const dashboardData = dashboard.data?.data;
  const peopleData = people.data?.data.people ?? [];
  const selectedPerson = peopleData.find((person) => person.id === selectedPersonId) ?? peopleData[0];
  const editCount = Object.keys(assignmentEdits).length;

  useEffect(() => {
    if (!selectedPersonId && peopleData[0]) setSelectedPersonId(peopleData[0].id);
  }, [peopleData.length, selectedPersonId]);

  useEffect(() => {
    const requestedView = new URLSearchParams(window.location.search).get("view");
    if (requestedView === "people" || requestedView === "flipbooks") setView(requestedView);
  }, []);

  useEffect(() => setFrameIndex(0), [selectedPerson?.id]);

  useEffect(() => {
    if (editCount === 0) return;
    const warnAboutUnsavedEdits = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener("beforeunload", warnAboutUnsavedEdits);
    return () => window.removeEventListener("beforeunload", warnAboutUnsavedEdits);
  }, [editCount]);

  return (
    <main className="admin-page">
      <header className="admin-heading">
        <div>
          <p className="eyebrow">Private tools</p>
          <h1>Face lab</h1>
          <p>Inspect processing geometry, manage known people, and compare daily face studies.</p>
        </div>
        {dashboardData ? <span className="pipeline-pill">{dashboardData.pipeline_version}</span> : null}
      </header>

      <nav className="admin-tabs" aria-label="Face administration views">
        <button type="button" aria-pressed={view === "processing"} onClick={() => switchView("processing")}>Processing</button>
        <button type="button" aria-pressed={view === "people"} onClick={() => switchView("people")}>People</button>
        <button type="button" aria-pressed={view === "flipbooks"} onClick={() => switchView("flipbooks")}>Flipbooks</button>
      </nav>

      {message ? <p className="admin-message" role="status">{message}</p> : null}
      {dashboard.isError || people.isError ? <p className="admin-error" role="alert">The face administration data could not be loaded.</p> : null}

      {view === "processing" ? (
        <ProcessingView
          loading={dashboard.isLoading}
          data={dashboardData}
          people={peopleData}
          edits={assignmentEdits}
          savedAssignments={savedAssignments}
          showBoundingBoxes={showBoundingBoxes}
          showLandmarks={showLandmarks}
          onEdit={queueAssignment}
        />
      ) : view === "people" ? (
        <section className="people-directory">
          <header>
            <div><p className="eyebrow">Known identities</p><h2>People</h2></div>
            <p>Create people here. Renaming and merging will live here once their data-safe behavior is defined.</p>
          </header>
          <div className="people-directory-body">
            <form className="new-person-form" onSubmit={addPerson}>
              <label htmlFor="new-person">New person</label>
              <div>
                <input id="new-person" value={newPersonName} maxLength={80} placeholder="Name" onChange={(event) => setNewPersonName(event.target.value)} />
                <button type="submit" disabled={createPerson.isPending || !newPersonName.trim()}>Add</button>
              </div>
            </form>
            <div className="people-list" role="list" aria-label="Known people">
              {peopleData.map((person) => (
                <button key={person.id} type="button" role="listitem" aria-pressed={person.id === selectedPerson?.id} onClick={() => setSelectedPersonId(person.id)}>
                  <span>{person.display_name}</span>
                  <small>{person.day_count} {person.day_count === 1 ? "day" : "days"} · {person.face_count} faces</small>
                </button>
              ))}
              {!people.isLoading && peopleData.length === 0 ? <p>No people yet. Name someone here, then assign their faces in Processing.</p> : null}
            </div>
          </div>
        </section>
      ) : (
        <section className="flipbook-workbench">
          <label className="flipbook-person-picker">
            <span>Person</span>
            <select value={selectedPerson?.id ?? ""} onChange={(event) => setSelectedPersonId(event.target.value)}>
              {peopleData.map((person) => <option key={person.id} value={person.id}>{person.display_name}</option>)}
            </select>
          </label>
          <Flipbook person={selectedPerson} frameIndex={frameIndex} onFrameChange={setFrameIndex} />
        </section>
      )}
      {view === "processing" ? (
        <div className="face-edit-toolbar" role="region" aria-label="Processing display and face assignments">
          <fieldset className="overlay-toggles">
            <legend>Show</legend>
            <label><input type="checkbox" checked={showBoundingBoxes} onChange={(event) => setShowBoundingBoxes(event.target.checked)} /> Bounding boxes</label>
            <label><input type="checkbox" checked={showLandmarks} onChange={(event) => setShowLandmarks(event.target.checked)} /> Landmarks</label>
          </fieldset>
          {editCount > 0 ? (
            <>
              <div className="face-edit-summary">
                <strong>{editCount} unsaved {editCount === 1 ? "change" : "changes"}</strong>
                <span>{saveError || "Selections stay on this device until you save them."}</span>
              </div>
              <button className="quiet-button" type="button" disabled={assignFaces.isPending} onClick={discardAssignments}>Discard</button>
              <button className="save-face-edits" type="button" disabled={assignFaces.isPending} onClick={() => void saveAssignments()}>
                {assignFaces.isPending ? "Saving…" : "Save changes"}
              </button>
            </>
          ) : null}
        </div>
      ) : null}
    </main>
  );

  async function addPerson(event: FormEvent) {
    event.preventDefault();
    setMessage("");
    try {
      const response = await createPerson.mutateAsync({ data: { display_name: newPersonName } });
      setNewPersonName("");
      setSelectedPersonId(response.data.id);
      await people.refetch();
      setMessage(`${response.data.display_name} is ready for face assignments.`);
    } catch {
      setMessage("That person could not be created.");
    }
  }

  function switchView(next: AdminView) {
    setView(next);
    const url = new URL(window.location.href);
    if (next === "people" || next === "flipbooks") url.searchParams.set("view", next);
    else url.searchParams.delete("view");
    window.history.replaceState(null, "", url);
  }

  function queueAssignment(face: AdminFace, personId: string) {
    setMessage("");
    setSaveError("");
    const savedPersonId = savedAssignments[face.id] ?? face.person_id ?? "";
    setAssignmentEdits((current) => {
      const next = { ...current };
      if (personId === savedPersonId) delete next[face.id];
      else next[face.id] = personId;
      return next;
    });
  }

  function discardAssignments() {
    setAssignmentEdits({});
    setSaveError("");
  }

  async function saveAssignments() {
    const submitted = { ...assignmentEdits };
    const assignments = Object.entries(submitted).map(([face_id, personId]) => ({
      face_id,
      person_id: personId || null,
    }));
    if (assignments.length === 0) return;
    setSaveError("");
    try {
      await assignFaces.mutateAsync({ data: { assignments } });
      setSavedAssignments((current) => ({ ...current, ...submitted }));
      setAssignmentEdits((current) => {
        const next = { ...current };
        for (const [faceId, personId] of Object.entries(submitted)) {
          if (next[faceId] === personId) delete next[faceId];
        }
        return next;
      });
      setMessage(`Saved ${assignments.length} face ${assignments.length === 1 ? "assignment" : "assignments"}.`);
      void Promise.all([dashboard.refetch(), people.refetch()]).then(() => {
        setSavedAssignments((current) => {
          const next = { ...current };
          for (const faceId of Object.keys(submitted)) delete next[faceId];
          return next;
        });
      });
    } catch {
      setSaveError("Nothing was saved. Check the connection and try again.");
    }
  }

}

function ProcessingView({ loading, data, people, edits, savedAssignments, showBoundingBoxes, showLandmarks, onEdit }: {
  loading: boolean;
  data: AdminFaceDashboard | undefined;
  people: PersonFlipbook[];
  edits: AssignmentMap;
  savedAssignments: AssignmentMap;
  showBoundingBoxes: boolean;
  showLandmarks: boolean;
  onEdit: (face: AdminFace, personId: string) => void;
}) {
  if (loading && !data) return <p className="admin-empty">Loading processing data…</p>;
  if (!data) return null;
  const metrics = [
    ["Pending", data.queue.pending],
    ["Leased", data.queue.leased],
    ["Complete", data.queue.complete],
    ["Failed", data.queue.failed],
    ["Faces", data.summary.detected_faces],
    ["Unknown", data.summary.unknown_faces],
  ];
  return (
    <section className="processing-view">
      <div className="admin-metrics" aria-label="Processing summary">
        {metrics.map(([label, value]) => <article key={label}><span>{label}</span><strong>{Number(value).toLocaleString()}</strong></article>)}
      </div>
      <div className="admin-section-heading">
        <div><p className="eyebrow">Most recent 60</p><h2>Photo diagnostics</h2></div>
        <p>Boxes and facial contours use the normalized geometry stored with each result.</p>
      </div>
      <div className="debug-photo-list">
        {data.photos.map((photo) => <DebugPhoto key={photo.id} photo={photo} people={people} edits={edits} savedAssignments={savedAssignments} showBoundingBoxes={showBoundingBoxes} showLandmarks={showLandmarks} onEdit={onEdit} />)}
        {data.photos.length === 0 ? <p className="admin-empty">No photos are queued yet.</p> : null}
      </div>
    </section>
  );
}

function DebugPhoto({ photo, people, edits, savedAssignments, showBoundingBoxes, showLandmarks, onEdit }: {
  photo: AdminPhoto;
  people: PersonFlipbook[];
  edits: AssignmentMap;
  savedAssignments: AssignmentMap;
  showBoundingBoxes: boolean;
  showLandmarks: boolean;
  onEdit: (face: AdminFace, personId: string) => void;
}) {
  return (
    <article className="debug-photo" data-status={photo.status}>
      <header>
        <div>
          <span className="processing-state">{photo.status}</span>
          <strong>{formatTimestamp(photo.captured_at)}</strong>
        </div>
        <span className="face-count-pill" data-empty={photo.faces.length === 0}>{photo.faces.length === 0 ? "No face" : `${photo.faces.length} ${photo.faces.length === 1 ? "face" : "faces"}`}</span>
      </header>
      <div className="debug-photo-body">
        <a className="face-debug-link" href={photo.photo_url} aria-label={`Open original capture from ${formatTimestamp(photo.captured_at)}`}>
          <div className="face-debug-canvas">
            <img src={photo.thumbnail_url} alt={`Capture from ${formatTimestamp(photo.captured_at)}`} loading="lazy" decoding="async" />
            {showBoundingBoxes || showLandmarks ? (
              <div className="face-debug-overlay" aria-hidden="true">
                {showLandmarks ? (
                  <svg className="face-landmark-overlay" viewBox="0 0 100 100" preserveAspectRatio="none">
                    {photo.faces.map((face) => <FaceLandmarks key={face.id} face={face} />)}
                  </svg>
                ) : null}
                {showBoundingBoxes ? photo.faces.map((face) => (
                  <div key={face.id} className="debug-face-box" style={boxStyle(face)}>
                    <span>{face.ordinal + 1}</span>
                  </div>
                )) : null}
              </div>
            ) : null}
          </div>
        </a>
        <div className="debug-results">
          <code className="debug-photo-id">{photo.id}</code>
          <dl>
            <div><dt>Attempts</dt><dd>{photo.attempt_count}</dd></div>
            <div><dt>Runtime</dt><dd>{photo.processing_millis == null ? "—" : `${photo.processing_millis} ms`}</dd></div>
            <div><dt>Image</dt><dd>{photo.oriented_width && photo.oriented_height ? `${photo.oriented_width} × ${photo.oriented_height}` : "—"}</dd></div>
          </dl>
          {photo.leased_by ? <p className="lease-note">Leased by <code>{photo.leased_by}</code>{photo.lease_expires_at ? ` until ${formatTimestamp(photo.lease_expires_at)}` : ""}</p> : null}
          {photo.last_error ? <p className="processing-error">{photo.last_error}</p> : null}
          <div className="face-result-list">
            {photo.faces.map((face) => (
              <article key={face.id}>
                <img src={face.crop_url} alt="" width="72" height="72" loading="lazy" />
                <div>
                  <strong>Face {face.ordinal + 1}</strong>
                  <small>{face.landmark_model.startsWith("mediapipe-") ? `${face.landmarks.length}-point mesh · ${face.embedding_dimension}D` : `${Math.round(face.detector_confidence * 100)}% · ${face.landmarks.length} landmarks · ${face.embedding_dimension}D`}</small>
                  <small>{face.landmark_model} · {face.embedding_model}</small>
                </div>
                <FaceAssignment face={face} people={people} edits={edits} savedAssignments={savedAssignments} onEdit={onEdit} />
              </article>
            ))}
            {photo.status === "complete" && photo.faces.length === 0 ? <p className="no-face-result">No face detected</p> : null}
          </div>
        </div>
      </div>
    </article>
  );
}

function FaceAssignment({ face, people, edits, savedAssignments, onEdit }: {
  face: AdminFace;
  people: PersonFlipbook[];
  edits: AssignmentMap;
  savedAssignments: AssignmentMap;
  onEdit: (face: AdminFace, personId: string) => void;
}) {
  const hasEdit = Object.prototype.hasOwnProperty.call(edits, face.id);
  const wasJustSaved = Object.prototype.hasOwnProperty.call(savedAssignments, face.id);
  const selectedPersonId = hasEdit ? edits[face.id] : savedAssignments[face.id] ?? face.person_id ?? "";
  const personName = people.find((person) => person.id === selectedPersonId)?.display_name;
  const feedback = hasEdit
    ? selectedPersonId ? `Not saved · ${personName ?? "person"}` : "Not saved · Unknown"
    : selectedPersonId
      ? `${wasJustSaved ? "Saved" : "Confirmed"} as ${personName ?? face.person_name ?? "person"}`
      : wasJustSaved ? "Saved as Unknown" : "Unassigned";

  return (
    <label className="face-assignment" data-state={hasEdit ? "edited" : wasJustSaved ? "saved" : "idle"}>
      <span className="sr-only">Person for face {face.ordinal + 1}</span>
      <select value={selectedPersonId} onChange={(event) => onEdit(face, event.target.value)}>
        <option value="">Unknown</option>
        {people.map((person) => <option key={person.id} value={person.id}>{person.display_name}</option>)}
      </select>
      <small role="status" aria-live="polite">{feedback}</small>
    </label>
  );
}

const MEDIAPIPE_CONTOURS = [
  [10, 338, 297, 332, 284, 251, 389, 356, 454, 323, 361, 288, 397, 365, 379, 378, 400, 377, 152, 148, 176, 149, 150, 136, 172, 58, 132, 93, 234, 127, 162, 21, 54, 103, 67, 109, 10],
  [33, 7, 163, 144, 145, 153, 154, 155, 133, 173, 157, 158, 159, 160, 161, 246, 33],
  [263, 249, 390, 373, 374, 380, 381, 382, 362, 398, 384, 385, 386, 387, 388, 466, 263],
  [70, 63, 105, 66, 107],
  [336, 296, 334, 293, 300],
  [168, 6, 197, 195, 5, 4, 1, 19, 94, 2],
  [98, 97, 2, 326, 327],
  [61, 146, 91, 181, 84, 17, 314, 405, 321, 375, 291, 308, 324, 318, 402, 317, 14, 87, 178, 88, 95, 78, 61],
  [78, 191, 80, 81, 82, 13, 312, 311, 310, 415, 308],
];
const MEDIAPIPE_IRISES = [[468, 469, 470, 471, 472, 468], [473, 474, 475, 476, 477, 473]];

function FaceLandmarks({ face }: { face: AdminFace }) {
  if (face.landmarks.length < 478) {
    return <>{face.landmarks.map((point, index) => <circle key={index} className="legacy-landmark" cx={point.x * 100} cy={point.y * 100} r="0.8" />)}</>;
  }
  return (
    <g>
      {MEDIAPIPE_CONTOURS.map((indices, index) => <polyline key={`contour-${index}`} points={landmarkPoints(face, indices)} />)}
      {MEDIAPIPE_IRISES.map((indices, index) => <polygon key={`iris-${index}`} className="iris-contour" points={landmarkPoints(face, indices)} />)}
    </g>
  );
}

function landmarkPoints(face: AdminFace, indices: number[]) {
  return indices.map((index) => `${face.landmarks[index].x * 100},${face.landmarks[index].y * 100}`).join(" ");
}

function Flipbook({ person, frameIndex, onFrameChange }: {
  person?: PersonFlipbook;
  frameIndex: number;
  onFrameChange: (index: number) => void;
}) {
  const frames = person?.frames ?? [];
  const safeIndex = Math.min(frameIndex, Math.max(0, frames.length - 1));
  const frame = frames[safeIndex];
  if (!person) return <section className="flipbook-panel"><p className="admin-empty">Create a person to start a daily flipbook.</p></section>;
  return (
    <section className="flipbook-panel">
      <header>
        <div><p className="eyebrow">Daily face study</p><h2>{person.display_name}</h2></div>
        <p>{person.day_count} days · {person.face_count} confirmed faces</p>
      </header>
      {frame ? (
        <>
          <figure className="flipbook-stage">
            <a href={frame.photo_url} aria-label={`Open original photo from ${formatTimestamp(frame.captured_at)}`}>
              <img src={frame.crop_url} alt={`${person.display_name} on ${formatDay(frame.capture_day)}`} />
            </a>
            <figcaption><strong>{formatDay(frame.capture_day)}</strong><span>{safeIndex + 1} of {frames.length}</span></figcaption>
          </figure>
          <div className="flipbook-controls">
            <button type="button" disabled={safeIndex <= 0} onClick={() => onFrameChange(safeIndex - 1)} aria-label="Newer day">←</button>
            <input type="range" min="0" max={Math.max(0, frames.length - 1)} value={safeIndex} onChange={(event) => onFrameChange(Number(event.target.value))} aria-label="Flipbook day" />
            <button type="button" disabled={safeIndex >= frames.length - 1} onClick={() => onFrameChange(safeIndex + 1)} aria-label="Older day">→</button>
          </div>
          <div className="flipbook-strip" aria-label={`Daily frames for ${person.display_name}`}>
            {frames.map((candidate, index) => (
              <button key={candidate.face_id} type="button" aria-pressed={index === safeIndex} title={formatDay(candidate.capture_day)} onClick={() => onFrameChange(index)}>
                <img src={candidate.crop_url} alt="" width="72" height="72" loading="lazy" />
                <span>{shortDay(candidate.capture_day)}</span>
              </button>
            ))}
          </div>
        </>
      ) : <p className="admin-empty">Assign this person a face in Processing to create their first daily frame.</p>}
    </section>
  );
}

function boxStyle(face: AdminFace) {
  return {
    left: `${face.bounds.x * 100}%`,
    top: `${face.bounds.y * 100}%`,
    width: `${face.bounds.width * 100}%`,
    height: `${face.bounds.height * 100}%`,
  };
}

function formatTimestamp(value: string) {
  const timestamp = Date.parse(value);
  return Number.isNaN(timestamp) ? value : new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" }).format(timestamp);
}

function formatDay(value: string) {
  const timestamp = Date.parse(`${value}T12:00:00`);
  return Number.isNaN(timestamp) ? value : new Intl.DateTimeFormat(undefined, { dateStyle: "full" }).format(timestamp);
}

function shortDay(value: string) {
  const timestamp = Date.parse(`${value}T12:00:00`);
  return Number.isNaN(timestamp) ? value : new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" }).format(timestamp);
}
