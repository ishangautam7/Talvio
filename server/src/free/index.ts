import type { Server } from "bun";
import type { Database } from "bun:sqlite";
import { authenticate } from "../account";
import type { Ctx } from "../app";
import {
  ApiError,
  invalidRequest,
  isObject,
  json,
  notFound,
  rateLimited,
  readJson,
} from "../http";
import type { SocketData } from "../hub";
import { RateLimit } from "../rateLimit";
import { type FreeConfig, isAzureHost } from "./config";
import { FreeLedger, type Reservation, subjectFor } from "./ledger";
import { SseMeter, wavSeconds } from "./metering";

/**
 * Free credit (PROTOCOL.md, "Free credit"): OpenAI-compatible chat,
 * transcription and speech on the operator's upstream keys, metered against a
 * per-account allowance. Each request reserves its worst-case cost, goes
 * upstream, then is charged its real cost; a failure upstream costs nothing.
 */

export const MODEL_IDS = {
  chat: "talvio-free-chat",
  transcribe: "talvio-free-transcribe",
  speech: "talvio-free-speech",
} as const;

export const CREDIT_HEADER = "x-talvio-credit-remaining";
export const CHAT_MAX_BYTES = 4 * 1024 * 1024;
export const AUDIO_MAX_BYTES = 25 * 1024 * 1024;
export const SPEECH_MAX_CHARS = 4096;
/** Below this many affordable output tokens a chat request is refused rather than cut short. */
const MIN_OUTPUT_TOKENS = 256;
/** What one image costs in prompt tokens, at most, for the reservation (a 2048 px tile set). */
const IMAGE_TOKENS = 1600;
/** The slowest audio the apps could send, for the reservation: 16 kbit/s. */
const WORST_BITS_PER_SECOND = 16_000;
/** What the apps send: 48 kbit/s MP3. */
const TYPICAL_BITS_PER_SECOND = 48_000;

const OPENAI_VOICES = new Set([
  "alloy",
  "ash",
  "ballad",
  "coral",
  "echo",
  "fable",
  "nova",
  "onyx",
  "sage",
  "shimmer",
  "verse",
  "marin",
  "cedar",
]);
const ELEVENLABS_VOICE = /^[A-Za-z0-9]{20}$/;

/** Upstream statuses that mean the request itself was malformed, passed on with the upstream's message. */
const CLIENT_FAULTS = new Set([400, 413, 415, 422]);

type Route = (req: Request, server: Server<SocketData>) => Promise<Response>;

export class Free {
  readonly ledger: FreeLedger;
  private readonly limiter: RateLimit;
  /** Strings that must never reach a client or a log: the keys and the upstream hosts. */
  private readonly hidden: string[];
  private readonly routes: Map<string, Route>;

  constructor(
    readonly config: FreeConfig,
    db: Database,
    private readonly ctx: Pick<Ctx, "now" | "log" | "store">,
  ) {
    this.ledger = new FreeLedger(db, config.limitUsd);
    this.limiter = new RateLimit(config.perMinute, 60 * 1000, ctx.now);
    this.hidden = [config.chat, config.stt, config.tts]
      .flatMap((u) => (u ? [u.apiKey, new URL(u.baseUrl).host] : []))
      .filter((s) => s.length >= 4)
      // Longest first, so a host inside another host is not half-replaced.
      .sort((a, b) => b.length - a.length);
    this.routes = new Map<string, Route>([
      ["GET /v1/free/balance", async (req) => this.balance(req)],
      ["GET /v1/free/models", async (req) => this.models(req)],
      ["POST /v1/free/chat/completions", (req, s) => this.chat(req, s)],
      [
        "POST /v1/free/audio/transcriptions",
        (req, s) => this.transcribe(req, s),
      ],
      ["POST /v1/free/audio/speech", (req, s) => this.speech(req, s)],
    ]);
  }

  get enabled(): boolean {
    return !!(this.config.chat || this.config.stt || this.config.tts);
  }

  route(req: Request, url: URL, server: Server<SocketData>): Promise<Response> {
    const handler = this.routes.get(`${req.method} ${url.pathname}`);
    if (!handler) throw notFound();
    return handler(req, server);
  }

  prune(): void {
    this.limiter.prune();
  }

