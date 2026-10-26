# Talvio account & device protocol (v1)

The contract between the account server (`server/`), the desktop app
(`src-tauri/`), and the phone app (`mobile/`). Change it here first, then in all
three.

What it is for: one account signs in on a computer and a phone, and the phone
can then use the computer as its speech engine. Speak on the phone, the computer
transcribes with its own model and pastes at its cursor (or sends the text back);
type on the phone, the computer pastes it. **The server stores accounts and
devices, plus the provider API keys a user syncs (encrypted, see Secrets). It
never stores a recording or a transcript**: relay messages pass
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
| 400  | `invalid_name`        | secret name does not match the pattern (see Secrets)    |
| 400  | `invalid_value`       | secret value not a string of 1–8192 chars               |
| 400  | `invalid_time`        | secret `updatedAt` not integer ms, or > 5 min ahead     |
| 400  | `too_many`            | account already holds 100 secret records                |
| 401  | `invalid_credentials` | wrong email or password (also: account has no password) |
| 401  | `unauthorized`        | missing, unknown, or revoked bearer token               |
| 404  | `not_found`           | device id not on this account, unknown route            |
| 409  | `email_taken`         | sign-up with an email that already has an account       |
| 409  | `stale`               | secret write older than the stored record (see Secrets) |
| 413  | `payload_too_large`   | HTTP body over 64 KiB                                   |
| 429  | `rate_limited`        | too many attempts; `Retry-After` header in seconds      |
| 503  | `google_unavailable`  | server has no Google credentials configured             |
| 500  | `internal`            | anything else                                           |

## Finding the server

The server's address can change, so neither app hard-codes it. Both read it
from a discovery address, defined once per app:

```
GET https://domain.gautamishan.com.np
→ 200, JSON: { "apiBaseUrl": "http://192.168.101.69:8787" }
```

- The value is an `http://` or `https://` origin (`http://192.168.101.69:8787`,
  `https://sync.example.com`); a path, query and fragment are ignored. A bare
  host name with an optional port (`sync.example.com:8443`) is also accepted
  and means `https://`. Clients read `apiBaseUrl`; if that key is missing they
  read the legacy `domain`, and if that is missing too, the object's only string
  field. A key that is present but unusable is not replaced by a later one.
  Anything else (not JSON, no usable field, a user/password part, a scheme
  other than http/https, a loopback host in a release build) is ignored as if
  the request had failed.
- The server URL is then that origin, `scheme://host[:port]`. Plain http is
  accepted in every build, because the address arrives in the https discovery
  answer: discovery itself is fetched over https only, with a 10 s timeout, no
  redirects to other hosts, and never carries a token or any user data.
- When: at launch, and again when the current server cannot be reached
  (connect failure or timeout, not an HTTP error answer), at most once a minute.
  The last good value is saved and used until a new one arrives, so the apps
  keep working while the discovery address is down or answers nothing. With no
  saved value and no answer, the app falls back to its built-in default
  (`http://localhost:8787` in development builds; release builds show that the
  server cannot be found and keep retrying).
- Discovery is the only source of the server address: the desktop app has no
  setting to choose or type a server (its Account tab only shows which server
  is in use), and a server typed into an earlier version is ignored. A
  development build's `--dart-define` / environment override can still pin the
  phone app's server.
- Sessions: a session follows the discovered server, so when the address
  changes a signed-in app reconnects to the new one with the same token (the
  server, its database and the tokens are the same; only the address moved).
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
| `GET /v1/secrets`              | ✓    | → `{ secrets: Secret[] }`, see Secrets                              |
| `PUT /v1/secrets/{name}`       | ✓    | `{ value, updatedAt }` → `{ secret: Secret }`                       |
| `DELETE /v1/secrets/{name}`    | ✓    | query `updatedAt` → `{ secret: Secret }` (a tombstone)              |
| `GET /v1/ws`                   | ✓    | WebSocket upgrade, see below                                        |

Emails are trimmed and lower-cased. A token that has not been used for 180 days
is deleted and answers `unauthorized`. Rate limits (per server instance): 20
auth requests per minute per IP (an IPv6 address counts by its /64; Google's
callback is not counted), and 10 failed logins per 15 minutes per email (a login
still being checked counts as failed until it succeeds).

