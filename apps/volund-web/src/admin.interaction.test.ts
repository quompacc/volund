// @vitest-environment happy-dom

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { catalogApi, identityApi, jobsApi, operationsApi, policyApi } from "./api";
import { mountAdministration } from "./admin";
import type { CurrentSession, ManagedUser, OwnSession, Setting } from "./types";

const actor: CurrentSession = {
  sessionId: "session-owner",
  userId: "owner",
  email: "owner@example.test",
  displayName: "Owner",
  role: "owner",
  mustChangePassword: false,
};

const session: OwnSession = {
  id: "session-owner",
  current: true,
  createdAtUnixMs: Date.UTC(2026, 7, 29),
  lastSeenAtUnixMs: Date.UTC(2026, 7, 29),
  idleExpiresAtUnixMs: Date.UTC(2026, 7, 30),
  absoluteExpiresAtUnixMs: Date.UTC(2026, 8, 29),
  clientAddress: "127.0.0.1",
  userAgent: "VÖLUND test",
};

const lockedUser: ManagedUser = {
  id: "locked-user",
  email: "locked@example.test",
  displayName: "Locked User",
  role: "editor",
  status: "locked",
  createdAtUnixMs: Date.UTC(2026, 7, 29),
  lastLoginAtUnixMs: null,
  lockedUntilUnixMs: Date.UTC(2026, 7, 29, 1),
  mustChangePassword: false,
};

const setting: Setting = {
  key: "security.sessionIdleMinutes",
  domain: "security",
  valueType: "integer",
  value: 480,
  origin: "persisted",
  editable: true,
  sensitive: false,
  revision: 1,
  effect: "immediate",
  constraints: { minimum: 5, maximum: 1440 },
};

