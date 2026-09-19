import type { components } from "./schema";

// Narrowed views of the generated onboarding schemas. The wire status is a
// plain string in OpenAPI; the client narrows it to the documented set.
type Schemas = components["schemas"];

export type SignupRequest = Schemas["SignupRequest"];
export type CreatePersonRequest = Schemas["CreatePersonRequest"];
export type PersonEnrollment = Schemas["EnrollmentSummary"];
export type HouseholdPerson = Schemas["HouseholdPerson"];
export type HouseholdSummary = Schemas["HouseholdSummary"];

export type EnrollmentPhotoStatus =
  | "uploading"
  | "processing"
  | "enrolled"
  | "retake"
  | "failed";

export type EnrollmentPhoto = Omit<Schemas["EnrollmentPhoto"], "status"> & {
  status: EnrollmentPhotoStatus;
};

export type EnrollmentStatus = Omit<Schemas["EnrollmentStatus"], "photos"> & {
  photos: EnrollmentPhoto[];
};

/** The number of guided poses a person needs before their profile is active. */
export const REQUIRED_ENROLLMENT_PHOTOS = 5;
