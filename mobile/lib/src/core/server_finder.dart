import 'dart:async';
import 'dart:convert';
import 'dart:io' as io;

import 'package:flutter/foundation.dart';
import 'package:http/http.dart' as http;
import 'package:http/io_client.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'config.dart';

/// The server URL a discovery answer names (PROTOCOL.md, "Finding the
/// server"): `apiBaseUrl`, else the legacy `domain`, else the answer's only
/// string field, as an origin (`http(s)://host[:port]`). Null when it names
/// none.
String? serverFromDiscovery(String body, {bool release = kReleaseMode}) {
  final Object? json;
  try {
    json = jsonDecode(body);
  } on FormatException {
    return null;
  }
  if (json is! Map<String, Object?>) return null;
  final strings = json.values.whereType<String>();
  final value = json.containsKey('apiBaseUrl')
      ? json['apiBaseUrl']
      : json.containsKey('domain')
      ? json['domain']
      : strings.length == 1
      ? strings.single
      : null;
  if (value is! String) return null;
  return serverOrigin(value, release: release);
}

/// The origin of a server address: an `http://` or `https://` URL, or a bare
/// `host[:port]` (taken as https), reduced to `scheme://host[:port]`. Null
/// for a user part, another scheme, an invalid host or port, and in a
/// [release] build a loopback host.
///
/// Plain http is accepted in every build: the production server is an http
/// address on the local network. The discovery request itself is always
/// https (and follows redirects on its own host only), so only that trusted
/// answer can name an http server; nothing the user types ever does.
String? serverOrigin(String value, {bool release = kReleaseMode}) {
  final text = value.trim();
  final uri = Uri.tryParse(text.contains('://') ? text : 'https://$text');
  if (uri == null ||
      (uri.scheme != 'https' && uri.scheme != 'http') ||
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

/// Finds the account server, the only way the app learns it: asks
/// [discoveryUrl] when turned on (at launch, unless a development build pins
/// the server) and again when the
/// server cannot be reached, at most once per [interval], and keeps the last
/// good answer across launches. The address is not live everywhere yet, so a
/// failed lookup is normal: it is logged once per run of failures and retried.
class ServerFinder extends ChangeNotifier {
  ServerFinder(
    this._prefs, {
    http.Client? httpClient,
    DateTime Function()? now,
    this.release = kReleaseMode,
    Future<List<String>> Function(String host)? resolveFallback,
    http.Client Function(String host, String ip)? pinnedClient,
  }) : _http =
           httpClient ??
           IOClient(io.HttpClient()..connectionTimeout = _timeout),
       _now = now ?? DateTime.now,
       _resolveFallback = resolveFallback ?? resolveViaDnsOverHttps,
       _pinnedClient = pinnedClient ?? _ipPinnedClient;

  static const interval = Duration(minutes: 1);
  static const _timeout = Duration(seconds: 20);
  static const _key = 'server_found';

  final SharedPreferences _prefs;
  final http.Client _http;
  final DateTime Function() _now;

  /// Looks a host up without the phone's own DNS (see [_fetch]).
  final Future<List<String>> Function(String host) _resolveFallback;

  /// A client that reaches the discovery host at one address while still
  /// checking its certificate against the host name.
  final http.Client Function(String host, String ip) _pinnedClient;

  /// Whether this is a release build, which takes no loopback host.
  final bool release;

  var _enabled = false;
  var _looking = false;
  var _failing = false;
  String? _lastError;
  DateTime? _lastLookUp;
  Timer? _next;

  /// The server discovery last named; null until it first answers.
  String? get server => _prefs.getString(_key);

  /// Whether a lookup is under way.
  bool get looking => _looking;

  /// Why the last lookup found nothing, in plain technical words
  /// ("SocketException: …", "HTTP 403", "the answer named no usable
  /// server"); null after a lookup that worked. Shown under "Can't find
  /// the server" so a failure on a real phone can be told apart from the
  /// others without a debugger attached.
  String? get lastError => _lastError;

  /// Off only while a development build pins the server. Turning it on
  /// looks the server up at once.
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
    final found = await _fetch().timeout(
      _timeout,
      onTimeout: () {
        _lastError = 'no answer within ${_timeout.inSeconds} s';
        return null;
      },
    );
    _looking = false;
    if (found != null) {
      _failing = false;
      _lastError = null;
      await _prefs.setString(_key, found);
    } else if (!_failing) {
      _failing = true;
      debugPrint(
        'Talvio: no server address from $discoveryUrl; trying again later',
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

  /// The server [discoveryUrl] names. Null on any failure, with the reason in
  /// [lastError].
  ///
  /// When the phone's own DNS cannot find the discovery host, the host is
  /// looked up over DNS-over-HTTPS instead and the request goes to that
  /// address, with TLS still checked against the host name. Phones on
  /// Private DNS or a resolver that does not know the name yet answered
  /// "No address associated with hostname" here, while the browser, which
  /// does its own secure DNS, loaded the same address fine.
  Future<String?> _fetch() async {
    _hostLookupFailed = false;
    final found = await _fetchWith(_http);
    if (found != null || !_hostLookupFailed) return found;

    final host = Uri.parse(discoveryUrl).host;
    final firstError = _lastError;
    List<String> addresses;
    try {
      addresses = await _resolveFallback(host);
    } on Exception catch (e) {
      _lastError = _clip('$firstError; secure DNS failed too: $e');
      return null;
    }
    if (addresses.isEmpty) {
      _lastError = _clip('$firstError; secure DNS found no address either');
      return null;
    }
    for (final ip in addresses.take(2)) {
      final client = _pinnedClient(host, ip);
      try {
        final viaFallback = await _fetchWith(client);
        if (viaFallback != null) {
          debugPrint('Talvio: found the server through secure DNS ($ip)');
          return viaFallback;
        }
      } finally {
        client.close();
      }
    }
    _lastError = _clip('$firstError; through secure DNS: $_lastError');
    return null;
  }

  var _hostLookupFailed = false;

  static String _clip(String text) =>
      text.length > 240 ? '${text.substring(0, 240)}…' : text;

  /// One discovery request through [client], following redirects on its own
  /// host only (the answer decides where sign-ins and tokens go). Sends
  /// nothing but the request itself.
  Future<String?> _fetchWith(http.Client client) async {
    var url = Uri.parse(discoveryUrl);
    try {
      for (var hop = 0; hop < 5; hop++) {
        final request = http.Request('GET', url)
          ..followRedirects = false
          ..headers['Accept'] = 'application/json';
        final response = await http.Response.fromStream(
          await client.send(request),
        );
        final location = response.headers['location'];
        if (response.statusCode ~/ 100 == 3 && location != null) {
          final next = url.resolve(location);
          if (next.scheme != 'https' || next.authority != url.authority) {
            _lastError = 'redirected to another address (${next.authority})';
            return null;
          }
          url = next;
          continue;
        }
        if (response.statusCode != 200) {
          _lastError = 'HTTP ${response.statusCode}';
          return null;
        }
        final body = utf8.decode(response.bodyBytes, allowMalformed: true);
        final found = serverFromDiscovery(body, release: release);
        if (found == null) {
          final shown = body.length > 80 ? '${body.substring(0, 80)}…' : body;
          _lastError = 'the answer named no usable server: $shown';
        }
        return found;
      }
      _lastError = 'too many redirects';
    } on Exception catch (e) {
      // Offline, DNS (the address does not resolve yet), refused, TLS.
      final text = '$e';
      _hostLookupFailed = isHostLookupFailure(text);
      _lastError = text.length > 160 ? '${text.substring(0, 160)}…' : text;
    }
    return null;
  }

  /// An HttpClient that connects to [ip] for [host], doing TLS with [host]
  /// as the server name so the certificate is checked exactly as usual.
  static http.Client _ipPinnedClient(String host, String ip) {
    final client = io.HttpClient()
      ..connectionTimeout = _timeout
      ..findProxy = (_) => 'DIRECT';
    client.connectionFactory = (uri, proxyHost, proxyPort) async {
      Future<io.Socket> connect() async {
        // Only the discovery host is pinned; anything else connects as usual.
        final target = uri.host == host ? ip : uri.host;
        final plain = await io.Socket.connect(
          target,
          uri.port,
          timeout: _timeout,
        );
        if (uri.scheme != 'https') return plain;
        return io.SecureSocket.secure(plain, host: uri.host);
      }

      return io.ConnectionTask.fromSocket(connect(), () {});
    };
    return IOClient(client);
  }
}

/// Whether an error is the phone's DNS not finding a host (Android:
/// "Failed host lookup" / "No address associated with hostname").
bool isHostLookupFailure(String error) {
  final text = error.toLowerCase();
  return text.contains('host lookup') ||
      text.contains('no address associated') ||
      text.contains('nodename nor servname');
}

/// The IPv4 addresses in a DNS-over-HTTPS JSON answer (the format both
/// Cloudflare's and Google's resolvers use): `Answer` records of type 1 (A).
List<String> parseDnsJsonAnswer(String body) {
  final Object? json;
  try {
    json = jsonDecode(body);
  } on FormatException {
    return const [];
  }
  if (json is! Map<String, Object?>) return const [];
  final answers = json['Answer'];
  if (answers is! List) return const [];
  return [
    for (final record in answers)
      if (record is Map<String, Object?> &&
          record['type'] == 1 &&
          record['data'] is String &&
          io.InternetAddress.tryParse(record['data'] as String)?.type ==
              io.InternetAddressType.IPv4)
        record['data'] as String,
  ];
}

/// Looks [host] up over DNS-over-HTTPS: Cloudflare (1.1.1.1), then Google
/// (8.8.8.8). Both are addressed by IP, so this needs no DNS of its own, and
/// both are https, so the answer cannot be altered on the way.
Future<List<String>> resolveViaDnsOverHttps(String host) async {
  final client = IOClient(
    io.HttpClient()..connectionTimeout = const Duration(seconds: 8),
  );
  try {
    for (final endpoint in [
      Uri.https('1.1.1.1', '/dns-query', {'name': host, 'type': 'A'}),
      Uri.https('8.8.8.8', '/resolve', {'name': host, 'type': 'A'}),
    ]) {
      try {
        final response = await client
            .get(endpoint, headers: {'Accept': 'application/dns-json'})
            .timeout(const Duration(seconds: 8));
        if (response.statusCode != 200) continue;
        final addresses = parseDnsJsonAnswer(response.body);
        if (addresses.isNotEmpty) return addresses;
      } on Exception {
        continue;
      }
    }
  } finally {
    client.close();
  }
  return const [];
}
