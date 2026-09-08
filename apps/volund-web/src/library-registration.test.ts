// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiError, libraryApi } from "./api";
import { bindLibraryActions, libraryPanel } from "./libraries";
import type { LibraryPathValidation, ManagedLibrary } from "./types";

function setup() {
  const host = document.createElement("div");
  host.innerHTML = `<div id="admin-message"></div>${libraryPanel([])}`;
  document.body.append(host);
  const form = host.querySelector<HTMLFormElement>("#create-library")!;
  for (const [key, value] of Object.entries({ key: "cad", name: "CAD", path: "/srv/cad" })) {
    form.querySelector<HTMLInputElement>(`[name=${key}]`)!.value = value;
  }
  const refresh = vi.fn().mockResolvedValue(undefined);
  bindLibraryActions(host, refresh);
  const submit = () => form.dispatchEvent(new Event("submit", { cancelable: true }));
  return { host, form, refresh, submit };
}

function confirm() {
  const input = document.querySelector<HTMLInputElement>("dialog input")!;
  input.value = "ADD LIBRARY cad";
  input.dispatchEvent(new Event("input"));
  document.querySelector<HTMLButtonElement>("dialog [type=submit]")!.click();
}

describe("library registration interaction matrix", () => {
  afterEach(() => { vi.restoreAllMocks(); document.body.replaceChildren(); });

  it("keeps invalid paths out of confirmation and creation", async () => {
    vi.spyOn(libraryApi, "validatePath").mockRejectedValue(new ApiError(400, "library path cannot be resolved"));
    const create = vi.spyOn(libraryApi, "create");
    const { host, refresh, submit } = setup();
    submit();
    await vi.waitFor(() => expect(host.textContent).toContain("Bibliothekspfad ist ungültig, fehlt oder ist nicht zugreifbar"));
    expect(document.querySelector("dialog")).toBeNull();
    expect(create).not.toHaveBeenCalled();
    expect(refresh).not.toHaveBeenCalled();
  });

  it("shows duplicate registration conflicts in German and preserves inputs", async () => {
    vi.spyOn(libraryApi, "validatePath").mockResolvedValue({} as LibraryPathValidation);
    vi.spyOn(libraryApi, "create").mockRejectedValue(new ApiError(409, "library key or filesystem path is already registered"));
    const { host, form, submit } = setup();
    submit();
    await vi.waitFor(() => expect(document.querySelector("dialog")).not.toBeNull());
    confirm();
    await vi.waitFor(() => expect(host.textContent).toContain("Bibliotheksschlüssel oder Pfad ist bereits registriert"));
    expect(form.querySelector<HTMLInputElement>("[name=key]")!.value).toBe("cad");
    expect(form.querySelector<HTMLInputElement>("[name=path]")!.value).toBe("/srv/cad");
  });

  it("requires exact confirmation and preserves inputs after cancellation", async () => {
    vi.spyOn(libraryApi, "validatePath").mockResolvedValue({} as LibraryPathValidation);
    const create = vi.spyOn(libraryApi, "create");
    const { host, form, submit } = setup();
    submit();
    await vi.waitFor(() => expect(document.querySelector("dialog")).not.toBeNull());
    const input = document.querySelector<HTMLInputElement>("dialog input")!;
    input.value = "wrong"; input.dispatchEvent(new Event("input"));
    expect(document.querySelector<HTMLButtonElement>("dialog [type=submit]")!.disabled).toBe(true);
    document.querySelector<HTMLButtonElement>("dialog [type=button]")!.click();
    await vi.waitFor(() => expect(host.textContent).toContain("abgebrochen"));
    expect(create).not.toHaveBeenCalled();
    expect(form.querySelector<HTMLInputElement>("[name=path]")!.value).toBe("/srv/cad");
    submit();
    await vi.waitFor(() => expect(document.querySelector("dialog")).not.toBeNull());
    document.querySelector<HTMLButtonElement>("dialog [type=button]")!.click();
    await vi.waitFor(() => expect(document.querySelector("dialog")).toBeNull());
  });

  it("allows a confirmed retry after API failure without losing inputs", async () => {
    vi.spyOn(libraryApi, "validatePath").mockResolvedValue({} as LibraryPathValidation);
    const create = vi.spyOn(libraryApi, "create").mockRejectedValueOnce(new Error("Netzwerkfehler")).mockResolvedValueOnce({} as ManagedLibrary);
    const { host, refresh, submit } = setup();
    submit();
    await vi.waitFor(() => expect(document.querySelector("dialog")).not.toBeNull());
    confirm();
    await vi.waitFor(() => expect(host.textContent).toContain("Netzwerkfehler"));
    expect(refresh).not.toHaveBeenCalled();
    submit();
    await vi.waitFor(() => expect(document.querySelector("dialog")).not.toBeNull());
    confirm();
    await vi.waitFor(() => expect(refresh).toHaveBeenCalledOnce());
    expect(create).toHaveBeenNthCalledWith(2, { key: "cad", name: "CAD", path: "/srv/cad", confirmation: "ADD LIBRARY cad" });
  });

  it("ignores double submit while validation or creation is pending", async () => {
    let validated!: (value: LibraryPathValidation) => void;
    const validate = vi.spyOn(libraryApi, "validatePath").mockImplementation(() => new Promise((resolve) => { validated = resolve; }));
    let created!: (value: ManagedLibrary) => void;
    const create = vi.spyOn(libraryApi, "create").mockImplementation(() => new Promise((resolve) => { created = resolve; }));
    const { refresh, submit } = setup();
    submit(); submit();
    expect(validate).toHaveBeenCalledOnce();
    validated({} as LibraryPathValidation);
    await vi.waitFor(() => expect(document.querySelector("dialog")).not.toBeNull());
    confirm();
    await vi.waitFor(() => expect(create).toHaveBeenCalledOnce());
    submit();
    expect(validate).toHaveBeenCalledOnce();
    expect(document.querySelector("dialog")).toBeNull();
    created({} as ManagedLibrary);
    await vi.waitFor(() => expect(refresh).toHaveBeenCalledOnce());
    await Promise.resolve();
    submit();
    expect(validate).toHaveBeenCalledTimes(2);
    validated({} as LibraryPathValidation);
    await vi.waitFor(() => expect(document.querySelector("dialog")).not.toBeNull());
    document.querySelector<HTMLButtonElement>("dialog [type=button]")!.click();
  });
});
