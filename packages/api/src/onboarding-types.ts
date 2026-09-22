import type { components } from "./schema";

// Narrowed views of the generated onboarding schemas. The wire status is a
// plain string in OpenAPI; the client narrows it to the documented set.
type Schemas = components["schemas"];

export type SignupRequest = Schemas["SignupRequest"];
export type CreatePersonRequest = Schemas["CreatePersonRequest"];
export type PersonEnrollment = Schemas["EnrollmentSummary"];
/** An account's standing in its household. */
export type HouseholdRole = "admin" | "member";

/** Whether a person in the grid has a login of their own. */
export type HouseholdAccount = "linked" | "none";

export type HouseholdPerson = Omit<
  Schemas["HouseholdPerson"],
  "role" | "account"
> & {
  role: HouseholdRole | null;
  account: HouseholdAccount;
};

export type HouseholdSummary = Omit<
  Schemas["HouseholdSummary"],
  "role" | "people"
> & {
  role: HouseholdRole;
  people: HouseholdPerson[];
};

export type EnrollmentPhotoStatus =
  "uploading" | "processing" | "enrolled" | "retake" | "failed";

export type EnrollmentPhoto = Omit<Schemas["EnrollmentPhoto"], "status"> & {
  status: EnrollmentPhotoStatus;
};

export type EnrollmentStatus = Omit<Schemas["EnrollmentStatus"], "photos"> & {
  photos: EnrollmentPhoto[];
};

/** The number of guided poses a person needs before their profile is active. */
export const REQUIRED_ENROLLMENT_PHOTOS = 5;

/**
 * An account's standing request to be deleted. Deletion is carried out by an
 * operator, not by the app, so the request is what the Account screen shows.
 * Hand-written until the route carries an OpenAPI schema.
 */
export type DeletionRequest = {
  user_id: string;
  username: string;
  /** RFC 3339, UTC. */
  requested_at: string;
  fulfilled_at?: string;
};