### Secrets: provider API keys that follow the account

A user's provider API keys (OpenAI, Azure, ElevenLabs, web search, …) are kept
per account so every computer they sign in on gets them. The account is always
the bearer token's account; one account can never read or write another's.

```jsonc
// Secret
{ "name": "post_process:openai", "value": "sk-…",
  "updatedAt": 1761190000000,   // ms, the writing CLIENT's clock at write time
  "deleted": false }            // a tombstone: deleted true, value ""
```

- `GET /v1/secrets` → 200 `{ "secrets": [Secret] }`, sorted by name, tombstones
  included so deletions reach the other devices.
- `PUT /v1/secrets/{name}` with `{ "value": "<string>", "updatedAt": <ms> }` →
  200 `{ "secret": Secret }`.
- `DELETE /v1/secrets/{name}?updatedAt=<ms>` → 200 `{ "secret": Secret }` with
  `deleted: true` and `value: ""`. Deleting a name that was never stored still
  creates a tombstone, so a device that comes back online later with an older
  copy loses.
- **Last writer wins by `updatedAt`.** When the stored record (live or tombstone)
  has a larger `updatedAt` than the request, nothing is written and the answer is
  409 `{ "error": { "code": "stale", "message": … }, "secret": Secret }` with the
  current record. An equal `updatedAt` overwrites, so a retried write is
  harmless.
- Validation, checked after authentication: `name` matches
  `^(post_process|cloud_stt|assistant_tts|web_search):[a-z0-9_.-]{1,64}$`
  (it may be percent-encoded in the path, e.g. `%3A` for the colon) else
  `invalid_name`; `value` is a string of 1–8192 characters else `invalid_value`
  (DELETE has none); `updatedAt` is a non-negative integer of milliseconds no
  more than 5 minutes ahead of the server's clock else `invalid_time` (a missing
  or non-numeric query parameter on DELETE too). At most 100 records (live and
  tombstones) per account: a write that would add a 101st name answers
  `too_many`; rewriting an existing name is always allowed. Bodies over 64 KiB
  answer `payload_too_large`.
- Deleting the account deletes its secrets.
- At rest each value is AES-256-GCM encrypted with a per-write random nonce and
  `` `${accountId}\n${name}` `` (the account id, a line feed, the name) as
  associated data. Values are never logged.
- **Transport:** on a LAN deployment the server URL is plain `http://`, so API
  keys cross the network unencrypted, exactly like passwords and tokens. Until
  https is in front of the server, the network it runs on must be trusted.

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
   - phone: `talvio://auth/callback`, or `zynveliq://auth/callback` from a
     phone app built before the rename from Zynveliq

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
While a step runs long (a transcription, a command, an answer), the computer
repeats its latest `progress` unchanged about every 5 seconds, so a request
that is still working never reads as lost; a phone treats a repeat as nothing
new, apart from restarting its timeout.

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
(today `dictate`, `text`, `windows.*`, and every `agent.*` kind) passes this gate first,
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

### Windows: choosing where phone input goes (capability `windows`)

A computer whose `capabilities` include `windows` lets an approved phone list
its open windows, aim `text` and `dictate` at one of them, and press keys in it
(Enter after a dictation, Tab, Esc, arrows, Ctrl+C). It is gated by the
approval rules above **and** by a second switch on the computer, "Let phones
see and use windows", off by default; with it off every request below answers
`remote_windows_off` (and `text` / `dictate` with a `target` do too).

```jsonc
// phone → computer
{ "kind": "windows.list", "requestId": "uuid" }
{ "kind": "windows.keys", "requestId": "uuid",
  "target": "w1",            // optional: a window id from windows.list; absent = the window in front
  "keys": ["enter"],          // 1–20 key combos, pressed in order
  "stay": false }             // optional, see below
{ "kind": "text", "requestId": "uuid", "text": "…", "target": "w1", "stay": false }
{ "kind": "dictate", "requestId": "uuid", "deliver": "paste", "audio": "…", "target": "w1", "stay": false }

// computer → phone
{ "kind": "result", "requestId": "uuid", "ok": true,
  "windows": [ { "id": "w1", "title": "Claude Code — zynveliq", "app": "Windows Terminal", "front": true } ] }
{ "kind": "result", "requestId": "uuid", "ok": true, "pressed": 1 }   // windows.keys
```

