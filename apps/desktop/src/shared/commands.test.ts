import { describe, expect, it } from "vitest";
import {
  enrollSigningIdentity,
  enrollSshProfile,
  deleteSshProfile,
  browseRemoteDirectory,
  executeDeploy,
  executeRestore,
  getFoundationStatus,
  getSigningIdentityStatus,
  listBackups,
  listSshProfiles,
  previewDeploy,
  previewCaptureSelection,
  previewRestore,
} from "./commands";

describe("foundation bridge", () => {
  it("matches the validation release status in browser preview", async () => {
    const status = await getFoundationStatus();
    expect(status.liveOperationsEnabled).toBe(true);
    expect(status.iteration).toContain("operator path in progress");
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
    await expect(deleteSshProfile("profile-001")).rejects.toThrow("desktop runtime");
    await expect(browseRemoteDirectory("profile-001", "/srv")).rejects.toThrow("desktop runtime");
  });

  it("never previews or restores a backup from the browser preview", async () => {
    await expect(listBackups("repository-001")).resolves.toEqual([]);
    const request = { repositoryId: "repository-001", backupId: "backup-001", destination: "C:/restore" };
    await expect(previewRestore(request)).rejects.toThrow("desktop runtime");
    await expect(executeRestore(request)).rejects.toThrow("desktop runtime");
  });

  it("never previews or deploys a backup from the browser preview", async () => {
    const request = {
      repositoryId: "repository-001",
      backupId: "backup-001",
      targetProfileId: "profile-target",
      targetPath: "/srv/app",
    };
    await expect(previewDeploy(request)).rejects.toThrow("desktop runtime");
    await expect(executeDeploy(request)).rejects.toThrow("desktop runtime");
  });

  it("never previews a capture selection from the browser preview", async () => {
    await expect(previewCaptureSelection({
      profileId: "profile-001",
      repositoryId: "repository-001",
      items: [{ kind: "remote_path", absolutePath: "/srv" }],
    })).rejects.toThrow("desktop runtime");
  });
});
