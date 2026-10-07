import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SshProfilePanel } from "./SshProfilePanel";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const commands = vi.hoisted(() => ({
  deleteSshProfile: vi.fn(), enrollSshProfile: vi.fn(), listSshProfiles: vi.fn(), pickSshKeyPath: vi.fn(),
}));

vi.mock("../shared/commands", async (importOriginal) => ({
  ...await importOriginal<typeof import("../shared/commands")>(), ...commands, hasTauriRuntime: () => true,
}));

describe("SSH profile loading", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    container = document.createElement("div"); document.body.append(container); root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount()); container.remove(); vi.clearAllMocks();
  });

  it("does not flash an empty state or enrollment form while profiles load", async () => {
    let finish!: (profiles: never[]) => void;
    commands.listSshProfiles.mockReturnValue(new Promise((resolve) => { finish = resolve; }));
    await act(async () => root.render(<SshProfilePanel onProfilesChanged={vi.fn()} t={(key) => key} />));

    expect(container.textContent).toContain("readinessLoading");
    expect(container.textContent).not.toContain("serversEmpty");
    expect(container.textContent).not.toContain("setupServerTitle");

    await act(async () => finish([]));
    await vi.waitFor(() => expect(container.textContent).toContain("setupServerTitle"));
  });

  it("keeps enrollment collapsed when a saved server exists", async () => {
    commands.listSshProfiles.mockResolvedValue([
      { profileId: "server", label: "VDS", host: "vds.example", port: 22, user: "backup" },
    ]);
    await act(async () => root.render(<SshProfilePanel onProfilesChanged={vi.fn()} t={(key) => key} />));

    await vi.waitFor(() => expect(container.textContent).toContain("VDS"));
    expect(container.querySelector(".server-form")).toBeNull();
    expect(container.querySelector('[aria-label="serversAdd"]')).not.toBeNull();
  });

  it("does not show an empty state or form when the server registry cannot be read", async () => {
    commands.listSshProfiles.mockRejectedValueOnce(new Error("registry unavailable")).mockResolvedValueOnce([]);
    await act(async () => root.render(<SshProfilePanel onProfilesChanged={vi.fn()} t={(key) => key} />));

    await vi.waitFor(() => expect(container.querySelector('[role="alert"]')).not.toBeNull());
    expect(container.textContent).not.toContain("serversEmpty");
    expect(container.textContent).not.toContain("setupServerTitle");
    await act(async () => retryButton(container).click());
    await vi.waitFor(() => expect(container.textContent).toContain("setupServerTitle"));
    expect(commands.listSshProfiles).toHaveBeenCalledTimes(2);
  });
});

function retryButton(container: HTMLElement): HTMLButtonElement {
  const button = [...container.querySelectorAll("button")].find((item) => item.textContent?.includes("readinessRefresh"));
  if (!(button instanceof HTMLButtonElement)) throw new Error("Retry button not found");
  return button;
}

describe("password login enrollment", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(async () => {
    container = document.createElement("div"); document.body.append(container); root = createRoot(container);
    commands.listSshProfiles.mockResolvedValue([]);
    commands.enrollSshProfile.mockResolvedValue({ profileId: "p", label: "VDS", host: "vds.example", port: 22, user: "root" });
    await act(async () => root.render(<SshProfilePanel onProfilesChanged={vi.fn()} t={(key) => key} />));
    await vi.waitFor(() => expect(container.querySelector(".server-form")).not.toBeNull());
  });

  afterEach(async () => {
    await act(async () => root.unmount()); container.remove(); vi.clearAllMocks();
  });

  const field = (label: string): HTMLInputElement => {
    const match = [...container.querySelectorAll("label.field")].find((item) => item.querySelector(".field__label")?.textContent?.startsWith(label));
    const input = match?.querySelector("input");
    if (!input) throw new Error(`Field not found: ${label}`);
    return input;
  };
  const type = async (input: HTMLInputElement, value: string) => act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set?.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  const mode = (label: string) => container.querySelector<HTMLButtonElement>(`.server-form__modes [aria-label="${label}"]`) as HTMLButtonElement;

  it("offers a masked password field instead of a key file once password login is chosen", async () => {
    expect(() => field("setupKey")).not.toThrow();
    await act(async () => mode("setupAuthPassword").click());
    expect(() => field("setupKey")).toThrow();
    const password = field("setupPassword");
    expect(password.type).toBe("password");
    expect(password.autocomplete).toBe("off");
    await act(async () => container.querySelector<HTMLButtonElement>('[aria-label="setupPasswordShow"]')?.click());
    expect(field("setupPassword").type).toBe("text");
  });

  it("warns when logging in as root but still allows it", async () => {
    await act(async () => mode("setupAuthPassword").click());
    expect(container.querySelector(".server-form__warning")).toBeNull();
    await type(field("setupUser"), "root");
    expect(container.querySelector(".server-form__warning")?.textContent).toContain("setupRootWarning");
  });

  it("sends the password once, then wipes it from the form", async () => {
    await type(field("setupLabel"), "VDS"); await type(field("setupHost"), "vds.example"); await type(field("setupUser"), "root");
    await type(field("setupHostKey"), "ssh-ed25519 AAAA");
    await act(async () => mode("setupAuthPassword").click());
    await type(field("setupPassword"), "S3cret-Pass!");
    await act(async () => container.querySelector<HTMLInputElement>(".server-form__ack input")?.click());
    await act(async () => container.querySelector<HTMLFormElement>(".server-form")?.requestSubmit());
    await vi.waitFor(() => expect(commands.enrollSshProfile).toHaveBeenCalledOnce());
    expect(commands.enrollSshProfile).toHaveBeenCalledWith(expect.objectContaining({ authKind: "password", password: "S3cret-Pass!", user: "root" }));
    await vi.waitFor(() => expect(container.textContent).not.toContain("S3cret-Pass!"));
    expect(JSON.stringify([...container.querySelectorAll("input")].map((input) => input.value))).not.toContain("S3cret");
  });

  it("drops a typed password when switching back to a key", async () => {
    await act(async () => mode("setupAuthPassword").click());
    await type(field("setupPassword"), "typed-secret");
    await act(async () => mode("setupAuthKey").click());
    await act(async () => mode("setupAuthPassword").click());
    expect(field("setupPassword").value).toBe("");
  });
});
