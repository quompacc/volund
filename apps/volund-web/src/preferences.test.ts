import { describe, expect, it } from "vitest";
import { preferredModelFile, settingString } from "./preferences";
import type { ModelFile, Setting } from "./types";

const setting = (key: string, value: unknown): Setting => ({
  key, value, domain: "test", valueType: "string", origin: "default",
  editable: true, sensitive: false, revision: 0, effect: "immediate", constraints: {},
});

const file = (id: string, role: ModelFile["role"], primary = false, missing = false): ModelFile => ({
  id, path: `${id}.step`, byteSize: 1, role, primary, format: "step", missing,
  rootKey: "test", rootName: "Test", modifiedAtUnixMs: 0,
  revision: 1, caption: "", description: "", notes: "", printable: false, printed: false,
  preSupported: false, upAxis: null, supportHint: "", orientation: [0, 0, 0], lifecycleState: "available", lifecycleRevision: 1,
});

describe("instance preferences", () => {
  it("reads string preferences and falls back for missing or malformed values", () => {
    expect(settingString([setting("instance.locale", "de-DE")], "instance.locale", "en-US")).toBe("de-DE");
    expect(settingString([setting("instance.locale", 42)], "instance.locale", "en-US")).toBe("en-US");
    expect(settingString([], "instance.locale", "en-US")).toBe("en-US");
  });

  it("selects image-first or primary media without selecting missing files", () => {
    const files = [file("missing-image", "image", false, true), file("primary", "master-cad", true), file("image", "image")];
    expect(preferredModelFile(files, false)?.id).toBe("primary");
    expect(preferredModelFile(files, true)?.id).toBe("image");
    expect(preferredModelFile([file("missing", "image", false, true)], true)).toBeUndefined();
  });
});
