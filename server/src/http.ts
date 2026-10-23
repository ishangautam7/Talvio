/** A failure that maps to one of PROTOCOL.md's error codes. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    message: string,
    readonly headers: Record<string, string> = {},
    /** Extra top-level fields beside `error`, such as the current record of a `stale` write. */
    readonly extra: Record<string, unknown> = {},
  ) {
    super(message);
  }
}

export const invalidRequest = (message: string) =>
  new ApiError(400, "invalid_request", message);

export const unauthorized = () =>
  new ApiError(
    401,
    "unauthorized",
    "Sign in again: this session is not valid.",
  );

export const notFound = (message = "No such route.") =>
  new ApiError(404, "not_found", message);

export const rateLimited = (retryAfterSeconds: number) =>
  new ApiError(429, "rate_limited", "Too many attempts. Try again later.", {
    "retry-after": String(retryAfterSeconds),
  });

const MAX_BODY_BYTES = 64 * 1024;

const payloadTooLarge = () =>
  new ApiError(
    413,
    "payload_too_large",
    "The request body is larger than 64 KiB.",
  );

export function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function json(
  body: unknown,
  status = 200,
  headers: Record<string, string> = {},
): Response {
  return Response.json(body, {
    status,
    headers: { "cache-control": "no-store", ...headers },
  });
}

export function errorResponse(error: ApiError): Response {
  return json(
    { ...error.extra, error: { code: error.code, message: error.message } },
    error.status,
    error.headers,
  );
}

export const noContent = () => new Response(null, { status: 204 });

export const redirect = (location: string) =>
  new Response(null, {
    status: 302,
    headers: {
      location,
      "cache-control": "no-store",
      "referrer-policy": "no-referrer",
    },
  });

/** A page for a browser in the Google flow; `text` is always one of our own fixed sentences. */
export const htmlPage = (status: number, text: string) =>
  new Response(
    `<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Talvio</title><p>${text}</p>`,
    {
      status,
      headers: {
        "content-type": "text/html; charset=utf-8",
        "content-security-policy": "default-src 'none'",
        "cache-control": "no-store",
        "referrer-policy": "no-referrer",
      },
    },
  );

/** Reads a JSON object body, refusing more than 64 KiB even when no Content-Length was sent. */
export async function readJson(req: Request): Promise<Record<string, unknown>> {
  if (Number(req.headers.get("content-length")) > MAX_BODY_BYTES)
    throw payloadTooLarge();
  const chunks: Uint8Array[] = [];
  let size = 0;
  if (req.body) {
    for await (const chunk of req.body) {
      size += chunk.byteLength;
      if (size > MAX_BODY_BYTES) throw payloadTooLarge();
      chunks.push(chunk);
    }
  }
  let value: unknown;
  try {
    value = JSON.parse(Buffer.concat(chunks).toString("utf8"));
  } catch {
    throw invalidRequest("The body is not valid JSON.");
  }
  if (!isObject(value)) throw invalidRequest("The body must be a JSON object.");
  return value;
}