describe("administration interactions", () => {
  let host: HTMLElement;

  beforeEach(() => {
    host = document.createElement("main");
    document.body.replaceChildren(host);
    vi.spyOn(identityApi, "sessions").mockResolvedValue([session]);
    vi.spyOn(identityApi, "settings").mockResolvedValue([setting]);
    vi.spyOn(identityApi, "preferences").mockResolvedValue({ previewAutoLoad: "selected", background: "dark",
      gridVisible: true, contrast: "balanced", renderStyle: "solid", problemMinimumSeverity: "warning", revision: 0 });
    vi.spyOn(identityApi, "users").mockResolvedValue([lockedUser]);
    vi.spyOn(catalogApi, "quarantines").mockResolvedValue({ items: [], limit: 100, offset: 0, total: 0 });
    vi.spyOn(operationsApi, "health").mockResolvedValue({
      state: "healthy", checkedAtUnixMs: Date.UTC(2026, 7, 29), version: "0.32.0",
      database: { state: "healthy", serverVersion: 170011, schemaTables: 27, expectedSchemaTables: 27, appliedMigrations: 16, expectedMigrations: 16 },
      workers: [],
      backup: { key: "backup", state: "healthy", lastOutcome: "success", lastSucceededAtUnixMs: Date.UTC(2026, 7, 29), lastFailedAtUnixMs: null, expectedIntervalSeconds: 86400, reasons: [] },
      libraries: [], recentScans: [],
    });
    vi.spyOn(jobsApi, "list").mockResolvedValue({ items: [], limit: 50, offset: 0, total: 0 });
    vi.spyOn(policyApi, "profiles").mockResolvedValue([]);
    vi.spyOn(policyApi, "schedules").mockResolvedValue([]);
    vi.spyOn(policyApi, "retentionPreview").mockResolvedValue({ artifactRuns: 0, artifactFiles: 0, artifactBytes: 0, diagnostics: 0, reasonCodes: [] });
  });

  afterEach(() => vi.restoreAllMocks());

  it.each([
    ["#create-user", "createUser"],
    ["#invite-user", "inviteUser"],
    ["#change-own-password", "changeOwnPassword"],
    ["#user-preferences", "updatePreferences"],
    ["[data-setting]", "updateSetting"],
  ] as const)("blocks repeated submission of %s and permits retry after failure", async (selector, method) => {
    let reject!: (reason: Error) => void;
    const request = vi.spyOn(identityApi, method).mockImplementation(() =>
      new Promise<never>((_, fail) => { reject = fail; }));
    mountAdministration(host, actor);
    await vi.waitFor(() => expect(host.querySelector(selector)).not.toBeNull());
    submit(selector);
    submit(selector);
    expect(request).toHaveBeenCalledTimes(1);
    expect(host.querySelector<HTMLButtonElement>(`${selector} button`)!.disabled).toBe(true);
    reject(new Error("Verbindung unterbrochen <Test>"));
    await vi.waitFor(() => expect(host.textContent).toContain("Verbindung unterbrochen <Test>"));
    expect(host.querySelector("test")).toBeNull();
    expect(host.querySelector<HTMLButtonElement>(`${selector} button`)!.disabled).toBe(false);
    submit(selector);
    expect(request).toHaveBeenCalledTimes(2);
    reject(new Error("Erneut unterbrochen"));
    await vi.waitFor(() => expect(host.textContent).toContain("Erneut unterbrochen"));
  });

  it("opens only one status confirmation and releases the action after cancellation", async () => {
    const update = vi.spyOn(identityApi, "updateUser").mockResolvedValue(lockedUser);
    mountAdministration(host, actor);
    await vi.waitFor(() => expect(host.querySelector("[data-user-status=locked-user]")).not.toBeNull());
    const button = host.querySelector<HTMLButtonElement>("[data-user-status=locked-user]")!;
    button.click(); button.click();
    expect(document.querySelectorAll("dialog")).toHaveLength(1);
    expect(button.disabled).toBe(true);
    document.querySelector<HTMLButtonElement>("dialog [type=button]")!.click();
    await vi.waitFor(() => expect(button.disabled).toBe(false));
    expect(update).not.toHaveBeenCalled();
    button.click();
    confirm("SET USER STATUS locked-user active");
    await vi.waitFor(() => expect(update).toHaveBeenCalledTimes(1));
  });

  it("offers a single retry after a load failure and recovers", async () => {
    vi.mocked(identityApi.sessions).mockRejectedValueOnce(new Error("temporary <failure>"));
    mountAdministration(host, actor);
    await vi.waitFor(() => expect(host.querySelector("#admin-retry")).not.toBeNull());
    expect(host.querySelector("failure")).toBeNull();
    const retry = host.querySelector<HTMLButtonElement>("#admin-retry")!;
    retry.click(); retry.click();
    await vi.waitFor(() => expect(host.querySelectorAll("[role=tab]")).toHaveLength(6));
    expect(identityApi.sessions).toHaveBeenCalledTimes(2);
  });

  it("does not overwrite a replacement view when a disposed request fails", async () => {
    let reject!: (reason: Error) => void;
    vi.mocked(identityApi.sessions).mockReturnValueOnce(new Promise((_, fail) => { reject = fail; }));
    const dispose = mountAdministration(host, actor);
    const content = host.querySelector<HTMLElement>("#admin-content")!;
    dispose();
    content.innerHTML = "Replacement view";
    reject(new Error("late failure"));
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(content.textContent).toBe("Replacement view");
  });

  it("keeps the selected area across repeated refresh failures and retry", async () => {
    mountAdministration(host, actor);
    await vi.waitFor(() => expect(host.querySelector("[data-admin-area=operations]")).not.toBeNull());
    host.querySelector<HTMLButtonElement>("[data-admin-area=operations]")!.click();
    vi.mocked(jobsApi.list).mockRejectedValueOnce(new Error("first failure"))
      .mockRejectedValueOnce(new Error("second failure"));
    submit("#job-filters");
    await vi.waitFor(() => expect(host.textContent).toContain("first failure"));
    host.querySelector<HTMLButtonElement>("#admin-retry")!.click();
    await vi.waitFor(() => expect(host.textContent).toContain("second failure"));
    expect(host.querySelector<HTMLButtonElement>("#admin-retry")!.disabled).toBe(false);
    host.querySelector<HTMLButtonElement>("#admin-retry")!.click();
    await vi.waitFor(() => expect(host.querySelector("[data-admin-area=operations]")?.getAttribute("aria-selected")).toBe("true"));
    expect(host.querySelector("#admin-retry")).toBeNull();
  });

  it("reports unavailable quarantine without claiming the inventory is empty", async () => {
    vi.mocked(catalogApi.quarantines).mockRejectedValue(new Error("HTTP 500"));
    mountAdministration(host, actor);
    await vi.waitFor(() => expect(host.querySelector("[data-admin-area=storage]")).not.toBeNull());
    host.querySelector<HTMLButtonElement>("[data-admin-area=storage]")!.click();
    const panel = host.querySelector<HTMLElement>("[data-admin-panel=storage]")!;
    expect(panel.textContent).toContain("Der Bestand ist unbekannt");
    expect(panel.textContent).not.toContain("Keine Originaldateien befinden sich in Quarantäne");
    expect(host.querySelectorAll("[role=tab]")).toHaveLength(6);
  });

  it("loads subsequent job and quarantine pages without losing the active area", async () => {
    vi.mocked(jobsApi.list).mockResolvedValue({ items: [], limit: 50, offset: 0, total: 51 });
    vi.mocked(catalogApi.quarantines).mockResolvedValue({ items: [], limit: 100, offset: 0, total: 101 });
    mountAdministration(host, actor);
    await vi.waitFor(() => expect(host.querySelector("[data-job-page=next]")).not.toBeNull());
    host.querySelector<HTMLButtonElement>("[data-admin-area=operations]")!.click();
    host.querySelector<HTMLButtonElement>("[data-job-page=next]")!.click();
    await vi.waitFor(() => expect(jobsApi.list).toHaveBeenLastCalledWith({ offset: 50 }));
    expect(host.querySelector("[data-admin-area=operations]")?.getAttribute("aria-selected")).toBe("true");
    host.querySelector<HTMLButtonElement>("[data-admin-area=storage]")!.click();
    host.querySelector<HTMLButtonElement>("[data-quarantine-page=next]")!.click();
    await vi.waitFor(() => expect(catalogApi.quarantines).toHaveBeenLastCalledWith(100));
    expect(host.querySelector("[data-admin-area=storage]")?.getAttribute("aria-selected")).toBe("true");
  });

  it("returns from a page emptied by concurrent changes instead of trapping the user", async () => {
    vi.mocked(jobsApi.list).mockImplementation(async (filters = {}) => filters.offset === 50
      ? { items: [], limit: 50, offset: 50, total: 50 }
      : { items: [], limit: 50, offset: 0, total: 51 });
    vi.mocked(catalogApi.quarantines).mockImplementation(async (offset = 0) => offset === 100
      ? { items: [], limit: 100, offset: 100, total: 100 }
      : { items: [], limit: 100, offset: 0, total: 101 });
    mountAdministration(host, actor);
    await vi.waitFor(() => expect(host.querySelector("[data-job-page=next]")).not.toBeNull());
    host.querySelector<HTMLButtonElement>("[data-job-page=next]")!.click();
    await vi.waitFor(() => expect(jobsApi.list).toHaveBeenLastCalledWith({ offset: 0 }));
    host.querySelector<HTMLButtonElement>("[data-quarantine-page=next]")!.click();
    await vi.waitFor(() => expect(catalogApi.quarantines).toHaveBeenLastCalledWith(0));
    expect(host.textContent).toContain("Keine passenden Aufträge");
    expect(host.textContent).toContain("Keine Originaldateien befinden sich in Quarantäne");
  });

  it("creates an invitation link and unlocks a locked account", async () => {
    const invitation = vi.spyOn(identityApi, "inviteUser").mockResolvedValue({
      user: lockedUser,
      activationToken: "one-time-token",
      expiresAtUnixMs: Date.UTC(2026, 8, 5),
    });
    const update = vi.spyOn(identityApi, "updateUser").mockResolvedValue({ ...lockedUser, status: "active" });
    mountAdministration(host, actor);
    await vi.waitFor(() => expect(host.querySelector("#invite-user")).not.toBeNull());
    host.querySelector<HTMLButtonElement>("[data-admin-area=access]")!.click();

    setValue("#invite-user [name=displayName]", "Invited User");
    setValue("#invite-user [name=email]", "invite@example.test");
    setValue("#invite-user [name=role]", "viewer");
    submit("#invite-user");
    await vi.waitFor(() => expect(invitation).toHaveBeenCalledWith({
      displayName: "Invited User", email: "invite@example.test", role: "viewer",
    }));
    await vi.waitFor(() => expect(host.querySelector("#admin-message")?.textContent).toContain("?invite=one-time-token"));
    expect(host.querySelector("#admin-message")?.closest("[hidden]")).toBeNull();

    host.querySelector<HTMLButtonElement>("[data-user-status=locked-user]")!.click();
    confirm("SET USER STATUS locked-user active");
    await vi.waitFor(() => expect(update).toHaveBeenCalledWith("locked-user", { status: "active" }));
  });

  it("confirms session revocation and role changes while preserving cancelled roles", async () => {
    const otherSession = { ...session, id: "session-other", current: false };
    vi.mocked(identityApi.sessions).mockResolvedValue([session, otherSession]);
    const revoke = vi.spyOn(identityApi, "revokeSession").mockResolvedValue();
    const update = vi.spyOn(identityApi, "updateUser").mockResolvedValue({
      ...lockedUser,
      role: "viewer",
    });
    mountAdministration(host, actor);
    await vi.waitFor(() => expect(host.querySelector("[data-revoke-session=session-other]")).not.toBeNull());

    host.querySelector<HTMLButtonElement>("[data-revoke-session=session-other]")!.click();
    expect(revoke).not.toHaveBeenCalled();
    confirm("REVOKE SESSION session-other");
    await vi.waitFor(() => expect(revoke).toHaveBeenCalledWith("session-other"));

    host.querySelector<HTMLButtonElement>("[data-admin-area=access]")!.click();
    const role = host.querySelector<HTMLSelectElement>("[data-user-role=locked-user]")!;
    role.value = "viewer";
    role.dispatchEvent(new Event("change"));
    document.querySelector<HTMLButtonElement>("dialog [type=button]")!.click();
    await vi.waitFor(() => expect(role.value).toBe("editor"));
    expect(update).not.toHaveBeenCalled();

    role.value = "viewer";
    role.dispatchEvent(new Event("change"));
    confirm("SET USER ROLE locked-user viewer");
    await vi.waitFor(() => expect(update).toHaveBeenCalledWith("locked-user", { role: "viewer" }));
  });

  it("shows a protected owner accurately without offering owner escalation", async () => {
    const administrator = { ...actor, role: "administrator" as const };
    const ownerUser = { ...lockedUser, id: "owner-user", role: "owner" as const, status: "active" as const };
    vi.mocked(identityApi.users).mockResolvedValue([ownerUser, lockedUser]);
    mountAdministration(host, administrator);
    await vi.waitFor(() => expect(host.querySelector("[data-user-role=owner-user]")).not.toBeNull());
    host.querySelector<HTMLButtonElement>("[data-admin-area=access]")!.click();
    const ownerRole = host.querySelector<HTMLSelectElement>("[data-user-role=owner-user]")!;
    const regularRole = host.querySelector<HTMLSelectElement>("[data-user-role=locked-user]")!;
    expect(ownerRole.querySelector<HTMLOptionElement>("option[selected]")?.value).toBe("owner");
    expect(ownerRole.disabled).toBe(true);
    expect([...regularRole.options].map((option) => option.value)).not.toContain("owner");
  });

  it("submits typed settings and reports a password confirmation error", async () => {
    const updateSetting = vi.spyOn(identityApi, "updateSetting").mockResolvedValue({ ...setting, value: 30 });
    const changePassword = vi.spyOn(identityApi, "changeOwnPassword").mockResolvedValue();
    mountAdministration(host, actor);
    await vi.waitFor(() => expect(host.querySelector("[data-setting]")).not.toBeNull());

    setValue("[data-setting] [name=value]", "30");
    submit("[data-setting]");
    await vi.waitFor(() => expect(updateSetting).toHaveBeenCalledWith(setting, 30, undefined));
    await vi.waitFor(() => expect(catalogApi.quarantines).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(host.querySelector("#change-own-password")).not.toBeNull());

    setValue("#change-own-password [name=currentPassword]", "old password value");
    setValue("#change-own-password [name=newPassword]", "new password value");
    setValue("#change-own-password [name=confirmation]", "different value");
    submit("#change-own-password");
    await vi.waitFor(() => expect(host.querySelector("#admin-message")?.textContent).toContain("stimmen nicht überein"));
    expect(changePassword).not.toHaveBeenCalled();
  });

  it("gives every rendered form control an accessible name", async () => {
    mountAdministration(host, actor);
    await vi.waitFor(() => expect(host.querySelectorAll("input, select").length).toBeGreaterThan(0));
    for (const control of host.querySelectorAll<HTMLInputElement | HTMLSelectElement>("input, select")) {
      expect(control.getAttribute("aria-label") || control.closest("label")?.textContent).toBeTruthy();
    }
  });

  it("keeps access errors visible in the active area", async () => {
    vi.spyOn(identityApi, "inviteUser").mockRejectedValue(new Error("E-Mail bereits vergeben"));
    mountAdministration(host, actor);
    await vi.waitFor(() => expect(host.querySelector("#invite-user")).not.toBeNull());
    host.querySelector<HTMLButtonElement>("[data-admin-area=access]")!.click();
    submit("#invite-user");
    await vi.waitFor(() => expect(host.querySelector("#admin-message")?.textContent).toContain("E-Mail bereits vergeben"));
    expect(host.querySelector("#admin-message")?.closest("[hidden]")).toBeNull();
  });

  it("resets the password form after an asynchronous successful change", async () => {
    vi.spyOn(identityApi, "changeOwnPassword").mockResolvedValue();
    mountAdministration(host, actor);
    await vi.waitFor(() => expect(host.querySelector("#change-own-password")).not.toBeNull());
    setValue("#change-own-password [name=currentPassword]", "old password value");
    setValue("#change-own-password [name=newPassword]", "new password value");
    setValue("#change-own-password [name=confirmation]", "new password value");
    submit("#change-own-password");
    await vi.waitFor(() => expect(host.querySelector("#admin-message")?.textContent).toContain("Passwort geändert"));
    expect(host.querySelector<HTMLInputElement>("#change-own-password [name=newPassword]")!.value).toBe("");
  });

  it("shows one administration area at a time and preserves it after refresh", async () => {
    mountAdministration(host, actor);
    await vi.waitFor(() => expect(host.querySelectorAll("[role=tab]")).toHaveLength(6));
    expect(host.querySelector("[data-admin-panel=account]")?.hasAttribute("hidden")).toBe(false);
    expect(host.querySelector("[data-admin-panel=operations]")?.hasAttribute("hidden")).toBe(true);

    const operationsTab = host.querySelector<HTMLButtonElement>("[data-admin-area=operations]")!;
    operationsTab.click();
    expect(operationsTab.getAttribute("aria-selected")).toBe("true");
    expect(host.querySelector("[data-admin-panel=account]")?.hasAttribute("hidden")).toBe(true);
    expect(host.querySelector("[data-admin-panel=operations]")?.hasAttribute("hidden")).toBe(false);

    submit("#job-filters");
    await vi.waitFor(() => expect(jobsApi.list).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(host.querySelector("[data-admin-area=operations]")?.getAttribute("aria-selected")).toBe("true"));

    host.querySelector<HTMLButtonElement>("[data-admin-area=operations]")!.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
    expect(host.querySelector("[data-admin-area=storage]")?.getAttribute("aria-selected")).toBe("true");
  });
});

function setValue(selector: string, value: string): void {
  const control = document.querySelector<HTMLInputElement | HTMLSelectElement>(selector);
  if (!control) throw new Error(`missing control ${selector}`);
  control.value = value;
}

function submit(selector: string): void {
  const form = document.querySelector<HTMLFormElement>(selector);
  if (!form) throw new Error(`missing form ${selector}`);
  form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
}

function confirm(text: string): void {
  const input = document.querySelector<HTMLInputElement>("dialog input");
  if (!input) throw new Error("missing confirmation input");
  input.value = text;
  input.dispatchEvent(new Event("input"));
  document.querySelector<HTMLButtonElement>("dialog [type=submit]")!.click();
}
