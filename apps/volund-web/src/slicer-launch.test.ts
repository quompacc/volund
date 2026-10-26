// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { fileDetailMarkup } from "./model-presentation";
import { loadSlicerTargets, mountSlicerButtons, SLICER_REACTION_MS, watchSlicerLaunch } from "./slicer-launch";
import type { ModelFile } from "./types";

/** A window stand-in whose focus, visibility, and navigation the test controls. */
function fakeWindow(focused = true) {
  const events = new EventTarget();
  const documentEvents = new EventTarget();
  const doc = Object.assign(documentEvents, { visibilityState: "visible", hasFocus: () => focused });
  return Object.assign(events, {
    document: doc,
    location: { href: "" },
    setTimeout: (callback: () => void, ms: number) => window.setTimeout(callback, ms),
    clearTimeout: (id: number) => window.clearTimeout(id),
  }) as unknown as Window & { document: { visibilityState: string } };
}

const printable: ModelFile = {
  id: "file-1", path: "parts/body.stl", rootKey: "models", rootName: "Models",
  role: "printable-mesh", primary: false, format: "stl", byteSize: 42,
  modifiedAtUnixMs: 0, missing: false, revision: 1, caption: "", description: "",
  notes: "", printable: true, printed: false, preSupported: false, upAxis: null,
  supportHint: "", orientation: [0, 0, 0], lifecycleState: "available", lifecycleRevision: 1,
};

afterEach(() => { vi.useRealTimers(); });

describe("slicer availability", () => {
  it("treats a failing target request as unavailable, not as no targets", async () => {
    expect(await loadSlicerTargets(() => Promise.reject(new Error("503")))).toBeNull();
    expect(await loadSlicerTargets(() => Promise.resolve([]))).toEqual([]);
  });

  it("explains an unavailable server configuration instead of offering buttons", () => {
    const markup = fileDetailMarkup(printable, false, null);
    expect(markup).toContain("auf dem Server nicht vollständig eingerichtet");
    expect(markup).not.toContain("data-slicer-target");
    expect(markup).not.toContain("Kein Slicer-Ziel ist konfiguriert");
  });
});

describe("slicer launch watch", () => {
  it("reports no reaction when the page keeps focus and stays visible", () => {
    vi.useFakeTimers();
    const onNoReaction = vi.fn();
    watchSlicerLaunch(fakeWindow(), onNoReaction);
    vi.advanceTimersByTime(SLICER_REACTION_MS);
    expect(onNoReaction).toHaveBeenCalledOnce();
  });

  it("stays quiet when a local program takes focus or hides the page", () => {
    vi.useFakeTimers();
    for (const react of [
      (win: ReturnType<typeof fakeWindow>) => win.dispatchEvent(new Event("blur")),
      (win: ReturnType<typeof fakeWindow>) => {
        win.document.visibilityState = "hidden";
        win.document.dispatchEvent(new Event("visibilitychange"));
      },
    ]) {
      const win = fakeWindow();
      const onNoReaction = vi.fn();
      watchSlicerLaunch(win, onNoReaction);
      react(win);
      vi.advanceTimersByTime(SLICER_REACTION_MS);
      expect(onNoReaction).not.toHaveBeenCalled();
    }
    const unfocused = vi.fn();
    watchSlicerLaunch(fakeWindow(false), unfocused);
    vi.advanceTimersByTime(SLICER_REACTION_MS);
    expect(unfocused).not.toHaveBeenCalled();
  });

  it("launches the slicer link and falls back to the download link without a reaction", async () => {
    vi.useFakeTimers();
    const dialog = document.createElement("div");
    dialog.innerHTML = '<button data-slicer-target="prusa"></button><p id="slicer-status"></p>';
    const win = fakeWindow();
    const handoff = {
      targetId: "prusa", launchUrl: "prusaslicer://open?file=x",
      downloadUrl: "https://vault.test/api/v1/slicer-download/t", expiresAtUnixMs: 0,
    };
    const create = vi.fn().mockResolvedValue(handoff);
    mountSlicerButtons(dialog, create, win);
    dialog.querySelector("button")!.click();
    await vi.waitFor(() => expect(win.location.href).toBe(handoff.launchUrl));
    expect(create).toHaveBeenCalledWith("prusa");
    vi.advanceTimersByTime(SLICER_REACTION_MS);
    const status = dialog.querySelector("#slicer-status")!;
    expect(status.textContent).toContain("Kein Slicer hat reagiert");
    expect(status.querySelector("a")!.getAttribute("href")).toBe(handoff.downloadUrl);
  });

  it("shows the server's reason when a handoff is refused", async () => {
    const dialog = document.createElement("div");
    dialog.innerHTML = '<button data-slicer-target="prusa"></button><p id="slicer-status"></p>';
    mountSlicerButtons(dialog, () => Promise.reject(new Error("too many open slicer handoffs")), fakeWindow());
    const button = dialog.querySelector("button")!;
    button.click();
    await vi.waitFor(() => expect(dialog.querySelector("#slicer-status")!.textContent).toBe("too many open slicer handoffs"));
    expect(button.disabled).toBe(false);
  });
});
