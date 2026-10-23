# Talvio account server

The small server behind Talvio accounts. It signs people in (email and password,
or Google) on the desktop app and the phone app, keeps the list of devices on each
account, and relays messages between a person's own devices, which is what lets a
phone use the computer as its microphone and keyboard.

It stores **accounts and devices only**. Recordings and transcripts pass through
memory on their way from one device to another and are never written down or
logged.

Every route, message, limit and error code is defined in [PROTOCOL.md](PROTOCOL.md).
Change the protocol there first, then here and in both apps.

Built on Bun alone: `Bun.serve` for HTTP and WebSockets, `bun:sqlite` for storage,
`Bun.password` (argon2id) for passwords. There are no runtime dependencies.

## Run locally

```sh
cd server
bun install      # dev tooling only: Bun's types and TypeScript
bun run dev      # restarts on changes; `bun start` runs without watching
bun test
bunx tsc --noEmit
```

It listens on `http://localhost:8787` and keeps its database in `./data/`.

## How the apps find it

The apps don't hard-code this server's address. They read it from
`https://domain.gautamishan.com.np`, which must answer JSON naming the current
domain:

```json
{ "domain": "sync.example.com" }
```

The apps then use `https://sync.example.com`, so the server has to be reachable
over https at that name, for example through a tunnel or a reverse proxy in
front of port 8787:

```sh
bun run start                                  # http on :8787
cloudflared tunnel --url http://localhost:8787 # prints https://<random>.trycloudflare.com
```

When the domain changes, update what that address answers (and `PUBLIC_URL`).
Signed-in apps move to the new domain on their own, without signing in again.
Development builds also accept a plain `http://` URL there, such as
`http://192.168.1.5:8787` for a server on your network. The full rules are under
"Finding the server" in [PROTOCOL.md](PROTOCOL.md). Whoever controls that
address decides where people's sign-ins go, so keep it locked down.

## Configuration

| Variable               | Default                  | Meaning                                                                                                                                |
| ---------------------- | ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------- |
| `PORT`                 | `8787`                   | Port to listen on.                                                                                                                     |
| `HOST`                 | `0.0.0.0`                | Address to listen on.                                                                                                                  |
| `DATABASE_PATH`        | `./data/talvio.db`       | SQLite file. Its folder is created if missing. A `talvio.db` that does not exist yet, with a `zynveliq.db` beside it, opens that one.  |
| `PUBLIC_URL`           | `http://localhost:$PORT` | The address people's browsers reach the server at. Google sends them back to `$PUBLIC_URL/v1/auth/google/callback`.                    |
| `GOOGLE_CLIENT_ID`     | unset                    | Google OAuth client. Google sign-in is on only when both Google variables are set.                                                     |
| `GOOGLE_CLIENT_SECRET` | unset                    | Stays on the server; the apps never see it.                                                                                            |
| `TRUST_PROXY`          | unset                    | `1` takes the client's address from the last `X-Forwarded-For` entry, for rate limiting. Set it only behind exactly one reverse proxy. |

## Google sign-in

1. In the [Google Cloud Console](https://console.cloud.google.com/), open
   **APIs & Services → OAuth consent screen** and set up the app (name, support
   email; the scopes are `openid`, `email` and `profile`).
2. Open **Credentials → Create credentials → OAuth client ID** and pick the type
   **Web application**.
3. Under **Authorized redirect URIs**, add `{PUBLIC_URL}/v1/auth/google/callback`,
   for example `https://sync.example.com/v1/auth/google/callback`. It has to match
   `PUBLIC_URL` exactly.
4. Set `GOOGLE_CLIENT_ID` and `GOOGLE_CLIENT_SECRET` from the client you created
   and restart the server. `GET /v1/config` then answers `"google": true`.

The apps never talk to Google's token endpoint themselves: the server redeems
Google's code, then hands the app a one-time login code that only the app holding
the PKCE verifier can use.

## Deploy

```sh
docker build -t talvio-server server
docker run -d --name talvio-server --restart unless-stopped \
  -p 127.0.0.1:8787:8787 \
  -v talvio-data:/data \
  -e PUBLIC_URL=https://sync.example.com -e TRUST_PROXY=1 \
  -e GOOGLE_CLIENT_ID=... -e GOOGLE_CLIENT_SECRET=... \
  talvio-server
```

The image runs as a non-root user and keeps the database on the `/data` volume.

**A deployment from before the rename to Talvio** used the image and container
name `zynveliq-server`, the volume `zynveliq-data` and the file
`/data/zynveliq.db`. Keep that volume: mount it as before
(`-v zynveliq-data:/data`) and the server keeps opening `zynveliq.db`, since it
only starts a new `talvio.db` where neither file exists (it logs the old file
when it uses it). In the backup command, use your container's name and that
file.

- **Serve it over HTTPS.** Passwords and session tokens travel in requests, so put
  a TLS-terminating reverse proxy in front and never expose plain HTTP. The proxy
  must pass WebSocket upgrades on `/v1/ws` (Caddy does by default:
  `sync.example.com { reverse_proxy 127.0.0.1:8787 }`).
- **Run one instance.** Who is online, the socket routing and the rate limits live
  in the process's memory, so two instances would not see each other's devices.
  The account data itself is in SQLite.
- **Back up the database.** Losing it signs everyone out and deletes every account.
  The file is in WAL mode, so copy it with SQLite rather than `cp` while the server
  runs:

  ```sh
  docker exec talvio-server bun -e \
    'new (require("bun:sqlite").Database)("/data/talvio.db").run("VACUUM INTO ?", ["/data/backup.db"])'
  ```

  then move `/data/backup.db` off the machine.

Logs are one line per request (method, path, status, time) plus WebSocket connect
and disconnect counts. They never contain tokens, passwords, codes, email
addresses or message contents.

## Adding a phone ↔ computer feature

New kinds of phone ↔ computer messages, including the reserved `agent.*` family,
need no change here: they travel inside `relay` bodies, which the server never
reads, and their capability names pass through `hello` untouched. A new top-level
WebSocket message type would be one entry in the `handlers` table in
`src/hub.ts`.
