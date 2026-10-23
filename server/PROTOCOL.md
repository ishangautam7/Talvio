# Zynveliq account & device protocol (v1)

The contract between the account server (`server/`), the desktop app
(`src-tauri/`), and the phone app (`mobile/`). Change it here first, then in all
three.

What it is for: one account signs in on a computer and a phone, and the phone
can then use the computer as its speech engine. Speak on the phone, the computer
transcribes with its own model and pastes at its cursor (or sends the text back);
type on the phone, the computer pastes it. **The server stores accounts and
devices only. It never stores a recording or a transcript**: relay messages pass
through memory and are dropped.

All JSON is UTF-8, field names are camelCase, times are Unix milliseconds.
Lengths in characters count Unicode code points.

## Objects

```jsonc
// User
{ "id": "uuid", "email": "a@b.com", "name": "Ishan" /* or null */,
  "hasPassword": true, "googleLinked": false, "createdAt": 1729000000000 }

// Device: one per signed-in app install. A session token belongs to a device.
{ "id": "uuid", "name": "ISHAN-PC", "platform": "windows",   // windows|macos|linux|android|ios
  "kind": "desktop",                                          // desktop|mobile
  "appVersion": "2.2.0" /* or null */, "online": true,
  "lastSeenAt": 1729000000000, "createdAt": 1729000000000,
  "current": false,    // true only for the device making the request
  "capabilities": ["dictate", "text"] }  // what it handles; [] while offline (see `hello`)

// DeviceInfo: sent by a client when it signs in
{ "name": "ISHAN-PC", "platform": "windows", "kind": "desktop", "appVersion": "2.2.0" }

// AuthResponse
{ "token": "zv1_<base64url of 32 random bytes>", "user": User, "device": Device }
```

Validation: `name` 1–64 chars after trim (clients send the host name, or
"Android phone" / "iPhone"); `platform` and `kind` must be one of the values
above; `appVersion` ≤ 32 chars or absent.

## Errors

Any non-2xx JSON response is `{ "error": { "code": "...", "message": "English sentence" } }`.
Clients show their own translated text per `code` and fall back to a generic
message for a code they don't know.

| HTTP | code                  | when                                                    |
| ---- | --------------------- | ------------------------------------------------------- |
| 400  | `invalid_request`     | malformed JSON, missing/invalid field                   |
| 400  | `invalid_email`       | not an email address                                    |
| 400  | `weak_password`       | shorter than 8 characters (max 256)                     |
| 400  | `invalid_code`        | Google login code unknown, used, expired, or PKCE fails |
| 401  | `invalid_credentials` | wrong email or password (also: account has no password) |
| 401  | `unauthorized`        | missing, unknown, or revoked bearer token               |
| 404  | `not_found`           | device id not on this account, unknown route            |
| 409  | `email_taken`         | sign-up with an email that already has an account       |
| 413  | `payload_too_large`   | HTTP body over 64 KiB                                   |
| 429  | `rate_limited`        | too many attempts; `Retry-After` header in seconds      |
| 503  | `google_unavailable`  | server has no Google credentials configured             |
| 500  | `internal`            | anything else                                           |

## Finding the server

The server's host name can change, so neither app hard-codes it. Both read it
from a discovery address, defined once per app:

```
GET https://domain.gautamishan.com.np
→ 200, JSON: { "domain": "sync.example.com" }
```

- The value is a host name with an optional port (`sync.example.com`,
  `sync.example.com:8443`) or an `https://` URL of one (path, query and fragment
  are ignored). Clients read `domain`; if that key is missing they take the
  object's only string field. Anything else (not JSON, no usable field, a
  user/password part, a non-https scheme, a loopback host in a release build)
  is ignored as if the request had failed. Development builds also accept an
  explicit `http://` URL (e.g. `http://192.168.1.5:8787` for a server on the
  LAN); release builds never do.
- The server URL is then `https://<host[:port]>`. Discovery is fetched over
  https only, with a 10 s timeout, no redirects to other hosts, and never
  carries a token or any user data.
