export interface MockModel {
  id: string;
  name: string;
  description: string;
  collection: string;
  formats: string[];
  fileCount: number;
  changed: string;
  cover: "voron" | "forge" | "bracket";
}

export const MOCK_MODELS: MockModel[] = [
  {
    id: "voron-24",
    name: "VORON 2.4",
    description: "Komplette Druckerbaugruppe mit Fertigungsdaten und Dokumentation.",
    collection: "3D-Drucker",
    formats: ["STEP", "STL", "3MF"],
    fileCount: 148,
    changed: "Heute, 16:30",
    cover: "voron",
  },
  {
    id: "forge-assembly",
    name: "Forge Test Assembly",
    description: "Validierte Referenzbaugruppe für den nativen CAD-Konverter.",
    collection: "Prüfmodelle",
    formats: ["STEP", "IGES", "BREP"],
    fileCount: 6,
    changed: "Heute, 17:24",
    cover: "forge",
  },
  {
    id: "precision-bracket",
    name: "Precision Bracket Set",
    description: "Haltervarianten, Zeichnungen und CNC-Parameter.",
    collection: "Werkstatt",
    formats: ["STL", "PDF", "XLSX"],
    fileCount: 12,
    changed: "Gestern",
    cover: "bracket",
  },
];

export function mockFileTotal(models: MockModel[]): number {
  return models.reduce((total, model) => total + model.fileCount, 0);
}
