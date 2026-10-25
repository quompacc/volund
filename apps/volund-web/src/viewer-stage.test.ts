// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import { systemPrefersLightStage } from "./viewer";

describe("system viewer stage", () => {
  it("follows an explicit page theme before the operating system preference", () => {
    const root = document.createElement("html");
    root.dataset.theme = "dark";
    expect(systemPrefersLightStage(root)).toBe(false);
    root.dataset.theme = "light";
    expect(systemPrefersLightStage(root)).toBe(true);
  });

  it("uses the operating system preference when no page theme is chosen", () => {
    const root = document.createElement("html");
    const matchMedia = vi.spyOn(window, "matchMedia");
    matchMedia.mockReturnValue({ matches: true } as MediaQueryList);
    expect(systemPrefersLightStage(root)).toBe(false);
    matchMedia.mockReturnValue({ matches: false } as MediaQueryList);
    expect(systemPrefersLightStage(root)).toBe(true);
    matchMedia.mockRestore();
  });
});
