export function parentDirectory(path: string): string {
  const separator = path.lastIndexOf("/");
  return separator < 0 ? "" : path.slice(0, separator);
}

export function moveTargetPath(directory: string, sourcePath: string): string {
  const fileName = sourcePath.split("/").at(-1) ?? sourcePath;
  return directory ? `${directory}/${fileName}` : fileName;
}
