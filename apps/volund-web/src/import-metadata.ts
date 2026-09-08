import type { ImportMetadataRequest } from "./types";

export interface ImportMetadataValues {
  modelName: string;
  kind: string;
  libraryRootId: string;
  description: string;
  authorName: string;
  tags: string[];
  collectionIds: string[];
  targetAction?: "create" | "update" | "extend";
  targetModel?: { id: string; revision: number } | null;
  licenseKind?: "not-specified" | "spdx" | "custom";
  licenseValue?: string;
  primaryItemId?: string | null;
  thumbnailItemId?: string | null;
}

export function parseTags(value: string): string[] {
  const tags: string[] = [];
  for (const candidate of value.split(",")) {
    const tag = candidate.trim();
    if (tag && !tags.some((existing) => existing.toLocaleLowerCase() === tag.toLocaleLowerCase())) {
      tags.push(tag);
    }
  }
  return tags;
}

export function appendTags(existing: string[], value: string): string[] {
  const tags = [...existing];
  for (const tag of parseTags(value)) {
    if (!tags.some((current) => current.toLocaleLowerCase() === tag.toLocaleLowerCase())) tags.push(tag);
  }
  return tags;
}

export function suggestedTags(available: string[], selected: string[], query: string, limit = 8): string[] {
  const selectedKeys = new Set(selected.map((tag) => tag.toLocaleLowerCase()));
  const needle = query.trim().toLocaleLowerCase();
  return available.filter((tag, index) =>
    available.findIndex((candidate) => candidate.toLocaleLowerCase() === tag.toLocaleLowerCase()) === index
    && !selectedKeys.has(tag.toLocaleLowerCase())
    && (!needle || tag.toLocaleLowerCase().includes(needle)))
    .slice(0, limit);
}

export function buildMetadataRequest(values: ImportMetadataValues): ImportMetadataRequest {
  const modelName = values.modelName.trim();
  if (!modelName) throw new Error("Bitte einen Modellnamen eingeben.");
  if (!isModelKind(values.kind)) throw new Error("Bitte einen gültigen Modelltyp wählen.");
  if (!values.libraryRootId) throw new Error("Bitte eine Zielbibliothek wählen.");
  if (values.targetAction !== undefined && values.targetAction !== "create" && !values.targetModel) {
    throw new Error("Bitte das bestehende Zielmodell wählen.");
  }
  if (values.licenseKind && values.licenseKind !== "not-specified" && !values.licenseValue?.trim()) {
    throw new Error("Bitte einen Lizenzwert eingeben.");
  }
  return {
    modelName,
    kind: values.kind,
    libraryRootId: values.libraryRootId,
    description: values.description.trim(),
    authorName: values.authorName.trim() || null,
    tags: values.tags.reduce(appendTags, []),
    collectionIds: [...new Set(values.collectionIds)],
    targetAction: values.targetAction ?? "create",
    targetModelId: values.targetModel?.id ?? null,
    expectedModelRevision: values.targetModel?.revision ?? null,
    licenseKind: values.licenseKind ?? "not-specified",
    licenseValue: values.licenseKind && values.licenseKind !== "not-specified"
      ? values.licenseValue?.trim() || null : null,
    primaryItemId: values.primaryItemId ?? null,
    thumbnailItemId: values.thumbnailItemId ?? null,
  };
}

function isModelKind(value: string): value is ImportMetadataRequest["kind"] {
  return value === "part" || value === "assembly" || value === "project";
}