  /** The signed-in account and its ledger subject. */
  private account(req: Request): { userId: string; subject: string } {
    const device = authenticate(req, this.ctx as Ctx);
    // A device row cannot outlive its user (ON DELETE CASCADE).
    const user = this.ctx.store.userById(device.user_id)!;
    return { userId: user.id, subject: subjectFor(user.email) };
  }

  /** The account of a metered request, once the kind is on and the rate limit allows it. */
  private metered(req: Request, configured: boolean) {
    const account = this.account(req);
    if (!configured) throw disabled();
    const wait = this.limiter.take(account.userId);
    if (wait > 0) throw rateLimited(wait);
    return account;
  }

  private remainingHeader(subject: string): Record<string, string> {
    return {
      [CREDIT_HEADER]: this.ledger.balance(subject).remainingUsd.toFixed(6),
    };
  }

  private exhausted(subject: string): ApiError {
    return new ApiError(
      402,
      "credit_exhausted",
      `Your free $${this.config.limitUsd} is used up. Add your own API key in Talvio to keep going.`,
      this.remainingHeader(subject),
    );
  }

  private reserve(
    account: { userId: string; subject: string },
    amountUsd: number,
  ): Reservation {
    const reservation = this.ledger.reserve(
      account.subject,
      account.userId,
      amountUsd,
    );
    if (!reservation) throw this.exhausted(account.subject);
    return reservation;
  }

  private scrub(text: string): string {
    let out = text;
    for (const secret of this.hidden) out = out.split(secret).join("[hidden]");
    return out;
  }

  /** Answers an upstream that did not succeed, without anything that names our key or host. */
  private async upstreamFailure(
    kind: string,
    response: Response,
  ): Promise<Response> {
    this.ctx.log(`free ${kind}: upstream answered ${response.status}`);
    if (!CLIENT_FAULTS.has(response.status)) return upstreamError(502);
    const text = await response.text().catch(() => "");
    let message = "";
    let param: unknown;
    try {
      const parsed = JSON.parse(text) as Record<string, unknown>;
      const error = isObject(parsed.error) ? parsed.error : parsed;
      if (typeof error.message === "string") message = error.message;
      else if (typeof parsed.detail === "string") message = parsed.detail;
      param = error.param;
    } catch {
      // Not JSON: the generic sentence below.
    }
    return upstreamError(
      response.status,
      this.scrub(message).slice(0, 500) ||
        "The AI provider refused the request.",
      typeof param === "string" ? this.scrub(param) : undefined,
    );
  }

  private unreachable(kind: string, error: unknown): Response {
    // The error's name only: a message can carry the URL.
    this.ctx.log(
      `free ${kind}: upstream unreachable (${error instanceof Error ? error.name : "error"})`,
    );
    return upstreamError(502);
  }

  private balance(req: Request): Response {
    const { subject } = this.account(req);
    return json({ ...this.ledger.balance(subject), enabled: this.enabled });
  }

  private models(req: Request): Response {
    this.account(req);
    if (!this.enabled) throw disabled();
    const ids: string[] = [];
    if (this.config.chat) ids.push(MODEL_IDS.chat);
    if (this.config.stt) ids.push(MODEL_IDS.transcribe);
    if (this.config.tts) ids.push(MODEL_IDS.speech);
    return json({
      object: "list",
      data: ids.map((id) => ({
        id,
        object: "model",
        created: 0,
        owned_by: "talvio",
      })),
    });
  }

