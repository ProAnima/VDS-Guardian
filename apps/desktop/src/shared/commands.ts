import { invoke } from "@tauri-apps/api/core";

export interface FoundationStatus {
  product: string;
  version: string;
  iteration: string;
  liveOperationsEnabled: boolean;
}

export type SigningIdentityState =
  | "not_enrolled"
  | "enrollment_pending"
  | "recovery_pending"
  | "ready";

export type EnrollmentDisposition = "enrolled" | "recovered" | "loaded";

export interface SigningIdentityDescriptor {
  credentialId: string;
  algorithm: string;
  keyId: string;
}

export interface SigningIdentityStatus {
  state: SigningIdentityState;
  identity: SigningIdentityDescriptor | null;
}

export interface SigningIdentityEnrollment {
  disposition: EnrollmentDisposition;
  identity: SigningIdentityDescriptor;
}

export const previewStatus: FoundationStatus = {
  product: "VDS Guardian",
  version: "0.1.0",
  iteration: "Milestone 1 — local repository foundation",
  liveOperationsEnabled: false,
};

export async function getFoundationStatus(): Promise<FoundationStatus> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    return previewStatus;
  }

  return invoke<FoundationStatus>("get_foundation_status");
}

export async function getSigningIdentityStatus(): Promise<SigningIdentityStatus> {
  if (!hasTauriRuntime()) {
    return { state: "not_enrolled", identity: null };
  }

  return invoke<SigningIdentityStatus>("get_signing_identity_status");
}

export async function enrollSigningIdentity(): Promise<SigningIdentityEnrollment> {
  if (!hasTauriRuntime()) {
    throw new Error("Signing enrollment requires the VDS Guardian desktop runtime.");
  }

  return invoke<SigningIdentityEnrollment>("enroll_signing_identity");
}

function hasTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}
