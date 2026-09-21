/** One stored chat row the panel can group. */
export type AssistantChatRow = {
  id: number;
  role: "user" | "assistant" | "tool" | "system_note";
  content: string;
  tool_name?: string | null;
};

/** One row the assistant panel actually paints. */
export type ChatItem =
  | { type: "user"; id: number; content: string }
  | { type: "assistant"; id: number; content: string }
  | { type: "reasoning"; id: number; steps: AssistantChatRow[] };

/**
 * Collapse tool traces and intermediate assistant text into a single
 * "reasoning" block. Older turns hide that block entirely; only the latest
 * turn keeps it, so the chat reads as questions and answers.
 *
 * While a turn is still running, even the latest assistant text stays in
 * reasoning — it may be a thought before the next tool call.
 */
export function groupAssistantMessages(
  messages: AssistantChatRow[],
  opts: { turnInProgress?: boolean } = {},
): ChatItem[] {
  const items: ChatItem[] = [];
  let i = 0;
  while (i < messages.length) {
    const m = messages[i];
    if (m.role === "user") {
      items.push({ type: "user", id: m.id, content: m.content });
      i += 1;
      continue;
    }

    const start = i;
    i += 1;
    while (i < messages.length && messages[i].role !== "user") i += 1;
    const chunk = messages.slice(start, i);
    const isLastTurn = i >= messages.length;
    const lastAsst = lastAssistantIndex(chunk);
    const finishedAnswer =
      lastAsst >= 0
      && lastAsst === chunk.length - 1
      && !(isLastTurn && opts.turnInProgress);
    const answer = finishedAnswer ? chunk[lastAsst] : null;
    const rawReasoning = finishedAnswer ? chunk.slice(0, lastAsst) : chunk;
    const steps = rawReasoning.filter((s) => s.content.trim());

    if (isLastTurn && steps.length) {
      items.push({ type: "reasoning", id: steps[0].id, steps });
    }
    if (answer && answer.content.trim()) {
      items.push({ type: "assistant", id: answer.id, content: answer.content });
    }
  }
  return items;
}

function lastAssistantIndex(chunk: AssistantChatRow[]): number {
  for (let n = chunk.length - 1; n >= 0; n -= 1) {
    if (chunk[n].role === "assistant" && chunk[n].content.trim()) return n;
  }
  return -1;
}
