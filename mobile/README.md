# Talvio for phones

The Android and iOS app. Sign in with the same account as Talvio on your
computer, and the phone becomes a wireless mic and keyboard for it: speak on
the phone, and the computer transcribes with its own speech model and pastes
at its cursor (or sends the text back); type on the phone, and the computer
pastes it. The account server stores accounts and devices only, never
recordings or text. The wire format is [`server/PROTOCOL.md`](../server/PROTOCOL.md).

## Run it

Needs Flutter 3.44 or newer (`flutter doctor`), and for Android the Android
SDK (`ANDROID_HOME`) with JDK 17; for iOS a Mac with Xcode.

```bash
cd mobile
flutter pub get
flutter run                     # the emulator, simulator or phone that is connected
flutter test
flutter analyze
dart format lib test            # this folder is not formatted by prettier
```

Start an account server first (see `server/`). Until the discovery address
(below) has answered once, a debug or profile build looks for it on the
development machine at port 8787: `http://10.0.2.2:8787` from the Android
emulator, `http://localhost:8787` from the iOS simulator.

## Choosing the server

The server's address changes from time to time, so it is not built in
([`PROTOCOL.md`](../server/PROTOCOL.md), "Finding the server"):

- **Automatic** (the default): the app asks `discoveryUrl` in
  `lib/src/core/config.dart` at launch, and again when the server does not
  answer (at most once a minute), and keeps the last answer. A signed-in
  session follows the server to its new address with the same token.
- **Custom**: **Change** at the bottom of the sign-in screen takes an address;
  **Use automatic** there goes back. A session signed in this way stays on
  that address. It can only change while signed out.
- At build time: `flutter run --dart-define=TALVIO_SERVER_URL=https://sync.example.com`
  (also works with `flutter build`) pins the server; the sign-in screen then
  offers no Change.

The discovery address does not answer yet. That is a normal state: the app
logs it once and keeps trying, and meanwhile uses its last answer or, in
development, the machine above.

### A real phone on your network

The phone cannot reach `localhost` on your computer. Start the server so it
listens on your LAN address, then either build with
`--dart-define=TALVIO_SERVER_URL=http://192.168.1.20:8787` or enter that
address under **Change**.

Plain `http://` only works in debug and profile builds. Release builds take
`https://` addresses only, and the app enforces that itself: its traffic goes
through `dart:io`, which ignores Android's cleartext setting and iOS App
Transport Security.

## Release builds

- No server address is needed: release builds use Automatic. Until
  discovery first answers, the sign-in screen says it can't find the server
  and keeps trying (it never falls back to the development machine). A
  `TALVIO_SERVER_URL` that is not a valid `https://` address stops the app
  at launch.
- Android signs with the key named in `android/key.properties` (`storeFile`,
  `storePassword`, `keyAlias`, `keyPassword`; never committed). Without that
  file the debug key signs it, which Play refuses.

## Google sign-in

Shown only when the server says it is set up (`GET /v1/config` →
`google: true`). The app opens an auth session (Custom Tabs on Android,
`ASWebAuthenticationSession` on iOS) at
`{server}/v1/auth/google/start` with a PKCE challenge, and the server sends the
browser back to **`talvio://auth/callback`**. Android catches that scheme with
flutter_web_auth_2's `CallbackActivity` (in `AndroidManifest.xml`); iOS needs no
registration because the auth session receives it directly.
The scheme was `zynveliq://` before the rename to Talvio; the server accepts
both, but a server older than the rename only accepts the old one, so update
the server before shipping this app against it.

## How the code is laid out

```
lib/src/
  core/       server URL and discovery, HTTP client, session storage, the relay socket
  features/   auth/, dictate/, account/: one folder per feature, its strings included
  shell/      the tabbed home screen; features.dart is where features register
```

`RelayClient` (`core/relay_client.dart`) is the one WebSocket. It is open only
while signed in and either the app is on screen or a request still waits for
its result (`busy`), so a phone locked while its computer asks to allow it
still gets the answer. `request(deviceId, body)` sends a request under a new
`requestId` and returns every `progress` and the final `result` for it;
`cancel(requestId)` asks the computer to stop. A request does not outlive its
socket: if the socket closes first, it fails with `RelayError.interrupted`,
because the computer may have acted on it and its result is lost.

### Adding a feature

A new feature (the phone-to-computer agent, for instance, whose relay kinds
all start with `agent.`) goes in its own folder under `lib/src/features/` and
registers in `lib/src/shell/features.dart`:

- a `FeatureTab` in `featureTabs`, with `requires:` set to the capability a
  computer must advertise for the tab to show;
- if computers send this phone requests of its own, a handler in
  `relayHandlers`. Its kind is then also advertised in `hello`.

Talking to a computer is `relay.request(computerId, {'kind': 'agent.…', …})`
and listening to the stream it returns. Anything stored for the signed-in
account should use a preference key starting with `account.` so sign-out
removes it.

## App identity

- Android `applicationId` / iOS bundle id: `com.talvio.mobile`
- Name on the home screen: Talvio
- `appVersion` sent to the server is `lib/src/core/config.dart`'s
  `appVersion`; keep it equal to `version` in `pubspec.yaml`.
