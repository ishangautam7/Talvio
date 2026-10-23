import 'package:flutter/foundation.dart';

import 'models.dart';
import 'server_finder.dart';

/// Kept equal to `version` in pubspec.yaml (no package_info dependency).
const appVersion = '1.0.0';

/// Where the app asks for the account server's current address
/// (PROTOCOL.md, "Finding the server"). The server's own address changes from
/// time to time, so it is never built in.
const discoveryUrl = 'https://domain.gautamishan.com.np';

/// The build's `--dart-define=TALVIO_SERVER_URL`, a developer override that
/// pins the server (discovery is then off), or null without one. Ignored in
/// [release] builds, which only ever use the server discovery names. An
/// invalid one throws: `main` calls this first, so such a build fails at
/// launch.
String? pinnedServerUrl({bool release = kReleaseMode}) {
  const fromBuild = String.fromEnvironment('TALVIO_SERVER_URL');
  if (release || fromBuild.isEmpty) return null;
  return serverOrigin(fromBuild, release: false) ??
      (throw StateError(
        'TALVIO_SERVER_URL is "$fromBuild". Build with '
        '--dart-define=TALVIO_SERVER_URL=http://192.168.1.20:8787',
      ));
}

/// Where the app goes while discovery has never answered: outside release
/// builds, a server on the development machine (the Android emulator reaches
/// its host at 10.0.2.2); a release build has nowhere to go.
String? developmentServerUrl() {
  if (kReleaseMode) return null;
  return defaultTargetPlatform == TargetPlatform.iOS
      ? 'http://localhost:8787'
      : 'http://10.0.2.2:8787';
}

/// How this phone introduces itself when it signs in.
DeviceInfo thisDevice() {
  final ios = defaultTargetPlatform == TargetPlatform.iOS;
  return DeviceInfo(
    name: ios ? 'iPhone' : 'Android phone',
    platform: ios ? 'ios' : 'android',
    kind: 'mobile',
    appVersion: appVersion,
  );
}
