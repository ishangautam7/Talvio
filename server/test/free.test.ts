import { afterEach, describe, expect, test } from "bun:test";
import { Database } from "bun:sqlite";
import {
  type FreeConfig,
  freeFromEnv,
  normalizeBaseUrl,
} from "../src/free/config";
import { CREDIT_HEADER, estimatePromptTokens } from "../src/free/index";
import { FreeLedger } from "../src/free/ledger";
import { SseMeter, wavSeconds } from "../src/free/metering";
import { type Harness, harness, type Json } from "./helpers";

const CHAT_KEY = "chat-key-SECRET-0123456789";
const STT_KEY = "stt-key-SECRET-0123456789";
const TTS_KEY = "tts-key-SECRET-0123456789";

interface Seen {
  path: string;
  headers: Headers;
  body: Json | null;
  form: Map<string, string | File> | null;
}

/** An OpenAI-compatible (and ElevenLabs) upstream whose answers each test can change. */
function fakeUpstream() {
  const seen: Seen[] = [];
  const behaviour = {
    usage: { prompt_tokens: 1000, completion_tokens: 500 } as Json | null,
    chatText: "Hello there",
    status: 200,
    errorBody: "" as string,
    duration: 12.5 as number | null,
    delayMs: 0,
  };
  const server = Bun.serve({
    port: 0,
    hostname: "127.0.0.1",
    async fetch(req) {
      const path = new URL(req.url).pathname;
      const isForm = req.headers
        .get("content-type")
        ?.startsWith("multipart/form-data");
      let body: Json | null = null;
      let form: Map<string, string | File> | null = null;
      if (isForm) {
        form = new Map();
        for (const [k, v] of await req.formData()) form.set(k, v as never);
      } else {
        const text = await req.text();
        body = text ? JSON.parse(text) : null;
      }
      seen.push({ path, headers: req.headers, body, form });
      if (behaviour.delayMs) await Bun.sleep(behaviour.delayMs);
      if (behaviour.status !== 200)
        return new Response(behaviour.errorBody, {
          status: behaviour.status,
          headers: { "content-type": "application/json" },
        });
      if (path.endsWith("/chat/completions")) {
        if (body?.stream) {
          const words = behaviour.chatText.split(" ");
          const chunks = words.map(
            (w, i) =>
              `data: ${JSON.stringify({ choices: [{ index: 0, delta: { content: (i ? " " : "") + w } }] })}\n\n`,
          );
          if (behaviour.usage)
            chunks.push(
              `data: ${JSON.stringify({ choices: [], usage: behaviour.usage })}\n\n`,
            );
          chunks.push("data: [DONE]\n\n");
          return new Response(
            new ReadableStream({
              start(c) {
                // Split mid-line on purpose: the meter must reassemble lines.
                const all = new TextEncoder().encode(chunks.join(""));
                for (let i = 0; i < all.length; i += 7)
                  c.enqueue(all.slice(i, i + 7));
                c.close();
              },
            }),
            { headers: { "content-type": "text/event-stream" } },
          );
        }
        return Response.json({
          id: "c1",
          object: "chat.completion",
          model: body?.model,
          choices: [
            {
              index: 0,
              message: { role: "assistant", content: behaviour.chatText },
              finish_reason: "stop",
            },
          ],
          ...(behaviour.usage ? { usage: behaviour.usage } : {}),
        });
      }
      if (path.endsWith("/audio/transcriptions")) {
        return Response.json({
          text: "transcribed words",
          ...(behaviour.duration === null
            ? {}
            : { duration: behaviour.duration }),
        });
      }
      if (path.endsWith("/audio/speech") || path.includes("/text-to-speech/")) {
        return new Response(new Uint8Array([0xff, 0xfb, 1, 2, 3]), {
          headers: { "content-type": "audio/mpeg" },
        });
      }
      return new Response("no", { status: 404 });
    },
  });
  const url = `http://127.0.0.1:${server.port}`;
  return { url, seen, behaviour, stop: () => server.stop(true) };
}

type Upstream = ReturnType<typeof fakeUpstream>;