  private async chat(
    req: Request,
    server: Server<SocketData>,
  ): Promise<Response> {
    const upstream = this.config.chat;
    const account = this.metered(req, !!upstream);
    const body = parseObject(await readCapped(req, CHAT_MAX_BYTES));
    if (!Array.isArray(body.messages) || body.messages.length === 0) {
      throw invalidRequest("messages must be a non-empty array.");
    }
    const { inputPerMTok, outputPerMTok } = upstream!;
    const promptTokens = estimatePromptTokens(body);
    const inputCost = (promptTokens * inputPerMTok) / 1e6;
    const requested =
      positiveInt(body.max_completion_tokens) ?? positiveInt(body.max_tokens);
    const affordable =
      outputPerMTok > 0
        ? Math.floor(
            ((this.ledger.available(account.subject) - inputCost) * 1e6) /
              outputPerMTok,
          )
        : Number.MAX_SAFE_INTEGER;
    const maxTokens = Math.min(
      requested ?? upstream!.maxOutputTokens,
      upstream!.maxOutputTokens,
      affordable,
    );
    if (maxTokens < Math.min(requested ?? MIN_OUTPUT_TOKENS, MIN_OUTPUT_TOKENS))
      throw this.exhausted(account.subject);
    const reservation = this.reserve(
      account,
      inputCost + (maxTokens * outputPerMTok) / 1e6,
    );

    const stream = body.stream === true;
    const outgoing: Record<string, unknown> = {
      ...body,
      model: upstream!.model,
      max_completion_tokens: maxTokens,
    };
    delete outgoing.max_tokens;
    delete outgoing.n;
    if (stream) {
      outgoing.stream_options = {
        ...(isObject(body.stream_options) ? body.stream_options : {}),
        include_usage: true,
      };
    }

    server.timeout(req, 0); // bounded by the upstream timeout below instead
    let response: Response;
    try {
      response = await fetch(`${upstream!.baseUrl}/chat/completions`, {
        method: "POST",
        headers: {
          ...authHeaders(upstream!.baseUrl, upstream!.apiKey),
          "content-type": "application/json",
          accept: stream ? "text/event-stream" : "application/json",
        },
        body: JSON.stringify(outgoing),
        signal: deadline(req, this.config.timeouts?.chat ?? 300_000),
      });
    } catch (error) {
      reservation.release();
      return this.unreachable("chat", error);
    }
    if (!response.ok) {
      reservation.release();
      return this.upstreamFailure("chat", response);
    }

    const settle = (usage: Usage | null, outputChars: number) => {
      const prompt = usage?.prompt_tokens ?? promptTokens;
      const completion = usage?.completion_tokens ?? Math.ceil(outputChars / 4);
      reservation.settle(
        {
          kind: "chat",
          model: upstream!.model,
          units: prompt + completion,
          unit: "tokens",
          costUsd: (prompt * inputPerMTok + completion * outputPerMTok) / 1e6,
        },
        this.ctx.now(),
      );
    };

    if (!stream) {
      let answer: Record<string, unknown>;
      try {
        answer = (await response.json()) as Record<string, unknown>;
      } catch (error) {
        reservation.release();
        return this.unreachable("chat", error);
      }
      settle(readUsage(answer.usage), answerChars(answer));
      return json(answer, 200, this.remainingHeader(account.subject));
    }

    const headers = {
      "content-type": "text/event-stream; charset=utf-8",
      "cache-control": "no-store",
      ...this.remainingHeader(account.subject),
    };
    const reader = response.body!.getReader();
    const meter = new SseMeter();
    let finished = false;
    const finish = () => {
      if (finished) return;
      finished = true;
      settle(meter.usage, meter.outputChars);
    };
    const body$ = new ReadableStream<Uint8Array>({
      async pull(controller) {
        try {
          const { done, value } = await reader.read();
          if (done) {
            finish();
            controller.close();
            return;
          }
          meter.push(value);
          controller.enqueue(value);
        } catch (error) {
          // Cut off mid-answer: what was generated was still billed upstream.
          finish();
          controller.error(error);
        }
      },
      cancel() {
        finish();
        void reader.cancel().catch(() => {});
      },
    });
    return new Response(body$, { status: 200, headers });
  }

