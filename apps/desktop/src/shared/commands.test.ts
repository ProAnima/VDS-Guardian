import { describe, expect, it } from "vitest";
import {
  enrollSigningIdentity,
  enrollSshProfile,
  getFoundationStatus,
  getSigningIdentityStatus,
  listSshProfiles,
} from "./commands";

describe("foundation bridge", () => {
  it("keeps live operations disabled in browser preview", async () => {
    const status = await getFoundationStatus();
    expect(status.liveOperationsEnabled).toBe(false);
    expect(status.iteration).toContain("Milestone 1");
  });

  it("reports a non-enrolled preview without creating an identity", async () => {
    await expect(getSigningIdentityStatus()).resolves.toEqual({
      state: "not_enrolled",
      identity: null,
    });
    await expect(enrollSigningIdentity()).rejects.toThrow("desktop runtime");
  });

  it("never creates an SSH profile from the browser preview", async () => {
    await expect(listSshProfiles()).resolves.toEqual([]);
    await expect(enrollSshProfile({
      label: "VDS",
      host: "vds.example",
      port: 22,
      user: "backup",
      hostKey: "ssh-ed25519 AAAA",
      keyPath: "C:/Keys/vds",
    })).rejects.toThrow("desktop runtime");
  });
});
