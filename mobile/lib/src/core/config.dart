import 'package:flutter/foundation.dart';

import 'models.dart';

/// Kept equal to `version` in pubspec.yaml (no package_info dependency).
const appVersion = '1.0.0';

/// Where the app asks for the account server's current address
/// (PROTOCOL.md, "Finding the server"). The server's own address changes from
/// time to time, so it is never built in.
const discoveryUrl = 'https://domain.gautamishan.com.np';

/// The build's `--dart-define=TALVIO_SERVER_URL`, which pins the server
/// (discovery and the Server setting are then off), or null without one. An
/// invalid one throws: `main` calls this first, so such a build fails at
/// launch.
String? pinnedServerUrl() {
  const fromBuild = String.fromEnvironment('TALVIO_SERVER_URL');
  if (fromBuild.isEmpty) return null;
  return normalizeServerUrl(fromBuild) ??
      (throw StateError(
        'TALVIO_SERVER_URL is "$fromBuild". Build with '
        '--dart-define=TALVIO_SERVER_URL=https://your-server',
      ));
}

/// Where Automatic goes while discovery has never answered: outside release
/// builds, a server on the development machine (the Android emulator reaches
/// its host at 10.0.2.2); a release build has nowhere to go.
String? developmentServerUrl() {
  if (kReleaseMode) return null;
  return defaultTargetPlatform == TargetPlatform.iOS
      ? 'http://localhost:8787'
      : 'http://10.0.2.2:8787';
}

/// The canonical form of a server address the user typed, or null when it is
/// not an http(s) base URL. A bare host gets `https://`. A [release] build
/// takes https only: dart:io ignores Android's cleartext policy and iOS App
/// Transport Security, so nothing else stops a password or token going out
/// in the clear.
String? normalizeServerUrl(String input, {bool release = kReleaseMode}) {
  final text = input.trim();
  if (text.isEmpty) return null;
  final uri = Uri.tryParse(text.contains('://') ? text : 'https://$text');
  if (uri == null ||
      (uri.scheme != 'https' && (release || uri.scheme != 'http')) ||
      uri.host.isEmpty ||
      uri.userInfo.isNotEmpty ||
      uri.hasQuery ||
      uri.hasFragment) {
    return null;
  }
  final path = uri.path.replaceFirst(RegExp(r'/+$'), '');
  return uri.replace(path: path).toString();
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
