// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { catalogApi } from "./api";
import { waitForArtifact } from "./mockup";

describe("preview polling lifecycle", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("does not issue another preview request after the view is disposed", async () => {
    vi.useFakeTimers();
    const previews = vi.spyOn(catalogApi, "previews").mockResolvedValue({ items: [], total: 0, limit: 50, offset: 0 });
    let cancelled = false;

    const waiting = waitForArtifact("file-1", "preview-glb", () => cancelled);
    cancelled = true;
    await vi.advanceTimersByTimeAsync(2000);
    await waiting;

    expect(previews).not.toHaveBeenCalled();
  });

  it("ends a missing-preview wait after the bounded polling window", async () => {
    vi.useFakeTimers();
    const previews = vi.spyOn(catalogApi, "previews").mockResolvedValue({ items: [], total: 0, limit: 50, offset: 0 });

    const waiting = waitForArtifact("file-without-preview", "preview-glb", () => false);
    await vi.runAllTimersAsync();

    await expect(waiting).resolves.toBeUndefined();
    expect(previews).toHaveBeenCalledTimes(90);
  });
});
