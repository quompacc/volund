// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import {
  THEME_STORAGE_KEY, applyThemePreference, browserThemeStorage, mountThemeToggle, nextThemePreference,
  readThemePreference, storeThemePreference, themeToggleMarkup, themeToggleTitle,
} from "./theme";

function memoryStorage(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
    removeItem: (key: string) => void values.delete(key),
    values,
  };
}

const brokenStorage = {
  getItem: () => { throw new Error("blocked"); },
  setItem: () => { throw new Error("blocked"); },
  removeItem: () => { throw new Error("blocked"); },
};

describe("theme preference", () => {
  it("reads only known stored values and falls back to the system theme", () => {
    expect(readThemePreference(memoryStorage({ [THEME_STORAGE_KEY]: "dark" }))).toBe("dark");
    expect(readThemePreference(memoryStorage({ [THEME_STORAGE_KEY]: "light" }))).toBe("light");
    expect(readThemePreference(memoryStorage({ [THEME_STORAGE_KEY]: "neon" }))).toBe("system");
    expect(readThemePreference(null)).toBe("system");
    expect(readThemePreference(brokenStorage)).toBe("system");
  });

  it("persists explicit choices and forgets the system default", () => {
    const storage = memoryStorage();
    storeThemePreference(storage, "dark");
    expect(storage.values.get(THEME_STORAGE_KEY)).toBe("dark");
    storeThemePreference(storage, "system");
    expect(storage.values.has(THEME_STORAGE_KEY)).toBe(false);
    expect(() => storeThemePreference(brokenStorage, "light")).not.toThrow();
  });

  it("sets and clears the root theme attribute", () => {
    const root = document.createElement("html");
    applyThemePreference("dark", root);
    expect(root.dataset.theme).toBe("dark");
    applyThemePreference("system", root);
    expect(root.hasAttribute("data-theme")).toBe(false);
  });

  it("cycles system, light and dark with descriptive labels", () => {
    expect(nextThemePreference("system")).toBe("light");
    expect(nextThemePreference("light")).toBe("dark");
    expect(nextThemePreference("dark")).toBe("system");
    expect(themeToggleMarkup("light")).toContain("icon-sun");
    expect(themeToggleMarkup("dark")).toContain("Dunkel");
    expect(themeToggleTitle("system")).toBe("Darstellung: System – wechseln zu Hell");
  });

  it("exposes browser storage when it is accessible", () => {
    expect(browserThemeStorage()).toBe(window.localStorage);
  });

  it("mounts a toggle that applies, stores and relabels the next theme", () => {
    const storage = memoryStorage();
    const root = document.createElement("html");
    const button = document.createElement("button");
    mountThemeToggle(button, root, storage);
    expect(button.getAttribute("aria-label")).toContain("Darstellung: System");
    button.click();
    expect(root.dataset.theme).toBe("light");
    expect(storage.values.get(THEME_STORAGE_KEY)).toBe("light");
    expect(button.textContent).toContain("Hell");
    button.click();
    expect(root.dataset.theme).toBe("dark");
  });
});
