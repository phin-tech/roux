export interface MultiLineEditorSeed {
  text: string;
  seeded: boolean;
}

export function resolveMultiLineEditorSeed(
  explicitText: string | null,
  selectedText: string | null,
): MultiLineEditorSeed {
  if (explicitText !== null) {
    return {
      text: explicitText,
      seeded: explicitText.length > 0,
    };
  }

  if (selectedText) {
    return {
      text: selectedText,
      seeded: true,
    };
  }

  return {
    text: "",
    seeded: false,
  };
}