  private async transcribe(
    req: Request,
    server: Server<SocketData>,
  ): Promise<Response> {
    const upstream = this.config.stt;
    const account = this.metered(req, !!upstream);
    const bytes = await readCapped(req, AUDIO_MAX_BYTES);
    let form: Awaited<ReturnType<Response["formData"]>>;
    try {
      form = await new Response(bytes, {
        headers: { "content-type": req.headers.get("content-type") ?? "" },
      }).formData();
    } catch {
      throw invalidRequest("The body must be multipart/form-data.");
    }
    const file = form.get("file");
    if (!(file instanceof Blob) || file.size === 0) {
      throw invalidRequest("file is required.");
    }
    const audio = new Uint8Array(await file.arrayBuffer());
    const fromHeader = wavSeconds(audio);
    const worst = Math.max(
      fromHeader ?? (audio.length * 8) / WORST_BITS_PER_SECOND,
      upstream!.minSeconds,
    );
    const price = (seconds: number) => (seconds * upstream!.perHour) / 3600;
    const reservation = this.reserve(account, price(worst));

    const fileName =
      (file instanceof File && file.name) ||
      (fromHeader !== null ? "audio.wav" : "audio.mp3");
    const send = (format: string) => {
      const out = new FormData();
      out.append("file", new Blob([audio], { type: file.type }), fileName);
      out.append("model", upstream!.model);
      out.append("response_format", format);
      for (const field of ["language", "prompt", "temperature"]) {
        const value = form.get(field);
        if (typeof value === "string" && value.length > 0)
          out.append(field, value);
      }
      return fetch(`${upstream!.baseUrl}/audio/transcriptions`, {
        method: "POST",
        headers: authHeaders(upstream!.baseUrl, upstream!.apiKey),
        body: out,
        signal: deadline(req, this.config.timeouts?.stt ?? 120_000),
      });
    };

    server.timeout(req, 0);
    let response: Response;
    try {
      // verbose_json reports the duration; a model without it (gpt-4o-transcribe) gets plain json.
      response = await send("verbose_json");
      if (response.status === 400) {
        await response.body?.cancel();
        response = await send("json");
      }
    } catch (error) {
      reservation.release();
      return this.unreachable("transcribe", error);
    }
    if (!response.ok) {
      reservation.release();
      return this.upstreamFailure("transcribe", response);
    }
    let answer: Record<string, unknown>;
    try {
      answer = (await response.json()) as Record<string, unknown>;
    } catch (error) {
      reservation.release();
      return this.unreachable("transcribe", error);
    }
    const text = typeof answer.text === "string" ? answer.text : "";
    const reported =
      typeof answer.duration === "number" && answer.duration > 0
        ? answer.duration
        : null;
    const seconds =
      reported ?? fromHeader ?? (audio.length * 8) / TYPICAL_BITS_PER_SECOND;
    const billed = Math.max(seconds, upstream!.minSeconds);
    reservation.settle(
      {
        kind: "transcribe",
        model: upstream!.model,
        units: Math.round(billed * 1000) / 1000,
        unit: "seconds",
        costUsd: price(billed),
      },
      this.ctx.now(),
    );
    const headers = this.remainingHeader(account.subject);
    if (form.get("response_format") === "text") {
      return new Response(text, {
        headers: {
          "content-type": "text/plain; charset=utf-8",
          "cache-control": "no-store",
          ...headers,
        },
      });
    }
    return json({ text }, 200, headers);
  }

  private async speech(
    req: Request,
    server: Server<SocketData>,
  ): Promise<Response> {
    const upstream = this.config.tts;
    const account = this.metered(req, !!upstream);
    const body = await readJson(req);
    if (typeof body.input !== "string" || body.input.trim().length === 0) {
      throw invalidRequest("input is required.");
    }
    const chars = [...body.input].length;
    if (chars > SPEECH_MAX_CHARS) {
      throw invalidRequest(
        `input must be at most ${SPEECH_MAX_CHARS} characters.`,
      );
    }
    const u = upstream!;
    const cost = (chars * u.perKChars) / 1000;
    const reservation = this.reserve(account, cost);
    const asked = typeof body.voice === "string" ? body.voice.trim() : "";
    const speed = typeof body.speed === "number" ? body.speed : undefined;

    let url: string;
    let headers: Record<string, string>;
    let payload: Record<string, unknown>;
    if (u.kind === "elevenlabs") {
      const voice = ELEVENLABS_VOICE.test(asked) ? asked : u.voice;
      url = `${u.baseUrl}/v1/text-to-speech/${encodeURIComponent(voice)}?output_format=mp3_44100_128`;
      headers = { "xi-api-key": u.apiKey };
      payload = { text: body.input, model_id: u.model };
      if (speed !== undefined)
        payload.voice_settings = { speed: clamp(speed, 0.7, 1.2) };
    } else {
      const voice = OPENAI_VOICES.has(asked.toLowerCase())
        ? asked.toLowerCase()
        : u.voice;
      url = `${u.baseUrl}/audio/speech`;
      headers = authHeaders(u.baseUrl, u.apiKey);
      payload = {
        model: u.model,
        input: body.input,
        voice,
        response_format: "mp3",
      };
      if (speed !== undefined) payload.speed = clamp(speed, 0.25, 4);
    }

    server.timeout(req, 0);
    let audio: ArrayBuffer;
    try {
      const response = await fetch(url, {
        method: "POST",
        headers: {
          ...headers,
          "content-type": "application/json",
          accept: "audio/mpeg",
        },
        body: JSON.stringify(payload),
        signal: deadline(req, this.config.timeouts?.tts ?? 60_000),
      });
      if (!response.ok) {
        reservation.release();
        return this.upstreamFailure("speech", response);
      }
      audio = await response.arrayBuffer();
    } catch (error) {
      reservation.release();
      return this.unreachable("speech", error);
    }
    reservation.settle(
      {
        kind: "speech",
        model: u.model,
        units: chars,
        unit: "characters",
        costUsd: cost,
      },
      this.ctx.now(),
    );
    return new Response(audio, {
      headers: {
        "content-type": "audio/mpeg",
        "cache-control": "no-store",
        ...this.remainingHeader(account.subject),
      },
    });
  }
}

