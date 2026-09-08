import type { CatalogOptions, CatalogSort, SortDirection } from "./types";

const SORTS: CatalogSort[] = ["path", "format", "size", "modified"];

export interface CatalogLocation extends CatalogOptions {
  root: string;
  file: string;
}

export function readCatalogLocation(search: string, limit: number): CatalogLocation {
  const parameters = new URLSearchParams(search);
  const sort = parameters.get("sort") as CatalogSort | null;
  const direction = parameters.get("direction") as SortDirection | null;
  const offset = Number.parseInt(parameters.get("offset") ?? "0", 10);
  return {
    root: parameters.get("root") ?? "",
    directory: (parameters.get("directory") ?? "").replace(/^\/+|\/+$/g, ""),
    query: parameters.get("q") ?? "",
    format: parameters.get("format") ?? "",
    sort: sort && SORTS.includes(sort) ? sort : "path",
    direction: direction === "desc" ? "desc" : "asc",
    offset: Number.isFinite(offset) && offset >= 0 ? offset : 0,
    limit,
    file: parameters.get("file") ?? "",
  };
}

export function writeCatalogLocation(location: CatalogLocation): string {
  const parameters = new URLSearchParams();
  if (location.root) parameters.set("root", location.root);
  if (location.directory) parameters.set("directory", location.directory);
  if (location.query) parameters.set("q", location.query);
  if (location.format) parameters.set("format", location.format);
  if (location.sort !== "path") parameters.set("sort", location.sort);
  if (location.direction !== "asc") parameters.set("direction", location.direction);
  if (location.offset > 0) parameters.set("offset", String(location.offset));
  if (location.file) parameters.set("file", location.file);
  const query = parameters.toString();
  return query ? `?${query}` : location.root ? `?root=${encodeURIComponent(location.root)}` : "/";
}

export function folderCrumbs(directory: string): Array<{ name: string; path: string }> {
  const crumbs = [{ name: "Wurzel", path: "" }];
  const segments = directory.split("/").filter(Boolean);
  segments.forEach((name, index) => crumbs.push({ name, path: segments.slice(0, index + 1).join("/") }));
  return crumbs;
}