- **Window ids** are opaque strings of at most 64 characters, made up by the
  computer and good only for the phone they were listed to. A window keeps its
  id across that phone's lists for as long as it stays open, so listing again
  does not lose a choice; an id stops working when its window closes or 30
  minutes after the phone's latest `windows.list`. A phone can only name a
  window the computer offered it. At most 60 windows are listed, the window in
  front first and marked `"front": true`; the computer's own windows are never
  listed.
- **A combo** is modifiers and one key joined by `+`, the syntax command mode's
  `press_keys` reads: `enter`, `shift+enter`, `tab`, `escape`, `up`, `down`,
  `left`, `right`, `backspace`, `delete`, `home`, `end`, `pageup`, `pagedown`,
  `space`, `f1`–`f12`, a letter or a digit, with `ctrl`, `alt`, `shift`, and
  `win` / `cmd`. Combos that close or quit an app or window, lock or log out,
  or delete past the bin are refused (`key_refused`) and nothing is pressed.
  Every combo is checked before the first is pressed; a `keys_failed` partway
  through may have pressed the ones before it.
  Enter is not held for confirmation the way command mode holds it: a person
  pressed it on purpose.
- **Aiming** brings the target forward, delivers the text or keys there, and
  then gives the foreground back to the window that had it before, so someone
  working at the computer is interrupted for a moment, not moved. `"stay": true`
  leaves the target in front instead, and means nothing without a `target`.
  When the target is already in front nothing moves.
- A phone sends `target` only to a computer that lists `windows`: an older one
  ignores the field and pastes at its cursor.
- The phone's timeout for `windows.list` and `windows.keys` is 20 s, restarted
  by every `progress` and 3 minutes while the last progress is `approval`.

Error codes these add: `remote_windows_off` (the switch is off), `target_gone`
(the id is unknown, expired, or its window has closed), `focus_failed` (the
window would not come forward, for example because it is not responding),
`key_refused` (a combo on the refused list), `keys_failed` (the keys could not
be pressed), and `unsupported` for a combo it cannot read.

### Live view: watching and using a window or screen (capability `screen`)

