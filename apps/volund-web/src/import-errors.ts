const messages: Record<string, string> = {
  "incoming capacity would be exceeded": "Die Kapazität für eingehende Dateien ist ausgeschöpft. Brich einen offenen Import ab oder erhöhe das Limit.",
  zip_incoming_capacity_exceeded: "Die Kapazität für eingehende Dateien ist ausgeschöpft. Brich einen offenen Import ab oder erhöhe das Limit.",
};

export function importErrorMessage(message: string): string {
  return messages[message] ?? message;
}
