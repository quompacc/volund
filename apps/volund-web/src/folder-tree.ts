export function treeNodeKey(rootKey: string, directory: string): string {
  return `${rootKey}:${directory}`;
}

export function ancestorPaths(directory: string): string[] {
  const segments = directory.split("/").filter(Boolean);
  return ["", ...segments.map((_, index) => segments.slice(0, index + 1).join("/"))];
}

export function folderCountLabel(count: number): string {
  return count === 1 ? "1 Datei" : `${count} Dateien`;
}