function config(up: Upstream, overrides: Partial<FreeConfig> = {}): FreeConfig {
  return {
    limitUsd: 1,
    perMinute: 1000,
    chat: {
      baseUrl: `${up.url}/v1`,
      apiKey: CHAT_KEY,
      model: "upstream-chat-model",
      inputPerMTok: 0.2,
      outputPerMTok: 1.2,
      maxOutputTokens: 8192,
    },
    stt: {
      baseUrl: `${up.url}/openai/v1`,
      apiKey: STT_KEY,
      model: "whisper-large-v3-turbo",
      perHour: 0.04,
      minSeconds: 10,
    },
    tts: {
      kind: "elevenlabs",
      baseUrl: up.url,
      apiKey: TTS_KEY,
      model: "eleven_flash_v2_5",
      voice: "DefaultVoice00000000",
      perKChars: 0.05,
    },
    ...overrides,
  };
}

let h: Harness | undefined;
let up: Upstream | undefined;
let logs: string[] = [];
afterEach(async () => {
  await h?.close();
  await up?.stop();
  h = undefined;
  up = undefined;
  logs = [];
});

async function setup(
  overrides: Partial<FreeConfig> | ((up: Upstream) => Partial<FreeConfig>) = {},
) {
  up = fakeUpstream();
  const extra = typeof overrides === "function" ? overrides(up) : overrides;
  h = await harness({ free: config(up, extra), log: (l) => logs.push(l) });
  const { token } = await h.signup();
  return token as string;
}

const chat = (token: string, body: Json = {}) =>
  h!.request("POST", "/v1/free/chat/completions", {
    token,
    body: {
      model: "whatever",
      messages: [{ role: "user", content: "hi" }],
      ...body,
    },
  });

const balance = async (token: string) =>
  (await h!.request("GET", "/v1/free/balance", { token })).body;

/** A 16 kHz mono 16-bit WAV of `seconds` of silence. */
function wav(seconds: number): Uint8Array {
  const data = Math.round(seconds * 16000) * 2;
  const buf = new ArrayBuffer(44 + data);
  const v = new DataView(buf);
  const put = (at: number, s: string) =>
    [...s].forEach((c, i) => v.setUint8(at + i, c.charCodeAt(0)));
  put(0, "RIFF");
  v.setUint32(4, 36 + data, true);
  put(8, "WAVE");
  put(12, "fmt ");
  v.setUint32(16, 16, true);
  v.setUint16(20, 1, true);
  v.setUint16(22, 1, true);
  v.setUint32(24, 16000, true);
  v.setUint32(28, 32000, true);
  v.setUint16(32, 2, true);
  v.setUint16(34, 16, true);
  put(36, "data");
  v.setUint32(40, data, true);
  return new Uint8Array(buf);
}

async function transcribe(
  token: string,
  audio: Uint8Array,
  name: string,
  extra: Record<string, string> = {},
) {
  const form = new FormData();
  form.append("file", new Blob([audio]), name);
  form.append("model", "talvio-free-transcribe");
  for (const [k, v] of Object.entries(extra)) form.append(k, v);
  return fetch(`${h!.base}/v1/free/audio/transcriptions`, {
    method: "POST",
    headers: { authorization: `Bearer ${token}` },
    body: form,
  });
}

