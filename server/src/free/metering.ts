import { isObject } from "../http";

/**
 * Reads a chat completion SSE stream as it passes through, for its `usage`
 * (the chunk `stream_options.include_usage` adds before `[DONE]`) and, in case
 * none comes, how many characters of answer went by.
 */
export class SseMeter {
  usage: { prompt_tokens: number; completion_tokens: number } | null = null;
  outputChars = 0;
  private readonly decoder = new TextDecoder();
  private pending = "";

  push(bytes: Uint8Array): void {
    this.pending += this.decoder.decode(bytes, { stream: true });
    const lines = this.pending.split("\n");
    this.pending = lines.pop() ?? "";
    for (const line of lines) this.line(line.trim());
  }

  private line(line: string): void {
    if (!line.startsWith("data:")) return;
    const data = line.slice(5).trim();
    if (!data || data === "[DONE]") return;
    let event: unknown;
    try {
      event = JSON.parse(data);
    } catch {
      return;
    }
    if (!isObject(event)) return;
    const usage = event.usage;
    if (
      isObject(usage) &&
      typeof usage.prompt_tokens === "number" &&
      typeof usage.completion_tokens === "number"
    ) {
      this.usage = {
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
      };
    }
    for (const choice of Array.isArray(event.choices) ? event.choices : []) {
      const delta = isObject(choice) ? choice.delta : undefined;
      if (!isObject(delta)) continue;
      for (const field of ["content", "reasoning_content", "reasoning"]) {
        const text = delta[field];
        if (typeof text === "string") this.outputChars += text.length;
      }
      for (const call of Array.isArray(delta.tool_calls)
        ? delta.tool_calls
        : []) {
        const fn = isObject(call) ? call.function : undefined;
        if (isObject(fn) && typeof fn.arguments === "string")
          this.outputChars += fn.arguments.length;
      }
    }
  }
}

/**
 * The duration of a RIFF/WAVE file from its `fmt ` and `data` chunks, or null
 * when the bytes are not a WAV file. A `data` size larger than what arrived
 * (a streaming writer's placeholder) counts what arrived.
 */
export function wavSeconds(bytes: Uint8Array): number | null {
  if (bytes.length < 12) return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const tag = (at: number) =>
    String.fromCharCode(
      bytes[at]!,
      bytes[at + 1]!,
      bytes[at + 2]!,
      bytes[at + 3]!,
    );
  if (tag(0) !== "RIFF" || tag(8) !== "WAVE") return null;
  let byteRate = 0;
  let offset = 12;
  while (offset + 8 <= bytes.length) {
    const id = tag(offset);
    const size = view.getUint32(offset + 4, true);
    const start = offset + 8;
    if (id === "fmt " && start + 12 <= bytes.length) {
      byteRate = view.getUint32(start + 8, true);
    } else if (id === "data") {
      if (byteRate <= 0) return null;
      const available = bytes.length - start;
      return Math.min(size, available) / byteRate;
    }
    offset = start + size + (size % 2);
  }
  return null;
}