A computer whose `capabilities` include `screen` streams one window or one whole
screen to an approved phone, live, and takes keys, text, taps and scrolls back.
The same two switches gate it as `windows` ("Phone dictation" and "Let phones
see and use windows"), with the same errors. Video, sound and input do **not** go through the account server: the
relay only carries the WebRTC handshake below, and the media then flows directly
between phone and computer (or through a TURN server when they cannot reach each
other), encrypted end to end by WebRTC (DTLS-SRTP).

`windows.list` also lists the screens when the computer has `screen`:

```jsonc
{ "kind": "result", "requestId": "uuid", "ok": true,
  "windows": [ … ],
  "screens": [ { "id": "s3", "name": "Display 1", "width": 2560, "height": 1440, "primary": true } ] }
```

Screen ids follow the window id rules (made up per list, per phone, 30 minutes).

**Starting** (phone → computer). One watch at a time per computer: a watch
from another phone answers `busy`, and a watch from the phone whose view is
running replaces it (that view ends with `replaced`), since a phone that asks
again has lost its view (the app was closed or crashed). A view that is not
live yet also ends (`connect_failed`) as soon as its phone is offline.

```jsonc
{ "kind": "screen.watch", "requestId": "uuid",
  "target": "w1",          // a window or screen id from windows.list
  "control": false,         // true: taps and scrolls are allowed too
  "audio": false }          // start with the computer's sound on (it can be switched later)
```

**Handshake.** The computer offers; the phone answers. Both sides trickle ICE
candidates. Messages from the phone carry the watch's own `requestId` and are
not requests: a computer delivers an `rtc.*` body to the running watch with
that `requestId` from the same device, and ignores it otherwise (no reply).

```jsonc
// computer → phone, as progress of the watch
{ "kind": "progress", "requestId": "uuid", "stage": "rtc.offer", "sdp": "…",
  "iceServers": [ { "urls": ["stun:…", "turn:…"], "username": "…", "credential": "…" } ] }
{ "kind": "progress", "requestId": "uuid", "stage": "rtc.ice",
  "candidate": { "candidate": "candidate:…", "sdpMid": "0", "sdpMLineIndex": 0 } }
{ "kind": "progress", "requestId": "uuid", "stage": "live" }   // media is flowing

// phone → computer
{ "kind": "rtc.answer", "requestId": "uuid", "sdp": "…" }
{ "kind": "rtc.ice", "requestId": "uuid",
  "candidate": { "candidate": "candidate:…", "sdpMid": "0", "sdpMLineIndex": 0 } }
```

The phone uses the `iceServers` from the offer. The computer gets them from the
account server (`GET /v1/turn`, below). There is exactly one offer per watch
and no renegotiation: switching between View and Control is a new watch (the
computer frees its one-watch slot before it sends the old watch's result, so
the new one is never `busy`). The phone waits 20 s for the offer (3 minutes
while the last progress is `approval`); the session must be `live` within 30
seconds of the offer or it ends with `connect_failed`. Once live, the view
lives as long as the WebRTC connection: losing the relay socket does not end
it, and the computer ends it when the connection fails or closes.

**Media.** One video track: H.264 Constrained Baseline, `packetization-mode=1`,
at most 1920 pixels on the long side and 30 frames a second, sent only when the
picture changes (plus one frame every two seconds so the phone can tell the
link is alive). One audio track: Opus, 48 kHz stereo, carrying what the computer
is playing, silent until the phone turns sound on. The computer adapts the
bitrate to the link.

**Input: the data channel `control`**, opened by the computer, ordered and
reliable. Every message is one JSON text message.

```jsonc
// phone → computer; "id" is the phone's own, echoed in the ack
{ "t": "keys",   "id": "k1", "keys": ["enter"] }        // as windows.keys
{ "t": "text",   "id": "k2", "text": "…" }               // as text, 1–10 000 chars
{ "t": "tap",    "id": "k3", "x": 0.42, "y": 0.81,       // 0..1 across the picture
  "button": "left", "count": 1, "confirmed": false }     // "left" | "right"; count 1 or 2
{ "t": "scroll", "id": "k4", "x": 0.5, "y": 0.5, "dy": 3, "dx": 0 }   // wheel notches; +dy scrolls down, +dx right
{ "t": "audio",  "on": true }                             // the computer's sound on or off
{ "t": "keyframe" }                                       // the phone lost the picture

// computer → phone
{ "t": "ack", "id": "k3", "ok": false, "error": "confirm", "label": "Send" }
{ "t": "caret", "x": 0.1, "y": 0.88, "w": 0.002, "h": 0.03 } // the text cursor (a thin rectangle), or {"t":"caret"} for none
{ "t": "target", "title": "…", "width": 1280, "height": 720, "state": "normal" } // "minimized" | "covered"
```

- Input is aimed at the watched window, as `windows.keys` aims (`stay` is
  implied: the window being watched stays in front while input arrives). On a
  watched screen it goes wherever the keyboard is.
- `tap` and `scroll` need `"control": true` in the watch, else `not_allowed`.
- A tap on a control whose label sends, buys, posts or deletes is not clicked:
  its ack is `error: "confirm"` with the control's `label`, and the phone asks
  its user before sending the same tap again with `"confirmed": true`.
- Text and taps are never delivered into a password field (`password_field`).
- Every input message (`keys`, `text`, `tap`, `scroll`) gets exactly one ack:
  `{ "t": "ack", "id", "ok": true }` and nothing else, or `ok: false` with an
  `error`. Ack error codes: those of `windows.keys` and `text`, plus
  `not_allowed`, `confirm`, `password_field` and `tap_failed`.
- Screen ids are only for `screen.watch`. `text`, `dictate` and `windows.keys`
  take window ids; input for a watched screen is sent without a `target`, or
  over the data channel.

**Ending.** The phone sends `cancel` for the watch; the computer's user can
press Stop on the "Phone is viewing" notice; switching either switch off,
signing out, revoking the phone, or the target closing ends it too. After 10
minutes with no input from the phone and no change in the picture the computer
ends it. Every ending is the watch's one `result`: `ok: true` when the phone
stopped it, otherwise an error: `stopped` (the computer's user pressed Stop),
`target_gone`, `idle`, `connect_failed`, `capture_failed`, `replaced`, `cancelled`,
`remote_input_off`, `remote_windows_off`, `not_approved`.

### Commands: telling the computer what to do (capability `command`)

A computer whose `capabilities` include `command` runs command mode for an
approved phone: "open YouTube", "play Believer by Imagine Dragons", "turn the
volume down", "email Ron about the meeting at 9". The computer's own router model
picks the tools and they run on the computer, visibly, in its own apps; the phone
only says what it wants and shows what happened. It is gated by the approval
rules above, "Phone dictation", and the computer's command mode switch
("Voice commands", off by default; off answers `command_mode_off`). One command
runs at a time per computer (`busy`); a new command replaces whatever an earlier
one left waiting for a confirmation or an answer.

A command is carried out whole, its sending step included: "email Ron that I'm
late" sends the email and answers `done` with "Sent to Ron." It answers
`awaiting_confirmation` only when the user's own words asked to check first
("let me read it before you send it", "just draft it", "napathau"), when the
computer's "Ask before sending or deleting" switch is on, or when the screen
agent met a payment, a permanent deletion or a security change, or a risky
control the request never mentioned. A send is reported `done` only once the
app showed it went (Gmail closed the draft); otherwise the command `failed`
with a sentence saying it may not have gone.

```jsonc
// phone → computer
{ "kind": "command", "requestId": "uuid",
  "text": "open youtube" }            // 1–2 000 chars, or instead:
{ "kind": "command", "requestId": "uuid",
  "audio": "<base64 WAV>" }           // as `dictate`'s audio; transcribed by the computer
{ "kind": "command.confirm", "requestId": "uuid", "id": 7, "accept": true }
{ "kind": "command.answer", "requestId": "uuid", "id": 7, "answer": "the work one" }

// computer → phone
{ "kind": "progress", "requestId": "uuid", "stage": "transcribing" }   // audio only
{ "kind": "progress", "requestId": "uuid", "stage": "working" }
{ "kind": "result", "requestId": "uuid", "ok": true,
  "transcript": "open youtube",
  "status": "done",                    // done | awaiting_confirmation | needs_answer | cancelled | failed | needs_computer_use
  "feedback": "Opened YouTube.",       // one or two short sentences, in English
  "id": 7,                             // with awaiting_confirmation / needs_answer: what to confirm or answer
  "pending": "Send the email to Ron",  // with awaiting_confirmation: what accepting does
  "question": "Which Ron?", "choices": ["Ron Weasley", "Ron Swanson"] }  // with needs_answer
```

- `status: "failed"` is still `ok: true`: the command ran and could not be
  carried out, and `feedback` says why ("I couldn't find an app called Spotfy.").
  `ok: false` is for the request itself: `command_mode_off`, `busy`,
  `not_approved`, `remote_input_off`, `bad_audio`, `no_speech`, `no_speech_model`,
  `transcription_failed`, `unsupported`, `cancelled`, and `not_configured`
  (command mode is on but has no model it can use).
- `command.confirm` with `accept: false` cancels what is waiting. An `id` that
  is no longer waiting answers `ok: true` with `status: "cancelled"`.
- Nothing the computer does in answer to a command is held for the phone beyond
  this: confirmation is the phone's Send button, never a second voice prompt.
- Timeouts: 45 s for every command request (`command`, `command.confirm`,
  `command.answer`), restarted by every `progress` and 3 minutes while the last
  progress is `approval`, plus the recording's length for audio. The model plans
  and the tools run after `working`, so that stage gets the long wait.
- A `command.confirm` or `command.answer` result repeats the original command's
  `transcript`.
- An audio command can also fail with `no_speech_model` (the computer has no
  speech model ready), as `dictate` can.

### Assistant: asking the computer's assistant (capability `assistant`)

A computer whose `capabilities` include `assistant` answers an approved phone
with its own assistant: the same provider, model, profile, memory, web search
and tools as the quick ask on the computer. When the computer's "Voice commands"
are on, the assistant can also **do** things there ("play Believer on YouTube",
"open Gmail and start an email to Ron"), through the same tools commands use;
a send the user asked for is carried out, and only one they asked to check
first (or one the computer's "Ask before sending or deleting" holds) waits for
a Do it on the computer's card or on the phone. Gated by the approval rules and "Phone
dictation" (off answers `remote_input_off`); a computer whose assistant is
switched off answers `assistant_off`.

```jsonc
// phone → computer
{ "kind": "assistant.ask", "requestId": "uuid",
  "text": "what's on my calendar tomorrow?",  // 1–4 000 chars, or instead:
  "audio": "<base64 WAV>",                     // as dictate's; transcribed by the computer
  "conversation": "c1" }                       // optional: continue an earlier answer's conversation

// computer → phone, as progress
{ "kind": "progress", "requestId": "uuid", "stage": "transcribing" }           // audio only
{ "kind": "progress", "requestId": "uuid", "stage": "thinking", "transcript": "…" }
{ "kind": "progress", "requestId": "uuid", "stage": "tool", "label": "Opening YouTube" }
{ "kind": "progress", "requestId": "uuid", "stage": "answer", "text": "…the answer so far…" }

// computer → phone, the result
{ "kind": "result", "requestId": "uuid", "ok": true,
  "transcript": "play believer on youtube",
  "answer": "Playing “Believer” by Imagine Dragons on YouTube.",   // Markdown
  "conversation": "c1",                       // pass back to continue it
  "actions": [ { "label": "Played “Believer” on YouTube", "status": "done" } ],
  "pending": { "id": 7, "summary": "Send the email to Ron" } }      // optional, see below
```

- `answer` progress carries the whole answer so far (not a delta), at most a
  few times a second, so a phone can simply replace what it shows; the result
  carries the final text.
- `actions` lists what the assistant did on the computer, each with the
  command's status (`done`, `failed`, `awaiting_confirmation`, …). A `pending`
  action is confirmed or cancelled with `command.confirm` (`id`), exactly as a
  command's is.
- A conversation lives on the computer, one per phone, for 30 minutes after its
  last question; an unknown or expired `conversation` starts a new one. It is
  the phone's own: it never mixes with the computer's quick ask or calls.
- Errors: `assistant_off`, `not_configured` (no model), `busy` (this phone's
  previous question is still being answered), `bad_audio`, `no_speech`,
  `no_speech_model`, `transcription_failed`, `failed` (the model call failed;
  the result's `message` says why in English), `cancelled`, `remote_input_off`,
  `not_approved`, `unsupported`.
- Timeouts: 60 s, restarted by every `progress` (3 minutes while the last
  progress is `approval`), plus the recording's length for audio.
- `thinking` always carries `transcript` (a typed question as given).
- A pending action is listed in `actions` too (`awaiting_confirmation`); a
  phone shows it once, as the pending card. `command.confirm` for it can be
  sent while no command is running on that computer (else `busy`).
- A conversation that expired or was unknown is answered with a new
  `conversation` id; that change is the only signal.
- A cancelled question ends with `ok: false`, `error: "cancelled"` and no
  answer; whatever `answer` progress arrived is all there is. `message` comes
  only with `failed`.
- Doing things needs the computer's "Voice commands" on; without it the
  assistant only answers, and `actions` is empty.
- Keys sent while watching (live view `keys`) count as input for the view's
  10-minute idle stop.

### TURN: `GET /v1/turn` (authenticated)

```jsonc
{ "iceServers": [ { "urls": ["stun:stun.cloudflare.com:3478"] },
                  { "urls": ["turn:turn.example.com:3478?transport=udp", "turns:turn.example.com:5349"],
                    "username": "1792800000:uuid", "credential": "…" } ],
  "ttl": 86400 }
```

The server always includes a public STUN server. It adds a TURN server when
configured: a coturn-style shared secret (`TURN_URLS`, `TURN_SECRET`; the
username is `<expiry unix seconds>:<user id>` and the credential the base64
HMAC-SHA1 of the username, the standard TURN REST scheme), or Cloudflare's TURN
service (`CLOUDFLARE_TURN_KEY_ID`, `CLOUDFLARE_TURN_API_TOKEN`), whose
short-lived credentials the server requests for each call. Credentials last
`ttl` seconds. Without TURN, phone and computer still connect on the same
network and across most home routers; strict NATs (many mobile networks) need
it.

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