const speech = (token: string, body: Json) =>
  fetch(`${h!.base}/v1/free/audio/speech`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${token}`,
      "content-type": "application/json",
    },
    body: JSON.stringify(body),
  });

describe("auth and configuration", () => {
  test("every route needs a session", async () => {
    await setup();
    for (const [method, path] of [
      ["GET", "/v1/free/balance"],
      ["GET", "/v1/free/models"],
      ["POST", "/v1/free/chat/completions"],
      ["POST", "/v1/free/audio/transcriptions"],
      ["POST", "/v1/free/audio/speech"],
    ] as const) {
      const reply = await h!.request(method, path, { token: "zv1_nope" });
      expect(reply.status).toBe(401);
      expect(reply.body.error.code).toBe("unauthorized");
    }
    expect(up!.seen).toHaveLength(0);
  });

  test("with no upstream the balance says disabled and the rest is 503", async () => {
    h = await harness();
    const { token } = await h.signup();
    expect(await balance(token)).toEqual({
      limitUsd: 1,
      usedUsd: 0,
      remainingUsd: 1,
      enabled: false,
    });
    for (const path of ["/v1/free/models", "/v1/free/chat/completions"]) {
      const reply = await h.request(
        path.endsWith("models") ? "GET" : "POST",
        path,
        { token, body: path.endsWith("models") ? undefined : {} },
      );
      expect(reply.status).toBe(503);
      expect(reply.body.error.code).toBe("free_disabled");
    }
  });

  test("models lists the configured kinds; a missing kind is 503", async () => {
    const token = await setup({ tts: null });
    const reply = await h!.request("GET", "/v1/free/models", { token });
    expect(reply.body.object).toBe("list");
    expect(reply.body.data.map((m: Json) => m.id)).toEqual([
      "talvio-free-chat",
      "talvio-free-transcribe",
    ]);
    const res = await speech(token, { input: "hi" });
    expect(res.status).toBe(503);
    expect(((await res.json()) as Json).error.code).toBe("free_disabled");
  });

  test("freeFromEnv needs URL, key and model; Azure moves to /openai/v1; ElevenLabs is detected", () => {
    expect(freeFromEnv({})).toMatchObject({ chat: null, stt: null, tts: null });
    const c = freeFromEnv({
      FREE_CREDIT_USD: "2",
      FREE_CHAT_BASE_URL: "https://res.cognitiveservices.azure.com/",
      FREE_CHAT_API_KEY: "k",
      FREE_CHAT_MODEL: "gpt-5.6-luna",
      FREE_STT_BASE_URL: "https://api.groq.com/openai/v1",
      FREE_STT_API_KEY: "k",
      FREE_TTS_BASE_URL: "https://api.elevenlabs.io",
      FREE_TTS_API_KEY: "k",
    });
    expect(c.limitUsd).toBe(2);
    expect(c.chat?.baseUrl).toBe(
      "https://res.cognitiveservices.azure.com/openai/v1",
    );
    expect(c.stt).toBeNull(); // no model
    expect(c.tts?.kind).toBe("elevenlabs");
    expect(c.tts?.model).toBe("eleven_flash_v2_5");
    expect(
      normalizeBaseUrl("https://x.openai.azure.com/openai/deployments"),
    ).toBe("https://x.openai.azure.com/openai/v1");
    expect(() => freeFromEnv({ FREE_CREDIT_USD: "lots" })).toThrow();
  });
});

describe("chat", () => {
  test("replaces the model, caps tokens, meters by usage and reports the balance", async () => {
    const token = await setup();
    const reply = await chat(token, { max_tokens: 100, n: 3, tools: [] });
    expect(reply.status).toBe(200);
    expect(reply.body.choices[0].message.content).toBe("Hello there");
    const sent = up!.seen[0]!;
    expect(sent.path).toBe("/v1/chat/completions");
    expect(sent.headers.get("authorization")).toBe(`Bearer ${CHAT_KEY}`);
    expect(sent.headers.get("api-key")).toBeNull(); // not an Azure host
    expect(sent.body!.model).toBe("upstream-chat-model");
    expect(sent.body!.max_completion_tokens).toBe(100);
    expect(sent.body!.max_tokens).toBeUndefined();
    expect(sent.body!.n).toBeUndefined();
    expect(sent.body!.tools).toEqual([]);
    const cost = (1000 * 0.2 + 500 * 1.2) / 1e6;
    const b = await balance(token);
    expect(b.usedUsd).toBeCloseTo(cost, 10);
    expect(b.remainingUsd).toBeCloseTo(1 - cost, 10);
    expect(Number(reply.headers.get(CREDIT_HEADER))).toBeCloseTo(1 - cost, 6);
  });

  test("streams the upstream's SSE through, forcing include_usage, and meters it", async () => {
    const token = await setup();
    const reply = await chat(token, {
      stream: true,
      stream_options: { foo: 1 },
    });
    expect(reply.headers.get("content-type")).toContain("text/event-stream");
    expect(reply.headers.get(CREDIT_HEADER)).toBe("1.000000");
    expect(reply.text).toContain('"content":"Hello"');
    expect(reply.text).toContain("[DONE]");
    expect(up!.seen[0]!.body!.stream_options).toEqual({
      foo: 1,
      include_usage: true,
    });
    expect((await balance(token)).usedUsd).toBeCloseTo(
      (1000 * 0.2 + 500 * 1.2) / 1e6,
      10,
    );
  });

  test("without usage the cost is estimated from characters", async () => {
    const token = await setup();
    up!.behaviour.usage = null;
    up!.behaviour.chatText = "x".repeat(400); // ~100 tokens out
    await chat(token, { stream: true });
    const prompt = estimatePromptTokens({
      messages: [{ role: "user", content: "hi" }],
      model: "whatever",
      stream: true,
    });
    expect((await balance(token)).usedUsd).toBeCloseTo(
      (prompt * 0.2 + 100 * 1.2) / 1e6,
      9,
    );
  });

  test("images are counted per image, not by their data size", () => {
    const image = `data:image/jpeg;base64,${"A".repeat(500_000)}`;
    const tokens = estimatePromptTokens({
      messages: [
        {
          role: "user",
          content: [{ type: "image_url", image_url: { url: image } }],
        },
      ],
    });
    expect(tokens).toBeLessThan(2000);
    expect(tokens).toBeGreaterThan(1600);
  });

  test("a body over 4 MiB is refused before going upstream", async () => {
    const token = await setup();
    const reply = await chat(token, {
      messages: [{ role: "user", content: "x".repeat(4 * 1024 * 1024) }],
    });
    expect(reply.status).toBe(413);
    expect(up!.seen).toHaveLength(0);
  });

  test("an upstream refusal keeps its status and message, never the key, and costs nothing", async () => {
    const token = await setup();
    up!.behaviour.status = 400;
    up!.behaviour.errorBody = JSON.stringify({
      error: {
        message: `Unsupported parameter: 'temperature' (key ${CHAT_KEY} at 127.0.0.1)`,
        param: "temperature",
      },
    });
    const reply = await chat(token, { temperature: 0 });
    expect(reply.status).toBe(400);
    expect(reply.body.error.code).toBe("upstream_error");
    expect(reply.body.error.param).toBe("temperature");
    expect(reply.body.error.message).toContain("Unsupported parameter");
    expect(reply.text).not.toContain(CHAT_KEY);
    expect((await balance(token)).usedUsd).toBe(0);
  });

  test("an upstream auth failure is a generic 502; logs and replies never hold a key", async () => {
    const token = await setup();
    up!.behaviour.status = 401;
    up!.behaviour.errorBody = JSON.stringify({
      error: { message: `Incorrect API key provided: ${CHAT_KEY}` },
    });
    const reply = await chat(token);
    expect(reply.status).toBe(502);
    expect(reply.body.error.code).toBe("upstream_error");
    expect(reply.text).not.toContain(CHAT_KEY);
    await up!.stop();
    const gone = await chat(token);
    expect(gone.status).toBe(502);
    const all = logs.join("\n");
    for (const key of [CHAT_KEY, STT_KEY, TTS_KEY])
      expect(all).not.toContain(key);
    expect(all).toContain("free chat: upstream answered 401");
    expect((await balance(token)).usedUsd).toBe(0);
  });

  test("rate limited per account", async () => {
    const token = await setup({ perMinute: 2 });
    expect((await chat(token)).status).toBe(200);
    expect((await chat(token)).status).toBe(200);
    const third = await chat(token);
    expect(third.status).toBe(429);
    expect(third.body.error.code).toBe("rate_limited");
    expect(third.headers.get("retry-after")).toBeTruthy();
  });
});

describe("credit", () => {
  test("402 once used up, with the message and the header, without going upstream", async () => {
    // 0.001 USD: one chat at 0.0008 fits, the next cannot reserve 256 output tokens.
    const token = await setup({ limitUsd: 0.001 });
    expect((await chat(token)).status).toBe(200);
    const before = up!.seen.length;
    const reply = await chat(token);
    expect(reply.status).toBe(402);
    expect(reply.body.error).toEqual({
      code: "credit_exhausted",
      message:
        "Your free $0.001 is used up. Add your own API key in Talvio to keep going.",
    });
    expect(reply.headers.get(CREDIT_HEADER)).toBe("0.000200");
    expect(up!.seen.length).toBe(before);
  });

  test("the default message names $1", async () => {
    const token = await setup();
    up!.behaviour.usage = { prompt_tokens: 0, completion_tokens: 1_000_000 };
    await chat(token);
    const reply = await chat(token);
    expect(reply.status).toBe(402);
    expect(reply.body.error.message).toBe(
      "Your free $1 is used up. Add your own API key in Talvio to keep going.",
    );
    expect((await balance(token)).remainingUsd).toBe(0);
  });

  test("output tokens are cut to what the balance covers", async () => {
    // 0.0005 USD left after the prompt covers ~400 tokens at $1.20/M.
    const token = await setup({ limitUsd: 0.0005 });
    await chat(token);
    expect(up!.seen[0]!.body!.max_completion_tokens).toBeLessThan(420);
    expect(up!.seen[0]!.body!.max_completion_tokens).toBeGreaterThan(256);
  });

  test("a request in flight holds its worst case; settling gives the rest back", async () => {
    const token = await setup({ limitUsd: 0.01 });
    up!.behaviour.delayMs = 300;
    // Worst case ≈ 8192 × $1.2/M ≈ $0.0098, leaving under 256 tokens for a second one.
    const first = chat(token);
    await Bun.sleep(100);
    const second = await chat(token);
    expect(second.status).toBe(402);
    expect((await first).status).toBe(200);
    // Charged only the real 0.0008, so another fits now.
    up!.behaviour.delayMs = 0;
    expect((await chat(token)).status).toBe(200);
  });

  test("the balance follows the email across deleting the account", async () => {
    const token = await setup();
    await chat(token);
    const used = (await balance(token)).usedUsd;
    expect(used).toBeGreaterThan(0);
    await h!.request("DELETE", "/v1/me", { token });
    const again = (await h!.signup()).token;
    expect((await balance(again)).usedUsd).toBe(used);
  });

  test("ledger reservations and settlement", () => {
    const ledger = new FreeLedger(new Database(":memory:"), 1);
    const a = ledger.reserve("s", "u", 0.6)!;
    expect(ledger.available("s")).toBeCloseTo(0.4, 10);
    expect(ledger.reserve("s", "u", 0.5)).toBeNull();
    a.settle(
      { kind: "chat", model: "m", units: 1, unit: "tokens", costUsd: 0.1 },
      1,
    );
    a.release(); // a second ending is ignored
    expect(ledger.available("s")).toBeCloseTo(0.9, 10);
    const b = ledger.reserve("s", "u", 0.5)!;
    b.release();
    expect(ledger.balance("s")).toEqual({
      limitUsd: 1,
      usedUsd: 0.1,
      remainingUsd: 0.9,
    });
  });
});

describe("transcription", () => {
  test("forwards multipart with the configured model and meters a WAV by its header", async () => {
    const token = await setup();
    up!.behaviour.duration = null;
    const res = await transcribe(token, wav(30), "a.wav", {
      language: "en",
      prompt: "Talvio",
    });
    expect(res.status).toBe(200);
    expect(await res.json()).toEqual({ text: "transcribed words" });
    expect(Number(res.headers.get(CREDIT_HEADER))).toBeCloseTo(
      1 - (30 * 0.04) / 3600,
      6,
    );
    const sent = up!.seen[0]!;
    expect(sent.path).toBe("/openai/v1/audio/transcriptions");
    expect(sent.headers.get("authorization")).toBe(`Bearer ${STT_KEY}`);
    expect(sent.form!.get("model")).toBe("whisper-large-v3-turbo");
    expect(sent.form!.get("language")).toBe("en");
    expect(sent.form!.get("prompt")).toBe("Talvio");
    expect(sent.form!.get("response_format")).toBe("verbose_json");
  });

  test("the upstream's duration wins, with the minimum billed length; text format", async () => {
    const token = await setup();
    up!.behaviour.duration = 3;
    const res = await transcribe(token, new Uint8Array(5000), "a.mp3", {
      response_format: "text",
    });
    expect(res.headers.get("content-type")).toContain("text/plain");
    expect(await res.text()).toBe("transcribed words");
    expect((await balance(token)).usedUsd).toBeCloseTo((10 * 0.04) / 3600, 8);
  });

  test("an MP3 with no reported duration is estimated at 48 kbit/s", async () => {
    const t = await setup((u) => ({
      stt: { ...config(u).stt!, minSeconds: 0 },
    }));
    up!.behaviour.duration = null;
    await transcribe(t, new Uint8Array(60_000), "a.mp3");
    expect((await balance(t)).usedUsd).toBeCloseTo((10 * 0.04) / 3600, 8);
  });

  test("an empty upload is refused", async () => {
    const token = await setup();
    const res = await transcribe(token, new Uint8Array(0), "a.wav");
    expect(res.status).toBe(400);
  });
});

describe("speech", () => {
  test("ElevenLabs adapter: voice id in the path, xi-api-key, metered by characters", async () => {
    const token = await setup();
    const res = await speech(token, {
      input: "Hello world",
      voice: "alloy",
      model: "tts-1",
      speed: 2,
    });
    expect(res.status).toBe(200);
    expect(res.headers.get("content-type")).toBe("audio/mpeg");
    expect(new Uint8Array(await res.arrayBuffer())[0]).toBe(0xff);
    const sent = up!.seen[0]!;
    expect(sent.path).toBe("/v1/text-to-speech/DefaultVoice00000000");
    expect(sent.headers.get("xi-api-key")).toBe(TTS_KEY);
    expect(sent.headers.get("authorization")).toBeNull();
    expect(sent.body).toEqual({
      text: "Hello world",
      model_id: "eleven_flash_v2_5",
      voice_settings: { speed: 1.2 },
    });
    expect((await balance(token)).usedUsd).toBeCloseTo((11 * 0.05) / 1000, 10);
    await speech(token, { input: "Hi", voice: "r1KmysJdVYZjJCm4mL3b" });
    expect(up!.seen[1]!.path).toBe("/v1/text-to-speech/r1KmysJdVYZjJCm4mL3b");
  });

  test("OpenAI-compatible speech passes a known voice and always asks for MP3", async () => {
    up = fakeUpstream();
    h = await harness({
      free: config(up, {
        tts: {
          kind: "openai",
          baseUrl: `${up.url}/v1`,
          apiKey: TTS_KEY,
          model: "gpt-4o-mini-tts",
          voice: "alloy",
          perKChars: 0.015,
        },
      }),
    });
    const { token } = await h.signup();
    await speech(token, {
      input: "Hey",
      voice: "Coral",
      response_format: "wav",
    });
    expect(up.seen[0]!.path).toBe("/v1/audio/speech");
    expect(up.seen[0]!.body).toEqual({
      model: "gpt-4o-mini-tts",
      input: "Hey",
      voice: "coral",
      response_format: "mp3",
    });
  });

  test("input is required and at most 4096 characters", async () => {
    const token = await setup();
    expect((await speech(token, { input: "" })).status).toBe(400);
    expect((await speech(token, { input: "x".repeat(4097) })).status).toBe(400);
    expect(up!.seen).toHaveLength(0);
  });
});

describe("metering helpers", () => {
  test("wavSeconds reads the header and refuses other bytes", () => {
    expect(wavSeconds(wav(2.5))).toBeCloseTo(2.5, 6);
    expect(
      wavSeconds(new Uint8Array([0xff, 0xfb, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])),
    ).toBeNull();
  });

  test("SseMeter reassembles split lines", () => {
    const meter = new SseMeter();
    const text = `data: {"choices":[{"delta":{"content":"abc"}}]}\n\ndata: {"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":4}}\n\ndata: [DONE]\n\n`;
    const bytes = new TextEncoder().encode(text);
    for (let i = 0; i < bytes.length; i += 5) meter.push(bytes.slice(i, i + 5));
    expect(meter.outputChars).toBe(3);
    expect(meter.usage).toEqual({ prompt_tokens: 3, completion_tokens: 4 });
  });
});
