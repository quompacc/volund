export type ThemePreference = "system" | "light" | "dark";

type ThemeStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

export const THEME_STORAGE_KEY = "volund.theme";
const ORDER: ThemePreference[] = ["system", "light", "dark"];
const LABELS: Record<ThemePreference, string> = { system: "System", light: "Hell", dark: "Dunkel" };
const ICONS: Record<ThemePreference, string> = { system: "icon-monitor", light: "icon-sun", dark: "icon-moon" };

/** Returns browser storage, or null when the browser blocks access (private mode, sandboxing). */
export function browserThemeStorage(): ThemeStorage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function readThemePreference(storage: ThemeStorage | null): ThemePreference {
  try {
    const value = storage?.getItem(THEME_STORAGE_KEY);
    return value === "light" || value === "dark" ? value : "system";
  } catch {
    return "system";
  }
}

export function storeThemePreference(storage: ThemeStorage | null, preference: ThemePreference): void {
  try {
    if (preference === "system") storage?.removeItem(THEME_STORAGE_KEY);
    else storage?.setItem(THEME_STORAGE_KEY, preference);
  } catch {
    // The choice still applies to the current page when it cannot be persisted.
  }
}

export function applyThemePreference(preference: ThemePreference, root: HTMLElement): void {
  if (preference === "system") delete root.dataset.theme;
  else root.dataset.theme = preference;
}

export function nextThemePreference(preference: ThemePreference): ThemePreference {
  return ORDER[(ORDER.indexOf(preference) + 1) % ORDER.length]!;
}

export function themeToggleMarkup(preference: ThemePreference): string {
  return `<span class="icon ${ICONS[preference]}" aria-hidden="true"></span><span class="theme-toggle-label">${LABELS[preference]}</span>`;
}

export function themeToggleTitle(preference: ThemePreference): string {
  return `Darstellung: ${LABELS[preference]} – wechseln zu ${LABELS[nextThemePreference(preference)]}`;
}

/** Wires a button that cycles System → Hell → Dunkel and persists the choice. */
export function mountThemeToggle(button: HTMLButtonElement, root: HTMLElement, storage: ThemeStorage | null): void {
  let preference = readThemePreference(storage);
  const render = (): void => {
    button.innerHTML = themeToggleMarkup(preference);
    button.title = themeToggleTitle(preference);
    button.setAttribute("aria-label", themeToggleTitle(preference));
  };
  render();
  button.addEventListener("click", () => {
    preference = nextThemePreference(preference);
    applyThemePreference(preference, root);
    storeThemePreference(storage, preference);
    render();
  });
}
