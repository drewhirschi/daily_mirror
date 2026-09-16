// Hand-written mirrors of the onboarding contract in
// docs/mobile-onboarding-plan.md. The server branch adds these schemas to the
// OpenAPI document, but mobile work needs the types before that lands.
// TODO(schema): replace with components["schemas"][...] once regenerated.

export interface SignupRequest {
  username: string;
  display_name: string;
  password: string;
}

export interface CreatePersonRequest {
  display_name: string;
}

export interface PersonEnrollment {
  enrolled: boolean;
  enrolled_photos: number;
  required_photos: number;
}

export interface HouseholdPerson {
  id: string;
  display_name: string;
  enrollment: PersonEnrollment;
}

export interface HouseholdSummary {
  id: string;
  display_name: string;
  grid_size: number;
  self_person_id: string | null;
  people: HouseholdPerson[];
}

export type EnrollmentPhotoStatus =
  "uploading" | "processing" | "enrolled" | "retake" | "failed";

export interface EnrollmentPhoto {
  photo_id: string;
  captured_at: string;
  status: EnrollmentPhotoStatus;
  face_count: number | null;
  thumbnail_url: string | null;
}

export interface EnrollmentStatus {
  person_id: string;
  enrolled: boolean;
  enrolled_photos: number;
  required_photos: number;
  photos: EnrollmentPhoto[];
}

/** The number of guided poses a person needs before their profile is active. */
export const REQUIRED_ENROLLMENT_PHOTOS = 5;