- When: at launch, and again when the current server cannot be reached
  (connect failure or timeout, not an HTTP error answer), at most once a minute.
  The last good value is saved and used until a new one arrives, so the apps
  keep working while the discovery address is down or answers nothing. With no
  saved value and no answer, the app falls back to its built-in default
  (`http://localhost:8787` in development builds; release builds show that the
  server cannot be found and keep retrying).
- **Automatic** (discovery) is the default. The user can type a server in the
  Server setting instead (**Custom**), which turns discovery off until they
  switch back to Automatic. A development build's `--dart-define` /
  environment override also pins the server.
- Sessions: in Automatic mode a session follows the discovered server, so when
  the domain changes a signed-in app reconnects to the new host with the same
  token (the server, its database and the tokens are the same; only the name
  moved). In Custom mode a session stays bound to the URL it signed in on.
  Discovery is therefore a trust anchor: whoever controls the discovery
  address decides where sign-ins and tokens go.
- Server side: `PUBLIC_URL` must be the current domain, and Google sign-in
  needs `{PUBLIC_URL}/v1/auth/google/callback` registered in the Google Cloud
  console for every domain the server is reachable at.

## HTTP API (base: the server URL, e.g. `https://sync.example.com`)

Authenticated routes take `Authorization: Bearer <token>`.

| Method & path                  | Auth | Body → Response                                                     |
| ------------------------------ | ---- | ------------------------------------------------------------------- |
| `GET /v1/health`               | –    | → `{ "ok": true }`                                                  |
| `GET /v1/config`               | –    | → `{ "protocol": 1, "google": true }` (google = credentials set)    |
| `POST /v1/auth/signup`         | –    | `{ email, password, name?, device: DeviceInfo }` → 201 AuthResponse |
| `POST /v1/auth/login`          | –    | `{ email, password, device: DeviceInfo }` → AuthResponse            |
| `GET /v1/auth/google/start`    | –    | query, see below → 302 to Google                                    |
| `GET /v1/auth/google/callback` | –    | Google's redirect → 302 to the client's `redirect_uri`              |
| `POST /v1/auth/exchange`       | –    | `{ code, codeVerifier, device: DeviceInfo }` → AuthResponse         |
| `POST /v1/auth/logout`         | ✓    | → 204; deletes this device and its token                            |
| `GET /v1/me`                   | ✓    | → `{ user: User, device: Device }`                                  |
| `DELETE /v1/me`                | ✓    | → 204; deletes the account, every device, closes every socket       |
| `GET /v1/devices`              | ✓    | → `{ devices: Device[] }` (current device first, then by name)      |
| `DELETE /v1/devices/{id}`      | ✓    | → 204; signs that device out and closes its socket with 4401        |
| `GET /v1/ws`                   | ✓    | WebSocket upgrade, see below                                        |

Emails are trimmed and lower-cased. A token that has not been used for 180 days
is deleted and answers `unauthorized`. Rate limits (per server instance): 20
auth requests per minute per IP (an IPv6 address counts by its /64; Google's
callback is not counted), and 10 failed logins per 15 minutes per email (a login
still being checked counts as failed until it succeeds).

### Google sign-in (both apps, same flow)

The Google client secret lives on the server only. The app opens the system
browser (desktop) or an auth session (phone: Custom Tabs / ASWebAuthenticationSession)
and gets back a one-time code that only it can redeem, because it holds the
PKCE verifier.

1. App makes `verifier` = 32 random bytes, base64url without padding, and
   `challenge` = base64url(SHA-256(verifier)) without padding.
2. App opens
   `{server}/v1/auth/google/start?redirect_uri=<enc>&code_challenge=<challenge>&code_challenge_method=S256`.
   `redirect_uri` must be exactly one of:
   - desktop: `http://127.0.0.1:<port>/callback` (a loopback listener the app
     started; port 1024–65535)
   - phone: `zynveliq://auth/callback`

   Anything else answers 400 with an HTML page and no redirect. If Google is not
   configured the server redirects to `redirect_uri?error=google_unavailable`.

3. Server stores `{state, redirect_uri, challenge}` for 10 minutes (single use)
   and redirects to Google (`scope=openid email profile`,
   `prompt=select_account`, its own callback
   `{PUBLIC_URL}/v1/auth/google/callback`).
