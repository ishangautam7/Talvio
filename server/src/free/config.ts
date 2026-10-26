/**
 * Free credit configuration (PROTOCOL.md, "Free credit"), read from the
 * environment only. The upstream API keys live here and in memory: never in
 * SQLite, a log line or an error sent to a client.
 */

export interface ChatUpstream {
  baseUrl: string;
  apiKey: string;
  model: string;
  /** USD per million prompt tokens. */
  inputPerMTok: number;
  /** USD per million completion tokens (reasoning included). */
  outputPerMTok: number;
  /** The most completion tokens one request may ask for. */
  maxOutputTokens: number;
}

export interface SttUpstream {
  baseUrl: string;
  apiKey: string;
  model: string;
  /** USD per hour of audio. */
  perHour: number;
  /** The upstream bills at least this many seconds per request. */
  minSeconds: number;
}

export interface TtsUpstream {
  /** `openai`: POST {base}/audio/speech. `elevenlabs`: POST {base}/v1/text-to-speech/{voice}. */
  kind: "openai" | "elevenlabs";
  baseUrl: string;
  apiKey: string;
  model: string;
  /** Used when the request names no voice, or one this upstream would not know. */
  voice: string;
  /** USD per 1000 characters of input. */
  perKChars: number;
}

export interface FreeConfig {
  /** Each account's allowance in USD. */
  limitUsd: number;
  /** Metered requests per minute per account. */
  perMinute: number;
  chat: ChatUpstream | null;
  stt: SttUpstream | null;
  tts: TtsUpstream | null;
  /** Upstream timeouts in ms; tests shorten them. */
  timeouts?: { chat?: number; stt?: number; tts?: number };
}

/**
 * Default prices, from the providers' public price pages (October 2026); the
 * operator overrides them with the `*_USD_*` variables when an upstream differs.
 * - Chat: OpenAI GPT-5.6 Luna (also on Azure OpenAI), $0.20 / $1.20 per million
 *   input / output tokens.
 * - Transcription: Groq whisper-large-v3-turbo, $0.04 per hour, 10 s minimum.
 * - Speech: ElevenLabs Flash v2.5 API, $0.05 per 1000 characters.
 */
export const DEFAULT_PRICES = {
  chatInputPerMTok: 0.2,
  chatOutputPerMTok: 1.2,
  sttPerHour: 0.04,
  sttMinSeconds: 10,
  ttsPerKChars: 0.05,
} as const;

export const DEFAULT_MAX_OUTPUT_TOKENS = 8192;
export const DEFAULT_TTS_VOICE = {
  openai: "alloy",
  // ElevenLabs' premade "Rachel".
  elevenlabs: "21m00Tcm4TlvDq8EwSzR",
} as const;
export const DEFAULT_TTS_MODEL = {
  openai: "gpt-4o-mini-tts",
  elevenlabs: "eleven_flash_v2_5",
} as const;

type Env = Record<string, string | undefined>;

function number(env: Env, name: string, fallback: number): number {
  const raw = env[name]?.trim();
  if (!raw) return fallback;
  const value = Number(raw);
  if (!Number.isFinite(value) || value < 0) {
    throw new Error(`${name} must be a non-negative number, not "${raw}".`);
  }
  return value;
}

function baseUrl(env: Env, name: string): string | undefined {
  const raw = env[name]?.trim();
  if (!raw) return undefined;
  return normalizeBaseUrl(raw);
}

/**
 * Azure OpenAI hosts are moved to their OpenAI-compatible `/openai/v1` surface,
 * like the desktop app's `llm_client.rs::effective_base_url`, so the endpoint
 * pasted from the Azure portal works as it is.
 */
export function normalizeBaseUrl(raw: string): string {
  const trimmed = raw.trim().replace(/\/+$/, "");
  const url = new URL(trimmed); // throws on a malformed URL before anything starts
  if (isAzureHost(url.hostname)) return `https://${url.host}/openai/v1`;
  return trimmed;
}

