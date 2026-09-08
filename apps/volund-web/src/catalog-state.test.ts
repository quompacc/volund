import { describe, expect, it } from "vitest";
import { folderCrumbs, readCatalogLocation, writeCatalogLocation } from "./catalog-state";

describe("catalog URL state", () => {
  it("parses, validates, and normalizes URL parameters", () => {
    const state = readCatalogLocation("?root=cad&directory=/printers/voron/&q=frame&format=step&sort=size&direction=desc&offset=100&file=abc", 100);
    expect(state).toEqual({
      root: "cad", directory: "printers/voron", query: "frame", format: "step",
      sort: "size", direction: "desc", offset: 100, limit: 100, file: "abc",
    });
    expect(readCatalogLocation("?sort=random&direction=up&offset=-1", 50).sort).toBe("path");
    expect(readCatalogLocation("?sort=random&direction=up&offset=-1", 50).offset).toBe(0);
  });

  it("serializes only meaningful non-default state", () => {
    const state = readCatalogLocation("?root=cad&directory=parts&q=mount&format=stl&sort=modified&direction=desc&offset=50&file=id", 50);
    expect(writeCatalogLocation(state)).toBe("?root=cad&directory=parts&q=mount&format=stl&sort=modified&direction=desc&offset=50&file=id");
    expect(writeCatalogLocation(readCatalogLocation("", 100))).toBe("/");
    const unicode = readCatalogLocation("?root=archiv&q=%C3%9Cbergr%C3%B6%C3%9Fe+%2B+100%25&offset=50", 50);
    expect(unicode.query).toBe("Übergröße + 100%");
    expect(readCatalogLocation(writeCatalogLocation(unicode), 50)).toEqual(unicode);
  });

  it("builds cumulative folder breadcrumbs", () => {
    expect(folderCrumbs("printers/voron/frame")).toEqual([
      { name: "Wurzel", path: "" },
      { name: "printers", path: "printers" },
      { name: "voron", path: "printers/voron" },
      { name: "frame", path: "printers/voron/frame" },
    ]);
  });
});
