import { invoke } from "@tauri-apps/api/core";

export interface FoundationStatus {
  product: string;
  version: string;
  iteration: string;
  liveOperationsEnabled: boolean;
}

export const previewStatus: FoundationStatus = {
  product: "VDS Guardian",
  version: "0.1.0",
  iteration: "Iteration 0 — production foundation",
  liveOperationsEnabled: false,
};

export async function getFoundationStatus(): Promise<FoundationStatus> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    return previewStatus;
  }

  return invoke<FoundationStatus>("get_foundation_status");
}