interface Usage {
  prompt_tokens: number;
  completion_tokens: number;
}

export function readUsage(value: unknown): Usage | null {
  if (!isObject(value)) return null;
  const { prompt_tokens, completion_tokens } = value;
  if (
    typeof prompt_tokens !== "number" ||
    typeof completion_tokens !== "number"
  )
    return null;
  return { prompt_tokens, completion_tokens };
}

/** Characters of a non-streamed answer's text and tool arguments, for the estimate when usage is missing. */
function answerChars(answer: Record<string, unknown>): number {
  let total = 0;
  for (const choice of Array.isArray(answer.choices) ? answer.choices : []) {
    const message = isObject(choice) ? choice.message : undefined;
    if (!isObject(message)) continue;
    if (typeof message.content === "string") total += message.content.length;
    for (const call of Array.isArray(message.tool_calls)
      ? message.tool_calls
      : []) {
      const fn = isObject(call) ? call.function : undefined;
      if (isObject(fn) && typeof fn.arguments === "string")
        total += fn.arguments.length;
    }
  }
  return total;
}

/**
 * A generous count of the prompt's tokens for the reservation: every character
 * of the request but image data at a third of a token (non-Latin scripts
 * tokenize densely), plus a fixed amount per image.
 */
export function estimatePromptTokens(body: Record<string, unknown>): number {
  let images = 0;
  const text = JSON.stringify(body, (_key, value) => {
    if (typeof value === "string" && value.startsWith("data:")) {
      images++;
      return "";
    }
    return value;
  });
  return Math.ceil(text.length / 3) + images * IMAGE_TOKENS;
}

function authHeaders(baseUrl: string, key: string): Record<string, string> {
  const headers: Record<string, string> = { authorization: `Bearer ${key}` };
  // Azure's /openai/v1 takes either; send both, as the desktop app does.
  if (isAzureHost(new URL(baseUrl).hostname)) headers["api-key"] = key;
  return headers;
}

/** Ends the upstream call at its timeout, or as soon as the client goes away. */
function deadline(req: Request, ms: number): AbortSignal {
  return AbortSignal.any([AbortSignal.timeout(ms), req.signal]);
}

function disabled(): ApiError {
  return new ApiError(
    503,
    "free_disabled",
    "Free credit is not available on this server.",
  );
}

function upstreamError(
  status: number,
  message = "The AI provider did not answer. Try again in a moment.",
  param?: string,
): Response {
  return json(
    {
      error: {
        code: "upstream_error",
        message,
        ...(param === undefined ? {} : { param }),
      },
    },
    status,
  );
}

/** The whole body, refusing more than `max` bytes even without a Content-Length. */
export async function readCapped(
  req: Request,
  max: number,
): Promise<Uint8Array> {
  const tooLarge = () =>
    new ApiError(
      413,
      "payload_too_large",
      `The request body is larger than ${Math.round(max / 1024 / 1024)} MiB.`,
    );
  if (Number(req.headers.get("content-length")) > max) throw tooLarge();
  const chunks: Uint8Array[] = [];
  let size = 0;
  if (req.body) {
    for await (const chunk of req.body) {
      size += chunk.byteLength;
      if (size > max) throw tooLarge();
      chunks.push(chunk);
    }
  }
  return Buffer.concat(chunks);
}

function parseObject(bytes: Uint8Array): Record<string, unknown> {
  let value: unknown;
  try {
    value = JSON.parse(Buffer.from(bytes).toString("utf8"));
  } catch {
    throw invalidRequest("The body is not valid JSON.");
  }
  if (!isObject(value)) throw invalidRequest("The body must be a JSON object.");
  return value;
}

function positiveInt(value: unknown): number | undefined {
  return typeof value === "number" && Number.isInteger(value) && value > 0
    ? value
    : undefined;
}

const clamp = (value: number, min: number, max: number) =>
  Math.min(max, Math.max(min, value));
