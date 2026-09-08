import type { ModelFile, Setting } from "./types";

export function settingString(settings: Setting[], key: string, fallback: string): string {
  const value = settings.find((setting) => setting.key === key)?.value;
  return typeof value === "string" ? value : fallback;
}

export function preferredModelFile(files: ModelFile[], imageFirst: boolean): ModelFile | undefined {
  const available = files.filter((file) => !file.missing);
  if (imageFirst) {
    const image = available.find((file) => file.role === "image");
    if (image) return image;
  }
  return available.find((file) => file.primary)
    ?? available.find((file) => file.role === "image")
    ?? available[0];
}
