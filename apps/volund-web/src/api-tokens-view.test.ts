// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import { apiTokenPanel, bindApiTokens, secretMarkup, type ApiToken } from "./api-tokens-view";

const token: ApiToken = {
  id: "t-1", name: "Backup <Skript>", prefix: "a1b2c3d4", scope: "read",
  createdAtUnixMs: 0, expiresAtUnixMs: Date.UTC(2027, 0, 25), lastUsedAtUnixMs: null,
};

function fakeWindow(confirmed: boolean) {
  const writeText = vi.fn().mockResolvedValue(undefined);
  return { win: Object.assign(Object.create(window), { confirm: vi.fn().mockReturnValue(confirmed), navigator: { clipboard: { writeText } } }) as Window, writeText };
}

describe("API token panel", () => {
  it("lists tokens by prefix only and offers write scope only to editors", () => {
    const markup = apiTokenPanel([token], false);
    expect(markup).toContain("Backup &lt;Skript&gt;");
    expect(markup).toContain("<code>vlnd_a1b2c3d4…</code>");
    expect(markup).toContain("noch nicht genutzt");
    expect(markup).not.toContain('value="catalog-write"');
    expect(apiTokenPanel([], true)).toContain('value="catalog-write"');
    expect(apiTokenPanel([], true)).toContain("Noch kein API-Token angelegt.");
    expect(secretMarkup({ ...token, token: "vlnd_<secret>" })).toContain("vlnd_&lt;secret&gt;");
  });

  it("creates a token, shows the secret once, copies it, and revokes after confirmation", async () => {
    const root = document.createElement("div");
    root.innerHTML = apiTokenPanel([token], true);
    const section = root.querySelector<HTMLElement>("#api-tokens")!;
    const created = { ...token, id: "t-2", name: "Import", scope: "catalog-write" as const, token: `vlnd_${"f".repeat(64)}` };
    const sources = { list: vi.fn(), create: vi.fn().mockResolvedValue(created), revoke: vi.fn().mockResolvedValue(undefined) };
    const refresh = vi.fn().mockResolvedValue(undefined);
    const message = vi.fn();
    const { win, writeText } = fakeWindow(true);
    bindApiTokens(section, root, sources, refresh, message, win);
    const form = section.querySelector<HTMLFormElement>("#create-api-token")!;
    (form.elements.namedItem("name") as HTMLInputElement).value = "  Import ";
    (form.elements.namedItem("scope") as HTMLSelectElement).value = "catalog-write";
    (form.elements.namedItem("days") as HTMLSelectElement).value = "365";
    form.dispatchEvent(new Event("submit", { cancelable: true }));
    await vi.waitFor(() => expect(root.querySelector("[data-token-secret]")?.textContent).toBe(created.token));
    expect(sources.create).toHaveBeenCalledWith("Import", "catalog-write", 365);
    expect(refresh).toHaveBeenCalled();
    root.querySelector<HTMLButtonElement>("[data-copy-token]")!.click();
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith(created.token));
    section.querySelector<HTMLButtonElement>('[data-revoke-token="t-1"]')!.click();
    await vi.waitFor(() => expect(sources.revoke).toHaveBeenCalledWith("t-1"));
    await vi.waitFor(() => expect(message).toHaveBeenCalledWith("API-Token widerrufen."));
  });

  it("does not revoke without confirmation and reports refusals", async () => {
    const root = document.createElement("div");
    root.innerHTML = apiTokenPanel([token], false);
    const section = root.querySelector<HTMLElement>("#api-tokens")!;
    const sources = { list: vi.fn(), create: vi.fn().mockRejectedValue(new Error("at most 20 active API tokens per account")), revoke: vi.fn() };
    const message = vi.fn();
    bindApiTokens(section, root, sources, vi.fn(), message, fakeWindow(false).win);
    section.querySelector<HTMLButtonElement>("[data-revoke-token]")!.click();
    expect(sources.revoke).not.toHaveBeenCalled();
    (section.querySelector<HTMLInputElement>('input[name="name"]')!).value = "X";
    section.querySelector<HTMLFormElement>("#create-api-token")!.dispatchEvent(new Event("submit", { cancelable: true }));
    await vi.waitFor(() => expect(message).toHaveBeenCalledWith("at most 20 active API tokens per account"));
    expect(root.querySelector("[data-token-secret]")).toBeNull();
  });
});
