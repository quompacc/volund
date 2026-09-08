import { describe, expect, it } from "vitest";
import { appViewUrl, readAppView, readModelId } from "./app-state";

describe("application view state", () => {
  it("opens the dashboard by default and preserves legacy catalog links", () => {
    expect(readAppView("")) .toBe("dashboard");
    expect(readAppView("?root=cad&directory=Parts")).toBe("raw");
  });

  it("accepts only known views and creates stable view URLs", () => {
    expect(readAppView("?view=model")).toBe("model");
    expect(readAppView("?view=administration")).toBe("administration");
    expect(readAppView("?view=tags")).toBe("tags");
    expect(readAppView("?view=import-history")).toBe("import-history");
    expect(readAppView("?view=model-problems")).toBe("model-problems");
    expect(readAppView("?view=unknown")).toBe("dashboard");
    expect(appViewUrl("imports")).toBe("?view=imports");
    expect(appViewUrl("import-history")).toBe("?view=import-history");
    expect(appViewUrl("raw")).toBe("?view=raw&root=cad");
    expect(appViewUrl("model", "model/id")).toBe("?view=model&model=model%2Fid");
    expect(appViewUrl("model-problems", "model/id")).toBe("?view=model-problems&model=model%2Fid");
    expect(readModelId("?view=model&model=model%2Fid")).toBe("model/id");
  });
});