4. Callback: server exchanges Google's code at Google's token endpoint over TLS
   and reads the `id_token` claims from that response (no client ever sends an
   id_token, so no JWKS check is needed; `aud`, `iss`, `exp` are still checked).
   It requires `email_verified`, then finds or creates the user:
   - by `google_sub` first;
   - else by email. If that account is already linked to another Google
     account (a different `google_sub`), sign-in is refused with
     `account_conflict`: addresses get handed to new people, and the Google id
     is who someone is. If that account's email was never verified (a
     password sign-up), Google's proof wins: the password is removed, every
     session of that account is revoked, and the account is linked. This stops
     someone from pre-registering another person's email with a password and
     keeping access after the real owner signs in with Google.
   - else a new user (name from Google).

   Then it creates a login code (32 random bytes, stored hashed, valid 2
   minutes, single use, bound to the user and the challenge) and redirects to
   `redirect_uri?code=<code>`. On failure: `redirect_uri?error=<code>` with
   `access_denied` (the user cancelled at Google), `account_conflict` (see
   above) or `google_failed`.

5. App calls `POST /v1/auth/exchange` with the code and its verifier.

Desktop's loopback listener answers the browser with a small HTML page ("You're
signed in. You can close this tab.") and stops listening. It gives up after 5
minutes or when the user cancels.

## WebSocket `GET /v1/ws`

Authenticated with the `Authorization: Bearer` header on the upgrade request
(native clients only; never a token in the URL). Bad token: HTTP 401, no
upgrade. Text frames only, each one JSON object with a `type`. Max frame 16 MiB.

Server → client

```jsonc
{ "type": "ready", "deviceId": "uuid", "user": User }      // first message
{ "type": "devices", "devices": [Device] }                  // right after ready, and on every change:
                                                            // a device of this user connects, disconnects,
                                                            // signs in, or is removed. `current` is per recipient.
{ "type": "relay", "from": "deviceId", "body": { ... } }    // a message from another device of this user
{ "type": "relay_failed", "id": "<echoed>", "to": "deviceId",
  "reason": "offline" }                                     // target not connected / not on this account
{ "type": "pong" }
```

Client → server

```jsonc
{ "type": "hello", "capabilities": ["dictate", "text"] }  // first message after `ready`; may be re-sent
                                                          // whenever capabilities change
{ "type": "relay", "id": "optional client id", "to": "deviceId", "body": { ... } }
{ "type": "ping" }    // every 25 s; the server drops a socket it has received no byte
                      // from for 75 s (a frame still arriving counts), and closes (1001)
                      // one that has read nothing the server sent it for 75 s
```

`capabilities` is a list of at most 64 strings of at most 64 chars
(`[a-z0-9._-]`), kept in memory for that socket only and published in every
`Device` of the `devices` message; the server never interprets it. A device
that has not said `hello` has `[]`.

The server only routes `relay` between devices of the same user and never
inspects or stores `body`. One socket per device: a second connection with the
same token closes the older one (close code 4000).

Close codes: **4401** = token revoked or account deleted (clear the session, do
not reconnect); 4000 = replaced by a newer connection with the same token (do
not reconnect: that would close the newer one in turn); anything else =
reconnect with backoff (1 s doubling to 30 s, with jitter).

## Phone ↔ computer messages (`body` of a relay)

Every body has a `kind`. A request carries a `requestId` (uuid) chosen by the
sender, and **any number** of replies may carry the same `requestId`: zero or
more `progress` messages, then exactly one `result`, which ends the request.
This is what lets a long-running request (an agent task) stream updates.

The set of kinds is open. Rules every implementation follows so new kinds can
be added without touching the others:

- A device lists the kinds it handles in `hello.capabilities`. Today a
  computer sends `["dictate", "text"]`; the phone sends `[]`. The phone shows a
  feature only for a computer whose capabilities include it.
- A request with a kind the receiver does not handle is answered with
  `{ "kind": "result", "requestId": …, "ok": false, "error": "unsupported" }`.
  Messages without a `requestId` that it does not understand are ignored.
- A request goes out with its `requestId` as the relay `id`, so the
  `relay_failed` the server echoes (target offline) can be matched to it. Its
  `cancel` goes out under the same relay `id`, so a cancel that cannot be
  delivered ends the request as offline too.
