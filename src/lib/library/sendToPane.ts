import {
  readLibraryItem,
  renderLibraryPrompt,
  writeToSession,
  type LibraryRead,
} from "$lib/tauri";
import { requestLibraryVariables } from "$lib/stores/libraryVariablePrompt";
import { readLibraryPromptDragData } from "./drag";

export async function renderLibraryReadForSend(
  read: LibraryRead,
  sessionId: string | null,
): Promise<string | null> {
  if (read.item.itemType === "skill") return read.body;

  const variables = await requestLibraryVariables({
    title: read.item.title,
    variables: read.item.variables,
    initialValues: {},
  });
  if (!variables) return null;

  return (
    await renderLibraryPrompt({
      itemId: read.item.id,
      sessionId,
      variables,
    })
  ).content;
}

export async function sendLibraryItemToPty(
  itemId: string,
  ptyId: string,
  sessionId: string | null,
): Promise<boolean> {
  const read = await readLibraryItem(itemId, sessionId);
  const content = await renderLibraryReadForSend(read, sessionId);
  if (content === null) return false;

  await writeToSession(ptyId, `${content}\r`);
  return true;
}

export async function sendDroppedLibraryPromptToPty(
  dataTransfer: DataTransfer | null,
  ptyId: string | null,
  sessionId: string | null,
): Promise<boolean> {
  const payload = readLibraryPromptDragData(dataTransfer);
  if (!payload || !ptyId) return false;

  return sendLibraryItemToPty(payload.itemId, ptyId, sessionId);
}