export function isAzureHost(host: string): boolean {
  const h = host.toLowerCase();
  return (
    h.endsWith(".openai.azure.com") ||
    h.endsWith(".services.ai.azure.com") ||
    h.endsWith(".cognitiveservices.azure.com") ||
    h.endsWith(".openai.azure.us")
  );
}

/** The free credit settings from the environment; each kind is null unless its URL, key and model are all set. */
export function freeFromEnv(env: Env): FreeConfig {
  const chatUrl = baseUrl(env, "FREE_CHAT_BASE_URL");
  const sttUrl = baseUrl(env, "FREE_STT_BASE_URL");
  const ttsUrl = baseUrl(env, "FREE_TTS_BASE_URL");
  const ttsKind: TtsUpstream["kind"] =
    env.FREE_TTS_KIND?.trim() === "elevenlabs" ||
    (env.FREE_TTS_KIND?.trim() !== "openai" &&
      !!ttsUrl &&
      new URL(ttsUrl).hostname.endsWith("elevenlabs.io"))
      ? "elevenlabs"
      : "openai";
  const chatKey = env.FREE_CHAT_API_KEY?.trim();
  const chatModel = env.FREE_CHAT_MODEL?.trim();
  const sttKey = env.FREE_STT_API_KEY?.trim();
  const sttModel = env.FREE_STT_MODEL?.trim();
  const ttsKey = env.FREE_TTS_API_KEY?.trim();
  return {
    limitUsd: number(env, "FREE_CREDIT_USD", 1),
    perMinute: number(env, "FREE_RATE_PER_MINUTE", 60),
    chat:
      chatUrl && chatKey && chatModel
        ? {
            baseUrl: chatUrl,
            apiKey: chatKey,
            model: chatModel,
            inputPerMTok: number(
              env,
              "FREE_CHAT_INPUT_USD_PER_MTOK",
              DEFAULT_PRICES.chatInputPerMTok,
            ),
            outputPerMTok: number(
              env,
              "FREE_CHAT_OUTPUT_USD_PER_MTOK",
              DEFAULT_PRICES.chatOutputPerMTok,
            ),
            maxOutputTokens: number(
              env,
              "FREE_CHAT_MAX_OUTPUT_TOKENS",
              DEFAULT_MAX_OUTPUT_TOKENS,
            ),
          }
        : null,
    stt:
      sttUrl && sttKey && sttModel
        ? {
            baseUrl: sttUrl,
            apiKey: sttKey,
            model: sttModel,
            perHour: number(
              env,
              "FREE_STT_USD_PER_HOUR",
              DEFAULT_PRICES.sttPerHour,
            ),
            minSeconds: number(
              env,
              "FREE_STT_MIN_SECONDS",
              DEFAULT_PRICES.sttMinSeconds,
            ),
          }
        : null,
    tts:
      ttsUrl && ttsKey
        ? {
            kind: ttsKind,
            baseUrl: ttsUrl,
            apiKey: ttsKey,
            model: env.FREE_TTS_MODEL?.trim() || DEFAULT_TTS_MODEL[ttsKind],
            voice: env.FREE_TTS_VOICE?.trim() || DEFAULT_TTS_VOICE[ttsKind],
            perKChars: number(
              env,
              "FREE_TTS_USD_PER_1K_CHARS",
              DEFAULT_PRICES.ttsPerKChars,
            ),
          }
        : null,
  };
}

/** A short, key-free description for the start-up log. */
export function describeFree(config: FreeConfig): string {
  const parts = [
    config.chat &&
      `chat ${new URL(config.chat.baseUrl).host} ${config.chat.model}`,
    config.stt &&
      `transcribe ${new URL(config.stt.baseUrl).host} ${config.stt.model}`,
    config.tts && `speech ${config.tts.kind} ${config.tts.model}`,
  ].filter(Boolean);
  return parts.length === 0
    ? "off"
    : `$${config.limitUsd} per account (${parts.join(", ")})`;
}
