import 'dart:async';
import 'dart:convert';
import 'dart:io' as io;

import 'package:flutter/foundation.dart';
import 'package:http/http.dart' as http;
import 'package:http/io_client.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'config.dart';

/// The server URL a discovery answer names (PROTOCOL.md, "Finding the
/// server"): `https://<host[:port]>`, or outside a [release] build also an
/// explicit `http://` one. Null when it names none.
String? serverFromDiscovery(String body, {bool release = kReleaseMode}) {
  final Object? json;
  try {
    json = jsonDecode(body);
  } on FormatException {
    return null;
  }
  if (json is! Map<String, Object?>) return null;
  final strings = json.values.whereType<String>();
  final value = json.containsKey('domain')
      ? json['domain']
      : strings.length == 1
      ? strings.single
      : null;
  if (value is! String) return null;
  final text = value.trim();
  final uri = Uri.tryParse(text.contains('://') ? text : 'https://$text');
  if (uri == null ||
      (uri.scheme != 'https' && (release || uri.scheme != 'http')) ||
      uri.userInfo.isNotEmpty ||
      !(RegExp(r'^[a-z0-9.-]+$').hasMatch(uri.host) ||
          io.InternetAddress.tryParse(uri.host) != null) ||
      (uri.hasPort && (uri.port < 1 || uri.port > 65535)) ||
      (release && _isLoopback(uri.host))) {
    return null;
  }
  return Uri(
    scheme: uri.scheme,
    host: uri.host,
    port: uri.hasPort ? uri.port : null,
  ).toString();
}

bool _isLoopback(String host) =>
    host == 'localhost' ||
    host.endsWith('.localhost') ||
    (io.InternetAddress.tryParse(host)?.isLoopback ?? false);

/// Finds the account server in Automatic mode: asks [discoveryUrl] when
/// turned on (at launch, or on switching back to Automatic) and again when the
/// server cannot be reached, at most once per [interval], and keeps the last
/// good answer across launches. The address is not live everywhere yet, so a
/// failed lookup is normal: it is logged once per run of failures and retried.
class ServerFinder extends ChangeNotifier {
  ServerFinder(
    this._prefs, {
    http.Client? httpClient,
    DateTime Function()? now,
    this.release = kReleaseMode,
  }) : _http =
           httpClient ??
           IOClient(io.HttpClient()..connectionTimeout = _timeout),
       _now = now ?? DateTime.now;

  static const interval = Duration(minutes: 1);
  static const _timeout = Duration(seconds: 10);
  static const _key = 'server_found';

  final SharedPreferences _prefs;
  final http.Client _http;
  final DateTime Function() _now;

  /// Whether this is a release build, which takes https only and no loopback
  /// host.
  final bool release;

  var _enabled = false;
  var _looking = false;
  var _failing = false;
  DateTime? _lastLookUp;
  Timer? _next;

  /// The server discovery last named; null until it first answers.
  String? get server => _prefs.getString(_key);

  /// Whether a lookup is under way.
  bool get looking => _looking;

  /// On only in Automatic mode. Turning it on looks the server up at once.
  set enabled(bool enabled) {
    if (enabled == _enabled) return;
    _enabled = enabled;
    _next?.cancel();
    _next = null;
    if (enabled) unawaited(lookUp());
  }

  /// Looks the server up now; also what Try again does.
  Future<void> lookUp() async {
    if (!_enabled || _looking) return;
    _next?.cancel();
    _next = null;
    _looking = true;
    _lastLookUp = _now();
    notifyListeners();
    final found = await _fetch().timeout(_timeout, onTimeout: () => null);
    _looking = false;
    if (found != null) {
      _failing = false;
      await _prefs.setString(_key, found);
    } else if (!_failing) {
      _failing = true;
      debugPrint(
        'Zynveliq: no server address from $discoveryUrl; trying again later',
      );
    }
    notifyListeners();
    // With no answer ever, there is nothing to use meanwhile: keep looking.
    if (server == null) unreachable();
  }

  /// The current server could not be reached (no answer at all, not an HTTP
  /// error): look it up again, at most once per [interval].
  void unreachable() {
    if (!_enabled || _looking || _next != null) return;
    final last = _lastLookUp;
    final wait = last == null
        ? Duration.zero
        : interval - _now().difference(last);
    _next = Timer(wait.isNegative ? Duration.zero : wait, () {
      _next = null;
      unawaited(lookUp());
    });
  }

  /// The server [discoveryUrl] names, following redirects on its own host
  /// only (the answer decides where sign-ins and tokens go). Sends nothing
  /// but the request itself. Null on any failure.
  Future<String?> _fetch() async {
    var url = Uri.parse(discoveryUrl);
    try {
      for (var hop = 0; hop < 5; hop++) {
        final request = http.Request('GET', url)
          ..followRedirects = false
          ..headers['Accept'] = 'application/json';
        final response = await http.Response.fromStream(
          await _http.send(request),
        );
        final location = response.headers['location'];
        if (response.statusCode ~/ 100 == 3 && location != null) {
          final next = url.resolve(location);
          if (next.scheme != 'https' || next.authority != url.authority) {
            return null;
          }
          url = next;
          continue;
        }
        if (response.statusCode != 200) return null;
        return serverFromDiscovery(
          utf8.decode(response.bodyBytes, allowMalformed: true),
          release: release,
        );
      }
    } on Exception {
      // Offline, DNS (the address does not resolve yet), refused, TLS.
    }
    return null;
  }
}