- `{ "kind": "cancel", "requestId": "uuid" }` asks the receiver to stop that
  request. It is best effort and is answered by the request's own `result`
  (`error: "cancelled"`) if it stopped; a finished or unknown request ignores it.
- **Reserved:** every kind starting with `agent.` (and the capability names of
  the same form) belongs to the phone-to-computer agent feature (prompts and
  commands the computer acts on), which is being built separately. Nothing in
  v1 sends or handles them, and nothing else may use that prefix. It uses the
  same `progress` / `result` / `cancel` rules; it may add fields to `progress`
  (for example `"stage": "agent.step"` with its own payload).

### Approval: the computer decides which phones may use it

Signing into the account is not enough to type into a computer: each computer
keeps its own list of approved device ids, and every request except `cancel`
(today `dictate` and `text`, and every `agent.*` kind) passes this gate first,
in the computer's shared dispatch, so a new kind inherits it.

1. "Phone dictation" off → `remote_input_off`, no prompt.
2. Sender approved → the request runs.
3. Sender refused earlier in this run of the app → `not_approved`, no prompt.
4. Otherwise the computer sends `{ "kind": "progress", "requestId", "stage": "approval" }`
   and asks its user: allow this phone (named, with its platform) to dictate
   and type on this computer? Several requests from the same phone share one
   prompt. Allow → the id is saved as approved and the waiting requests run.
   Don't allow, or no answer within 2 minutes → `not_approved` for every
   waiting request; Don't allow also refuses that phone without asking again
   until the app restarts or the user allows it in Settings → Account.

Approval is per device id, and a device id is per sign-in, so a phone that signs
out and back in asks again. Approvals are cleared when the computer signs out.
The user can allow or revoke any phone ahead of time in Settings → Account; a
revoke ends that phone's waiting requests with `not_approved`. A request from a
device that is not a `mobile` on this account is refused the same way.

The phone's timeout restarts on every `progress`, and while the last progress
is `approval` it waits up to 3 minutes. The phone never picks a computer it has
not been told to use: it remembers the user's choice and only selects a
computer on its own when the account has exactly one computer, so a device
that registers as a computer later is never chosen silently.

Phone → computer

```jsonc
{ "kind": "dictate", "requestId": "uuid", "deliver": "paste", // "paste" | "return"
  "audio": "<base64 WAV>" }   // RIFF, PCM 16-bit, mono, 16 000 Hz, at most 5 minutes
{ "kind": "text", "requestId": "uuid", "text": "…" }          // 1–10 000 chars, always pasted
```

Computer → phone

```jsonc
{ "kind": "progress", "requestId": "uuid", "stage": "transcribing" }  // or "approval", "pasting"
{ "kind": "result", "requestId": "uuid", "ok": true,
  "text": "…" /* dictate only: the transcript */, "pasted": true }
{ "kind": "result", "requestId": "uuid", "ok": false, "error": "<code>" }
```

`deliver: "paste"` pastes at the computer's cursor like a dictation, with two
differences: it is never followed by the computer's Auto-submit Enter, and with
its Direct (typing) paste method each run of control characters (line
breaks, tabs) is typed as one space, so phone text can never press a key. `"return"` only sends the text
back. When the computer's paste method is None, a `dictate` with `"paste"`
answers `ok` with the transcript and `"pasted": false`, and a `text` answers
`paste_off`. The phone treats a `relay_failed` for its
request as "computer offline", and no result within 60 s plus the recording's
length as a timeout (other kinds set their own).

Result error codes: `remote_input_off` (the computer's "Phone dictation" switch
is off, or was turned off or its session ended while the request transcribed;
nothing is pasted then), `busy` (it is already handling a phone request), `no_speech_model`,
`bad_audio` (not 16 kHz mono 16-bit WAV, empty, or too long), `no_speech`
(nothing was heard), `transcription_failed`, `paste_failed`, `paste_off` (the
computer's paste method is None, so typed text was not pasted), `unsupported`
(unknown `kind`, or a field value this version does not handle, such as a new
`deliver`), `cancelled`, `not_approved` (the computer's user did not allow
this phone, see Approval). A device shows a generic message for a code it does not
know, so a new kind may add codes of its own.
