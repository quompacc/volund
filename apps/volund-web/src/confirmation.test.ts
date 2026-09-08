// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from "vitest";
import { confirmExact } from "./confirmation";

afterEach(() => document.body.replaceChildren());

describe("exact action confirmation", () => {
  it("requires exact text and treats target/effect as text, not markup", async () => {
    const result = confirmExact("ADD <library>", "Effect <script>unsafe</script>");
    const dialog = document.querySelector("dialog")!;
    expect(dialog.open).toBe(true);
    expect(dialog.querySelector("script")).toBeNull();
    expect(dialog.textContent).toContain("ADD <library>");
    const input = dialog.querySelector("input")!;
    const submit = dialog.querySelector<HTMLButtonElement>("[type=submit]")!;
    input.value = "ADD <library> ";
    input.dispatchEvent(new Event("input"));
    expect(submit.disabled).toBe(true);
    dialog.querySelector("form")!.dispatchEvent(new Event("submit", { cancelable: true }));
    expect(dialog.isConnected).toBe(true);
    input.value = "ADD <library>";
    input.dispatchEvent(new Event("input"));
    expect(submit.disabled).toBe(false);
    submit.click();
    await expect(result).resolves.toBe("ADD <library>");
    expect(document.querySelector("dialog")).toBeNull();
  });

  it.each(["button", "cancel", "close"])("cancels through %s and returns focus", async (method) => {
    const trigger = document.createElement("button");
    document.body.append(trigger); trigger.focus();
    const result = confirmExact("APPLY", "Effect");
    const rejected = expect(result).rejects.toThrow("abgebrochen");
    const dialog = document.querySelector("dialog")!;
    if (method === "button") dialog.querySelector<HTMLButtonElement>("[type=button]")!.click();
    else dialog.dispatchEvent(new Event(method, { cancelable: true }));
    await rejected;
    expect(document.querySelector("dialog")).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });

  it("refuses a second simultaneous confirmation", async () => {
    const first = confirmExact("FIRST", "Effect");
    const rejected = expect(first).rejects.toThrow("abgebrochen");
    await expect(confirmExact("SECOND", "Effect")).rejects.toThrow("offene Bestätigung");
    expect(document.querySelectorAll("dialog")).toHaveLength(1);
    document.querySelector("dialog")!.dispatchEvent(new Event("cancel", { cancelable: true }));
    await rejected;
  });
});
